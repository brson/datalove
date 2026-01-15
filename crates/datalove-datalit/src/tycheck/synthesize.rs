//! Expression type synthesis.
//!
//! Provides synthesize for inferring types from expressions.
//!
//! Organization:
//! 1. Main synthesize function - entry point for type synthesis
//!    - Handles type hint propagation
//!    - Dispatches to expression-specific synthesis rules

use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use crate::ast::*;
use super::context::TypeContext;
use super::check::check;
use super::types::*;

// ============================================================================
// Type Synthesis
// ============================================================================

/// Synthesize a type for an expression.
pub fn synthesize<'db>(
    ctx: &mut TypeContext<'db>,
    expr: ExprFull<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let db = ctx.db;

    // Rule: Syn-TypedExpr - if type hint present, check against it.
    if let Some(type_hint_and_heap) = expr.type_hint(db) {
        let expected_type = convert_type_hint(db, type_hint_and_heap)?;
        check(ctx, expr, expected_type)?;
        return Ok(expected_type);
    }

    // Otherwise, synthesize from the expression.
    let expr_and_heap = expr.expr(db);
    let heap = expr_and_heap.heap;
    let expr_inner = &expr_and_heap.expr;

    let ty = match expr_inner {
        // Rule: Syn-Bool
        Expr::True | Expr::False => Type::Bool,

        // Rule: Syn-String
        Expr::String(_) => Type::String,

        // Rule: Syn-Int - default to u32 with range check.
        Expr::Int(i) => {
            let value_str = i.value.as_str(db);
            if value_str.parse::<u32>().is_ok() {
                Type::U32
            } else {
                // T001: Integer out of range.
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "integer literal out of range")
                        .code("T001")
                        .primary_label(ts.clone(), "value too large for u32")
                        .note("integer literals default to u32 type, which has a maximum value of 4,294,967,295")
                        .emit_type();
                }
                return Err(TypeError::IntOutOfRange);
            }
        }

        // Rule: Syn-Float - default to f32.
        Expr::Float(_) => Type::F32,

        // Rule: Syn-Hex - default to u32 (most common use case).
        Expr::Hex(h) => {
            let value_str = h.value.as_str(db);
            // Strip 0x/0X prefix and optional leading minus.
            let hex_part = value_str.trim_start_matches('-').trim_start_matches("0x").trim_start_matches("0X");
            if u32::from_str_radix(hex_part, 16).is_ok() && !value_str.starts_with('-') {
                Type::U32
            } else {
                // T001: Hex literal out of range for u32.
                if let Some(ts) = ctx.get_span(expr) {
                    DiagnosticBuilder::error(db, "hex literal out of range")
                        .code("T001")
                        .primary_label(ts.clone(), "value too large for u32")
                        .note("hex literals default to u32 type; use a type hint for other types")
                        .emit_type();
                }
                return Err(TypeError::IntOutOfRange);
            }
        }

        // Rule: Syn-AnonTuple - synthesize tuple by synthesizing each element.
        Expr::AnonTuple(t) => {
            let elements = &t.elements;
            let mut element_types = Vec::new();
            for elem in elements {
                let elem_type = synthesize(ctx, *elem)?;
                element_types.push(elem_type);
            }
            Type::AnonTuple(TypeAnonTuple { fields: element_types })
        }

        // Rule: Syn-AnonStruct - synthesize struct by synthesizing each field.
        Expr::AnonStruct(s) => {
            let fields = s.fields.clone();
            let mut field_types = Vec::new();
            for field in fields {
                let field_name = field.name;
                let field_value = field.value;
                let field_type = synthesize(ctx, field_value)?;
                field_types.push(TypeNamedField { name: field_name, ty: field_type });
            }
            Type::AnonStruct(TypeAnonStruct { fields: field_types })
        }

        // Rule: Syn-List - synthesize list by synthesizing all elements (must have same type).
        Expr::List(l) => {
            let elements = &l.elements;
            if elements.is_empty() {
                return Ok(empty_list_type(db, heap));
            }

            // Synthesize first element to get the expected type.
            let first_type = synthesize(ctx, elements[0])?;

            // Check remaining elements against first type.
            for elem in &elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if let Err(e) = check_element_compatible(db, first_type, elem_type) {
                    // Emit diagnostic with span info.
                    if let Some(ts) = ctx.get_span(*elem) {
                        match &e {
                            TypeError::TypeMismatch { expected, actual } => {
                                DiagnosticBuilder::error(db, "mismatched types in list")
                                    .code("T018")
                                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                    .note("all elements in a list must have the same type")
                                    .emit_type();
                            }
                            TypeError::HeapMismatch { expected_heap, actual_heap } => {
                                DiagnosticBuilder::error(db, "heap allocation mismatch in list")
                                    .code("T033")
                                    .primary_label(ts.clone(), &format!("expected {}, found {}", expected_heap, actual_heap))
                                    .note("all elements in a list must have compatible heap allocations")
                                    .emit_type();
                            }
                            _ => {}
                        }
                    }
                    return Err(e);
                }
            }

            Type::List(TypeList { element_type: first_type })
        }

        // Rule: Syn-Set - synthesize set by synthesizing all elements (must have same type).
        Expr::Set(s) => {
            let elements = s.elements.clone();
            if elements.is_empty() {
                return Ok(empty_set_type(db, heap));
            }

            // Synthesize first element to get the expected type.
            let first_type = synthesize(ctx, elements[0])?;

            // Check remaining elements against first type.
            for elem in &elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if let Err(e) = check_element_compatible(db, first_type, elem_type) {
                    // Emit diagnostic with span info.
                    if let Some(ts) = ctx.get_span(*elem) {
                        match &e {
                            TypeError::TypeMismatch { expected, actual } => {
                                DiagnosticBuilder::error(db, "mismatched types in set")
                                    .code("T019")
                                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                    .note("all elements in a set must have the same type")
                                    .emit_type();
                            }
                            TypeError::HeapMismatch { expected_heap, actual_heap } => {
                                DiagnosticBuilder::error(db, "heap allocation mismatch in set")
                                    .code("T034")
                                    .primary_label(ts.clone(), &format!("expected {}, found {}", expected_heap, actual_heap))
                                    .note("all elements in a set must have compatible heap allocations")
                                    .emit_type();
                            }
                            _ => {}
                        }
                    }
                    return Err(e);
                }
            }

            Type::Set(TypeSet { element_type: first_type })
        }

        // Rule: Syn-Map - synthesize map by synthesizing all keys and values (must have same types).
        Expr::Map(m) => {
            let entries = m.entries.clone();
            if entries.is_empty() {
                return Ok(empty_map_type(db, heap));
            }

            // Synthesize first entry to get the expected key and value types.
            let first_entry = &entries[0];
            let first_key_type = synthesize(ctx, first_entry.key)?;
            let first_value_type = synthesize(ctx, first_entry.value)?;

            // Check remaining entries against first types.
            for entry in &entries[1..] {
                let key_type = synthesize(ctx, entry.key)?;
                let value_type = synthesize(ctx, entry.value)?;

                // Check key compatibility.
                if let Err(e) = check_element_compatible(db, first_key_type, key_type) {
                    if let Some(ts) = ctx.get_span(entry.key) {
                        match &e {
                            TypeError::TypeMismatch { expected, actual } => {
                                DiagnosticBuilder::error(db, "mismatched key types in map")
                                    .code("T020")
                                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                    .note("all keys in a map must have the same type")
                                    .emit_type();
                            }
                            TypeError::HeapMismatch { expected_heap, actual_heap } => {
                                DiagnosticBuilder::error(db, "heap allocation mismatch in map keys")
                                    .code("T035")
                                    .primary_label(ts.clone(), &format!("expected {}, found {}", expected_heap, actual_heap))
                                    .note("all keys in a map must have compatible heap allocations")
                                    .emit_type();
                            }
                            _ => {}
                        }
                    }
                    return Err(e);
                }

                // Check value compatibility.
                if let Err(e) = check_element_compatible(db, first_value_type, value_type) {
                    if let Some(ts) = ctx.get_span(entry.value) {
                        match &e {
                            TypeError::TypeMismatch { expected, actual } => {
                                DiagnosticBuilder::error(db, "mismatched value types in map")
                                    .code("T021")
                                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                    .note("all values in a map must have the same type")
                                    .emit_type();
                            }
                            TypeError::HeapMismatch { expected_heap, actual_heap } => {
                                DiagnosticBuilder::error(db, "heap allocation mismatch in map values")
                                    .code("T036")
                                    .primary_label(ts.clone(), &format!("expected {}, found {}", expected_heap, actual_heap))
                                    .note("all values in a map must have compatible heap allocations")
                                    .emit_type();
                            }
                            _ => {}
                        }
                    }
                    return Err(e);
                }
            }

            Type::Map(TypeMap { key_type: first_key_type, value_type: first_value_type })
        }

        // Rule: Syn-Some - synthesize Option type by synthesizing inner.
        Expr::Some(s) => {
            let payload = s.payload;
            let inner_type = synthesize(ctx, payload)?;
            Type::Option(TypeOption { inner_type })
        }

        // Rule: Syn-Ok - synthesize Result type by synthesizing inner.
        Expr::Ok(o) => {
            let payload = o.payload;
            let inner_type = synthesize(ctx, payload)?;
            Type::Result(TypeResult { inner_type })
        }

        // Rule: Syn-Data - data values synthesize as Type::Data.
        Expr::Data(_) => Type::Data,

        // Rule: Syn-Error - error values synthesize as Type::Error.
        Expr::Error(_) => Type::Error,

        // Cannot synthesize for these - need type context.
        Expr::AnonEnum(_)
        | Expr::None
        | Expr::Er(_) => {
            // T016: Cannot synthesize type for anonymous enum, None, or Er.
            if let Some(ts) = ctx.get_span(expr) {
                let msg = match expr_and_heap.expr.clone() {
                    Expr::None => "cannot infer type for None value",
                    Expr::Er(_) => "cannot infer type for Er value",
                    _ => "cannot infer type for anonymous enum",
                };
                DiagnosticBuilder::error(db, msg)
                    .code("T016")
                    .primary_label(ts.clone(), "type annotation required")
                    .note("provide a type hint to specify the expected type")
                    .emit_type();
            }
            return Err(TypeError::CannotSynthesize);
        }

        // Rule: Syn-Tensor - synthesize tensor by synthesizing all elements (must have same type).
        Expr::Tensor(t) => {
            let shape = t.shape.clone();
            let elements = t.elements.clone();
            let rank = shape.len() as u32;

            if elements.is_empty() {
                return Ok(empty_tensor_type(db, heap, rank));
            }

            // Check element count matches shape product.
            if let Err(e) = check_tensor_element_count(&shape, elements.len()) {
                // T051: Tensor element count mismatch (synthesis mode).
                if let Some(ts) = ctx.get_span(expr) {
                    if let TypeError::ArityMismatch { expected, actual } = &e {
                        DiagnosticBuilder::error(db, "tensor has wrong number of elements")
                            .code("T051")
                            .primary_label(ts.clone(), &format!("expected {} element(s), found {}",
                                expected, actual))
                            .emit_type();
                    }
                }
                return Err(e);
            }

            // Synthesize first element to get the expected type.
            let first_type = synthesize(ctx, elements[0])?;

            // Check remaining elements against first type.
            for elem in &elements[1..] {
                let elem_type = synthesize(ctx, *elem)?;
                if let Err(e) = check_element_compatible(db, first_type, elem_type) {
                    // Emit diagnostic with span info.
                    if let Some(ts) = ctx.get_span(*elem) {
                        match &e {
                            TypeError::TypeMismatch { expected, actual } => {
                                DiagnosticBuilder::error(db, "mismatched types in tensor")
                                    .code("T052")
                                    .primary_label(ts.clone(), &format!("expected `{}`, found `{}`", expected, actual))
                                    .note("all elements in a tensor must have the same type")
                                    .emit_type();
                            }
                            TypeError::HeapMismatch { expected_heap, actual_heap } => {
                                DiagnosticBuilder::error(db, "heap allocation mismatch in tensor")
                                    .code("T053")
                                    .primary_label(ts.clone(), &format!("expected {}, found {}", expected_heap, actual_heap))
                                    .note("all elements in a tensor must have compatible heap allocations")
                                    .emit_type();
                            }
                            _ => {}
                        }
                    }
                    return Err(e);
                }
            }

            Type::Tensor(TypeTensor { element_type: first_type, rank })
        }

        Expr::ParseError(_) => {
            // T017: Cannot synthesize type for parse error.
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

    Ok(TypeAndHeap::new(db, heap, ty))
}
