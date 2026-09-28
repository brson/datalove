//! Statement type checking.
//!
//! Provides functions for type checking statements including function definitions.

use rmx::prelude::*;
use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::check::check_expr;
use crate::types::{convert_type_hint_with_aliases, type_to_string};

pub use crate::{Type, TypeError};

// ============================================================================
// Variable Declaration Helper
// ============================================================================

/// Check a variable declaration (let or var) and add binding to context.
///
/// If type hint is provided, checks value against it.
/// Otherwise, synthesizes type from value.
fn check_variable_decl<'db>(
    ctx: &mut TypeContext<'db>,
    name: bct::text::InternedText<'db>,
    value: ExprFun<'db>,
    type_hint: Option<datalit::ast::TypeHint<'db>>,
    is_mutable: bool,
) {
    let db = ctx.db;

    let var_type = match type_hint {
        Some(hint) => {
            match ctx.convert_hint(hint) {
                Ok(expected_type) => {
                    if let Some(err) = unordered_binding_error(ctx, &expected_type, value) {
                        ctx.add_error(err);
                        return;
                    }
                    match check_expr(ctx, value, &expected_type) {
                        Ok(()) => Some(expected_type),
                        Err(e) => {
                            ctx.add_error(e);
                            None
                        }
                    }
                }
                Err(e) => {
                    ctx.add_error(e);
                    None
                }
            }
        }
        None => {
            match ctx.synthesize_expr(value) {
                Ok(ty) => Some(ty),
                Err(e) => {
                    ctx.add_error(e);
                    None
                }
            }
        }
    };

    if let Some(ty) = var_type {
        ctx.add_variable(name, ty, is_mutable);
    }
}

/// Reject a binding whose type puts a type parameter where an order is needed.
///
/// The signature is checked where it is resolved; this is the same rule for a
/// type written inside the body, where `var seen: #{T} = #{}` asks `T` for an
/// ordering just as a parameter of that type would.
fn unordered_binding_error<'db>(
    ctx: &mut TypeContext<'db>,
    ty: &Type<'db>,
    at: ExprFun<'db>,
) -> Option<TypeError> {
    let Type::Datalit(dt) = ty else { return None };
    let bounds = ctx.type_param_bounds.clone();
    let ordered = |name: bct::text::InternedText<'db>| -> bool {
        bounds.get(&name).is_some_and(|bound| bound.implies_ord())
    };
    let param = datalove_datafun_common::generics::first_unordered_collection_key(dt, &ordered)?;
    Some(ctx.error_cannot_synthesize(at, &format!(
        "`{}` has not said it can be ordered, and this binding puts it in a set or a map. \
A set keeps its elements in order and a map keeps its keys in order, so say so with \
`with {{ {} is ord, }}`, which every type there is satisfies",
        param.as_str(ctx.db), param.as_str(ctx.db),
    )))
}

/// Check a statement.
pub fn check_statement<'db>(
    ctx: &mut TypeContext<'db>,
    statement: &Statement<'db>,
) {
    let db = ctx.db;

    match statement {
        Statement::Let(stmt) => {
            check_variable_decl(ctx, stmt.name, stmt.value, stmt.type_hint.clone(), false);
        }

        Statement::Var(stmt) => {
            match stmt.value {
                Some(value) => {
                    check_variable_decl(ctx, stmt.name, value, stmt.type_hint.clone(), true);
                }
                None => {
                    // Uninitialized var - type hint is required (parser enforces this).
                    if let Some(hint) = stmt.type_hint.clone() {
                        match ctx.convert_hint(hint) {
                            Ok(ty) => {
                                ctx.add_variable(stmt.name, ty, true);
                            }
                            Err(e) => {
                                ctx.add_error(e);
                            }
                        }
                    }
                    // If no type hint, parser already emitted error.
                }
            }
        }

        Statement::Set(stmt) => {
            let value = stmt.value;
            let place = &stmt.target;

            // Check that the root variable exists and is mutable.
            let root_name = place.root;
            match ctx.lookup_variable(root_name) {
                Some(_) => {
                    if let Some(false) = ctx.lookup_variable_mutability(root_name) {
                        let err = ctx.error_variable_not_mutable(stmt, root_name.text(db).as_str());
                        ctx.add_error(err);
                    }
                }
                None => {
                    let err = ctx.error_undefined_variable_set(stmt, root_name.text(db).as_str());
                    ctx.add_error(err);
                    return;
                }
            }

            // Walk the place steps to determine the target type.
            match typecheck_place_for_set(ctx, stmt, place, value) {
                Ok(()) => {}
                Err(e) => ctx.add_error(e),
            }
        }

        Statement::Fun(stmt) => {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let body = stmt.body(db);

            // Name resolution skips functions whose signature types do not
            // resolve, having already reported the error. Skip the body too,
            // since there are no parameter types to check it against.
            let Some(func_type) = ctx.lookup_function(name) else {
                return;
            };

            let param_types = func_type.param_types(db);
            let ret_ty = func_type.return_type(db);

            // A const parameter fixes a value at compile time and a type
            // parameter is erased to a `data`, so a parameter cannot be both.
            // The const argument would be the only thing saying what the type
            // parameter is, which means pinning it concretely for that
            // instantiation -- and specialization builds a copy by substituting
            // parameters into cloned blocks, so it cannot change the copy's
            // signature, its descriptor shapes, or the erasure decisions inside
            // its body. Left alone it goes wrong quietly: the const argument is
            // erased before the call, `comptime_values` cannot read a constant
            // back out of an `Erase`, and the parameter silently stays a
            // runtime one.
            //
            // Only the parameter is refused. A const parameter of an ordinary
            // type in a generic function is fine, because the two vary a
            // function along different axes: specialization deletes const
            // parameters and erasure replaces type parameters, so one copy per
            // const instantiation serves every type instantiation.
            if !stmt.type_params(db).is_empty() {
                for (i, param) in params.iter().enumerate().filter(|(_, p)| p.is_comptime) {
                    let generic = matches!(
                        param_types.get(i),
                        Some(Type::Datalit(ty))
                            if datalove_datafun_common::generics::contains_type_param(ty));
                    if generic {
                        ctx.add_error(TypeError::ComptimeParamOfGenericType {
                            func_name: name.as_str(db).to_string(),
                            param_name: param.name.as_str(db).to_string(),
                        });
                    }
                }
            }

            // Create new context for function body with parameters in scope.
            //
            // **A function body is not a closure.** It sees its parameters, the
            // consts in scope and the functions, and nothing else. The
            // enclosing scope's `let` and `var` bindings are dropped here
            // rather than merely saved: a script unit seeds `variables` with
            // every earlier unit's bindings, so leaving them visible let a body
            // name one and typecheck, and the mistake then surfaced from
            // lowering as "binding not available yet", which reads as a phase
            // ordering problem rather than the scoping error it is. A module
            // has no top-level bindings, so this was invisible there.
            //
            // Consts stay, which is why the filter is on `const_bindings`
            // rather than a clear.
            let saved_variables = ctx.variables.C();
            let consts_in_scope = ctx.const_bindings.C();
            ctx.variables.retain(|name, _| consts_in_scope.contains(name));
            // A type parameter is in scope in the body as well as the
            // signature, so a binding there can be annotated with one:
            // `var x: T` and `let pair: (T, T)` name a type the function has,
            // and without this they read as an unresolved alias.
            let saved_type_aliases = ctx.type_aliases.C();
            let saved_type_param_bounds = ctx.type_param_bounds.C();
            let bounds = stmt.type_bounds(db);
            for (i, name) in stmt.type_params(db).iter().enumerate() {
                ctx.type_aliases.insert(
                    *name,
                    Type::Datalit(datalove_datalit::tycheck::Type::Var(*name)),
                );
                if let Some(Some(bound)) = bounds.get(i) {
                    ctx.type_param_bounds.insert(*name, *bound);
                }
            }
            // Const bindings are scoped to the body the same way. Without this a
            // const parameter, or a const statement, stays registered after its
            // function ends and the next function's argument of that name passes
            // the const-binding check on the strength of it.
            let saved_const_bindings = ctx.const_bindings.C();
            let saved_return_type = ctx.expected_return_type.clone();
            let saved_is_void = ctx.is_void_function;

            // Add parameters to context.
            // Mut and Out params are mutable and can be assigned with `set`.
            for (param, param_ty) in params.iter().zip(param_types.iter()) {
                let is_mutable = matches!(param.mode, datalove_datafun_ast::ast::ParamMode::Mut | datalove_datafun_ast::ast::ParamMode::Out);
                ctx.add_variable(param.name, param_ty.clone(), is_mutable);
                // A const parameter is a compile-time constant, so a const
                // expression in the body may name it like any other const.
                if param.is_comptime {
                    ctx.add_const_binding(param.name);
                }
            }

            ctx.expected_return_type = Some(ret_ty);
            ctx.is_void_function = stmt.return_type(db).is_none();

            // Check function body.
            for stmt in body {
                check_statement(ctx, stmt);
            }

            // A function that owes a value has to have returned one by every
            // way out, and the end of the body is a way out. Lowering adds the
            // return there whatever the body did, so a body that simply ended
            // handed the caller a zeroed value of the return type: `0` for an
            // integer, and for a string a null buffer that prints as empty.
            if stmt.return_type(db).is_some()
                && datalove_datafun_ast::reachable::body_completes(body)
            {
                ctx.report_missing_return(stmt.local_index(db), name);
            }

            // Restore context.
            ctx.variables = saved_variables;
            ctx.type_aliases = saved_type_aliases;
            ctx.type_param_bounds = saved_type_param_bounds;
            ctx.const_bindings = saved_const_bindings;
            ctx.expected_return_type = saved_return_type;
            ctx.is_void_function = saved_is_void;
        }

        Statement::Ret(stmt) => {
            let ret_value = stmt.value;
            let expected_ty = ctx.expected_return_type.clone();

            match (ret_value, expected_ty) {
                (Some(value), Some(expected_ret_ty)) => {
                    // Has value - check if void (no declared return type).
                    if ctx.is_void_function {
                        // Void function with value - ERROR.
                        let err = ctx.error_void_function_returns_value(stmt);
                        ctx.add_error(err);
                    } else {
                        // Non-void function with value - check type.
                        if let Err(e) = check_expr(ctx, value, &expected_ret_ty) {
                            ctx.add_error(e);
                        }
                    }
                }
                (None, Some(_expected_ret_ty)) => {
                    // Bare ret - check if void (no declared return type).
                    if !ctx.is_void_function {
                        // Non-void function with bare ret - ERROR.
                        let err = ctx.error_function_requires_return_value(stmt);
                        ctx.add_error(err);
                    }
                    // Void function with bare ret - OK.
                }
                (Some(value), None) => {
                    // F011: Cannot synthesize return type.
                    let err = ctx.error_cannot_synthesize(value, "cannot infer return type");
                    ctx.add_error(err);
                }
                (None, None) => {
                    // Bare ret outside function - already an error from earlier checks.
                }
            }
        }

        Statement::Require(_) => {
            // TODO: implement require type checking.
        }

        Statement::Import(_) => {
            // TODO: implement import type checking.
            // This will be handled in typecheck_package_world.
        }

        Statement::If(stmt) => {
            let condition = stmt.condition;
            let then_binding = stmt.then_binding;
            let then_body = &stmt.then_body;
            let else_binding = stmt.else_binding;
            let else_body = &stmt.else_body;

            // If there's a binding, this is destructuring syntax.
            if let Some(binding_name) = then_binding {
                // Synthesize the condition type.
                let condition_ty = match ctx.synthesize_expr(condition) {
                    Ok(ty) => ty,
                    Err(e) => {
                        ctx.add_error(e);
                        return;
                    }
                };

                // Extract inner type from Option or Result.
                let inner_ty = match condition_ty {
                    Type::Datalit(datalit::tycheck::Type::Option(ref opt)) => {
                        opt.inner_type.clone()
                    }
                    Type::Datalit(datalit::tycheck::Type::Result(ref res)) => {
                        // F046: Result destructuring requires error-binding else branch.
                        if else_body.is_none() || else_binding.is_none() {
                            let err = ctx.error_result_requires_binding(condition);
                            ctx.add_error(err);
                            return;
                        }
                        res.inner_type.clone()
                    }
                    _ => {
                        // F017: If/match condition type mismatch.
                        let err = ctx.error_type_mismatch(
                            condition,
                            "Option or Result",
                            "other type",
                            "expected Option or Result type for destructuring"
                        );
                        ctx.add_error(err);
                        return;
                    }
                };

                // Convert datalit Type to datafun Type.
                let inner_type = Type::Datalit((*inner_ty).clone());
                let binding_ty = inner_type;

                ctx.with_scope(|ctx| {
                    ctx.variables.insert(binding_name, (binding_ty, false));
                    for stmt in then_body {
                        check_statement(ctx, stmt);
                    }
                });

                if let Some(else_stmts) = else_body {
                    ctx.with_scope(|ctx| {
                        if let Some(else_binding_name) = else_binding {
                            if let Type::Datalit(datalit::tycheck::Type::Result(_)) = condition_ty {
                                let error_ty = Type::Datalit(datalit::tycheck::Type::Error);
                                ctx.variables.insert(else_binding_name, (error_ty, false));
                            } else {
                                let actual_type = type_to_string(db, &condition_ty);
                                let err = ctx.error_type_mismatch(
                                    condition,
                                    "Result",
                                    &actual_type,
                                    "else binding requires Result type"
                                );
                                ctx.add_error(err);
                            }
                        }

                        for stmt in else_stmts {
                            check_statement(ctx, stmt);
                        }
                    });
                }
            } else {
                // No binding: check condition is bool type.
                let bool_type = Type::Datalit(datalit::tycheck::Type::Bool,
                );

                if let Err(e) = check_expr(ctx, condition, &bool_type) {
                    ctx.add_error(e);
                }

                ctx.with_scope(|ctx| {
                    for stmt in then_body {
                        check_statement(ctx, stmt);
                    }
                });

                if let Some(else_stmts) = else_body {
                    ctx.with_scope(|ctx| {
                        for stmt in else_stmts {
                            check_statement(ctx, stmt);
                        }
                    });
                }
            }
        }

        Statement::Loop(stmt) => {
            let body = &stmt.body;

            // Increment loop depth.
            ctx.loop_depth += 1;

            // Type check while condition (if present).
            if let Some(condition) = stmt.condition {
                let bool_type = Type::Datalit(datalit::tycheck::Type::Bool,
                );
                if let Err(e) = check_expr(ctx, condition, &bool_type) {
                    ctx.add_error(e);
                }
            }

            ctx.with_scope(|ctx| {
                for body_stmt in body {
                    check_statement(ctx, body_stmt);
                }
            });

            // Decrement loop depth.
            ctx.loop_depth -= 1;
        }

        Statement::Break(stmt) => {
            if ctx.loop_depth == 0 {
                let err = ctx.error_break_outside_loop(stmt);
                ctx.add_error(err);
            }
        }

        Statement::Continue(stmt) => {
            if ctx.loop_depth == 0 {
                let err = ctx.error_continue_outside_loop(stmt);
                ctx.add_error(err);
            }
        }

        Statement::DebugLog(stmt) => {
            let value = stmt.value;
            // Accept any type - just synthesize to verify the expression is valid.
            // Set ref_context since debuglog borrows its argument (doesn't consume).
            let old_ref_context = ctx.ref_context;
            ctx.ref_context = true;
            if let Err(e) = ctx.synthesize_expr(value) {
                ctx.add_error(e);
            }
            ctx.ref_context = old_ref_context;
        }

        Statement::TypeAlias(_) => {
            // Type aliases are handled in pass 0 (collect_type_aliases).
            // Nothing to do here.
        }

        Statement::NativeFun(stmt) => {
            // A native's signature is collected in name resolution, and it has
            // no body to check. What it does need is something to implement it,
            // and the only thing that does is a package's rider interface. This
            // walk runs over scripts and modules and not over rider interfaces,
            // so a native reaching here has no implementation waiting for it
            // and every call to it would go unresolved.
            ctx.report_native_fun_outside_rider(stmt.local_index, stmt.name);
        }

        Statement::Const(stmt) => {
            // Const bindings are typechecked like let bindings, except that the
            // value may only name other consts. The const evaluation happens
            // during lowering.
            let saved_in_const_expr = ctx.in_const_expr;
            ctx.in_const_expr = true;
            check_variable_decl(ctx, stmt.name, stmt.value, stmt.type_hint.clone(), false);
            ctx.in_const_expr = saved_in_const_expr;
            // Track this as a const binding for comptime arg validation.
            ctx.add_const_binding(stmt.name);
        }

        Statement::Match(stmt) => {
            // Synthesize input type - must be Enum.
            let input_ty = match ctx.synthesize_expr(stmt.input) {
                Ok(ty) => ty,
                Err(e) => {
                    ctx.add_error(e);
                    return;
                }
            };

            let enum_ty = match &input_ty {
                Type::Datalit(datalit::tycheck::Type::Enum(e)) => e.clone(),
                _ => {
                    let actual_str = type_to_string(db, &input_ty);
                    let err = ctx.error_type_mismatch(
                        stmt.input,
                        "enum",
                        &actual_str,
                        "match input must be an enum type"
                    );
                    ctx.add_error(err);
                    return;
                }
            };

            // Track which variant names are covered.
            let mut covered: Vec<bct::text::InternedText<'db>> = Vec::new();

            for case in &stmt.cases {
                let variant_name = match &case.kind {
                    MatchCaseKind::Atom { name } => *name,
                    MatchCaseKind::Term { name, .. } => *name,
                };

                // Check for duplicate case.
                if covered.iter().any(|n| *n == variant_name) {
                    let err = ctx.error_type_mismatch(
                        stmt.input,
                        "unique case",
                        &format!("duplicate case '{}'", variant_name.as_str(db)),
                        "duplicate match case"
                    );
                    ctx.add_error(err);
                    continue;
                }

                // Find variant in enum type.
                let variant = enum_ty.variants.iter().find(|v| v.name == variant_name);
                match (&case.kind, variant) {
                    (MatchCaseKind::Atom { name }, Some(v)) => {
                        if v.payload.is_some() {
                            let err = ctx.error_type_mismatch(
                                stmt.input,
                                "atom case",
                                &format!("variant '{}' has a payload", name.as_str(db)),
                                "use 'case term' to destructure payload"
                            );
                            ctx.add_error(err);
                        }
                        covered.push(variant_name);
                    }
                    (MatchCaseKind::Term { name, binding }, Some(v)) => {
                        match &v.payload {
                            Some(payload_ty) => {
                                let binding_type = Type::Datalit(*payload_ty.clone());
                                ctx.with_scope(|ctx| {
                                    ctx.add_variable(*binding, binding_type, false);
                                    for body_stmt in &case.body {
                                        check_statement(ctx, body_stmt);
                                    }
                                });
                                covered.push(variant_name);
                                continue; // Skip the body check below.
                            }
                            None => {
                                let err = ctx.error_type_mismatch(
                                    stmt.input,
                                    "term case",
                                    &format!("variant '{}' has no payload", name.as_str(db)),
                                    "use 'case atom' for payloadless variants"
                                );
                                ctx.add_error(err);
                                covered.push(variant_name);
                            }
                        }
                    }
                    (_, None) => {
                        let err = ctx.error_type_mismatch(
                            stmt.input,
                            "valid variant",
                            &format!("unknown variant '{}'", variant_name.as_str(db)),
                            "variant not found in enum type"
                        );
                        ctx.add_error(err);
                        covered.push(variant_name);
                    }
                }

                ctx.with_scope(|ctx| {
                    for body_stmt in &case.body {
                        check_statement(ctx, body_stmt);
                    }
                });
            }

            if let Some(default_stmts) = &stmt.default_body {
                ctx.with_scope(|ctx| {
                    for body_stmt in default_stmts {
                        check_statement(ctx, body_stmt);
                    }
                });
            }

            // Check exhaustiveness: all variants must be covered or default must exist.
            if stmt.default_body.is_none() {
                let uncovered: Vec<_> = enum_ty.variants.iter()
                    .filter(|v| !covered.iter().any(|c| *c == v.name))
                    .map(|v| v.name.as_str(db).to_string())
                    .collect();
                if !uncovered.is_empty() {
                    let err = ctx.error_type_mismatch(
                        stmt.input,
                        "exhaustive match",
                        &format!("missing variants: {}", uncovered.join(", ")),
                        "non-exhaustive match"
                    );
                    ctx.add_error(err);
                }
            }
        }

        Statement::ExprStatement(stmt) => {
            let expr = stmt.expr;
            let unit_type = Type::Datalit(datalit::tycheck::unit_type());
            match ctx.synthesize_expr(expr) {
                Ok(ty) => {
                    if ty != unit_type {
                        let actual_str = type_to_string(db, &ty);
                        let err = ctx.error_type_mismatch(
                            expr,
                            "unit",
                            &actual_str,
                            "function call statement must return unit"
                        );
                        ctx.add_error(err);
                    }
                }
                Err(e) => {
                    ctx.add_error(e);
                }
            }
        }

        Statement::ParseError(_) => {
            // Skip parse errors.
        }
    }
}

/// Typecheck a place in a set statement: root + steps, including the terminal step.
fn typecheck_place_for_set<'db>(
    ctx: &mut TypeContext<'db>,
    stmt: &StmtSet<'db>,
    place: &Place<'db>,
    value: ExprFun<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Start with the root variable type.
    let mut current_ty = ctx.lookup_variable(place.root).ok_or_else(|| {
        TypeError::DatalitError(format!("undefined variable: {}", place.root.text(db)))
    })?;

    let steps = &place.steps;
    for (i, step) in steps.iter().enumerate() {
        let is_last = i == steps.len() - 1;
        match step {
            PlaceStep::Field(field) => {
                current_ty = typecheck_field_step(ctx, &current_ty, field)
                    .map_err(|e| ctx.report_field_error(crate::FieldErrorSite::Set(stmt.local_index), e))?;
            }
            PlaceStep::Index(idx) => {
                if is_last {
                    // Terminal index — handle key/value checking.
                    typecheck_set_index(ctx, stmt, &current_ty, idx, value);
                    return Ok(());
                }
                // Intermediate index — must have error mode.
                if idx.error_mode.is_none() {
                    return Err(TypeError::DatalitError(
                        "bare index (upsert) cannot appear in intermediate set target position".to_string(),
                    ));
                }
                current_ty = typecheck_index_step(ctx, &current_ty, idx)?;
            }
        }
    }

    // No index steps — simple assignment or field projection.
    if let Err(e) = check_expr(ctx, value, &current_ty) {
        ctx.add_error(e);
    }
    Ok(())
}

/// Typecheck a field step, returning the field's type.
fn typecheck_field_step<'db>(
    ctx: &mut TypeContext<'db>,
    base_ty: &Type<'db>,
    field: &FieldSelector<'db>,
) -> Result<Type<'db>, TypeError> {
    crate::synthesize::resolve_field_type(ctx.db, base_ty, field)
}

/// Typecheck an intermediate index step, returning the element/value type.
fn typecheck_index_step<'db>(
    ctx: &mut TypeContext<'db>,
    base_ty: &Type<'db>,
    idx: &PlaceIndex<'db>,
) -> Result<Type<'db>, TypeError> {
    let (element_ty, index_type) = crate::synthesize::resolve_index_types(ctx.db, base_ty)
        .map_err(TypeError::DatalitError)?;

    if let Err(e) = check_expr(ctx, idx.index, &index_type) {
        ctx.add_error(e);
    }

    Ok(element_ty)
}

/// Typecheck the terminal index step of a set target.
fn typecheck_set_index<'db>(
    ctx: &mut TypeContext<'db>,
    stmt: &StmtSet<'db>,
    base_ty: &Type<'db>,
    idx_step: &PlaceIndex<'db>,
    value: ExprFun<'db>,
) {
    let db = ctx.db;

    match idx_step.error_mode {
        None => {
            // Bare index (upsert): only valid for maps.
            match base_ty {
                Type::Datalit(datalit::tycheck::Type::Map(map)) => {
                    let key_type = Type::Datalit(*map.key_type.clone());
                    let value_type = Type::Datalit(*map.value_type.clone());
                    if let Err(e) = check_expr(ctx, idx_step.index, &key_type) {
                        ctx.add_error(e);
                    }
                    if let Err(e) = check_expr(ctx, value, &value_type) {
                        ctx.add_error(e);
                    }
                }
                _ => {
                    ctx.add_error(TypeError::DatalitError(
                        format!(
                            "bare index in set requires map type, got {}; use '?' or '!' for list indexing",
                            type_to_string(db, base_ty)
                        ),
                    ));
                }
            }
        }
        Some(error_mode) => {
            let (element_ty, index_type) = match crate::synthesize::resolve_index_types(db, base_ty) {
                Ok(types) => types,
                Err(msg) => {
                    ctx.add_error(TypeError::DatalitError(msg));
                    return;
                }
            };
            // Reject set on view-producing index (e.g. `set t[i]? = new_row`
            // on rank > 1 tensor would replace the view, not the parent data).
            if crate::synthesize::is_view_producing_index(base_ty) {
                ctx.add_error(TypeError::ViewTypeMutBinding {
                    view_ty: type_to_string(db, &element_ty),
                });
                return;
            }
            if let Err(e) = check_expr(ctx, idx_step.index, &index_type) {
                ctx.add_error(e);
            }
            if let Err(e) = check_expr(ctx, value, &element_ty) {
                ctx.add_error(e);
            }
            // Verify function returns Option or Result depending on error_mode.
            match error_mode {
                IndexErrorMode::Option => {
                    if let Err(e) = require_option_return_type_for_set(ctx, stmt) {
                        ctx.add_error(e);
                    }
                }
                IndexErrorMode::Result => {
                    if let Err(e) = require_result_return_type_for_set(ctx, stmt) {
                        ctx.add_error(e);
                    }
                }
            }
        }
    }
}

/// Require function returns Option type for `set a[i]? = v`.
fn require_option_return_type_for_set<'db>(
    ctx: &mut TypeContext<'db>,
    _stmt: &StmtSet<'db>,
) -> Result<(), TypeError> {
    let expected_return = ctx.expected_return_type.clone()
        .expect("set with ? used outside function context");
    match expected_return {
        Type::Datalit(datalit::tycheck::Type::Option(_)) => Ok(()),
        _ => Err(TypeError::DatalitError(
            format!(
                "set with `?` index requires function to return Option type, got {}",
                type_to_string(ctx.db, &expected_return)
            ),
        )),
    }
}

/// Require function returns Result type for `set a[i]! = v`.
fn require_result_return_type_for_set<'db>(
    ctx: &mut TypeContext<'db>,
    _stmt: &StmtSet<'db>,
) -> Result<(), TypeError> {
    let expected_return = ctx.expected_return_type.clone()
        .expect("set with ! used outside function context");
    match expected_return {
        Type::Datalit(datalit::tycheck::Type::Result(_)) => Ok(()),
        _ => Err(TypeError::DatalitError(
            format!(
                "set with `!` index requires function to return Result type, got {}",
                type_to_string(ctx.db, &expected_return)
            ),
        )),
    }
}
