//! Expression type checking.
//!
//! Provides check for verifying expressions against expected types.

use datalove_diagnostic::DiagnosticBuilder;
use crate::ast::*;
use super::context::TypeContext;
use super::synthesize::synthesize;
use super::types::*;

/// Check an expression against an expected type.
pub fn check<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
    expected: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;

    let expr_and_heap = expr.expr(db);
    let actual_heap = expr_and_heap.heap;
    let expr_inner = expr_and_heap.expr.clone();
    let expected_type = expected.ty(db);

    // Extract the expected heap for the heap compatibility check.
    // All heaps in a type must match - datalove does not allow intermixed heaps.
    let expected_heap = match (&expr_inner, expected_type) {
        (Expr::None, Type::Option(_)) => expected.heap(db),
        (Expr::Some(_), Type::Option(_)) => expected.heap(db),
        (Expr::Ok(_), Type::Result(_)) => expected.heap(db),
        (Expr::Er(_), Type::Result(_)) => expected.heap(db),
        (Expr::Error(_), Type::Result(_)) => expected.heap(db),
        (_, Type::Option(opt)) => opt.inner_type.heap(db),
        (_, Type::Result(res)) => res.inner_type.heap(db),
        _ => expected.heap(db),
    };

    if !heaps_compatible(actual_heap, expected_heap) {
        // T037: General heap mismatch.
        if let Some(ts) = ctx.get_span(expr) {
            DiagnosticBuilder::error(db, "heap allocation mismatch")
                .code("T037")
                .primary_label(ts.clone(), &format!("expected {}, found {}",
                    heap_to_string(expected_heap),
                    heap_to_string(actual_heap)))
                .note("heap-allocated and stack-allocated values cannot be mixed")
                .emit_type();
        }
        return Err(TypeError::HeapMismatch {
            expected_heap: heap_to_string(expected_heap),
            actual_heap: heap_to_string(actual_heap),
        });
    }

    match (expr_inner, expected_type) {
        // Rule: Check-None
        (Expr::None, Type::Option(_)) => Ok(()),

        // Rule: Check-Some - explicit some constructor
        (Expr::Some(s), Type::Option(opt)) => {
            let payload = s.payload;
            check(ctx, payload, opt.inner_type)
        }

        // Rule: Check-Ok - explicit ok constructor
        (Expr::Ok(o), Type::Result(res)) => {
            let payload = o.payload;
            check(ctx, payload, res.inner_type)
        }

        // Rule: Check-Er - explicit er constructor
        (Expr::Er(e), Type::Result(_)) => {
            // Check that the payload is a valid error expression.
            let payload = e.payload;
            let payload_expr = payload.expr(db);
            match &payload_expr.expr {
                Expr::Error(_) => Ok(()),
                Expr::Data(_) => Ok(()), // data can be used as error payload
                _ => {
                    // T040: Er payload must be an error expression.
                    if let Some(ts) = ctx.get_span(payload) {
                        DiagnosticBuilder::error(db, "er payload must be an error expression")
                            .code("T040")
                            .primary_label(ts.clone(), "expected error expression")
                            .note("use `er error \"message\"` to construct a result error")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "error".to_string(),
                        actual: "non-error".to_string(),
                    })
                }
            }
        }

        // Rule: Check-ResultErr (implicit Err wrapping)
        (Expr::Error(_), Type::Result(_)) => {
            // Error expressions can check against any Result type (implicit Err wrapping).
            Ok(())
        }

        // Rule: Check-TypedInt - respect type hints on integer literals.
        // This must come before the bare integer patterns to ensure type hints are honored.
        // Only handles direct integer type hints, not Option/Result wrapped ones.
        (Expr::Int(_), _) if expr.type_hint(db).is_some() && is_direct_integer_type_hint(&expr.type_hint(db).unwrap().type_hint(db)) => {
            // Get the type from the hint and verify the literal value fits.
            let type_hint_and_heap = expr.type_hint(db).unwrap();
            let hinted_type = convert_type_hint(db, type_hint_and_heap)?;
            let hinted_type_inner = hinted_type.ty(db);

            // First check against the hinted type to ensure the literal is valid.
            let expr_without_hint = ExprFull::new(db, None, expr_and_heap.clone());
            check(ctx, expr_without_hint, hinted_type)?;

            // Now check if the hinted type matches or can widen to the expected type.
            if types_equivalent(db, hinted_type_inner, expected_type) {
                Ok(())
            } else if can_widen_to(hinted_type_inner, expected_type) {
                // Allow widening from hinted type to expected type.
                Ok(())
            } else {
                // T040: Type mismatch - cannot widen from hinted type.
                if let Some(ts) = ctx.get_span(expr) {
                    let msg = format!("mismatched types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T040")
                        .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, hinted_type_inner)))
                        .note("type hints on integer literals are respected; widening is only allowed within the same signedness (unsigned->unsigned or signed->signed)")
                        .emit_type();
                }
                Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, hinted_type_inner),
                })
            }
        }

        // Rule: Check-Subsume - try synthesis first.
        // IMPORTANT: Synthesize from the inner expression without type hint to avoid infinite recursion.
        (Expr::True | Expr::False | Expr::String(_), _) => {
            let expr_without_hint = ExprFull::new(db, None, expr_and_heap.clone());
            let synthesized = synthesize(ctx, expr_without_hint)?;
            if !types_equivalent(db, synthesized.ty(db), expected_type) {
                // T022: Type mismatch for primitive literal.
                if let Some(ts) = ctx.get_span(expr) {
                    let msg = format!("mismatched types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T022")
                        .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, synthesized.ty(db))))
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, synthesized.ty(db)),
                });
            }
            Ok(())
        }

        // Rule: Check-Int - integer literals against integer types.
        (Expr::Int(i), expected_int_ty) if is_fixed_int_type(expected_int_ty) || is_bigint_type(expected_int_ty) => {
            let value_str = i.value.as_str(db);
            check_int_fits_type(value_str, expected_int_ty).map_err(|e| {
                emit_int_range_error(ctx, expr, expected_int_ty);
                e
            })
        }

        // Rule: Check-Float
        (Expr::Float(_), Type::F32) => Ok(()),
        (Expr::Float(_), Type::F64) => Ok(()),

        // Rule: Check-Hex - hex literals against unsigned integer types, bigint, or float bit patterns.
        // Note: signed types fall through to synthesis to get TypeMismatch errors.
        (Expr::Hex(h), expected_hex_ty) if is_unsigned_int_type(expected_hex_ty) || is_bigint_type(expected_hex_ty) || is_float_type(expected_hex_ty) => {
            let value_str = h.value.as_str(db);
            check_hex_fits_type(value_str, expected_hex_ty).map_err(|e| {
                emit_hex_range_error(ctx, expr, expected_hex_ty);
                e
            })
        }

        // Rule: Check-AnonTuple
        (Expr::AnonTuple(t), Type::AnonTuple(expected_tuple)) => {
            let elements = t.elements.clone();
            let expected_fields = expected_tuple.fields.clone();

            if elements.len() != expected_fields.len() {
                // T038: Tuple arity mismatch.
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "tuple has wrong number of elements")
                        .code("T038")
                        .primary_label(ts.clone(), &format!("expected {} element(s), found {}",
                            expected_fields.len(), elements.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: elements.len(),
                });
            }

            for (elem, expected_field) in elements.iter().zip(expected_fields.iter()) {
                check(ctx, *elem, *expected_field)?;
            }

            Ok(())
        }

        // Rule: Check-AnonStruct
        (Expr::AnonStruct(s), Type::AnonStruct(expected_struct)) => {
            let fields = s.fields.clone();
            let expected_fields = expected_struct.fields.clone();

            if fields.len() != expected_fields.len() {
                // T039: Struct arity mismatch.
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "struct has wrong number of fields")
                        .code("T039")
                        .primary_label(ts.clone(), &format!("expected {} field(s), found {}",
                            expected_fields.len(), fields.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_fields.len(),
                    actual: fields.len(),
                });
            }

            for (field, expected_field) in fields.iter().zip(expected_fields.iter()) {
                let field_name = field.name;
                let expected_name = expected_field.name;

                if field_name != expected_name {
                    // T042: Struct field order mismatch.
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "struct fields in wrong order")
                            .code("T042")
                            .primary_label(ts.clone(), &format!("expected field `{}`, found `{}`",
                                expected_name.as_str(db), field_name.as_str(db)))
                            .note("struct fields must appear in the same order as the type definition")
                            .emit_type();
                    }
                    return Err(TypeError::FieldOrderMismatch);
                }

                check(ctx, field.value, expected_field.ty)?;
            }

            Ok(())
        }

        // Rule: Check-TypedAnonEnum - validate type hint against expected type.
        // When an anonymous enum expression has a direct enum type hint (not wrapped
        // in Option/Result), the hinted type must be equivalent to the expected type.
        (Expr::AnonEnum(_), Type::AnonEnum(_)) if expr.type_hint(db).is_some() && matches!(expr.type_hint(db).unwrap().type_hint(db), TypeHint::AnonEnum(_)) => {
            let type_hint_and_heap = expr.type_hint(db).unwrap();
            let hinted_type = convert_type_hint(db, type_hint_and_heap)?;
            let hinted_type_inner = hinted_type.ty(db);

            // Check if the hinted type matches the expected type.
            if !types_equivalent(db, hinted_type_inner, expected_type) {
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "mismatched types")
                        .code("T047")
                        .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, hinted_type_inner)))
                        .note("the type hint on this enum does not match the expected type from context")
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, hinted_type_inner),
                });
            }

            // Now check the expression without hint against the expected type.
            let expr_without_hint = ExprFull::new(db, None, expr_and_heap.clone());
            check(ctx, expr_without_hint, expected)
        }

        // Rule: Check-AnonEnum
        (Expr::AnonEnum(e), Type::AnonEnum(expected_enum)) => {
            let variant_name = e.variant_name;
            let expected_variants = expected_enum.variants.clone();

            let expected_variant = expected_variants
                .iter()
                .find(|v| v.name == variant_name)
                .ok_or_else(|| {
                    // T044: Enum variant not found.
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, &format!("variant `{}` not found in enum", variant_name.as_str(db)))
                            .code("T044")
                            .primary_label(ts.clone(), "variant not defined")
                            .emit_type();
                    }
                    TypeError::VariantNotFound(variant_name.as_str(db).to_string())
                })?;

            match (e.payload, expected_variant.payload) {
                (Some(payload), Some(expected_payload)) => {
                    check(ctx, payload, expected_payload)
                }
                (None, None) => Ok(()),
                (Some(_), None) => {
                    // T024: Enum variant payload mismatch (has payload, expected none).
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T024")
                            .primary_label(ts.clone(), "expected no payload, found payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "no payload".to_string(),
                        actual: "payload".to_string(),
                    })
                }
                (None, Some(_)) => {
                    // T025: Enum variant payload mismatch (no payload, expected payload).
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "enum variant payload mismatch")
                            .code("T025")
                            .primary_label(ts.clone(), "expected payload, found no payload")
                            .emit_type();
                    }
                    Err(TypeError::TypeMismatch {
                        expected: "payload".to_string(),
                        actual: "no payload".to_string(),
                    })
                }
            }
        }

        // Rule: Check-List
        (Expr::List(l), Type::List(expected_list)) => {
            let elements = l.elements.clone();
            let element_type = expected_list.element_type;

            for elem in elements {
                check(ctx, elem, element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Map
        (Expr::Map(m), Type::Map(expected_map)) => {
            let entries = m.entries.clone();
            let key_type = expected_map.key_type;
            let value_type = expected_map.value_type;

            for entry in entries {
                check(ctx, entry.key, key_type)?;
                check(ctx, entry.value, value_type)?;
            }

            Ok(())
        }

        // Rule: Check-Set
        (Expr::Set(s), Type::Set(expected_set)) => {
            let elements = s.elements.clone();
            let element_type = expected_set.element_type;

            for elem in elements {
                check(ctx, elem, element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Tensor
        (Expr::Tensor(t), Type::Tensor(expected_tensor)) => {
            let shape = t.shape.clone();
            let elements = t.elements.clone();
            let element_type = expected_tensor.element_type;

            // Verify rank matches.
            let rank = shape.len() as u32;
            if rank != expected_tensor.rank {
                // T048: Tensor rank mismatch.
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "tensor has wrong rank")
                        .code("T048")
                        .primary_label(ts.clone(), &format!("expected rank {}, found rank {}",
                            expected_tensor.rank, rank))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_tensor.rank as usize,
                    actual: rank as usize,
                });
            }

            // Calculate expected element count from shape.
            let expected_count = shape.iter().map(|&d| d as usize).product::<usize>();
            if elements.len() != expected_count {
                // T049: Tensor element count mismatch.
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "tensor has wrong number of elements")
                        .code("T049")
                        .primary_label(ts.clone(), &format!("expected {} element(s), found {}",
                            expected_count, elements.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_count,
                    actual: elements.len(),
                });
            }

            // Check all elements against expected element type.
            for elem in elements {
                check(ctx, elem, element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Data
        (Expr::Data(_), Type::Data) => Ok(()),

        // Rule: Check-Error
        (Expr::Error(_), Type::Error) => Ok(()),

        // Otherwise, try subsumption.
        _ => {
            // Synthesize from the inner expression without type hint to avoid infinite recursion.
            let ty_without_hint = ExprFull::new(db, None, expr_and_heap.clone());
            let synthesized = synthesize(ctx, ty_without_hint)?;
            if types_equivalent(db, synthesized.ty(db), expected_type) {
                Ok(())
            } else if can_widen_to(synthesized.ty(db), expected_type) {
                // Allow numeric widening.
                Ok(())
            } else {
                // T032: General type mismatch (subsumption fallback).
                if let Some(ts) = ctx.get_span(expr) {
                    let msg = format!("mismatched types");
                    DiagnosticBuilder::error(db, &msg)
                        .code("T032")
                        .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected_type),
                            type_to_string(db, synthesized.ty(db))))
                        .emit_type();
                }
                Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected_type),
                    actual: type_to_string(db, synthesized.ty(db)),
                })
            }
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if a type hint is a direct integer type (not wrapped in Option/Result).
fn is_direct_integer_type_hint<'db>(type_hint: &TypeHint<'db>) -> bool {
    matches!(
        type_hint,
        TypeHint::U8 | TypeHint::I8 | TypeHint::U16 | TypeHint::I16 |
        TypeHint::U32 | TypeHint::I32 | TypeHint::U64 | TypeHint::I64 | TypeHint::Int
    )
}

/// Emit a diagnostic for integer literal out of range.
fn emit_int_range_error<'db>(ctx: &TypeContext<'db>, expr: ExprFull<'db>, ty: &Type<'db>) {
    let db = ctx.db;
    let (code, note) = int_type_range_info(ty);
    let type_name = type_to_string(db, ty);
    if let Some(ts) = ctx.get_span(expr) {
        DiagnosticBuilder::error(db, &format!("integer literal out of range for type {}", type_name))
            .code(code)
            .primary_label(ts.clone(), "value out of range")
            .note(note)
            .emit_type();
    }
}

/// Emit a diagnostic for hex literal out of range.
fn emit_hex_range_error<'db>(ctx: &TypeContext<'db>, expr: ExprFull<'db>, ty: &Type<'db>) {
    let db = ctx.db;
    let (code, note) = hex_type_range_info(ty);
    // Float types use "bit pattern" terminology.
    let message = match ty {
        Type::F32 => "hex literal out of range for f32 bit pattern".to_string(),
        Type::F64 => "hex literal out of range for f64 bit pattern".to_string(),
        _ => format!("hex literal out of range for type {}", type_to_string(db, ty)),
    };
    if let Some(ts) = ctx.get_span(expr) {
        DiagnosticBuilder::error(db, &message)
            .code(code)
            .primary_label(ts.clone(), "value out of range")
            .note(note)
            .emit_type();
    }
}
