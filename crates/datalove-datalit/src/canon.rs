//! Canonicalization module for datalit expressions.
//!
//! Provides compile-time comparison and sorting of expressions for
//! map/set instantiation. The sorted order is memoized by Salsa.

use std::cmp::Ordering;


use crate::ast::*;

/// Compare two expressions for ordering (compile-time, no runtime needed).
///
/// Returns Ordering based on datalit's total ordering semantics.
/// Currently supports common literals only. Complex types (Data, Error,
/// enums, named types) will panic - to be addressed when new interpreter
/// is implemented.
fn cmp_expr<'db>(db: &'db dyn salsa::Database, a: ExprFull<'db>, b: ExprFull<'db>) -> Ordering {
    cmp_expr_inner(db, &a.expr(db).expr(db), &b.expr(db).expr(db))
}

fn cmp_expr_inner<'db>(db: &'db dyn salsa::Database, a: &Expr<'db>, b: &Expr<'db>) -> Ordering {
    use Expr::*;

    // First compare by type tag.
    let tag_a = expr_type_tag(a);
    let tag_b = expr_type_tag(b);
    if tag_a != tag_b {
        return tag_a.cmp(&tag_b);
    }

    // Same type, compare values.
    match (a, b) {
        (True, True) => Ordering::Equal,
        (False, False) => Ordering::Equal,
        (True, False) => Ordering::Greater,
        (False, True) => Ordering::Less,

        (Int(a_int), Int(b_int)) => {
            cmp_int_strings(a_int.value(db).as_str(db), b_int.value(db).as_str(db))
        }

        (Float(a_float), Float(b_float)) => {
            cmp_float_strings(a_float.value(db).as_str(db), b_float.value(db).as_str(db))
        }

        (Hex(a_hex), Hex(b_hex)) => {
            // Compare hex as unsigned integers.
            let a_val = parse_hex(a_hex.value(db).as_str(db));
            let b_val = parse_hex(b_hex.value(db).as_str(db));
            a_val.cmp(&b_val)
        }

        (String(a_str), String(b_str)) => {
            // Compare the raw string content (including quotes).
            a_str.value(db).as_str(db).cmp(b_str.value(db).as_str(db))
        }

        (AnonTuple(a_tuple), AnonTuple(b_tuple)) => {
            cmp_expr_lists(db, &a_tuple.elements(db), &b_tuple.elements(db))
        }

        (List(a_list), List(b_list)) => {
            cmp_expr_lists(db, &a_list.elements(db), &b_list.elements(db))
        }

        (Set(a_set), Set(b_set)) => {
            // For sets, we need to compare the sorted elements.
            let a_sorted = sorted_set_indices(db, *a_set);
            let b_sorted = sorted_set_indices(db, *b_set);
            let a_elements = a_set.elements(db);
            let b_elements = b_set.elements(db);

            // Compare lengths first.
            match a_sorted.len().cmp(&b_sorted.len()) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // Compare element by element in sorted order.
            for (a_idx, b_idx) in a_sorted.iter().zip(b_sorted.iter()) {
                match cmp_expr(db, a_elements[*a_idx], b_elements[*b_idx]) {
                    Ordering::Equal => continue,
                    ord => return ord,
                }
            }
            Ordering::Equal
        }

        (Map(a_map), Map(b_map)) => {
            // For maps, compare by sorted keys, then by values.
            let a_sorted = sorted_map_indices(db, *a_map);
            let b_sorted = sorted_map_indices(db, *b_map);
            let a_entries = a_map.entries(db);
            let b_entries = b_map.entries(db);

            // Compare lengths first.
            match a_sorted.len().cmp(&b_sorted.len()) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // Compare entry by entry in sorted order.
            for (a_idx, b_idx) in a_sorted.iter().zip(b_sorted.iter()) {
                let a_entry = &a_entries[*a_idx];
                let b_entry = &b_entries[*b_idx];

                // Compare keys first.
                match cmp_expr(db, a_entry.key(db), b_entry.key(db)) {
                    Ordering::Equal => {}
                    ord => return ord,
                }

                // Then compare values.
                match cmp_expr(db, a_entry.value(db), b_entry.value(db)) {
                    Ordering::Equal => continue,
                    ord => return ord,
                }
            }
            Ordering::Equal
        }

        (Tensor(a_tensor), Tensor(b_tensor)) => {
            // Compare shape first, then elements.
            match a_tensor.shape(db).cmp(&b_tensor.shape(db)) {
                Ordering::Equal => {}
                ord => return ord,
            }
            cmp_expr_lists(db, &a_tensor.elements(db), &b_tensor.elements(db))
        }

        (None, None) => Ordering::Equal,

        // Data and Error: compare their wrapped values.
        (Data(a_data), Data(b_data)) => {
            cmp_expr(db, a_data.value(db), b_data.value(db))
        }

        (Err(a_err), Err(b_err)) => {
            cmp_expr(db, a_err.value(db), b_err.value(db))
        }

        // Anonymous struct: compare fields by name, then by value.
        (AnonStruct(a_struct), AnonStruct(b_struct)) => {
            let a_fields = a_struct.fields(db);
            let b_fields = b_struct.fields(db);

            // Compare number of fields first.
            match a_fields.len().cmp(&b_fields.len()) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // Compare field by field (sorted by field name).
            let mut a_sorted: Vec<_> = a_fields.iter().collect();
            let mut b_sorted: Vec<_> = b_fields.iter().collect();
            a_sorted.sort_by_key(|f| f.name(db).as_str(db));
            b_sorted.sort_by_key(|f| f.name(db).as_str(db));

            for (a_field, b_field) in a_sorted.iter().zip(b_sorted.iter()) {
                // Compare field names first.
                match a_field.name(db).as_str(db).cmp(b_field.name(db).as_str(db)) {
                    Ordering::Equal => {}
                    ord => return ord,
                }
                // Then compare field values.
                match cmp_expr(db, a_field.value(db), b_field.value(db)) {
                    Ordering::Equal => continue,
                    ord => return ord,
                }
            }
            Ordering::Equal
        }

        // Anonymous enum: compare variant name, then payload.
        (AnonEnum(a_enum), AnonEnum(b_enum)) => {
            // Compare variant names first.
            match a_enum.variant_name(db).as_str(db).cmp(b_enum.variant_name(db).as_str(db)) {
                Ordering::Equal => {}
                ord => return ord,
            }

            // Then compare payloads (if any).
            match (a_enum.payload(db), b_enum.payload(db)) {
                (Option::None, Option::None) => Ordering::Equal,
                (Option::None, Option::Some(_)) => Ordering::Less,
                (Option::Some(_), Option::None) => Ordering::Greater,
                (Option::Some(a_payload), Option::Some(b_payload)) => cmp_expr(db, a_payload, b_payload),
            }
        }

        // Unsupported types - panic for now.
        (ParseError(_), _) | (_, ParseError(_)) => {
            panic!("cmp_expr: unsupported expression type for compile-time comparison")
        }

        // This should be unreachable since we already matched on type tags.
        _ => unreachable!("mismatched expression types after tag comparison"),
    }
}

/// Return a numeric tag for expression types to establish cross-type ordering.
fn expr_type_tag(expr: &Expr) -> u8 {
    use Expr::*;
    match expr {
        False => 0,
        True => 1,
        Int(_) => 2,
        Float(_) => 3,
        Hex(_) => 4,
        String(_) => 5,
        None => 6,
        Some(_) => 7,
        Ok(_) => 8,
        Er(_) => 9,
        AnonTuple(_) => 10,
        AnonStruct(_) => 11,
        AnonEnum(_) => 12,
        List(_) => 13,
        Set(_) => 14,
        Map(_) => 15,
        Tensor(_) => 16,
        Data(_) => 17,
        Err(_) => 18,
        ParseError(_) => 19,
    }
}

/// Compare two integer string representations.
fn cmp_int_strings(a: &str, b: &str) -> Ordering {
    // Handle negative numbers.
    let a_neg = a.starts_with('-');
    let b_neg = b.starts_with('-');

    match (a_neg, b_neg) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }

    let a_digits = if a_neg { &a[1..] } else { a };
    let b_digits = if b_neg { &b[1..] } else { b };

    // Compare by length first (for same-sign numbers).
    let len_cmp = a_digits.len().cmp(&b_digits.len());
    if len_cmp != Ordering::Equal {
        return if a_neg { len_cmp.reverse() } else { len_cmp };
    }

    // Same length, compare lexicographically.
    let lex_cmp = a_digits.cmp(b_digits);
    if a_neg { lex_cmp.reverse() } else { lex_cmp }
}

/// Compare two float string representations.
fn cmp_float_strings(a: &str, b: &str) -> Ordering {
    // Parse as f64 for comparison (handles most cases).
    let a_val: f64 = a.parse().unwrap_or(0.0);
    let b_val: f64 = b.parse().unwrap_or(0.0);

    // Use total_cmp for IEEE total ordering.
    a_val.total_cmp(&b_val)
}

/// Parse a hex string (with 0x prefix) to u64.
fn parse_hex(s: &str) -> u64 {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u64::from_str_radix(s, 16).unwrap_or(0)
}

/// Compare two lists of expressions lexicographically.
fn cmp_expr_lists<'db>(
    db: &'db dyn salsa::Database,
    a: &[ExprFull<'db>],
    b: &[ExprFull<'db>],
) -> Ordering {
    // Compare lengths first.
    match a.len().cmp(&b.len()) {
        Ordering::Equal => {}
        ord => return ord,
    }

    // Compare element by element.
    for (a_elem, b_elem) in a.iter().zip(b.iter()) {
        match cmp_expr(db, *a_elem, *b_elem) {
            Ordering::Equal => continue,
            ord => return ord,
        }
    }
    Ordering::Equal
}

/// Compute sorted indices for set elements.
///
/// Returns a vector of indices into the set's elements list, sorted by
/// the elements' ordering. Memoized by Salsa.
#[salsa::tracked]
pub fn sorted_set_indices<'db>(db: &'db dyn salsa::Database, set: ExprSet<'db>) -> Vec<usize> {
    let elements = set.elements(db);
    let mut indices: Vec<usize> = (0..elements.len()).collect();
    indices.sort_by(|&a, &b| cmp_expr(db, elements[a], elements[b]));
    indices
}

/// Compute sorted indices for map entries (by key).
///
/// Returns a vector of indices into the map's entries list, sorted by
/// the entries' keys. Memoized by Salsa.
#[salsa::tracked]
pub fn sorted_map_indices<'db>(db: &'db dyn salsa::Database, map: ExprMap<'db>) -> Vec<usize> {
    let entries = map.entries(db);
    let mut indices: Vec<usize> = (0..entries.len()).collect();
    indices.sort_by(|&a, &b| cmp_expr(db, entries[a].key(db), entries[b].key(db)));
    indices
}
