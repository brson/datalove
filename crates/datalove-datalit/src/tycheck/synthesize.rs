//! Expression type synthesis.
//!
//! Provides synthesize for inferring types from expressions.

use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use crate::ast::*;
use super::context::TypeContext;
use super::check::check;
use super::types::*;

/// Synthesize a type for an expression.
pub fn synthesize<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;

    // Rule: Syn-TypedExpr - if type hint present, check against it.
    if let Some(type_hint) = expr.type_hint(db) {
        let expected_type = convert_type_hint(db, &type_hint)?;
        check(ctx, expr, &expected_type)?;
        return Ok(expected_type);
    }

    // Otherwise, synthesize from the expression.
    let expr_inner = expr.expr(db);

    let ty = match &expr_inner {
        // Rule: Syn-Bool
        Expr::True | Expr::False => Type::Bool,

        // Rule: Syn-String
        Expr::String(_) => Type::String,

        // Rule: Syn-Int - default to int (bigint).
        Expr::Int(_) => Type::Int,

        // Rule: Syn-Float - default to f32.
        Expr::Float(_) => Type::F32,

        // Rule: Syn-Hex - default to int (bigint).
        Expr::Hex(_) => Type::Int,

        // Rule: Syn-AnonTuple - synthesize tuple by synthesizing each element.
        Expr::AnonTuple(t) => {
            let mut element_types = Vec::new();
            for elem in &t.elements {
                let elem_type = synthesize(ctx, *elem)?;
                element_types.push(elem_type);
            }
            Type::AnonTuple(TypeAnonTuple { fields: element_types })
        }

        // Rule: Syn-AnonStruct - synthesize struct by synthesizing each field.
        Expr::AnonStruct(s) => {
            let mut field_types = Vec::new();
            for field in &s.fields {
                let field_type = synthesize(ctx, field.value)?;
                field_types.push(TypeNamedField { name: field.name, ty: Box::new(field_type) });
            }
            Type::AnonStruct(TypeAnonStruct { fields: field_types })
        }

        // Rule: Syn-List - synthesize list by synthesizing all elements.
        Expr::List(l) => {
            if l.elements.is_empty() {
                return Ok(empty_list_type());
            }

            let first_type = synthesize(ctx, l.elements[0])?;

            for elem in &l.elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if let Err(e) = check_element_compatible(db, &first_type, &elem_type) {
                    if let Some(ts) = ctx.get_span(*elem) {
                        if let TypeError::TypeMismatch { expected, actual } = &e {
                            DiagnosticBuilder::error(db, "mismatched types in list")
                                .code("T018")
                                .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                .note("all elements in a list must have the same type")
                                .emit_type();
                        }
                    }
                    return Err(e);
                }
            }

            Type::List(TypeList { element_type: Box::new(first_type) })
        }

        // Rule: Syn-Set
        Expr::Set(s) => {
            if s.elements.is_empty() {
                return Ok(empty_set_type());
            }

            let first_type = synthesize(ctx, s.elements[0])?;

            for elem in &s.elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if let Err(e) = check_element_compatible(db, &first_type, &elem_type) {
                    if let Some(ts) = ctx.get_span(*elem) {
                        if let TypeError::TypeMismatch { expected, actual } = &e {
                            DiagnosticBuilder::error(db, "mismatched types in set")
                                .code("T019")
                                .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                .note("all elements in a set must have the same type")
                                .emit_type();
                        }
                    }
                    return Err(e);
                }
            }

            Type::Set(TypeSet { element_type: Box::new(first_type) })
        }

        // Rule: Syn-Map
        Expr::Map(m) => {
            if m.entries.is_empty() {
                return Ok(empty_map_type());
            }

            let first_entry = &m.entries[0];
            let first_key_type = synthesize(ctx, first_entry.key)?;
            let first_value_type = synthesize(ctx, first_entry.value)?;

            for entry in &m.entries[1..] {
                let key_type = synthesize(ctx, entry.key)?;
                let value_type = synthesize(ctx, entry.value)?;

                if let Err(e) = check_element_compatible(db, &first_key_type, &key_type) {
                    if let Some(ts) = ctx.get_span(entry.key) {
                        if let TypeError::TypeMismatch { expected, actual } = &e {
                            DiagnosticBuilder::error(db, "mismatched key types in map")
                                .code("T020")
                                .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                .note("all keys in a map must have the same type")
                                .emit_type();
                        }
                    }
                    return Err(e);
                }

                if let Err(e) = check_element_compatible(db, &first_value_type, &value_type) {
                    if let Some(ts) = ctx.get_span(entry.value) {
                        if let TypeError::TypeMismatch { expected, actual } = &e {
                            DiagnosticBuilder::error(db, "mismatched value types in map")
                                .code("T021")
                                .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                .note("all values in a map must have the same type")
                                .emit_type();
                        }
                    }
                    return Err(e);
                }
            }

            Type::Map(TypeMap { key_type: Box::new(first_key_type), value_type: Box::new(first_value_type) })
        }

        // Rule: Syn-Some
        Expr::Some(s) => {
            let inner_type = synthesize(ctx, s.payload)?;
            Type::Option(TypeOption { inner_type: Box::new(inner_type) })
        }

        // Rule: Syn-Ok
        Expr::Ok(o) => {
            let inner_type = synthesize(ctx, o.payload)?;
            Type::Result(TypeResult { inner_type: Box::new(inner_type) })
        }

        // Rule: Syn-Data
        Expr::Data(_) => Type::Data,

        // Rule: Syn-Error
        Expr::Error(_) => Type::Error,

        // Cannot synthesize for these - need type context.
        Expr::None | Expr::Er(_) => {
            if let Some(ts) = ctx.get_span(expr) {
                let msg = match &expr_inner {
                    Expr::None => "cannot infer type for None value",
                    Expr::Er(_) => "cannot infer type for Er value",
                    _ => "cannot infer type",
                };
                DiagnosticBuilder::error(db, msg)
                    .code("T016")
                    .primary_label(ts.clone(), "type annotation required")
                    .note("provide a type hint to specify the expected type")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }

        // Rule: Syn-Tensor
        Expr::Tensor(t) => {
            let rank = t.shape.len() as u32;

            if t.elements.is_empty() {
                return Ok(empty_tensor_type(rank));
            }

            if let Err(e) = check_tensor_element_count(&t.shape, t.elements.len()) {
                if let Some(ts) = ctx.get_span(expr) {
                    if let TypeError::ArityMismatch { expected, actual } = &e {
                        DiagnosticBuilder::error(db, "tensor has wrong number of elements")
                            .code("T051")
                            .primary_label(ts.clone(), &format!("expected {} element(s), found {}", expected, actual))
                            .emit_type();
                    }
                }
                return Err(e);
            }

            let first_type = synthesize(ctx, t.elements[0])?;

            for elem in &t.elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if let Err(e) = check_element_compatible(db, &first_type, &elem_type) {
                    if let Some(ts) = ctx.get_span(*elem) {
                        if let TypeError::TypeMismatch { expected, actual } = &e {
                            DiagnosticBuilder::error(db, "mismatched types in tensor")
                                .code("T052")
                                .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                .note("all elements in a tensor must have the same type")
                                .emit_type();
                        }
                    }
                    return Err(e);
                }
            }

            Type::Tensor(TypeTensor { element_type: Box::new(first_type), rank })
        }

        Expr::Table(_) => {
            if let Some(ts) = ctx.get_span(expr) {
                DiagnosticBuilder::error(db, "cannot synthesize type for table expression")
                    .code("T018")
                    .primary_label(ts.clone(), "table requires type hint")
                    .note("use a type hint to specify the table schema")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }

        Expr::ParseError(_) => {
            if let Some(ts) = ctx.get_span(expr) {
                DiagnosticBuilder::error(db, "cannot type-check expression with parse errors")
                    .code("T017")
                    .primary_label(ts.clone(), "parse error occurred here")
                    .note("fix the parse error before type checking")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }
    };

    Ok(ty)
}
