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
            match convert_type_hint_with_aliases(db, hint, &ctx.type_aliases) {
                Ok(expected_type) => {
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
                        match convert_type_hint_with_aliases(db, hint, &ctx.type_aliases) {
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

            // Handle set target.
            match &stmt.target {
                SetTarget::Name(name) => {
                    // Look up the variable type.
                    match ctx.lookup_variable(*name) {
                        Some(expected_type) => {
                            // Check mutability - reject assignment to immutable variables.
                            if let Some(false) = ctx.lookup_variable_mutability(*name) {
                                let err = ctx.error_variable_not_mutable(stmt, name.text(db).as_str());
                                ctx.add_error(err);
                            }
                            // Check that value matches the variable's type.
                            if let Err(e) = check_expr(ctx, value, &expected_type) {
                                ctx.add_error(e);
                            }
                        }
                        None => {
                            // Variable not found.
                            let err = ctx.error_undefined_variable_set(stmt, name.text(db).as_str());
                            ctx.add_error(err);
                        }
                    }
                }
                SetTarget::Proj(proj) => {
                    // Check that the root variable is mutable.
                    let root_name = get_proj_root_name(proj);
                    if let Some(false) = ctx.lookup_variable_mutability(root_name) {
                        let err = ctx.error_variable_not_mutable(stmt, root_name.text(db).as_str());
                        ctx.add_error(err);
                    }
                    // Typecheck projection target.
                    match typecheck_set_target_proj(ctx, proj) {
                        Ok(expected_type) => {
                            // Check that value matches the field's type.
                            if let Err(e) = check_expr(ctx, value, &expected_type) {
                                ctx.add_error(e);
                            }
                        }
                        Err(e) => ctx.add_error(e),
                    }
                }
            }
        }

        Statement::Fun(stmt) => {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let body = stmt.body(db);

            // Function signature should already be collected by resolve crate.
            let func_type = ctx.lookup_function(name)
                .expect("function should be in context from name resolution");

            let param_types = func_type.param_types(db);
            let ret_ty = func_type.return_type(db);

            // Create new context for function body with parameters in scope.
            let saved_variables = ctx.variables.C();
            let saved_return_type = ctx.expected_return_type.clone();
            let saved_is_void = ctx.is_void_function;

            // Add parameters to context.
            // Mut and Out params are mutable and can be assigned with `set`.
            for (param, param_ty) in params.iter().zip(param_types.iter()) {
                let is_mutable = matches!(param.mode, datalove_datafun_ast::ast::ParamMode::Mut | datalove_datafun_ast::ast::ParamMode::Out);
                ctx.add_variable(param.name, param_ty.clone(), is_mutable);
            }

            ctx.expected_return_type = Some(ret_ty);
            ctx.is_void_function = stmt.return_type(db).is_none();

            // Check function body.
            for stmt in body {
                check_statement(ctx, stmt);
            }

            // Restore context.
            ctx.variables = saved_variables;
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

                // Save all variables before entering then branch.
                let saved_variables = ctx.variables.C();

                // Add binding to context for then body (immutable binding from destructuring).
                ctx.variables.insert(binding_name, (binding_ty, false));

                // Type check then body.
                for stmt in then_body {
                    check_statement(ctx, stmt);
                }

                // Restore all variables after then branch.
                ctx.variables = saved_variables;

                // Type check else body if present.
                if let Some(else_stmts) = else_body {
                    // Save all variables before entering else branch.
                    let saved_variables = ctx.variables.C();

                    // If there's an else binding, bind error type for Result.
                    if let Some(else_binding_name) = else_binding {
                        if let Type::Datalit(datalit::tycheck::Type::Result(_)) = condition_ty {
                            // Bind Error type (immutable binding from destructuring).
                            let error_ty = Type::Datalit(datalit::tycheck::Type::Error,
                            );
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

                    // Restore all variables after else branch.
                    ctx.variables = saved_variables;
                }
            } else {
                // No binding: check condition is bool type.
                let bool_type = Type::Datalit(datalit::tycheck::Type::Bool,
                );

                if let Err(e) = check_expr(ctx, condition, &bool_type) {
                    ctx.add_error(e);
                }

                // Save variables before entering then branch.
                let saved_variables = ctx.variables.C();

                // Type check then body.
                for stmt in then_body {
                    check_statement(ctx, stmt);
                }

                // Restore variables after then branch.
                ctx.variables = saved_variables;

                // Type check else body if present.
                if let Some(else_stmts) = else_body {
                    // Save variables before entering else branch.
                    let saved_variables = ctx.variables.C();

                    for stmt in else_stmts {
                        check_statement(ctx, stmt);
                    }

                    // Restore variables after else branch.
                    ctx.variables = saved_variables;
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

            // Save variables before entering loop body.
            let saved_variables = ctx.variables.C();

            // Type check loop body.
            for body_stmt in body {
                check_statement(ctx, body_stmt);
            }

            // Restore variables after loop body.
            ctx.variables = saved_variables;

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

        Statement::Const(stmt) => {
            // Const bindings are typechecked like let bindings.
            // The const evaluation happens during lowering.
            check_variable_decl(ctx, stmt.name, stmt.value, stmt.type_hint.clone(), false);
        }

        Statement::ParseError(_) => {
            // Skip parse errors.
        }
    }
}

/// Typecheck a projection target in a set statement.
///
/// Returns the expected type of the final field being set.
fn typecheck_set_target_proj<'db>(
    ctx: &mut TypeContext<'db>,
    proj: &SetTargetProj<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // First, resolve the base to get the starting type.
    let base_ty = typecheck_set_target(ctx, &proj.base)?;

    // Base must be a datalit type (tuple or struct).
    let base_datalit_ty = match &base_ty {
        Type::Datalit(dt) => dt,
        _ => {
            return Err(TypeError::ProjectionOnNonAggregate {
                ty: type_to_string(db, &base_ty),
            });
        }
    };

    // Extract field type based on selector.
    match &proj.field {
        FieldSelector::Index(idx) => {
            // Index projection: base must be tuple.
            match base_datalit_ty {
                datalit::tycheck::Type::AnonTuple(tuple) => {
                    let idx_usize = *idx as usize;
                    if idx_usize >= tuple.fields.len() {
                        return Err(TypeError::FieldIndexOutOfBounds {
                            index: *idx,
                            tuple_size: tuple.fields.len(),
                        });
                    }
                    let field_ty = &tuple.fields[idx_usize];
                    let ty = Type::Datalit(field_ty.clone());
                    Ok(ty)
                }
                _ => {
                    Err(TypeError::ProjectionOnNonAggregate {
                        ty: type_to_string(db, &base_ty),
                    })
                }
            }
        }
        FieldSelector::Name(name) => {
            // Named projection: base must be struct.
            match base_datalit_ty {
                datalit::tycheck::Type::AnonStruct(struct_ty) => {
                    let name_str = name.text(db);
                    for field in &struct_ty.fields {
                        if field.name.text(db) == name_str {
                            let ty = Type::Datalit((*field.ty).clone());
                            return Ok(ty);
                        }
                    }
                    Err(TypeError::FieldNotFound {
                        field_name: name_str.S(),
                        ty: type_to_string(db, &base_ty),
                    })
                }
                _ => {
                    Err(TypeError::ProjectionOnNonAggregate {
                        ty: type_to_string(db, &base_ty),
                    })
                }
            }
        }
    }
}

/// Get the root variable name from a projection chain.
fn get_proj_root_name<'db>(proj: &SetTargetProj<'db>) -> bct::text::InternedText<'db> {
    match proj.base.as_ref() {
        SetTarget::Name(name) => *name,
        SetTarget::Proj(inner_proj) => get_proj_root_name(inner_proj),
    }
}

/// Typecheck a set target, returning its type.
fn typecheck_set_target<'db>(
    ctx: &mut TypeContext<'db>,
    target: &SetTarget<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    match target {
        SetTarget::Name(name) => {
            ctx.lookup_variable(*name).ok_or_else(|| {
                TypeError::DatalitError(format!("undefined variable: {}", name.text(db)))
            })
        }
        SetTarget::Proj(proj) => typecheck_set_target_proj(ctx, proj),
    }
}
