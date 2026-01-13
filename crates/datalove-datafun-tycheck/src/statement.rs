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
    type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
) {
    let db = ctx.db;

    let var_type = match type_hint {
        Some(hint) => {
            match convert_type_hint(db, hint) {
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

    if let Some(ty) = var_type {
        ctx.add_variable(name, ty);
    }
}

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
        match convert_type_hint(db, param.type_hint) {
            Ok(ty) => {
                param_types.push(ty);
                param_modes.push(param.mode);
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
            check_variable_decl(ctx, stmt.name, stmt.value, stmt.type_hint);
        }

        Statement::Var(stmt) => {
            check_variable_decl(ctx, stmt.name, stmt.value, stmt.type_hint);
        }

        Statement::Set(stmt) => {
            let value = stmt.value;

            // Handle set target.
            match &stmt.target {
                SetTarget::Name(name) => {
                    // Look up the variable type.
                    match ctx.lookup_variable(*name) {
                        Some(expected_type) => {
                            // Check that value matches the variable's type.
                            if let Err(e) = check_expr(ctx, value, expected_type) {
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
                    // Typecheck projection target.
                    match typecheck_set_target_proj(ctx, proj) {
                        Ok(expected_type) => {
                            // Check that value matches the field's type.
                            if let Err(e) = check_expr(ctx, value, expected_type) {
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
                ctx.add_variable(param.name, *param_ty);
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
            let expected_ty = ctx.expected_return_type;

            match (ret_value, expected_ty) {
                (Some(value), Some(expected_ret_ty)) => {
                    // Has value - check if void (no declared return type).
                    if ctx.is_void_function {
                        // Void function with value - ERROR.
                        let err = ctx.error_void_function_returns_value(stmt);
                        ctx.add_error(err);
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
                let inner_ty = match condition_ty.ty(db) {
                    Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                        opt.inner_type
                    }
                    Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                        // F046: Result destructuring requires error-binding else branch.
                        if else_body.is_none() || else_binding.is_none() {
                            let err = ctx.error_result_requires_binding(condition);
                            ctx.add_error(err);
                            return;
                        }
                        res.inner_type
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
                            let err = ctx.error_type_mismatch(
                                condition,
                                "Result",
                                &actual_type,
                                "else binding requires Result type"
                            );
                            ctx.add_error(err);
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
            let body = &stmt.body;

            // Increment loop depth.
            ctx.loop_depth += 1;

            // Type check while condition (if present).
            if let Some(condition) = stmt.condition {
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
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;

    // First, resolve the base to get the starting type.
    let base_ty = typecheck_set_target(ctx, &proj.base)?;

    // Base must be a datalit type (tuple or struct).
    let base_datalit_ty = match base_ty.ty(db) {
        Type::Datalit(dt) => dt,
        _ => {
            return Err(TypeError::ProjectionOnNonAggregate {
                ty: type_to_string(db, base_ty.ty(db)),
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
                    let heap = field_ty.heap(db);
                    let ty = Type::Datalit(field_ty.ty(db).clone());
                    Ok(TypeAndHeap::new(db, heap, ty))
                }
                _ => {
                    Err(TypeError::ProjectionOnNonAggregate {
                        ty: type_to_string(db, base_ty.ty(db)),
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
                            let heap = field.ty.heap(db);
                            let ty = Type::Datalit(field.ty.ty(db).clone());
                            return Ok(TypeAndHeap::new(db, heap, ty));
                        }
                    }
                    Err(TypeError::FieldNotFound {
                        field_name: name_str.to_string(),
                        ty: type_to_string(db, base_ty.ty(db)),
                    })
                }
                _ => {
                    Err(TypeError::ProjectionOnNonAggregate {
                        ty: type_to_string(db, base_ty.ty(db)),
                    })
                }
            }
        }
    }
}

/// Typecheck a set target, returning its type.
fn typecheck_set_target<'db>(
    ctx: &mut TypeContext<'db>,
    target: &SetTarget<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
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
