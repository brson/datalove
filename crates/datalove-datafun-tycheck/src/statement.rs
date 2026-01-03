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
            // Increment loop depth for body.
            ctx.loop_depth += 1;

            // Type check loop body.
            for body_stmt in stmt.body(db) {
                check_statement(ctx, body_stmt);
            }

            // Restore loop depth.
            ctx.loop_depth -= 1;
        }

        Statement::Break(_) => {
            if ctx.loop_depth == 0 {
                ctx.add_error(TypeError::BreakOutsideLoop);
            }
            // Break is valid - no further type checking needed.
        }

        Statement::Continue(_) => {
            if ctx.loop_depth == 0 {
                ctx.add_error(TypeError::ContinueOutsideLoop);
            }
            // Continue is valid - no further type checking needed.
        }

        Statement::ParseError(_) => {
            // Skip parse errors.
        }
    }
}
