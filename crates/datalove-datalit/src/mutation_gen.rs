//! Mutation-based error generation for testing error equivalence.
//!
//! Generates erroneous expressions by mutating valid ASTs or their pretty-printed source.
//! Used to verify both datalit and datafun parsers/typecheckers produce equivalent errors.

use rmx::prelude::*;
use bct::text::InternedText;
use rand::{Rng, SeedableRng};

use crate::ast::*;
use crate::ast_gen::{AstGenConfig, gen_expr_full_seeded};
use crate::pretty::pretty_print;

/// Result of applying a mutation.
#[derive(Clone, Debug)]
pub struct MutationResult {
    /// The mutated source text.
    pub source: String,
    /// Expected error codes (D0XX for parse errors, T0XX for type errors).
    pub expected_errors: Vec<&'static str>,
    /// Description of what mutation was applied.
    pub description: String,
}

/// Categories of mutations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MutationKind {
    /// Source-level mutations (parse errors).
    Source,
    /// AST-level mutations (type errors).
    Ast,
}

/// Mutations that can be applied to generate errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutation {
    // Source-level mutations (parse errors).

    /// Remove @ or # heap sigil from source.
    DeleteHeapSigil,
    /// Remove a bracket pair: (), {}, [], or <>.
    DeleteOpeningBracket,
    /// Cut off source mid-expression.
    TruncateSource,
    /// Remove comma between collection elements.
    DeleteComma,
    /// Add extra closing bracket.
    ExtraClosingBracket,

    // AST-level mutations (type errors).

    /// Replace integer with value outside type's range.
    OutOfRangeInt,
    /// Insert wrong-typed element in list/set/map.
    WrongElementType,
    /// Change one element's heap from @ to # or vice versa.
    HeapMismatch,
    /// Add or remove tuple/struct field to cause arity mismatch.
    ArityMismatch,
    /// Remove type hint from None/empty collection.
    RemoveTypeHint,
    /// Change enum variant to nonexistent name.
    WrongVariant,
}

impl Mutation {
    /// Get all mutations.
    pub fn all() -> &'static [Mutation] {
        &[
            Mutation::DeleteHeapSigil,
            Mutation::DeleteOpeningBracket,
            Mutation::TruncateSource,
            Mutation::DeleteComma,
            Mutation::ExtraClosingBracket,
            Mutation::OutOfRangeInt,
            Mutation::WrongElementType,
            Mutation::HeapMismatch,
            Mutation::ArityMismatch,
            Mutation::RemoveTypeHint,
            Mutation::WrongVariant,
        ]
    }

    /// Get all source-level mutations.
    pub fn source_mutations() -> &'static [Mutation] {
        &[
            Mutation::DeleteHeapSigil,
            Mutation::DeleteOpeningBracket,
            Mutation::TruncateSource,
            Mutation::DeleteComma,
            Mutation::ExtraClosingBracket,
        ]
    }

    /// Get all AST-level mutations.
    pub fn ast_mutations() -> &'static [Mutation] {
        &[
            Mutation::OutOfRangeInt,
            Mutation::WrongElementType,
            Mutation::HeapMismatch,
            Mutation::ArityMismatch,
            Mutation::RemoveTypeHint,
            Mutation::WrongVariant,
        ]
    }

    /// Get the kind of this mutation.
    pub fn kind(&self) -> MutationKind {
        match self {
            Mutation::DeleteHeapSigil
            | Mutation::DeleteOpeningBracket
            | Mutation::TruncateSource
            | Mutation::DeleteComma
            | Mutation::ExtraClosingBracket => MutationKind::Source,

            Mutation::OutOfRangeInt
            | Mutation::WrongElementType
            | Mutation::HeapMismatch
            | Mutation::ArityMismatch
            | Mutation::RemoveTypeHint
            | Mutation::WrongVariant => MutationKind::Ast,
        }
    }

    /// Apply this mutation to generate an erroneous expression.
    ///
    /// Returns None if the mutation is not applicable to the given expression.
    pub fn apply<'db>(
        &self,
        db: &'db dyn salsa::Database,
        expr: ExprFull<'db>,
        rng: &mut impl Rng,
    ) -> Option<MutationResult> {
        let source = pretty_print(db, expr);

        match self {
            Mutation::DeleteHeapSigil => apply_delete_heap_sigil(&source, rng),
            Mutation::DeleteOpeningBracket => apply_delete_opening_bracket(&source, rng),
            Mutation::TruncateSource => apply_truncate_source(&source, rng),
            Mutation::DeleteComma => apply_delete_comma(&source, rng),
            Mutation::ExtraClosingBracket => apply_extra_closing_bracket(&source, rng),
            Mutation::OutOfRangeInt => apply_out_of_range_int(db, expr, rng),
            Mutation::WrongElementType => apply_wrong_element_type(db, expr, rng),
            Mutation::HeapMismatch => apply_heap_mismatch(db, expr, rng),
            Mutation::ArityMismatch => apply_arity_mismatch(db, expr, rng),
            Mutation::RemoveTypeHint => apply_remove_type_hint(db, expr),
            Mutation::WrongVariant => apply_wrong_variant(db, expr, rng),
        }
    }
}

// ============================================================================
// Source-level mutation implementations
// ============================================================================

/// Delete a heap sigil (@ or #) from the source.
fn apply_delete_heap_sigil(source: &str, rng: &mut impl Rng) -> Option<MutationResult> {
    let sigil_positions: Vec<usize> = source
        .char_indices()
        .filter(|(_, c)| *c == '@' || *c == '#')
        .map(|(i, _)| i)
        .collect();

    if sigil_positions.is_empty() {
        return None;
    }

    let pos = sigil_positions[rng.gen_range(0..sigil_positions.len())];
    let mut result = source.to_string();
    result.remove(pos);

    Some(MutationResult {
        source: result,
        expected_errors: vec![], // Parse errors vary by context.
        description: format!("Deleted heap sigil at position {}", pos),
    })
}

/// Delete an opening bracket from the source.
fn apply_delete_opening_bracket(source: &str, rng: &mut impl Rng) -> Option<MutationResult> {
    let bracket_positions: Vec<(usize, char)> = source
        .char_indices()
        .filter(|(_, c)| *c == '(' || *c == '[' || *c == '{' || *c == '<')
        .collect();

    if bracket_positions.is_empty() {
        return None;
    }

    let (pos, bracket) = bracket_positions[rng.gen_range(0..bracket_positions.len())];
    let mut result = source.to_string();
    result.remove(pos);

    Some(MutationResult {
        source: result,
        expected_errors: vec![], // Parse errors vary by context.
        description: format!("Deleted opening bracket '{}' at position {}", bracket, pos),
    })
}

/// Truncate source mid-expression.
fn apply_truncate_source(source: &str, rng: &mut impl Rng) -> Option<MutationResult> {
    if source.len() <= 3 {
        return None;
    }

    // Truncate at a position between 1/4 and 3/4 of the source.
    let min_pos = source.len() / 4;
    let max_pos = 3 * source.len() / 4;
    if min_pos >= max_pos {
        return None;
    }

    let pos = rng.gen_range(min_pos..max_pos);

    // Find a valid UTF-8 boundary.
    let truncate_pos = source[..pos]
        .char_indices()
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(pos);

    let result = source[..truncate_pos].to_string();

    Some(MutationResult {
        source: result,
        expected_errors: vec!["D020"], // Unexpected end of input.
        description: format!("Truncated source at position {}", truncate_pos),
    })
}

/// Delete a comma between collection elements.
fn apply_delete_comma(source: &str, rng: &mut impl Rng) -> Option<MutationResult> {
    let comma_positions: Vec<usize> = source
        .char_indices()
        .filter(|(_, c)| *c == ',')
        .map(|(i, _)| i)
        .collect();

    if comma_positions.is_empty() {
        return None;
    }

    let pos = comma_positions[rng.gen_range(0..comma_positions.len())];
    let mut result = source.to_string();
    result.remove(pos);

    Some(MutationResult {
        source: result,
        expected_errors: vec![], // Parse errors vary by context.
        description: format!("Deleted comma at position {}", pos),
    })
}

/// Add extra closing bracket.
fn apply_extra_closing_bracket(source: &str, rng: &mut impl Rng) -> Option<MutationResult> {
    let brackets = [')', ']', '}', '>'];
    let bracket = brackets[rng.gen_range(0..brackets.len())];

    // Insert at a random position (preferring middle of source).
    let pos = rng.gen_range(source.len() / 4..3 * source.len() / 4).min(source.len());

    let mut result = source.to_string();
    result.insert(pos, bracket);

    Some(MutationResult {
        source: result,
        expected_errors: vec![], // Parse errors vary by context.
        description: format!("Inserted extra '{}' at position {}", bracket, pos),
    })
}

// ============================================================================
// AST-level mutation implementations
// ============================================================================

/// Replace an integer with a value outside the type's range.
fn apply_out_of_range_int<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
    rng: &mut impl Rng,
) -> Option<MutationResult> {
    // Check if the expression has a type hint for a fixed-width integer type.
    let type_hint = expr.type_hint(db)?;
    let th = type_hint.type_hint(db);

    // Determine the out-of-range value based on type.
    let (out_of_range_value, error_code) = match th {
        TypeHint::U8 => ("256", "T005"),
        TypeHint::I8 => ("128", "T006"),
        TypeHint::U16 => ("65536", "T007"),
        TypeHint::I16 => ("32768", "T008"),
        TypeHint::U32 => ("4294967296", "T009"),
        TypeHint::I32 => ("2147483648", "T010"),
        TypeHint::U64 => ("18446744073709551616", "T011"),
        TypeHint::I64 => ("9223372036854775808", "T012"),
        _ => return None, // Not a fixed-width integer type.
    };

    // Check if the expression is an integer literal.
    let expr_and_heap = expr.expr(db);
    match expr_and_heap.expr(db) {
        Expr::Int(_) | Expr::Hex(_) => {}
        _ => return None,
    }

    // Build mutated source: keep type hint, replace value.
    let heap = expr_and_heap.heap(db);
    let heap_str = match heap {
        Heap::Local => "@",
        Heap::Global => "#",
        Heap::Omitted => "",
    };

    let mut type_hint_str = String::new();
    type_hint_str.push_str(": ");
    pretty_type_hint_and_heap(db, type_hint, &mut type_hint_str);
    type_hint_str.push_str(" / ");
    type_hint_str.push_str(heap_str);
    type_hint_str.push_str(out_of_range_value);

    Some(MutationResult {
        source: type_hint_str,
        expected_errors: vec![error_code],
        description: format!("Replaced integer with out-of-range value {}", out_of_range_value),
    })
}

/// Insert a wrong-typed element in a collection.
///
/// Uses source-level string manipulation to avoid salsa tracked function issues.
fn apply_wrong_element_type<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
    _rng: &mut impl Rng,
) -> Option<MutationResult> {
    let expr_and_heap = expr.expr(db);
    let outer_heap = expr_and_heap.heap(db);

    // Check if this is a list with at least one element.
    if let Expr::List(list) = expr_and_heap.expr(db) {
        let elements = list.elements(db);
        if elements.is_empty() {
            return None;
        }

        // Get the type of first element to determine what's "wrong".
        let first_elem = elements[0];
        let first_heap = first_elem.expr(db).heap(db);
        let first_expr = first_elem.expr(db).expr(db);

        // Determine the wrong element to insert based on first element's type.
        let wrong_elem_str = match first_expr {
            Expr::Int(_) | Expr::Hex(_) | Expr::Float(_) => {
                // Add a string where number expected.
                let heap_str = match first_heap {
                    Heap::Local => "@",
                    Heap::Global => "#",
                    Heap::Omitted => "",
                };
                format!("{}\"wrong\"", heap_str)
            }
            Expr::String(_) => {
                // Add an int where string expected.
                let heap_str = match first_heap {
                    Heap::Local => "@",
                    Heap::Global => "#",
                    Heap::Omitted => "",
                };
                format!("{}42", heap_str)
            }
            Expr::True | Expr::False => {
                // Add an int where bool expected.
                let heap_str = match first_heap {
                    Heap::Local => "@",
                    Heap::Global => "#",
                    Heap::Omitted => "",
                };
                format!("{}42", heap_str)
            }
            _ => return None, // Complex nested type, skip.
        };

        // Build list source via string manipulation.
        let elem_strs: Vec<String> = elements.iter().map(|e| pretty_print(db, *e)).collect();

        // Build outer heap prefix.
        let outer_heap_str = match outer_heap {
            Heap::Local => "@",
            Heap::Global => "#",
            Heap::Omitted => "",
        };

        // Build final source with wrong element appended.
        // Use prefix type hint format: ": type / expr"
        let mut all_elems = elem_strs;
        all_elems.push(wrong_elem_str);
        let list_body = format!("{}[{}]", outer_heap_str, all_elems.join(", "));

        let source = if let Some(th) = expr.type_hint(db) {
            let mut type_str = String::new();
            pretty_type_hint_and_heap(db, th, &mut type_str);
            format!(": {} / {}", type_str, list_body)
        } else {
            list_body
        };

        Some(MutationResult {
            source,
            expected_errors: vec!["T018"], // List element type mismatch.
            description: "Inserted wrong-typed element in list".to_string(),
        })
    } else {
        None
    }
}

/// Change one element's heap from @ to # or vice versa.
///
/// Uses source-level string manipulation to avoid salsa tracked function issues.
fn apply_heap_mismatch<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
    _rng: &mut impl Rng,
) -> Option<MutationResult> {
    let expr_and_heap = expr.expr(db);
    let outer_heap = expr_and_heap.heap(db);

    // Check if this is a list with at least 2 elements.
    if let Expr::List(list) = expr_and_heap.expr(db) {
        let elements = list.elements(db);
        if elements.len() < 2 {
            return None;
        }

        // Get first element's heap to determine what to flip to.
        let first_heap = elements[0].expr(db).heap(db);
        let new_heap = match first_heap {
            Heap::Local => Heap::Global,
            Heap::Global => Heap::Local,
            Heap::Omitted => Heap::Local, // Change omitted to explicit.
        };
        let new_heap_char = match new_heap {
            Heap::Local => '@',
            Heap::Global => '#',
            Heap::Omitted => return None, // Can't set to omitted.
        };

        // Build list source via string manipulation.
        let mut elem_strs: Vec<String> = elements.iter().map(|e| pretty_print(db, *e)).collect();

        // Modify the last element's heap.
        // Format is: [: type / @value] or [@value] (without type hint).
        // The heap sigil is either after " / " (with type hint) or at the start (without).
        let last_idx = elem_strs.len() - 1;
        let last_str = &elem_strs[last_idx];

        let modified = if let Some(sep_idx) = last_str.find(" / ") {
            // Has type hint: modify the heap after " / ".
            let value_start = sep_idx + 3; // Skip " / ".
            let value_part = &last_str[value_start..];
            let stripped_value = if value_part.starts_with('@') || value_part.starts_with('#') {
                &value_part[1..]
            } else {
                value_part
            };
            format!("{}{}{}", &last_str[..value_start], new_heap_char, stripped_value)
        } else {
            // No type hint: modify the heap at start.
            let stripped = if last_str.starts_with('@') || last_str.starts_with('#') {
                &last_str[1..]
            } else {
                last_str.as_str()
            };
            format!("{}{}", new_heap_char, stripped)
        };
        elem_strs[last_idx] = modified;

        // Build outer heap prefix.
        let outer_heap_str = match outer_heap {
            Heap::Local => "@",
            Heap::Global => "#",
            Heap::Omitted => "",
        };

        // Build type hint if present.
        let type_hint_str = if let Some(th) = expr.type_hint(db) {
            let mut s = String::new();
            s.push_str("<");
            pretty_type_hint_and_heap(db, th, &mut s);
            s.push_str(">");
            s
        } else {
            String::new()
        };

        let source = format!("{}[{}]{}", outer_heap_str, elem_strs.join(", "), type_hint_str);

        Some(MutationResult {
            source,
            expected_errors: vec!["T033"], // List element heap mismatch.
            description: "Changed element heap to cause mismatch".to_string(),
        })
    } else {
        None
    }
}

/// Add or remove tuple/struct field to cause arity mismatch.
///
/// Uses source-level string manipulation to avoid salsa tracked function issues.
fn apply_arity_mismatch<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
    _rng: &mut impl Rng,
) -> Option<MutationResult> {
    let type_hint = expr.type_hint(db)?;
    let expr_and_heap = expr.expr(db);
    let heap = expr_and_heap.heap(db);

    let heap_str = match heap {
        Heap::Local => "@",
        Heap::Global => "#",
        Heap::Omitted => "",
    };

    match (type_hint.type_hint(db), expr_and_heap.expr(db)) {
        (TypeHint::AnonTuple(_th), Expr::AnonTuple(t)) => {
            let elements = t.elements(db);
            if elements.len() < 2 {
                // Need at least 2 elements to remove one and still have a tuple.
                return None;
            }

            // Pretty-print elements, then remove last one.
            let elem_strs: Vec<String> = elements[..elements.len() - 1]
                .iter()
                .map(|e| pretty_print(db, *e))
                .collect();

            // Build type hint string.
            let mut type_str = String::new();
            pretty_type_hint_and_heap(db, type_hint, &mut type_str);

            // Build tuple with fewer elements: `: type / @(elem1, elem2)`
            let source = format!(": {} / {}({})", type_str, heap_str, elem_strs.join(", "));

            Some(MutationResult {
                source,
                expected_errors: vec!["T038"], // Tuple arity mismatch.
                description: "Removed tuple element to cause arity mismatch".to_string(),
            })
        }
        (TypeHint::AnonStruct(_th), Expr::AnonStruct(s)) => {
            let fields = s.fields(db);
            if fields.len() < 2 {
                // Need at least 2 fields to remove one.
                return None;
            }

            // Pretty-print fields, then remove last one.
            let field_strs: Vec<String> = fields[..fields.len() - 1]
                .iter()
                .map(|f| {
                    let name = f.name(db).as_str(db);
                    let value = pretty_print(db, f.value(db));
                    format!("{}: {}", name, value)
                })
                .collect();

            // Build type hint string.
            let mut type_str = String::new();
            pretty_type_hint_and_heap(db, type_hint, &mut type_str);

            // Build struct with fewer fields: `: type / @{field1: v1, field2: v2}`
            let source = format!(": {} / {}{{{}}}", type_str, heap_str, field_strs.join(", "));

            Some(MutationResult {
                source,
                expected_errors: vec!["T039"], // Struct missing field.
                description: "Removed struct field to cause arity mismatch".to_string(),
            })
        }
        _ => None,
    }
}

/// Remove type hint from None/empty collection.
fn apply_remove_type_hint<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
) -> Option<MutationResult> {
    let _type_hint = expr.type_hint(db)?;
    let expr_and_heap = expr.expr(db);
    let heap = expr_and_heap.heap(db);

    // Check if this is an expression that requires a type hint.
    let needs_hint = match expr_and_heap.expr(db) {
        Expr::None => true,
        Expr::AnonEnum(_) => true,
        Expr::List(l) if l.elements(db).is_empty() => true,
        Expr::Set(s) if s.elements(db).is_empty() => true,
        Expr::Map(m) if m.entries(db).is_empty() => true,
        _ => false,
    };

    if !needs_hint {
        return None;
    }

    // Create expression without type hint.
    let new_expr = ExprFull::new(db, None, *expr_and_heap);
    let source = pretty_print(db, new_expr);

    let error_code = match expr_and_heap.expr(db) {
        Expr::None => "T016", // Cannot synthesize type for None.
        Expr::AnonEnum(_) => "T016", // Cannot synthesize type for anonymous enum.
        Expr::List(_) => "T013", // Cannot synthesize type for empty list.
        Expr::Set(_) => "T014", // Cannot synthesize type for empty set.
        Expr::Map(_) => "T015", // Cannot synthesize type for empty map.
        _ => "T016",
    };

    Some(MutationResult {
        source,
        expected_errors: vec![error_code],
        description: "Removed type hint from expression that requires one".to_string(),
    })
}

/// Change enum variant to nonexistent name.
fn apply_wrong_variant<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
    _rng: &mut impl Rng,
) -> Option<MutationResult> {
    let type_hint = expr.type_hint(db)?;
    let expr_and_heap = expr.expr(db);
    let heap = expr_and_heap.heap(db);

    match expr_and_heap.expr(db) {
        Expr::AnonEnum(e) => {
            // Create enum with nonexistent variant name.
            let wrong_name = InternedText::new(db, "NonexistentVariant12345");
            let new_enum = ExprAnonEnum::new(db, wrong_name, e.payload(db));
            let new_expr = ExprFull::new(
                db,
                Some(type_hint),
                ExprAndHeap::new(db, heap, Expr::AnonEnum(new_enum)),
            );

            let source = pretty_print(db, new_expr);

            Some(MutationResult {
                source,
                expected_errors: vec!["T044"], // Variant not found.
                description: "Changed enum variant to nonexistent name".to_string(),
            })
        }
        Expr::NamedEnum(e) => {
            // Create enum with nonexistent variant name.
            let wrong_name = InternedText::new(db, "NonexistentVariant12345");
            let new_enum = ExprNamedEnum::new(db, e.enum_name(db), wrong_name, e.payload(db));
            let new_expr = ExprFull::new(
                db,
                Some(type_hint),
                ExprAndHeap::new(db, heap, Expr::NamedEnum(new_enum)),
            );

            let source = pretty_print(db, new_expr);

            Some(MutationResult {
                source,
                expected_errors: vec!["T046"], // Variant not found (named enum).
                description: "Changed named enum variant to nonexistent name".to_string(),
            })
        }
        _ => None,
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Pretty-print a type hint with heap.
fn pretty_type_hint_and_heap<'db>(
    db: &'db dyn salsa::Database,
    th: TypeHintAndHeap<'db>,
    out: &mut String,
) {
    match th.heap(db) {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }

    pretty_type_hint(db, th.type_hint(db), out);
}

/// Pretty-print a type hint.
fn pretty_type_hint<'db>(
    db: &'db dyn salsa::Database,
    th: TypeHint<'db>,
    out: &mut String,
) {
    match th {
        TypeHint::Bool => out.push_str("bool"),
        TypeHint::U8 => out.push_str("u8"),
        TypeHint::I8 => out.push_str("i8"),
        TypeHint::U16 => out.push_str("u16"),
        TypeHint::I16 => out.push_str("i16"),
        TypeHint::U32 => out.push_str("u32"),
        TypeHint::I32 => out.push_str("i32"),
        TypeHint::U64 => out.push_str("u64"),
        TypeHint::I64 => out.push_str("i64"),
        TypeHint::F32 => out.push_str("f32"),
        TypeHint::Int => out.push_str("int"),
        TypeHint::String => out.push_str("string"),
        TypeHint::Data => out.push_str("data"),
        TypeHint::Error => out.push_str("error"),

        TypeHint::AnonTuple(t) => {
            out.push('(');
            for (i, field) in t.fields(db).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_type_hint_and_heap(db, *field, out);
            }
            out.push(')');
        }

        TypeHint::NamedTuple(t) => {
            out.push_str("tuple ");
            out.push_str(t.name(db).as_str(db));
            out.push_str(" (");
            for (i, field) in t.fields(db).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_type_hint_and_heap(db, *field, out);
            }
            out.push(')');
        }

        TypeHint::AnonStruct(s) => {
            out.push('{');
            for (i, field) in s.fields(db).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name(db).as_str(db));
                out.push_str(": ");
                pretty_type_hint_and_heap(db, field.type_hint(db), out);
            }
            out.push('}');
        }

        TypeHint::NamedStruct(s) => {
            out.push_str("struct ");
            out.push_str(s.name(db).as_str(db));
            out.push_str(" {");
            for (i, field) in s.fields(db).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name(db).as_str(db));
                out.push_str(": ");
                pretty_type_hint_and_heap(db, field.type_hint(db), out);
            }
            out.push('}');
        }

        TypeHint::AnonEnum(e) => {
            out.push_str("enum {");
            for (i, variant) in e.variants(db).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(variant.name(db).as_str(db));
                if let Some(payload) = variant.payload(db) {
                    out.push('(');
                    pretty_type_hint_and_heap(db, payload, out);
                    out.push(')');
                }
            }
            out.push('}');
        }

        TypeHint::NamedEnum(e) => {
            out.push_str("enum ");
            out.push_str(e.name(db).as_str(db));
            out.push_str(" {");
            for (i, variant) in e.variants(db).iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(variant.name(db).as_str(db));
                if let Some(payload) = variant.payload(db) {
                    out.push('(');
                    pretty_type_hint_and_heap(db, payload, out);
                    out.push(')');
                }
            }
            out.push('}');
        }

        TypeHint::List(l) => {
            out.push('[');
            pretty_type_hint_and_heap(db, l.element_type(db), out);
            out.push(']');
        }

        TypeHint::Map(m) => {
            out.push_str("map<");
            pretty_type_hint_and_heap(db, m.key_type(db), out);
            out.push_str(", ");
            pretty_type_hint_and_heap(db, m.value_type(db), out);
            out.push('>');
        }

        TypeHint::Set(s) => {
            out.push_str("set<");
            pretty_type_hint_and_heap(db, s.element_type(db), out);
            out.push('>');
        }

        TypeHint::Option(o) => {
            out.push('?');
            pretty_type_hint_and_heap(db, o.inner_type(db), out);
        }

        TypeHint::Result(r) => {
            out.push('!');
            pretty_type_hint_and_heap(db, r.inner_type(db), out);
        }

        TypeHint::Tensor(t) => {
            out.push_str("tensor<");
            pretty_type_hint_and_heap(db, t.element_type(db), out);
            out.push_str(", ");
            out.push_str(&t.rank(db).to_string());
            out.push('>');
        }

        TypeHint::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message(db).as_str(db));
            out.push('>');
        }
    }
}

/// Generate a mutated expression from a seed.
///
/// Combines AST generation with mutation application for comprehensive testing.
pub fn gen_mutated_expr<'db>(
    db: &'db dyn salsa::Database,
    seed: u64,
    mutation: Mutation,
    config: AstGenConfig,
) -> Option<MutationResult> {
    let expr = gen_expr_full_seeded(db, seed, config);
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));
    mutation.apply(db, expr, &mut rng)
}
