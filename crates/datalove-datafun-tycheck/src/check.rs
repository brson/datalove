//! Expression checking (bidirectional typechecking).
//!
//! Provides check_expr for checking expressions against expected types,
//! and helpers for checking collection elements.
//!
//! Organization (parallel to datalit/tycheck/check.rs):
//! 1. Element checking helper - type and heap compatibility for collection elements
//! 2. Main check_expr function - entry point for type checking
//! 3. Collection checking helpers - list, set, map, tensor, tuple, struct, enum

use rmx::prelude::*;
use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::types::*;

pub use crate::{Type, TypeAndHeap, TypeError};

// ============================================================================
// Element Checking Helper
// ============================================================================

/// Check an element's type against expected type.
///
/// This does type coercion checking for collection elements.
fn check_element_type_and_heap<'db>(
    db: &'db dyn crate::Db,
    _elem_expr: ExprFun<'db>,
    elem_ty: TypeAndHeap<'db>,
    expected_type: &crate::TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    // Extract actual datalit type.
    let actual_datalit_ty = match elem_ty.ty(db) {
        Type::Datalit(dt) => dt,
        _ => return Ok(()), // Non-datalit types handled elsewhere.
    };

    // Check type compatibility with coercion.
    check_type_coercion(db, actual_datalit_ty, expected_type)?;

    Ok(())
}


// ============================================================================
// Type Checking
// ============================================================================

/// Check an expression against an expected type.
pub fn check_expr<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFun<'db>,
    expected: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expr_kind = expr.expr(db);

    match expr_kind {
        // Handle None literals specially - they can check against any Option type.
        ExprFunKind::None(_lit) => {
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(_)) => {
                    // None checks against any Option<T>.
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, "None", "None requires Option type"))
                }
            }
        }

        // Handle Some expressions - check against Option type.
        ExprFunKind::Some(some_expr) => {
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
                    // Check payload against inner type.
                    let expected_inner = TypeAndHeap::new(Type::Datalit((*opt.inner_type).clone())
                    );
                    check_expr(ctx, some_expr.payload, &expected_inner)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, "some", "some requires Option type"))
                }
            }
        }

        // Handle Ok expressions - check against Result type.
        ExprFunKind::Ok(ok_expr) => {
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Result(res)) => {
                    // Check payload against inner type.
                    let expected_inner = TypeAndHeap::new(Type::Datalit((*res.inner_type).clone())
                    );
                    check_expr(ctx, ok_expr.payload, &expected_inner)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, "ok", "ok requires Result type"))
                }
            }
        }

        // Handle Er expressions - check against Result type.
        ExprFunKind::Er(er_expr) => {
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::Result(_)) => {
                    // Check payload against Error type.
                    let error_ty = TypeAndHeap::new(Type::Datalit(datalit::tycheck::Type::Error)
                    );
                    check_expr(ctx, er_expr.payload, &error_ty)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                _ => {
                    let expected_str = type_to_string(db, expected.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, "er", "er requires Result type"))
                }
            }
        }

        // Handle anonymous enum expressions - check against expected enum type.
        ExprFunKind::AnonEnum(enum_expr) => {
            match expected.ty(db) {
                Type::Datalit(datalit::tycheck::Type::AnonEnum(_)) => {
                    // Check the variant and payload against expected enum type.
                    check_enum_variant(ctx, enum_expr.variant_name, enum_expr.payload, &expected)?;
                    ctx.store_expr_type(expr, &expected);
                    Ok(())
                }
                Type::Datalit(datalit::tycheck::Type::Data) => {
                    // Any type can coerce to Data - but we need a concrete type.
                    // Try to get the type hint, otherwise synthesize will fail.
                    if let Some(type_hint) = enum_expr.type_hint {
                        let enum_ty = convert_type_hint(db, type_hint)?;
                        check_enum_variant(ctx, enum_expr.variant_name, enum_expr.payload, &enum_ty)?;
                        ctx.store_expr_type(expr, &expected);
                        Ok(())
                    } else {
                        Err(ctx.error_cannot_synthesize(expr, "anonymous enum requires type hint when coercing to data"))
                    }
                }
                _ => {
                    let expected_str = type_to_string(db, expected.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, "enum", "expected enum type"))
                }
            }
        }

        // Handle table expressions - check rows against expected column types.
        ExprFunKind::Table(table_expr) => {
            match expected.ty(db) {
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
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle list expressions - check elements against expected element type.
        ExprFunKind::List(list_expr) => {
            match expected.ty(db) {
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
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle set expressions - check elements against expected element type.
        ExprFunKind::Set(set_expr) => {
            match expected.ty(db) {
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
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle map expressions - check entries against expected key/value types.
        ExprFunKind::Map(map_expr) => {
            match expected.ty(db) {
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
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle tuple expressions - check elements against expected field types.
        ExprFunKind::AnonTuple(tuple_expr) => {
            match expected.ty(db) {
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
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle integer literals specially - they can coerce to expected integer types.
        ExprFunKind::Int(_int_expr) => {
            match expected.ty(db) {
                Type::Datalit(expected_datalit_ty) => {
                    // Check if expected type is a numeric type.
                    if is_numeric_type(expected.ty(db)) {
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
                    if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected.ty(db));
                        let actual_str = type_to_string(db, synthesized.ty(db));
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle float literals specially - they can coerce to expected float types.
        ExprFunKind::Float(_float_expr) => {
            match expected.ty(db) {
                Type::Datalit(expected_datalit_ty) => {
                    // Check if expected type is a float type.
                    if is_float_type(expected.ty(db)) {
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
                    if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                        Ok(())
                    } else {
                        let expected_str = type_to_string(db, expected.ty(db));
                        let actual_str = type_to_string(db, synthesized.ty(db));
                        Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                    }
                }
                _ => {
                    let synthesized = ctx.synthesize_expr(expr)?;
                    let expected_str = type_to_string(db, expected.ty(db));
                    let actual_str = type_to_string(db, synthesized.ty(db));
                    Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
                }
            }
        }

        // Handle unary negation on signed fixed integers.
        // When expected type is a signed fixed int (i8, i16, i32, i64), check operand against it.
        ExprFunKind::UnaryOp(unary) if unary.op == UnaryOp::Neg => {
            if is_signed_fixed_int_type(expected.ty(db)) {
                // Check operand against expected type.
                check_expr(ctx, unary.operand, expected)?;
                ctx.store_expr_type(expr, &expected);
                return Ok(());
            }
            // Otherwise fall through to default synthesis behavior.
            let synthesized = ctx.synthesize_expr(expr)?;
            if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                return Ok(());
            }
            let expected_str = type_to_string(db, expected.ty(db));
            let actual_str = type_to_string(db, synthesized.ty(db));
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
        }

        // For other non-datalit expressions, use synthesis + comparison with coercion support.
        _ => {
            let synthesized = ctx.synthesize_expr(expr)?;

            // Check for exact type match first.
            if types_equivalent(db, synthesized.ty(db), expected.ty(db)) {
                return Ok(());
            }

            // If exact match fails, try numeric widening.
            if let (Type::Datalit(synth_ty), Type::Datalit(expect_ty)) = (synthesized.ty(db), expected.ty(db)) {
                if datalit::tycheck::can_widen_to(synth_ty, expect_ty) {
                    return Ok(());
                }
            }

            // If widening fails, check for automatic coercion to Data.
            if let Type::Datalit(datalit::tycheck::Type::Data) = expected.ty(db) {
                // Any type can coerce to data.
                return Ok(());
            }

            // No match or coercion possible.
            let expected_str = type_to_string(db, expected.ty(db));
            let actual_str = type_to_string(db, synthesized.ty(db));
            Err(ctx.error_type_mismatch(expr, &expected_str, &actual_str, "type mismatch"))
        }
    }
}

/// Check if a type can be coerced to an expected type.
///
/// Delegates to datalit's check_type_coercion after extracting the expected type.
fn check_type_coercion<'db>(
    db: &'db dyn crate::Db,
    actual: &datalit::tycheck::Type<'db>,
    expected: &crate::TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    match expected.ty(db) {
        Type::Datalit(expected_dt) => {
            datalit::tycheck::check_type_coercion(db, actual, expected_dt).map_err(TypeError::from)
        }
        _ => Ok(()), // Non-datalit types handled elsewhere.
    }
}

// ============================================================================
// Collection Checking Helpers
// ============================================================================

/// Check list elements against expected type.
pub fn check_list_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual list type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the list type.
    let elem_type = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::List(list_ty)) => {
            &list_ty.element_type
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check each element against expected element type using bidirectional checking.
    let expected_elem_ty = TypeAndHeap::new(Type::Datalit((**elem_type).clone()),
    );
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check set elements against expected type.
pub fn check_set_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual set type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the element type from the set type.
    let elem_type = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Set(set_ty)) => &set_ty.element_type,
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check each element against expected element type using bidirectional checking.
    let expected_elem_ty = TypeAndHeap::new(Type::Datalit((**elem_type).clone()),
    );
    for elem in elements {
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check map entries against expected type.
pub fn check_map_entries<'db>(
    ctx: &mut TypeContext<'db>,
    entries: &[ExprMapEntry<'db>],
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual map type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the key and value types from the map type.
    let (key_type, value_type) = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Map(map_ty)) => {
            (&map_ty.key_type, &map_ty.value_type)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Check each entry against expected types using bidirectional checking.
    let expected_key_ty = TypeAndHeap::new(Type::Datalit((**key_type).clone()),
    );
    let expected_value_ty = TypeAndHeap::new(Type::Datalit((**value_type).clone()),
    );
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
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let elements = &tensor_expr.elements;
    let shape = &tensor_expr.shape;

    // Unwrap Option/Result wrappers to get the actual tensor type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract tensor type info.
    let (elem_type, expected_rank) = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Tensor(tensor_ty)) => {
            (tensor_ty.element_type.clone(), tensor_ty.rank)
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
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
    let expected_elem = TypeAndHeap::new(Type::Datalit(*elem_type),
    );
    for elem in elements {
        let elem_ty = ctx.synthesize_expr(*elem)?;
        check_element_type_and_heap(db, *elem, elem_ty, &expected_elem)?;
    }

    Ok(())
}

/// Check tuple elements against expected type.
pub fn check_tuple_elements<'db>(
    ctx: &mut TypeContext<'db>,
    elements: &[ExprFun<'db>],
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual tuple type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the tuple type.
    let expected_fields = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonTuple(tuple_ty)) => {
            tuple_ty.fields.C()
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
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
        let expected_elem_ty = TypeAndHeap::new(Type::Datalit(expected_field.clone()),
        );
        check_expr(ctx, *elem, &expected_elem_ty)?;
    }

    Ok(())
}

/// Check struct fields against expected type.
pub fn check_struct_fields<'db>(
    ctx: &mut TypeContext<'db>,
    fields: &[ExprStructField<'db>],
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual struct type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the field types from the struct type.
    let expected_fields = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonStruct(struct_ty)) => {
            struct_ty.fields.C()
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
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

        let expected_ty = TypeAndHeap::new(Type::Datalit((*expected_field.ty).clone()),
        );
        let field_value_ty = ctx.synthesize_expr(field.value)?;
        check_element_type_and_heap(db, field.value, field_value_ty, &expected_ty)?;
    }

    Ok(())
}

/// Check enum variant exists in type hint.
pub fn check_enum_variant<'db>(
    ctx: &mut TypeContext<'db>,
    variant_name: bct::text::InternedText<'db>,
    payload: Option<ExprFun<'db>>,
    expected_ty: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    // Unwrap Option/Result wrappers to get the actual enum type.
    let inner_ty = unwrap_wrapper_types(db, expected_ty);

    // Extract the variants from the enum type.
    let expected_variants = match inner_ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonEnum(enum_ty)) => {
            enum_ty.variants.C()
        }
        _ => return Ok(()), // Type mismatch will be caught elsewhere.
    };

    // Look up the variant by name.
    let expected_variant = expected_variants
        .iter()
        .find(|v| v.name == variant_name)
        .ok_or_else(|| {
            TypeError::VariantNotFound(variant_name.as_str(db).S())
        })?;

    // Check payload type if both have payloads.
    // If payload presence differs (expression has payload but type hint doesn't, or vice versa),
    // don't fail here - let the type comparison at a higher level catch the mismatch.
    // This matches datalit's Check-TypedAnonEnum behavior which compares enum types rather
    // than individual variant payloads.
    if let (Some(payload_expr), Some(expected_payload_ty)) = (payload, expected_variant.payload.as_ref()) {
        // Synthesize payload type and check against expected.
        let payload_ty = ctx.synthesize_expr(payload_expr)?;
        let actual_datalit_ty = match payload_ty.ty(db) {
            Type::Datalit(dt) => dt,
            _ => return Ok(()), // Non-datalit types handled elsewhere.
        };
        let expected_ty = TypeAndHeap::new(Type::Datalit((**expected_payload_ty).clone()),
        );
        check_type_coercion(db, actual_datalit_ty, &expected_ty)?;
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
            let expected_col_ty = TypeAndHeap::new(Type::Datalit((*col.ty).clone()),
            );
            check_expr(ctx, *elem, &expected_col_ty)?;
        }
    }

    Ok(())
}
