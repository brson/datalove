//! Expression type checking.
//!
//! Provides check for verifying expressions against expected types.

use rmx::prelude::*;
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use crate::ast::*;
use super::context::TypeContext;
use super::synthesize::{synthesize, synthesize_unhinted};
use super::types::*;

/// Check an expression against an expected type.
pub fn check<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
    expected: &Type<'db>,
) -> Result<(), TypeError> {
    let db = ctx.db;
    let expr_inner = expr.expr(db);

    // Rule: Check-Hinted - a hint says what the expression is, and it has to
    // be what is expected. Nothing converts on the way from one to the other.
    if let Some(type_hint) = expr.type_hint(db) {
        let hinted = ctx.convert_hint(expr, &type_hint)?;
        if !types_equivalent(db, &hinted, expected) {
            if let Some(ts) = ctx.get_span(expr) {
                DiagnosticBuilder::error(db, "mismatched types")
                    .code("T040")
                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                        type_to_string(db, expected),
                        type_to_string(db, &hinted)))
                    .note("the type hint disagrees with the expected type")
                    .emit_type();
            }
            return Err(TypeError::TypeMismatch {
                expected: type_to_string(db, expected),
                actual: type_to_string(db, &hinted),
            });
        }
    }

    match (expr_inner.clone(), expected) {
        // Rule: Check-Group
        (Expr::Group(g), _) => check(ctx, g.inner, expected),

        // Rule: Check-None
        (Expr::None, Type::Option(_)) => Ok(()),

        // Rule: Check-Some - explicit some constructor
        (Expr::Some(s), Type::Option(opt)) => {
            check(ctx, s.payload, &opt.inner_type)
        }

        // Rule: Check-Ok - explicit ok constructor
        (Expr::Ok(o), Type::Result(res)) => {
            check(ctx, o.payload, &res.inner_type)
        }

        // Rule: Check-Er - the payload is the error, and an error is written
        // as one, as datafun has it.
        (Expr::Er(e), Type::Result(_)) => check(ctx, e.payload, &Type::Error),

        // `none` and `er` have no type of their own to report, so what they
        // are checked against is what is wrong.
        (Expr::None, _) => Err(constructor_mismatch(ctx, expr, expected, "none")),
        (Expr::Er(_), _) => Err(constructor_mismatch(ctx, expr, expected, "er")),

        // Rule: Check-Subsume - try synthesis first.
        (Expr::True | Expr::False | Expr::String(_), _) => {
            let synthesized = synthesize_unhinted(ctx, expr)?;
            if !types_equivalent(db, &synthesized, expected) {
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "mismatched types")
                        .code("T022")
                        .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected),
                            type_to_string(db, &synthesized)))
                        .emit_type();
                }
                return Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected),
                    actual: type_to_string(db, &synthesized),
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

        // Rule: Check-Hex
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
            let expected_fields = &expected_tuple.fields;

            if elements.len() != expected_fields.len() {
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
                check(ctx, *elem, expected_field)?;
            }

            Ok(())
        }

        // Rule: Check-AnonStruct
        (Expr::AnonStruct(s), Type::AnonStruct(expected_struct)) => {
            let fields = s.fields.clone();
            let expected_fields = &expected_struct.fields;

            if fields.len() != expected_fields.len() {
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
                if field.name != expected_field.name {
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "struct fields in wrong order")
                            .code("T042")
                            .primary_label(ts.clone(), &format!("expected field `{}`, found `{}`",
                                expected_field.name.as_str(db), field.name.as_str(db)))
                            .note("struct fields must appear in the same order as the type definition")
                            .emit_type();
                    }
                    return Err(TypeError::FieldOrderMismatch);
                }

                check(ctx, field.value, &expected_field.ty)?;
            }

            Ok(())
        }


        // Rule: Check-List
        (Expr::List(l), Type::List(expected_list)) => {
            for elem in &l.elements {
                check(ctx, *elem, &expected_list.element_type)?;
            }
            Ok(())
        }

        // Rule: Check-Map
        (Expr::Map(m), Type::Map(expected_map)) => {
            for entry in &m.entries {
                check(ctx, entry.key, &expected_map.key_type)?;
                check(ctx, entry.value, &expected_map.value_type)?;
            }
            Ok(())
        }

        // Rule: Check-Set
        (Expr::Set(s), Type::Set(expected_set)) => {
            for elem in &s.elements {
                check(ctx, *elem, &expected_set.element_type)?;
            }
            Ok(())
        }

        // Rule: Check-Tensor
        (Expr::Tensor(t), Type::Tensor(expected_tensor)) => {
            let rank = t.shape.len() as u32;
            if rank != expected_tensor.rank {
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

            let expected_count = t.shape.iter().map(|&d| d as usize).product::<usize>();
            if t.elements.len() != expected_count {
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "tensor has wrong number of elements")
                        .code("T049")
                        .primary_label(ts.clone(), &format!("expected {} element(s), found {}",
                            expected_count, t.elements.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_count,
                    actual: t.elements.len(),
                });
            }

            for elem in &t.elements {
                check(ctx, *elem, &expected_tensor.element_type)?;
            }

            Ok(())
        }

        // Rule: Check-Table
        (Expr::Table(t), Type::Table(expected_table)) => {
            let expected_columns = &expected_table.columns;

            if t.header.len() != expected_columns.len() {
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "table has wrong number of columns")
                        .code("T054")
                        .primary_label(ts.clone(), &format!("expected {} column(s), found {}",
                            expected_columns.len(), t.header.len()))
                        .emit_type();
                }
                return Err(TypeError::ArityMismatch {
                    expected: expected_columns.len(),
                    actual: t.header.len(),
                });
            }

            for (i, (header_name, expected_col)) in t.header.iter().zip(expected_columns.iter()).enumerate() {
                if *header_name != expected_col.name {
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "table column name mismatch")
                            .code("T055")
                            .primary_label(ts.clone(), &format!("column {}: expected `{}`, found `{}`",
                                i + 1, expected_col.name.as_str(db), header_name.as_str(db)))
                            .note("table column names must match the type definition in order")
                            .emit_type();
                    }
                    return Err(TypeError::FieldOrderMismatch);
                }
            }

            for (row_idx, row) in t.rows.iter().enumerate() {
                if row.elements.len() != expected_columns.len() {
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "table row has wrong number of columns")
                            .code("T056")
                            .primary_label(ts.clone(), &format!("row {}: expected {} column(s), found {}",
                                row_idx + 1, expected_columns.len(), row.elements.len()))
                            .emit_type();
                    }
                    return Err(TypeError::ArityMismatch {
                        expected: expected_columns.len(),
                        actual: row.elements.len(),
                    });
                }

                for (elem, expected_col) in row.elements.iter().zip(expected_columns.iter()) {
                    check(ctx, *elem, &expected_col.ty)?;
                }
            }

            Ok(())
        }

        // Rule: Check-Atom
        (Expr::Atom(a), Type::Atom(expected_atom)) => {
            if a.name == expected_atom.name {
                Ok(())
            } else {
                Err(variant_mismatch(ctx, expr, expected, &format!("atom {}", a.name.as_str(db)), "atom name mismatch"))
            }
        }

        // Rule: Check-AtomVariant - an atom is a value of any enum listing it.
        (Expr::Atom(a), Type::Enum(expected_enum)) => {
            let found = expected_enum.variants.iter()
                .any(|v| v.name == a.name && v.payload.is_none());
            if found {
                Ok(())
            } else {
                Err(variant_mismatch(ctx, expr, expected, &format!("atom {}", a.name.as_str(db)), "atom is not a variant of this enum"))
            }
        }

        // Rule: Check-Term
        (Expr::Term(t), Type::Term(expected_term)) => {
            if t.name == expected_term.name {
                check(ctx, t.payload, &expected_term.payload)
            } else {
                Err(variant_mismatch(ctx, expr, expected, &format!("term {}", t.name.as_str(db)), "term name mismatch"))
            }
        }

        // Rule: Check-TermVariant - a term is a value of any enum listing it.
        (Expr::Term(t), Type::Enum(expected_enum)) => {
            let payload_ty = expected_enum.variants.iter()
                .find(|v| v.name == t.name)
                .and_then(|v| v.payload.as_deref());
            match payload_ty {
                Some(payload_ty) => check(ctx, t.payload, payload_ty),
                None => Err(variant_mismatch(ctx, expr, expected, &format!("term {}", t.name.as_str(db)), "term is not a variant of this enum")),
            }
        }

        // Rule: Check-Enum
        (Expr::Enum(e), Type::Enum(_)) => check(ctx, e.variant, expected),

        (Expr::Enum(_), _) => {
            if let Some(ts) = ctx.get_span(expr) {
                DiagnosticBuilder::error(db, "mismatched types")
                    .code("T058")
                    .primary_label(ts.clone(), &format!("expected `{}`, found enum literal",
                        type_to_string(db, expected)))
                    .emit_type();
            }
            Err(TypeError::TypeMismatch {
                expected: type_to_string(db, expected),
                actual: "enum literal".to_string(),
            })
        }

        // Rule: Check-Data - the payload has a type of its own, which is the
        // type the data carries.
        (Expr::Data(d), Type::Data) => synthesize(ctx, d.value).map(|_| ()),

        // Rule: Check-Error
        (Expr::Error(e), Type::Error) => synthesize(ctx, e.value).map(|_| ()),

        // Otherwise, try subsumption.
        _ => {
            let synthesized = synthesize_unhinted(ctx, expr)?;
            if types_equivalent(db, &synthesized, expected) {
                Ok(())
            } else {
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "mismatched types")
                        .code("T032")
                        .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                            type_to_string(db, expected),
                            type_to_string(db, &synthesized)))
                        .emit_type();
                }
                Err(TypeError::TypeMismatch {
                    expected: type_to_string(db, expected),
                    actual: type_to_string(db, &synthesized),
                })
            }
        }
    }
}

/// Report a `none` or `er` checked against a type it is no value of.
fn constructor_mismatch<'db>(
    ctx: &TypeContext<'db>,
    expr: ExprFull<'db>,
    expected: &Type<'db>,
    constructor: &str,
) -> TypeError {
    let db = ctx.db;
    let wants = if constructor == "none" { "an option" } else { "a result" };
    if let Some(ts) = ctx.get_span(expr) {
        DiagnosticBuilder::error(db, "mismatched types")
            .code("T032")
            .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                type_to_string(db, expected), constructor))
            .note(&format!("`{constructor}` is a value of {wants} type only"))
            .emit_type();
    }
    TypeError::TypeMismatch {
        expected: type_to_string(db, expected),
        actual: constructor.to_string(),
    }
}

/// Report an atom or term that is not the one the expected type names.
fn variant_mismatch<'db>(
    ctx: &TypeContext<'db>,
    expr: ExprFull<'db>,
    expected: &Type<'db>,
    actual: &str,
    note: &str,
) -> TypeError {
    let db = ctx.db;
    if let Some(ts) = ctx.get_span(expr) {
        DiagnosticBuilder::error(db, "mismatched types")
            .code("T057")
            .primary_label(ts.clone(), &format!("expected `{}`, found `{}`",
                type_to_string(db, expected), actual))
            .note(note)
            .emit_type();
    }
    TypeError::TypeMismatch {
        expected: type_to_string(db, expected),
        actual: actual.to_string(),
    }
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
