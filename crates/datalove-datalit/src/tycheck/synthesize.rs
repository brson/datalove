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
        let expected_type = ctx.convert_hint(expr, &type_hint)?;
        check(ctx, expr, &expected_type)?;
        return Ok(expected_type);
    }
    synthesize_unhinted(ctx, expr)
}

/// Synthesize a type for an expression from what it is, leaving aside any
/// hint written on it.
///
/// Checking reaches for this when it has to know what an expression is by
/// itself, having already weighed the hint against what was expected.
pub fn synthesize_unhinted<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
) -> Result<Type<'db>, TypeError> {
    let db = ctx.db;
    let expr_inner = expr.expr(db);

    let ty = match &expr_inner {
        // Rule: Syn-Bool
        Expr::True | Expr::False => Type::Bool,

        // Rule: Syn-String
        Expr::String(_) => Type::String,

        // Rule: Syn-Int - default to int (bigint).
        Expr::Int(_) => Type::Int,

        // Rule: Syn-Float - default to f64.
        Expr::Float(_) => Type::F64,

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

        // Rule: Syn-Data - the payload has a type of its own, which is the
        // type the data carries.
        Expr::Data(d) => {
            synthesize(ctx, d.value)?;
            Type::Data
        }

        // Rule: Syn-Error
        Expr::Error(e) => {
            synthesize(ctx, e.value)?;
            Type::Error
        }

        // Rule: Syn-Group
        Expr::Group(g) => synthesize(ctx, g.inner)?,

        // Rule: Syn-Atom
        Expr::Atom(a) => Type::Atom(TypeAtom { name: a.name }),

        // Rule: Syn-Term
        Expr::Term(t) => {
            let payload = synthesize(ctx, t.payload)?;
            Type::Term(TypeTerm { name: t.name, payload: Box::new(payload) })
        }

        // Cannot synthesize for these - need type context.
        Expr::None | Expr::Er(_) | Expr::Enum(_) => {
            if let Some(ts) = ctx.get_span(expr) {
                let msg = match &expr_inner {
                    Expr::None => "cannot infer type for None value",
                    Expr::Er(_) => "cannot infer type for Er value",
                    Expr::Enum(_) => "cannot infer type for enum literal",
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

        // Rule: Syn-Table
        //
        // Each column takes the type of its element in the first row, and
        // every later row has to agree, as a list's elements agree with its
        // first. A table with no rows has unit columns, as an empty list has
        // a unit element.
        Expr::Table(t) => {
            let columns = t.header.len();
            for (row_idx, row) in t.rows.iter().enumerate() {
                if row.elements.len() != columns {
                    if let Some(ts) = ctx.get_span(expr) {
                        DiagnosticBuilder::error(db, "table row has wrong number of columns")
                            .code("T056")
                            .primary_label(ts.clone(), &format!("row {}: expected {} column(s), found {}",
                                row_idx + 1, columns, row.elements.len()))
                            .emit_type();
                    }
                    return Err(TypeError::ArityMismatch { expected: columns, actual: row.elements.len() });
                }
            }

            let column_types = match t.rows.first() {
                Some(first) => first.elements.iter()
                    .map(|elem| synthesize(ctx, *elem))
                    .collect::<Result<Vec<_>, _>>()?,
                None => vec![unit_type(); columns],
            };

            for row in t.rows.iter().skip(1) {
                for ((elem, column_type), name) in row.elements.iter().zip(&column_types).zip(&t.header) {
                    let elem_type = synthesize(ctx, *elem)?;
                    if let Err(e) = check_element_compatible(db, column_type, &elem_type) {
                        if let Some(ts) = ctx.get_span(*elem) {
                            if let TypeError::TypeMismatch { expected, actual } = &e {
                                DiagnosticBuilder::error(db, "mismatched types in table column")
                                    .code("T060")
                                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                    .note(&format!("every element of column `{}` has the type of its first row's", name.as_str(db)))
                                    .emit_type();
                            }
                        }
                        return Err(e);
                    }
                }
            }

            Type::Table(TypeTable {
                columns: t.header.iter().zip(column_types)
                    .map(|(name, ty)| TypeNamedField { name: *name, ty: Box::new(ty) })
                    .collect(),
            })
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
