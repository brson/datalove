//! Expression checking (bidirectional typechecking).
//!
//! Provides check_expr for checking expressions against expected types,
//! and helpers for checking collection elements.
//!
//! Organization (parallel to datalit/tycheck/check.rs):
//! 1. Element checking helper - type compatibility for collection elements
//! 2. Main check_expr function - entry point for type checking
//! 3. Collection checking helpers - list, set, map, tensor, tuple, struct, enum

use rmx::prelude::*;
use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::types::*;

pub use crate::{Type, TypeError};

// ============================================================================
// Type Checking
// ============================================================================

/// Check an expression against an expected type.
pub fn check_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    expected: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    match expr_kind {
        // Handle None literals specially - they can check against any Option type.
        ExprFunKind::None(_lit) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // None checks against any Option<T>.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "None", "None requires Option type"))
                }
            }
        }

        // Handle Some expressions - check against Option type.
        ExprFunKind::Some(some_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                    // Check payload against inner type.
                    let expected_inner = Type::Datalit(*opt.inner_type.clone());
                    check_expr(ctx, some_expr.payload, &expected_inner)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "some", "some requires Option type"))
                }
            }
        }

        // Handle Ok expressions - check against Result type.
        ExprFunKind::Ok(ok_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                    // Check payload against inner type.
                    let expected_inner = Type::Datalit(*res.inner_type.clone());
                    check_expr(ctx, ok_expr.payload, &expected_inner)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "ok", "ok requires Result type"))
                }
            }
        }

        // Handle Er expressions - check against Result type.
        ExprFunKind::Er(er_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                    // Check payload against Error type.
                    let error_ty = Type::Datalit(datalit::tycheck::Type::Error
                    );
                    check_expr(ctx, er_expr.payload, &error_ty)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected);
                    Err(ctx.error_type_mismatch(expr, &expected_str, "er", "er requires Result type"))
                }
            }
        }


        // Handle table expressions - check rows against expected column types.
        ExprFunKind::Table(table_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Table(table_ty)) => {
                    // Check rows against expected column types.
                    check_table_rows(ctx, &table_expr.header, &table_expr.rows, table_ty)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle list expressions - check elements against expected element type.
        ExprFunKind::List(list_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::List(_)) => {
                    // Check elements against expected element type (with coercion).
                    check_list_elements(ctx, &list_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle set expressions - check elements against expected element type.
        ExprFunKind::Set(set_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Set(_)) => {
                    // Check elements against expected element type (with coercion).
                    check_set_elements(ctx, &set_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle map expressions - check entries against expected key/value types.
        ExprFunKind::Map(map_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Map(_)) => {
                    // Check entries against expected key/value types (with coercion).
                    check_map_entries(ctx, &map_expr.entries, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tensor expressions - check elements against expected element type.
        ExprFunKind::Tensor(tensor_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::Tensor(_)) => {
                    // Check shape and elements against expected type.
                    check_tensor_shape_and_elements(ctx, tensor_expr.C(), &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tuple expressions (datalit) - check elements against expected field types.
        ExprFunKind::AnonTuple(tuple_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::AnonTuple(_)) => {
                    // Check elements against expected field types.
                    check_tuple_elements(ctx, &tuple_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tuple expressions (datafun) - check elements against expected field types.
        ExprFunKind::Tuple(tuple_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::AnonTuple(_)) => {
                    // Check elements against expected field types.
                    check_tuple_elements(ctx, &tuple_expr.elements, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle struct expressions - check fields against expected field types.
        ExprFunKind::AnonStruct(struct_expr) => {
            match expected {
                Type::Datalit(datalit::tycheck::Type::AnonStruct(_)) => {
                    // Check fields against expected field types.
                    check_struct_fields(ctx, &struct_expr.fields, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle integer literals specially - they can coerce to expected integer types.
        ExprFunKind::Int(int_expr) => {
            match expected {
                Type::Datalit(expected_datalit_ty) => {
                    // Check if expected type is a numeric type.
                    if is_numeric_type(expected) {
                        // Validate the literal value fits in the expected type.
                        let value_str = int_expr.value.as_str(db);
                        check_int_fits_wrapped_type(value_str, expected_datalit_ty, db)?;
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // If expected is Data, allow coercion (any type coerces to data).
                    if let datalit::tycheck::Type::Data = expected_datalit_ty {
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // Otherwise, synthesize and compare.
                    let synthesized = ctx.synthesize_expr(expr)?;
                    if types_equivalent(db, &synthesized, expected) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = type_to_string(db, &synthesized);
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle float literals specially - they can coerce to expected float types.
        ExprFunKind::Float(_float_expr) => {
            match expected {
                Type::Datalit(expected_datalit_ty) => {
                    // Check if expected type is a float type.
                    if is_float_type(expected) {
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // If expected is Data, allow coercion (any type coerces to data).
                    if let datalit::tycheck::Type::Data = expected_datalit_ty {
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // Otherwise, synthesize and compare.
                    let synthesized = ctx.synthesize_expr(expr)?;
                    if types_equivalent(db, &synthesized, expected) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = type_to_string(db, &synthesized);
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle hex literals specially - they can coerce to expected integer types.
        ExprFunKind::Hex(hex_expr) => {
            match expected {
                Type::Datalit(expected_datalit_ty) => {
                    // Check if expected type is a numeric type.
                    if is_numeric_type(expected) {
                        // Validate the hex value fits in the expected type.
                        let value_str = hex_expr.value.as_str(db);
                        check_hex_fits_wrapped_type(value_str, expected_datalit_ty, db)?;
                        ctx.store_expr_type(expr, &expected);
                        return Ok(());
                    }
                    // Otherwise, synthesize and compare.
                    let synthesized = ctx.synthesize_expr(expr)?;
                    if types_equivalent(db, &synthesized, expected) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected);
                        let actual_str = type_to_string(db, &synthesized);
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected);
                    let actual_str = type_to_string(db, &synthesized);
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle unary negation on signed fixed integers.
        // When expected type is a signed fixed int (i8, i16, i32, i64), check operand against it.
        ExprFunKind::UnaryOp(unary) if unary.op == UnaryOp::Neg => {
            if is_signed_fixed_int_type(expected) {
                // Special case: integer literal operand needs negated value validation.
                // e.g., -2147483648 is valid i32 even though 2147483648 isn't.
                if let ExprFunKind::Int(int_expr) = unary.operand.expr(db) {
                    if let Type::Datalit(expected_datalit_ty) = expected {
                        let value_str = int_expr.value.as_str(db);
                        let negated = format!("-{}", value_str);
                        check_int_fits_wrapped_type(&negated, expected_datalit_ty, db)?;
                        ctx.store_expr_type(unary.operand, expected);
                        ctx.store_expr_type(expr, expected);
                        return Ok(());
                    }
                }
                // For other operands, check normally.
                check_expr(ctx, unary.operand, expected)?;
                ctx.store_expr_type(expr, &expected);
                return Ok(());
            }
            // Otherwise fall through to default synthesis behavior.
            let synthesized = ctx.synthesize_expr(expr)?;
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }
            let expected_str = type_to_string(db, expected);
            let actual_str = type_to_string(db, &synthesized);
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
        }

        // Handle clone/coerce operator (@) - explicit lossless conversion.
        ExprFunKind::CloneCoerce(ref cc_expr) => {
            // Synthesize the operand's type.
            let operand_ty = ctx.synthesize_expr(cc_expr.operand)?;

            // Check if the conversion is valid using can_clone_coerce_to.
            if can_clone_coerce_to(&operand_ty, expected, db) {
                ctx.store_expr_type(expr, expected);
                return Ok(());
            }

            // Conversion not valid - report error.
            let expected_str = type_to_string(db, expected);
            let actual_str = type_to_string(db, &operand_ty);
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "@ operator cannot convert between these types"))
        }

        // Handle binary operations - bidirectional type propagation for arithmetic.
        ExprFunKind::BinOp(ref binop) => {
            let op = binop.op;
            let is_checked = matches!(op,
                BinOp::AddChecked | BinOp::SubChecked | BinOp::MulChecked | BinOp::DivChecked);
            let is_optional = matches!(op,
                BinOp::AddOptional | BinOp::SubOptional | BinOp::MulOptional | BinOp::DivOptional);

            // For checked/optional ops, verify the function's return type is compatible.
            // Checked ops require Result return type, optional ops require Option return type.
            // If this check fails, fall through to synthesis which will produce the proper error.
            let return_type_ok = if is_checked {
                matches!(
                    ctx.expected_return_type,
                    Some(Type::Datalit(datalit::tycheck::Type::Result(_)))
                )
            } else if is_optional {
                matches!(
                    ctx.expected_return_type,
                    Some(Type::Datalit(datalit::tycheck::Type::Option(_)))
                )
            } else {
                true
            };

            // Checked/optional ops on fixed ints: propagate expected type to operands.
            if return_type_ok && (is_checked || is_optional) && is_fixed_int_type(expected) {
                let lhs_result = check_expr(ctx, binop.lhs, expected);
                let rhs_result = check_expr(ctx, binop.rhs, expected);

                if lhs_result.is_ok() && rhs_result.is_ok() {
                    ctx.store_expr_type(expr, expected);
                    return Ok(());
                }
                // Fall through to synthesis if checking fails.
            }

            // Bigint division: propagate int to operands.
            if return_type_ok && matches!(op, BinOp::DivChecked | BinOp::DivOptional) && is_bigint_type(expected) {
                let lhs_result = check_expr(ctx, binop.lhs, expected);
                let rhs_result = check_expr(ctx, binop.rhs, expected);

                if lhs_result.is_ok() && rhs_result.is_ok() {
                    ctx.store_expr_type(expr, expected);
                    return Ok(());
                }
            }

            // Bare arithmetic on floats: propagate float type.
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                && is_float_type(expected)
            {
                let lhs_result = check_expr(ctx, binop.lhs, expected);
                let rhs_result = check_expr(ctx, binop.rhs, expected);

                if lhs_result.is_ok() && rhs_result.is_ok() {
                    ctx.store_expr_type(expr, expected);
                    return Ok(());
                }
            }

            // Fall through to default synthesis behavior.
            // Note: Bare arithmetic on fixed ints is a type error - they must use
            // @ to widen first, or use checked/optional operators (+!, +?, etc.).
            let synthesized = ctx.synthesize_expr(expr)?;
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }
            if let Type::Datalit(datalit::tycheck::Type::Data) = expected {
                return Ok(());
            }
            // No match or coercion possible - try auto-adapt or report error.
            ctx.check_type_mismatch_or_adapt(expr, expected, &synthesized, "type mismatch")
        }

        // For other non-datalit expressions, use synthesis + comparison.
        // No implicit widening - numeric conversions require @.
        _ => {
            let synthesized = ctx.synthesize_expr(expr)?;

            // Check for exact type match first.
            if types_equivalent(db, &synthesized, expected) {
                return Ok(());
            }

            // Check for automatic coercion to Data.
            if let Type::Datalit(datalit::tycheck::Type::Data) = expected {
                // Any type can coerce to data.
                return Ok(());
            }

            // No match or coercion possible - try auto-adapt or report error.
            ctx.check_type_mismatch_or_adapt(expr, expected, &synthesized, "type mismatch")
        }
    }
}

// ============================================================================
// Collection Checking Helpers
// ============================================================================

/// Check list elements against expected type.
pub fn check_list_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual list type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the list type.
    // Callers ensure expected_ty is a List type before calling.
    let expected_elem_ty = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::List(list_ty)) => {
            Type::Datalit((*list_ty.element_type).clone())
        }
        _ => unreachable!("check_list_elements called with non-list type"),
    };
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check set elements against expected type.
pub fn check_set_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual set type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the set type.
    // Callers ensure expected_ty is a Set type before calling.
    let expected_elem_ty = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::Set(set_ty)) => {
            Type::Datalit((*set_ty.element_type).clone())
        }
        _ => unreachable!("check_set_elements called with non-set type"),
    };
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check map entries against expected type.
pub fn check_map_entries<'db>(
    ctx: &mut TypeContext<'db>,
    entries: &[ExprMapEntry<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual map type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the key and value types from the map type.
    // Callers ensure expected_ty is a Map type before calling.
    let (expected_key_ty, expected_value_ty) = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::Map(map_ty)) => {
            (
                Type::Datalit((*map_ty.key_type).clone()),
                Type::Datalit((*map_ty.value_type).clone()),
            )
        }
        _ => unreachable!("check_map_entries called with non-map type"),
    };
    for entry in entries {
        check_expr(ctx, entry.key, &expected_key_ty)?;
        check_expr(ctx, entry.value, &expected_value_ty)?;
    }

    Ok(())
}

/// Check tensor shape and elements against expected type.
pub fn check_tensor_shape_and_elements<'db>(
    ctx: &mut TypeContext<'db>,
    tensor_expr: ExprTensor<'db>,
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let elements = &tensor_expr.elements;
    let shape = &tensor_expr.shape;

    // Unwrap Option/Result wrappers to get the actual tensor type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract tensor type info.
    // Callers ensure expected_ty is a Tensor type before calling.
    let (elem_type, expected_rank) = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::Tensor(tensor_ty)) => {
            (tensor_ty.element_type.clone(), tensor_ty.rank)
        }
        _ => unreachable!("check_tensor_shape_and_elements called with non-tensor type"),
    };

    // Check rank matches type hint.
    let actual_rank = shape.len() as u32;
    if actual_rank != expected_rank {
        return Err(TypeError::ArityMismatch {
            expected: expected_rank as usize,
            actual: actual_rank as usize,
        });
    }

    // Check element count matches shape product.
    datalit::tycheck::check_tensor_element_count(shape, elements.len())?;

    // Check each element against expected element type.
    let expected_elem = Type::Datalit(*elem_type);
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem)?;
    }

    Ok(())
}

/// Check tuple elements against expected type.
pub fn check_tuple_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual tuple type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the tuple type.
    // Callers ensure expected_ty is a Tuple type before calling.
    let expected_fields = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::AnonTuple(tuple_ty)) => {
            tuple_ty.fields.C()
        }
        _ => unreachable!("check_tuple_elements called with non-tuple type"),
    };

    // Check arity.
    if elements.len() != expected_fields.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_fields.len(),
            actual: elements.len(),
        });
    }

    // Check each element against expected field type using bidirectional checking.
    for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
        let expected_elem_ty = Type::Datalit(expected_field.clone(),
        );
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check struct fields against expected type.
pub fn check_struct_fields<'db>(
    ctx: &mut TypeContext<'db>,
    fields: &[ExprStructField<'db>],
    expected_ty: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual struct type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the struct type.
    // Callers ensure expected_ty is a Struct type before calling.
    let expected_fields = match inner_ty {
        Type::Datalit(datalit::tycheck::Type::AnonStruct(struct_ty)) => {
            struct_ty.fields.C()
        }
        _ => unreachable!("check_struct_fields called with non-struct type"),
    };

    // Check arity.
    if fields.len() != expected_fields.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_fields.len(),
            actual: fields.len(),
        });
    }

    // Check each field against expected field type.
    for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
        // Check field name matches.
        if field.name != expected_field.name {
            return Err(TypeError::FieldOrderMismatch);
        }

        let expected_field_ty = Type::Datalit(*expected_field.ty.clone());
        check_expr(ctx, field.value, &expected_field_ty)?;
    }

    Ok(())
}


/// Check table rows against expected table type.
pub fn check_table_rows<'db>(
    ctx: &mut TypeContext<'db>,
    header: &[bct::text::InternedText<'db>],
    rows: &[ExprTableRow<'db>],
    table_ty: &datalit::tycheck::TypeTable<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expected_columns = &table_ty.columns;

    // Validate column count matches.
    if header.len() != expected_columns.len() {
        return Err(TypeError::ArityMismatch {
            expected: expected_columns.len(),
            actual: header.len(),
        });
    }

    // Validate column names match (in order).
    for (h, c) in header.iter().zip(expected_columns.iter()) {
        if h.as_str(db) != c.name.as_str(db) {
            return Err(TypeError::FieldOrderMismatch);
        }
    }

    // Check each row's elements against column types.
    for row in rows {
        if row.elements.len() != expected_columns.len() {
            return Err(TypeError::ArityMismatch {
                expected: expected_columns.len(),
                actual: row.elements.len(),
            });
        }

        for (elem, col) in row.elements.iter().zip(expected_columns.iter()) {
            let expected_col_ty = Type::Datalit(*col.ty.clone());
            check_expr(ctx, *elem, &expected_col_ty)?;
        }
    }

    Ok(())
}
