//! Mutation-based error generation for testing error equivalence.
//!
//! Generates erroneous expressions by mutating valid ASTs or their pretty-printed source.
//! Used to verify both datalit and datafun parsers/typecheckers produce equivalent errors.

use rmx::prelude::*;
use rand::Rng;

use crate::ast::*;
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
    /// Add or remove tuple/struct field to cause arity mismatch.
    ArityMismatch,
    /// Remove type hint from None/empty collection.
    RemoveTypeHint,


    /// Duplicate a field name in struct.
    DuplicateField,
    /// Use wrong field name in struct (field not in type hint).
    WrongFieldName,
    /// Remove a closing bracket.
    DeleteClosingBracket,


    /// Swap key and value types in map entry.
    SwapMapKeyValue,
}

impl Mutation {
    /// Get all mutations.
    pub fn all() -> &'static [Mutation] {
        &[
            Mutation::DeleteOpeningBracket,
            Mutation::TruncateSource,
            Mutation::DeleteComma,
            Mutation::ExtraClosingBracket,
            Mutation::DeleteClosingBracket,
            Mutation::OutOfRangeInt,
            Mutation::WrongElementType,
            Mutation::ArityMismatch,
            Mutation::RemoveTypeHint,
            Mutation::DuplicateField,
            Mutation::WrongFieldName,
            Mutation::SwapMapKeyValue,
        ]
    }

    /// Get all source-level mutations.
    pub fn source_mutations() -> &'static [Mutation] {
        &[
            Mutation::DeleteOpeningBracket,
            Mutation::TruncateSource,
            Mutation::DeleteComma,
            Mutation::ExtraClosingBracket,
            Mutation::DeleteClosingBracket,
        ]
    }

    /// Get all AST-level mutations.
    pub fn ast_mutations() -> &'static [Mutation] {
        &[
            Mutation::OutOfRangeInt,
            Mutation::WrongElementType,
            Mutation::ArityMismatch,
            Mutation::RemoveTypeHint,
            Mutation::DuplicateField,
            Mutation::WrongFieldName,
            Mutation::SwapMapKeyValue,
        ]
    }

    /// Get the kind of this mutation.
    pub fn kind(&self) -> MutationKind {
        match self {
            Mutation::DeleteOpeningBracket
            | Mutation::TruncateSource
            | Mutation::DeleteComma
            | Mutation::ExtraClosingBracket
            | Mutation::DeleteClosingBracket => MutationKind::Source,

            Mutation::OutOfRangeInt
            | Mutation::WrongElementType
            | Mutation::ArityMismatch
            | Mutation::RemoveTypeHint
            | Mutation::DuplicateField
            | Mutation::WrongFieldName
            | Mutation::SwapMapKeyValue => MutationKind::Ast,
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
            Mutation::DeleteOpeningBracket => apply_delete_opening_bracket(&source, rng),
            Mutation::TruncateSource => apply_truncate_source(&source, rng),
            Mutation::DeleteComma => apply_delete_comma(&source, rng),
            Mutation::ExtraClosingBracket => apply_extra_closing_bracket(&source, rng),
            Mutation::OutOfRangeInt => apply_out_of_range_int(db, expr, rng),
            Mutation::WrongElementType => apply_wrong_element_type(db, expr, rng),
            Mutation::ArityMismatch => apply_arity_mismatch(db, expr, rng),
            Mutation::RemoveTypeHint => apply_remove_type_hint(db, expr),
            Mutation::DuplicateField => apply_duplicate_field(db, expr),
            Mutation::WrongFieldName => apply_wrong_field_name(db, expr),
            Mutation::DeleteClosingBracket => apply_delete_closing_bracket(&source, rng),
            Mutation::SwapMapKeyValue => apply_swap_map_key_value(db, expr),
        }
    }
}

// ============================================================================
// Source-level mutation implementations
// ============================================================================

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
    let mut result = source.S();
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

    let result = source[..truncate_pos].S();

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
    let mut result = source.S();
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

    let mut result = source.S();
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
    _rng: &mut impl Rng,
) -> Option<MutationResult> {
    // Check if the expression has a type hint for a fixed-width integer type.
    let th = expr.type_hint(db)?;

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
    let inner_expr = expr.expr(db);
    match inner_expr {
        Expr::Int(_) | Expr::Hex(_) => {}
        _ => return None,
    }

    // Build mutated source: keep type hint, replace value.
    let mut type_hint_str = String::new();
    type_hint_str.push_str(": ");
    pretty_type_hint(db, th, &mut type_hint_str);
    type_hint_str.push_str(" / ");
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
    let inner_expr = expr.expr(db);
    

    // Check if this is a list with at least one element.
    if let Expr::List(list) = inner_expr {
        let elements = list.elements.C();
        if elements.is_empty() {
            return None;
        }

        // Get the type of first element to determine what's "wrong".
        let first_elem = elements[0];
        let first_expr = first_elem.expr(db);

        // Determine the wrong element to insert based on first element's type.
        let wrong_elem_str = match first_expr {
            Expr::Int(_) | Expr::Hex(_) | Expr::Float(_) => {
                // Add a string where number expected.
                
                format!("{}\"wrong\"", "")
            }
            Expr::String(_) => {
                // Add an int where string expected.
                
                format!("{}42", "")
            }
            Expr::True | Expr::False => {
                // Add an int where bool expected.
                
                format!("{}42", "")
            }
            _ => return None, // Complex nested type, skip.
        };

        // Build list source via string manipulation.
        let elem_strs: Vec<String> = elements.iter().map(|e| pretty_print(db, *e)).collect();

        // Build type prefix.
        

        // Build final source with wrong element appended.
        // Use prefix type hint format: ": type / expr"
        let mut all_elems = elem_strs;
        all_elems.push(wrong_elem_str);
        let list_body = format!("{}[{}]", "", all_elems.join(", "));

        let source = if let Some(th) = expr.type_hint(db) {
            let mut type_str = String::new();
            pretty_type_hint(db, th, &mut type_str);
            format!(": {} / {}", type_str, list_body)
        } else {
            list_body
        };

        Some(MutationResult {
            source,
            expected_errors: vec!["T018"], // List element type mismatch.
            description: "Inserted wrong-typed element in list".S(),
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
    let inner_expr = expr.expr(db);
    

    

    match (type_hint.clone(), inner_expr) {
        (TypeHint::AnonTuple(_th), Expr::AnonTuple(t)) => {
            let elements = t.elements.C();
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
            pretty_type_hint(db, type_hint, &mut type_str);

            // Build tuple with fewer elements: `: type / (elem1, elem2)`
            // Trailing comma for 1-tuples to distinguish from grouping parens.
            let trailing = if elem_strs.len() == 1 { "," } else { "" };
            let source = format!(": {} / ({}{})", type_str, elem_strs.join(", "), trailing);

            Some(MutationResult {
                source,
                expected_errors: vec!["T038"], // Tuple arity mismatch.
                description: "Removed tuple element to cause arity mismatch".S(),
            })
        }
        (TypeHint::AnonStruct(_th), Expr::AnonStruct(s)) => {
            let fields = s.fields.C();
            if fields.len() < 2 {
                // Need at least 2 fields to remove one.
                return None;
            }

            // Pretty-print fields, then remove last one.
            let field_strs: Vec<String> = fields[..fields.len() - 1]
                .iter()
                .map(|f| {
                    let name = f.name.as_str(db);
                    let value = pretty_print(db, f.value);
                    format!("{}: {}", name, value)
                })
                .collect();

            // Build type hint string.
            let mut type_str = String::new();
            pretty_type_hint(db, type_hint, &mut type_str);

            // Build struct with fewer fields: `: type / {field1: v1, field2: v2}`
            let source = format!(": {} / {}{{{}}}", type_str, "", field_strs.join(", "));

            Some(MutationResult {
                source,
                expected_errors: vec!["T039"], // Struct missing field.
                description: "Removed struct field to cause arity mismatch".S(),
            })
        }
        _ => None,
    }
}

/// Remove type hint from None/empty collection.
///
/// Uses source-level string manipulation to avoid salsa tracked function issues.
fn apply_remove_type_hint<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
) -> Option<MutationResult> {
    let _type_hint = expr.type_hint(db)?;
    let inner_expr = expr.expr(db);
    

    // Check if this is an expression that requires a type hint.
    let (needs_hint, error_code) = match inner_expr {
        Expr::None => (true, "T016"), // Cannot synthesize type for None.

        Expr::List(l) if l.elements.is_empty() => (true, "T013"), // Cannot synthesize type for empty list.
        Expr::Set(s) if s.elements.is_empty() => (true, "T014"), // Cannot synthesize type for empty set.
        Expr::Map(m) if m.entries.is_empty() => (true, "T015"), // Cannot synthesize type for empty map.
        _ => (false, ""),
    };

    if !needs_hint {
        return None;
    }

    // Build source string without type hint.
    

    let source = match inner_expr {
        Expr::None => format!("{}none", ""),

        Expr::List(_) => format!("{}[]", ""),
        Expr::Set(_) => "#{}".S(),
        Expr::Map(_) => "%{}".S(),
        _ => return None,
    };

    Some(MutationResult {
        source,
        expected_errors: vec![error_code],
        description: "Removed type hint from expression that requires one".S(),
    })
}


/// Delete a closing bracket from the source.
fn apply_delete_closing_bracket(source: &str, rng: &mut impl Rng) -> Option<MutationResult> {
    let bracket_positions: Vec<(usize, char)> = source
        .char_indices()
        .filter(|(_, c)| *c == ')' || *c == ']' || *c == '}' || *c == '>')
        .collect();

    if bracket_positions.is_empty() {
        return None;
    }

    let (pos, bracket) = bracket_positions[rng.gen_range(0..bracket_positions.len())];
    let mut result = source.S();
    result.remove(pos);

    Some(MutationResult {
        source: result,
        expected_errors: vec![], // Parse errors vary by context.
        description: format!("Deleted closing bracket '{}' at position {}", bracket, pos),
    })
}

/// Duplicate a field name in struct.
fn apply_duplicate_field<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
) -> Option<MutationResult> {
    let type_hint = expr.type_hint(db)?;
    let inner_expr = expr.expr(db);
    

    

    // Check if this is an anon struct with at least 2 fields.
    if let (TypeHint::AnonStruct(_), Expr::AnonStruct(s)) = (type_hint.clone(), inner_expr) {
        let fields = s.fields.C();
        if fields.len() < 2 {
            return None;
        }

        // Duplicate first field's name in second field.
        let first_name = fields[0].name.as_str(db);
        let mut field_strs: Vec<String> = fields.iter().map(|f| {
            let name = f.name.as_str(db);
            let value = pretty_print(db, f.value);
            format!("{}: {}", name, value)
        }).collect();

        // Replace second field name with first field name.
        let second_value = pretty_print(db, fields[1].value);
        field_strs[1] = format!("{}: {}", first_name, second_value);

        // Build type hint string.
        let mut type_str = String::new();
        pretty_type_hint(db, type_hint, &mut type_str);

        let source = format!(": {} / {}{{{}}}", type_str, "", field_strs.join(", "));

        Some(MutationResult {
            source,
            expected_errors: vec!["T040"], // Duplicate field.
            description: "Duplicated field name in struct".S(),
        })
    } else {
        None
    }
}

/// Use wrong field name in struct (field not in type hint).
fn apply_wrong_field_name<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
) -> Option<MutationResult> {
    let type_hint = expr.type_hint(db)?;
    let inner_expr = expr.expr(db);
    

    

    // Check if this is an anon struct.
    if let (TypeHint::AnonStruct(_), Expr::AnonStruct(s)) = (type_hint.clone(), inner_expr) {
        let fields = s.fields.C();
        if fields.is_empty() {
            return None;
        }

        // Change first field name to nonexistent.
        let mut field_strs: Vec<String> = fields.iter().map(|f| {
            let name = f.name.as_str(db);
            let value = pretty_print(db, f.value);
            format!("{}: {}", name, value)
        }).collect();

        let first_value = pretty_print(db, fields[0].value);
        field_strs[0] = format!("nonexistent_field_xyz: {}", first_value);

        // Build type hint string.
        let mut type_str = String::new();
        pretty_type_hint(db, type_hint, &mut type_str);

        let source = format!(": {} / {}{{{}}}", type_str, "", field_strs.join(", "));

        Some(MutationResult {
            source,
            expected_errors: vec!["T042"], // Unknown field.
            description: "Changed field name to nonexistent".S(),
        })
    } else {
        None
    }
}


/// Swap key and value types in a map entry.
fn apply_swap_map_key_value<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
) -> Option<MutationResult> {
    let type_hint = expr.type_hint(db)?;
    let inner_expr = expr.expr(db);
    

    // Check if this is a map with at least one entry where key/value types differ.
    if let (TypeHint::Map(map_th), Expr::Map(m)) = (type_hint.clone(), inner_expr) {
        let entries = m.entries.C();
        if entries.is_empty() {
            return None;
        }

        // Check if key and value types are different (otherwise swap is not an error).
        let key_type = *map_th.key_type;
        let val_type = *map_th.value_type;
        let mut key_str = String::new();
        let mut val_str = String::new();
        pretty_type_hint(db, key_type, &mut key_str);
        pretty_type_hint(db, val_type, &mut val_str);
        if key_str == val_str {
            return None; // Same types, swap won't cause error.
        }

        // Build map entries with first entry's key/value swapped.
        let entry_strs: Vec<String> = entries.iter().enumerate().map(|(i, e)| {
            let key_pp = pretty_print(db, e.key);
            let val_pp = pretty_print(db, e.value);
            if i == 0 {
                // Swap key and value for first entry.
                format!("{}: {}", val_pp, key_pp)
            } else {
                format!("{}: {}", key_pp, val_pp)
            }
        }).collect();

        // Build type prefix.
        

        let map_body = format!("%{{ {} }}", entry_strs.join(", "));

        // Build type hint string.
        let mut type_str = String::new();
        pretty_type_hint(db, type_hint, &mut type_str);

        let source = format!(": {} / {}", type_str, map_body);

        Some(MutationResult {
            source,
            expected_errors: vec!["T019"], // Map key type mismatch.
            description: "Swapped map key and value".S(),
        })
    } else {
        None
    }
}

// ============================================================================
// Helper functions
// ============================================================================

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
        TypeHint::Index => out.push_str("index"),
        TypeHint::Offset => out.push_str("offset"),
        TypeHint::F32 => out.push_str("f32"),
        TypeHint::F64 => out.push_str("f64"),
        TypeHint::Int => out.push_str("int"),
        TypeHint::String => out.push_str("string"),
        TypeHint::Data => out.push_str("data"),
        TypeHint::Error => out.push_str("error"),

        TypeHint::AnonTuple(t) => {
            out.push('(');
            for (i, field) in t.fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_type_hint(db, field.clone(), out);
            }
            if t.fields.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }

        TypeHint::AnonStruct(s) => {
            out.push('{');
            for (i, field) in s.fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name.as_str(db));
                out.push_str(": ");
                pretty_type_hint(db, *field.type_hint.clone(), out);
            }
            out.push('}');
        }


        TypeHint::List(l) => {
            out.push('[');
            pretty_type_hint(db, *l.element_type.clone(), out);
            out.push(']');
        }

        TypeHint::Map(m) => {
            out.push_str("%{");
            pretty_type_hint(db, *m.key_type.clone(), out);
            out.push_str(" = ");
            pretty_type_hint(db, *m.value_type.clone(), out);
            out.push('}');
        }

        TypeHint::Set(s) => {
            out.push_str("#{");
            pretty_type_hint(db, *s.element_type.clone(), out);
            out.push('}');
        }

        TypeHint::Option(o) => {
            out.push('?');
            pretty_type_hint(db, *o.inner_type.clone(), out);
        }

        TypeHint::Result(r) => {
            out.push('!');
            pretty_type_hint(db, *r.inner_type.clone(), out);
        }

        TypeHint::Tensor(t) => {
            out.push_str("[|");
            pretty_type_hint(db, *t.element_type.clone(), out);
            out.push_str(", ");
            out.push_str(&t.rank.S());
            out.push_str("|]");
        }

        TypeHint::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message.as_str(db));
            out.push('>');
        }

        TypeHint::Table(t) => {
            out.push_str("{| ");
            let columns = &t.columns;
            for (i, col) in columns.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(col.name.as_str(db));
                out.push_str(": ");
                pretty_type_hint(db, *col.type_hint.clone(), out);
            }
            out.push_str(" |}");
        }

        TypeHint::Alias(alias) => {
            out.push_str(alias.name.as_str(db));
        }

        TypeHint::Atom(a) => {
            out.push_str("atom ");
            out.push_str(a.name.as_str(db));
        }

        TypeHint::Term(t) => {
            out.push_str("term ");
            out.push_str(t.name.as_str(db));
            out.push(' ');
            pretty_type_hint(db, *t.payload.clone(), out);
        }

        TypeHint::Enum(e) => {
            out.push_str("enum{");
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if let Some(payload) = &v.payload {
                    out.push_str("term ");
                    out.push_str(v.name.as_str(db));
                    out.push(' ');
                    pretty_type_hint(db, *payload.clone(), out);
                } else {
                    out.push_str("atom ");
                    out.push_str(v.name.as_str(db));
                }
            }
            out.push('}');
        }
    }
}
