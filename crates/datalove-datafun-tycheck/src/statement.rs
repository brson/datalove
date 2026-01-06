//! Statement type checking.
//!
//! Provides functions for type checking statements including function definitions.

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::check::check_expr;
use crate::types::{convert_type_hint, type_to_string, unit_type};
use crate::ModuleId;

pub use crate::{Type, TypeAndHeap, TypeFunction, TypeError};

/// Collect function signature without checking body (first pass).
pub fn collect_function_signature<'db>(
    ctx: &mut TypeContext<'db>,
    stmt: &StmtFun<'db>,
    module_id: Option<ModuleId>,
) {
    let db = ctx.db;
    let name = stmt.name(db);
    let params = stmt.params(db);
    let return_type = stmt.return_type(db);

    // Convert parameter types and collect modes.
    let mut param_types = Vec::new();
    let mut param_modes = Vec::new();
    for param in params {
        match convert_type_hint(db, param.type_hint(db)) {
            Ok(ty) => {
                param_types.push(ty);
                param_modes.push(param.mode(db));
            }
            Err(e) => {
                ctx.add_error(e);
                return;
            }
        }
    }

    // Convert return type (default to Void if not specified).
    let ret_ty = match return_type {
        Some(type_hint) => {
            match convert_type_hint(db, type_hint) {
                Ok(ty) => ty,
                Err(e) => {
                    ctx.add_error(e);
                    return;
                }
            }
        }
        None => {
            // Functions without explicit return type return unit `()`.
            unit_type(db)
        }
    };

    // Create function type and add to context with AST.
    let func_type = TypeFunction::new(db, param_types, param_modes, ret_ty);
    ctx.add_function_with_ast(name, func_type, *stmt, module_id);
}

/// Check a statement.
pub fn check_statement<'db>(
    ctx: &mut TypeContext<'db>,
    statement: &Statement<'db>,
) {
    let db = ctx.db;

    match statement {
        Statement::Let(stmt) => {
            let name = stmt.name(db);
            let value = stmt.value(db);

            // If type hint is provided, check against it.
            // Otherwise, synthesize type from value.
            let var_type = match stmt.type_hint(db) {
                Some(type_hint) => {
                    // Convert type hint to expected type and check value.
                    match convert_type_hint(db, type_hint) {
                        Ok(expected_type) => {
                            match check_expr(ctx, value, expected_type) {
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
                    // Synthesize type from value.
                    match ctx.synthesize_expr(value) {
                        Ok(ty) => Some(ty),
                        Err(e) => {
                            ctx.add_error(e);
                            None
                        }
                    }
                }
            };

            // Add variable to context if we got a type.
            if let Some(ty) = var_type {
                ctx.add_variable(name, ty);
            }
        }

        Statement::Var(stmt) => {
            let name = stmt.name(db);
            let value = stmt.value(db);

            // Same as let: if type hint provided, check against it.
            let var_type = match stmt.type_hint(db) {
                Some(type_hint) => {
                    match convert_type_hint(db, type_hint) {
                        Ok(expected_type) => {
                            match check_expr(ctx, value, expected_type) {
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

            // Add mutable variable to context if we got a type.
            if let Some(ty) = var_type {
                ctx.add_variable(name, ty);
            }
        }

        Statement::Set(stmt) => {
            let name = stmt.name(db);
            let value = stmt.value(db);

            // Look up the variable type.
            match ctx.lookup_variable(name) {
                Some(expected_type) => {
                    // Check that value matches the variable's type.
                    if let Err(e) = check_expr(ctx, value, expected_type) {
                        ctx.add_error(e);
                    }
                }
                None => {
                    // Variable not found.
                    ctx.add_error(TypeError::DatalitError(
                        format!("undefined variable: {}", name.text(db))
                    ));
                }
            }
        }

        Statement::Fun(stmt) => {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let body = stmt.body(db);

            // Function signature should already be collected in first pass.
            // Look it up to get param types and return type.
            let func_type = match ctx.lookup_function(name) {
                Some(func_type) => func_type,
                None => {
                    // Function not in context (shouldn't happen in normal flow).
                    // Collect signature now for error resilience.
                    collect_function_signature(ctx, stmt, None);
                    match ctx.lookup_function(name) {
                        Some(func_type) => func_type,
                        None => return, // Errors already recorded.
                    }
                }
            };

            let param_types = func_type.param_types(db);
            let ret_ty = func_type.return_type(db);

            // Create new context for function body with parameters in scope.
            let saved_variables = ctx.variables.clone();
            let saved_return_type = ctx.expected_return_type;
            let saved_is_void = ctx.is_void_function;

            // Add parameters to context.
            for (param, param_ty) in params.iter().zip(param_types.iter()) {
                ctx.add_variable(param.name(db), *param_ty);
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
            let ret_value = stmt.value(db);
            let expected_ty = ctx.expected_return_type;

            match (ret_value, expected_ty) {
                (Some(value), Some(expected_ret_ty)) => {
                    // Has value - check if void (no declared return type).
                    if ctx.is_void_function {
                        // Void function with value - ERROR.
                        ctx.add_error(TypeError::DatalitError(
                            "void function cannot return a value".to_string()
                        ));
                    } else {
                        // Non-void function with value - check type.
                        if let Err(e) = check_expr(ctx, value, expected_ret_ty) {
                            ctx.add_error(e);
                        }
                    }
                }
                (None, Some(_expected_ret_ty)) => {
                    // Bare ret - check if void (no declared return type).
                    if !ctx.is_void_function {
                        // Non-void function with bare ret - ERROR.
                        ctx.add_error(TypeError::DatalitError(
                            "function requires return value".to_string()
                        ));
                    }
                    // Void function with bare ret - OK.
                }
                (Some(value), None) => {
                    // F011: Cannot synthesize return type.
                    ctx.add_error(ctx.error_cannot_synthesize(value, "cannot infer return type"));
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
            let condition = stmt.condition(db);
            let then_binding = stmt.then_binding(db);
            let then_body = stmt.then_body(db);
            let else_binding = stmt.else_binding(db);
            let else_body = stmt.else_body(db);

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
                let inner_ty = match condition_ty.ty(db) {
                    Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                        opt.inner_type(db)
                    }
                    Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                        // F046: Result destructuring requires error-binding else branch.
                        if else_body.is_none() || else_binding.is_none() {
                            ctx.add_error(ctx.error_result_requires_binding(condition));
                            return;
                        }
                        res.inner_type(db)
                    }
                    _ => {
                        // F017: If/match condition type mismatch.
                        ctx.add_error(ctx.error_type_mismatch(
                            condition,
                            "Option or Result",
                            "other type",
                            "expected Option or Result type for destructuring"
                        ));
                        return;
                    }
                };

                // Convert datalit TypeAndHeap to datafun TypeAndHeap.
                let inner_heap = inner_ty.heap(db);
                let inner_type = Type::Datalit(inner_ty.ty(db).clone());
                let binding_ty = TypeAndHeap::new(db, inner_heap, inner_type);

                // Add binding to context for then body (save old if shadowing).
                let old_binding = ctx.variables.insert(binding_name, binding_ty);

                // Type check then body.
                for stmt in then_body {
                    check_statement(ctx, stmt);
                }

                // Restore old binding or remove.
                if let Some(old) = old_binding {
                    ctx.variables.insert(binding_name, old);
                } else {
                    ctx.variables.remove(&binding_name);
                }

                // Type check else body if present.
                if let Some(else_stmts) = else_body {
                    // If there's an else binding, bind error type for Result.
                    if let Some(else_binding_name) = else_binding {
                        if let Type::Datalit(datalit::tycheck::Type::Result(_)) = condition_ty.ty(db) {
                            // Bind Error type (save old if shadowing).
                            let error_ty = TypeAndHeap::new(
                                db,
                                datalit::ast::Heap::Omitted,
                                Type::Datalit(datalit::tycheck::Type::Error),
                            );
                            let old_else_binding = ctx.variables.insert(else_binding_name, error_ty);

                            for stmt in else_stmts {
                                check_statement(ctx, stmt);
                            }

                            // Restore old binding or remove.
                            if let Some(old) = old_else_binding {
                                ctx.variables.insert(else_binding_name, old);
                            } else {
                                ctx.variables.remove(&else_binding_name);
                            }
                        } else {
                            let actual_type = type_to_string(db, condition_ty.ty(db));
                            ctx.add_error(ctx.error_type_mismatch(
                                condition,
                                "Result",
                                &actual_type,
                                "else binding requires Result type"
                            ));
                        }
                    } else {
                        for stmt in else_stmts {
                            check_statement(ctx, stmt);
                        }
                    }
                }
            } else {
                // No binding: check condition is bool type.
                let bool_type = TypeAndHeap::new(
                    db,
                    datalit::ast::Heap::Omitted,
                    Type::Datalit(datalit::tycheck::Type::Bool),
                );

                if let Err(e) = check_expr(ctx, condition, bool_type) {
                    ctx.add_error(e);
                }

                // Type check then body.
                for stmt in then_body {
                    check_statement(ctx, stmt);
                }

                // Type check else body if present.
                if let Some(else_stmts) = else_body {
                    for stmt in else_stmts {
                        check_statement(ctx, stmt);
                    }
                }
            }
        }

        Statement::Loop(stmt) => {
            let carries = stmt.carries(db);
            let brings = stmt.brings(db);
            let body = stmt.body(db);

            // Save variables that might be shadowed.
            let saved_variables = ctx.variables.clone();

            // Type check carry init expressions and bind carry names.
            let mut carry_types = Vec::new();
            for carry in carries {
                let carry_name = carry.name(db);
                let init = carry.init(db);
                let carry_type_hint = carry.type_hint(db);

                let var_type = match carry_type_hint {
                    Some(type_hint) => {
                        match convert_type_hint(db, type_hint) {
                            Ok(expected_type) => {
                                match check_expr(ctx, init, expected_type) {
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
                        match ctx.synthesize_expr(init) {
                            Ok(ty) => Some(ty),
                            Err(e) => {
                                ctx.add_error(e);
                                None
                            }
                        }
                    }
                };

                if let Some(ty) = var_type {
                    carry_types.push(ty);
                    ctx.add_variable(carry_name, ty);
                }
            }

            // Process bring bindings to get expected types for break.
            let mut bring_types = Vec::new();
            for bring in brings {
                let bring_type_hint = bring.type_hint(db);

                let bring_ty = match bring_type_hint {
                    Some(type_hint) => {
                        match convert_type_hint(db, type_hint) {
                            Ok(ty) => Some(ty),
                            Err(e) => {
                                ctx.add_error(e);
                                None
                            }
                        }
                    }
                    None => {
                        // No type hint for bring - will be inferred from break values.
                        // For now, we can't infer without seeing break values first.
                        // Type checking for brings without type hints would need multiple passes.
                        // For Phase 2, require type hints on bring bindings.
                        ctx.add_error(TypeError::DatalitError(
                            "bring binding requires type hint".to_string()
                        ));
                        None
                    }
                };

                if let Some(ty) = bring_ty {
                    bring_types.push(ty);
                }
            }

            // Push loop context for break/continue validation.
            ctx.loop_contexts.push(crate::context::LoopContext {
                carry_types,
                bring_types: bring_types.clone(),
            });

            // Type check while condition (if present).
            // Condition is checked after carries are bound so it can use carry variables.
            if let Some(condition) = stmt.condition(db) {
                let bool_type = TypeAndHeap::new(
                    db,
                    datalit::ast::Heap::Omitted,
                    Type::Datalit(datalit::tycheck::Type::Bool),
                );
                if let Err(e) = check_expr(ctx, condition, bool_type) {
                    ctx.add_error(e);
                }
            }

            // Type check loop body.
            for body_stmt in body {
                check_statement(ctx, body_stmt);
            }

            // For loops with carries, the body must not fall through.
            // Every path must explicitly break, continue, or return.
            // (Loops with only brings can fallthrough - it just means "keep looping".)
            if !carries.is_empty() && !must_diverge(db, body) {
                ctx.add_error(TypeError::LoopBodyFallthrough);
            }

            // Pop loop context.
            ctx.loop_contexts.pop();

            // Bind bring names in outer scope after the loop.
            for (bring, bring_ty) in brings.iter().zip(bring_types.iter()) {
                ctx.add_variable(bring.name(db), *bring_ty);
            }

            // Restore shadowed variables (but keep bring bindings).
            for (name, ty) in saved_variables {
                // Don't restore if it's a bring binding.
                let is_bring = brings.iter().any(|b| b.name(db) == name);
                if !is_bring {
                    ctx.variables.insert(name, ty);
                }
            }
        }

        Statement::Break(stmt) => {
            let values = stmt.values(db);

            // Clone bring_types to avoid borrow conflicts.
            let bring_types = ctx.loop_contexts.last().map(|lc| lc.bring_types.clone());

            match bring_types {
                None => {
                    ctx.add_error(TypeError::BreakOutsideLoop);
                }
                Some(bring_types) => {
                    let expected = bring_types.len();
                    let actual = values.len();

                    if expected != actual {
                        ctx.add_error(TypeError::BreakArityMismatch { expected, actual });
                    } else {
                        // Type check each break value against the corresponding bring type.
                        for (value, expected_ty) in values.iter().zip(bring_types.iter()) {
                            if let Err(e) = check_expr(ctx, *value, *expected_ty) {
                                ctx.add_error(e);
                            }
                        }
                    }
                }
            }
        }

        Statement::Continue(stmt) => {
            let values = stmt.values(db);

            // Clone carry_types to avoid borrow conflicts.
            let carry_types = ctx.loop_contexts.last().map(|lc| lc.carry_types.clone());

            match carry_types {
                None => {
                    ctx.add_error(TypeError::ContinueOutsideLoop);
                }
                Some(carry_types) => {
                    let expected = carry_types.len();
                    let actual = values.len();

                    if expected != actual {
                        ctx.add_error(TypeError::ContinueArityMismatch { expected, actual });
                    } else {
                        // Type check each continue value against the corresponding carry type.
                        for (value, expected_ty) in values.iter().zip(carry_types.iter()) {
                            if let Err(e) = check_expr(ctx, *value, *expected_ty) {
                                ctx.add_error(e);
                            }
                        }
                    }
                }
            }
        }

        Statement::DebugLog(stmt) => {
            let value = stmt.value(db);
            // Accept any type - just synthesize to verify the expression is valid.
            if let Err(e) = ctx.synthesize_expr(value) {
                ctx.add_error(e);
            }
        }

        Statement::ParseError(_) => {
            // Skip parse errors.
        }
    }
}

/// Check if a statement list must diverge (all paths end in break/continue/return).
///
/// Used to validate that loops with carry/bring don't have fall-through paths.
fn must_diverge<'db>(db: &'db dyn salsa::Database, stmts: &[Statement<'db>]) -> bool {
    for stmt in stmts {
        if stmt_must_diverge(db, stmt) {
            return true;
        }
    }
    false
}

/// Check if a single statement must diverge.
fn stmt_must_diverge<'db>(db: &'db dyn salsa::Database, stmt: &Statement<'db>) -> bool {
    match stmt {
        Statement::Break(_) | Statement::Continue(_) | Statement::Ret(_) => true,

        Statement::If(if_stmt) => {
            // Both branches must diverge for the if to diverge.
            let then_body = if_stmt.then_body(db);
            let else_body = if_stmt.else_body(db);

            if let Some(else_stmts) = else_body {
                must_diverge(db, then_body) && must_diverge(db, else_stmts)
            } else {
                // No else branch means the "fall through" path doesn't diverge.
                false
            }
        }

        // Loops don't count as diverging for this analysis.
        // A loop might break or might loop forever, but either way
        // we can't say it "must diverge" from the caller's perspective.
        Statement::Loop(_) => false,

        // Other statements don't diverge.
        _ => false,
    }
}
