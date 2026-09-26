//! Pretty printing for type hints and expressions in worldfile generation.

use datalove_datalit::ast::{Expr, ExprFull, TypeHint};

/// Pretty print a TypeHint to a string.
pub fn pretty_type_hint<'db>(
    db: &'db dyn salsa::Database,
    th: TypeHint<'db>,
) -> String {
    let mut out = String::new();
    write_type_hint(db, &th, &mut out);
    out
}

/// Pretty print an expression to a string.
pub fn pretty_expr<'db>(
    db: &'db dyn salsa::Database,
    expr: Expr<'db>,
) -> String {
    let mut out = String::new();
    write_expr(db, &expr, &mut out);
    out
}

fn write_expr<'db>(
    db: &'db dyn salsa::Database,
    expr: &Expr<'db>,
    out: &mut String,
) {
    match expr {
        Expr::True => out.push_str("true"),
        Expr::False => out.push_str("false"),
        Expr::None => out.push_str("none"),
        Expr::Some(s) => {
            out.push_str("some ");
            write_expr_full(db, s.payload, out);
        }
        Expr::Ok(o) => {
            out.push_str("ok ");
            write_expr_full(db, o.payload, out);
        }
        Expr::Er(e) => {
            out.push_str("er ");
            write_expr_full(db, e.payload, out);
        }

        Expr::Int(i) => {
            out.push_str(i.value.as_str(db));
        }

        Expr::Float(f) => {
            out.push_str(f.value.as_str(db));
        }

        Expr::Hex(h) => {
            out.push_str(h.value.as_str(db));
        }

        Expr::String(s) => {
            out.push_str(s.value.as_str(db));
        }

        Expr::AnonTuple(t) => {
            out.push('(');
            for (i, elem) in t.elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr_full(db, *elem, out);
            }
            // A one-element tuple takes a trailing comma, since `(x)` on its
            // own is the expression `x` in brackets.
            if t.elements.len() == 1 {
                out.push(',');
            }
            out.push(')');
        }

        Expr::AnonStruct(s) => {
            out.push('{');
            for (i, field) in s.fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name.as_str(db));
                out.push_str(" = ");
                write_expr_full(db, field.value, out);
            }
            out.push('}');
        }


        Expr::List(l) => {
            out.push('[');
            for (i, elem) in l.elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr_full(db, *elem, out);
            }
            out.push(']');
        }

        Expr::Map(m) => {
            out.push_str("%{");
            for (i, entry) in m.entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr_full(db, entry.key, out);
                out.push_str(" = ");
                write_expr_full(db, entry.value, out);
            }
            out.push('}');
        }

        Expr::Set(s) => {
            out.push_str("#{");
            for (i, elem) in s.elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr_full(db, *elem, out);
            }
            out.push('}');
        }

        Expr::Tensor(t) => {
            out.push_str("[| ");
            let shape = &t.shape;
            let elements = &t.elements;
            write_tensor_elements(db, shape, elements, out);
            out.push_str(" |]");
        }

        Expr::Data(d) => {
            out.push_str("data ");
            write_expr_full(db, d.value, out);
        }

        Expr::Error(e) => {
            out.push_str("error ");
            write_expr_full(db, e.value, out);
        }

        Expr::Atom(a) => {
            out.push_str("atom ");
            out.push_str(a.name.as_str(db));
        }

        Expr::Term(t) => {
            out.push_str("term ");
            out.push_str(t.name.as_str(db));
            out.push(' ');
            write_expr_full(db, t.payload, out);
        }

        Expr::Enum(e) => {
            out.push_str("enum { ");
            write_expr_full(db, e.variant, out);
            out.push_str(" }");
        }

        Expr::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message.as_str(db));
            out.push('>');
        }

        Expr::Table(t) => {
            out.push_str("{| ");
            for (i, name) in t.header.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(name.as_str(db));
            }
            for row in &t.rows {
                out.push_str("; ");
                for (i, elem) in row.elements.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_expr_full(db, *elem, out);
                }
            }
            out.push_str(" |}");
        }
    }
}

fn write_expr_full<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFull<'db>,
    out: &mut String,
) {
    if let Some(th) = expr.type_hint(db) {
        out.push_str(": ");
        write_type_hint(db, &th, out);
        out.push_str(" / ");
    }
    write_expr(db, &expr.expr(db), out);
}

fn write_type_hint<'db>(
    db: &'db dyn salsa::Database,
    th: &TypeHint<'db>,
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
                write_type_hint(db, field, out);
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
                write_type_hint(db, &field.type_hint, out);
            }
            out.push('}');
        }


        TypeHint::List(l) => {
            out.push('[');
            write_type_hint(db, &l.element_type, out);
            out.push(']');
        }

        TypeHint::Map(m) => {
            out.push_str("%{");
            write_type_hint(db, &m.key_type, out);
            out.push_str(" = ");
            write_type_hint(db, &m.value_type, out);
            out.push('}');
        }

        TypeHint::Set(s) => {
            out.push_str("#{");
            write_type_hint(db, &s.element_type, out);
            out.push('}');
        }

        TypeHint::Option(o) => {
            out.push('?');
            write_type_hint(db, &o.inner_type, out);
        }

        TypeHint::Result(r) => {
            out.push('!');
            write_type_hint(db, &r.inner_type, out);
        }

        TypeHint::Tensor(t) => {
            out.push_str("[|");
            write_type_hint(db, &t.element_type, out);
            out.push_str(", ");
            out.push_str(&t.rank.to_string());
            out.push_str("|]");
        }

        TypeHint::Table(t) => {
            out.push_str("{| ");
            for (i, col) in t.columns.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(col.name.as_str(db));
                out.push_str(": ");
                write_type_hint(db, &col.type_hint, out);
            }
            out.push_str(" |}");
        }

        TypeHint::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message.as_str(db));
            out.push('>');
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
            write_type_hint(db, &t.payload, out);
        }

        TypeHint::Enum(e) => {
            out.push_str("enum{");
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                match &v.payload {
                    Some(p) => {
                        out.push_str("term ");
                        out.push_str(v.name.as_str(db));
                        out.push(' ');
                        write_type_hint(db, p, out);
                    }
                    None => {
                        out.push_str("atom ");
                        out.push_str(v.name.as_str(db));
                    }
                }
            }
            out.push('}');
        }
    }
}

/// Write tensor elements with multi-comma layout.
fn write_tensor_elements<'db>(
    db: &'db dyn datalove_datalit::Db,
    shape: &[u32],
    elements: &[ExprFull<'db>],
    out: &mut String,
) {
    if elements.is_empty() || shape.is_empty() {
        return;
    }
    let rank = shape.len();
    write_tensor_group(db, shape, elements, 0, out);

    // When the outermost dimension is 1, the highest comma level (rank - 1)
    // never appears as a separator. Emit trailing commas so the parser can
    // infer the correct rank.
    if rank > 1 && shape[0] == 1 {
        for _ in 0..(rank - 1) {
            out.push(',');
        }
    }
}

fn write_tensor_group<'db>(
    db: &'db dyn datalove_datalit::Db,
    shape: &[u32],
    elements: &[ExprFull<'db>],
    dim: usize,
    out: &mut String,
) {
    let rank = shape.len();

    if dim == rank - 1 {
        for (i, elem) in elements.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            write_expr_full(db, *elem, out);
        }
        return;
    }

    let group_size: usize = shape[dim + 1..].iter().map(|&d| d as usize).product();
    let num_groups = shape[dim] as usize;
    let comma_count = rank - dim - 1;

    for (i, chunk) in elements.chunks(group_size).enumerate().take(num_groups) {
        if i > 0 {
            for _ in 0..comma_count {
                out.push(',');
            }
            out.push(' ');
        }
        write_tensor_group(db, shape, chunk, dim + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datalit::Database;

    #[test]
    fn test_pretty_primitive_types() {
        let db = Database::default();
        test_pretty_primitive_types_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_primitive_types_inner<'db>(db: &'db dyn salsa::Database) {
        let _ = db;
        let cases = [
            (TypeHint::Bool, "bool"),
            (TypeHint::U8, "u8"),
            (TypeHint::I8, "i8"),
            (TypeHint::U16, "u16"),
            (TypeHint::I16, "i16"),
            (TypeHint::U32, "u32"),
            (TypeHint::I32, "i32"),
            (TypeHint::U64, "u64"),
            (TypeHint::I64, "i64"),
            (TypeHint::F32, "f32"),
            (TypeHint::F64, "f64"),
            (TypeHint::Int, "int"),
            (TypeHint::String, "string"),
        ];

        for (ty_hint, expected) in cases {
            let output = pretty_type_hint(db, ty_hint);
            assert_eq!(output, expected, "Pretty print mismatch for {}", expected);
        }
    }

    #[test]
    fn test_pretty_expr_bool() {
        let db = Database::default();
        test_pretty_expr_bool_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_expr_bool_inner<'db>(db: &'db dyn salsa::Database) {
        assert_eq!(pretty_expr(db, Expr::True), "true");
        assert_eq!(pretty_expr(db, Expr::False), "false");
        assert_eq!(pretty_expr(db, Expr::None), "none");
    }

    #[test]
    fn test_pretty_type_list() {
        let db = Database::default();
        test_pretty_type_list_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_list_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintList;
        let _ = db;

        let list = TypeHint::List(TypeHintList { element_type: Box::new(TypeHint::U32) });
        assert_eq!(pretty_type_hint(db, list), "[u32]");
    }

    #[test]
    fn test_pretty_type_option() {
        let db = Database::default();
        test_pretty_type_option_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_option_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintOption;
        let _ = db;

        let opt = TypeHint::Option(TypeHintOption { inner_type: Box::new(TypeHint::String) });
        assert_eq!(pretty_type_hint(db, opt), "?string");
    }

    #[test]
    fn test_pretty_type_result() {
        let db = Database::default();
        test_pretty_type_result_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_result_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintResult;
        let _ = db;

        let res = TypeHint::Result(TypeHintResult { inner_type: Box::new(TypeHint::I32) });
        assert_eq!(pretty_type_hint(db, res), "!i32");
    }

    #[test]
    fn test_pretty_type_map() {
        let db = Database::default();
        test_pretty_type_map_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_map_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintMap;
        let _ = db;

        let map = TypeHint::Map(TypeHintMap {
            key_type: Box::new(TypeHint::String),
            value_type: Box::new(TypeHint::I32),
        });
        assert_eq!(pretty_type_hint(db, map), "%{string = i32}");
    }

    #[test]
    fn test_pretty_type_set() {
        let db = Database::default();
        test_pretty_type_set_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_set_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintSet;
        let _ = db;

        let set = TypeHint::Set(TypeHintSet { element_type: Box::new(TypeHint::U64) });
        assert_eq!(pretty_type_hint(db, set), "#{u64}");
    }

    #[test]
    fn test_pretty_type_tuple() {
        let db = Database::default();
        test_pretty_type_tuple_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_tuple_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintAnonTuple;
        let _ = db;

        let tuple = TypeHint::AnonTuple(TypeHintAnonTuple {
            fields: vec![TypeHint::U32, TypeHint::Bool],
        });
        assert_eq!(pretty_type_hint(db, tuple), "(u32, bool)");
    }

    #[test]
    fn test_pretty_type_struct() {
        let db = Database::default();
        test_pretty_type_struct_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_type_struct_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::{TypeHintAnonStruct, TypeHintNamedField};
        use bct::text::InternedText;

        let f1 = TypeHintNamedField {
            name: InternedText::new(db, "x".to_string()),
            type_hint: Box::new(TypeHint::I32),
        };
        let f2 = TypeHintNamedField {
            name: InternedText::new(db, "y".to_string()),
            type_hint: Box::new(TypeHint::I32),
        };

        let struct_ty = TypeHint::AnonStruct(TypeHintAnonStruct { fields: vec![f1, f2] });
        assert_eq!(pretty_type_hint(db, struct_ty), "{x: i32, y: i32}");
    }

    #[test]
    fn test_pretty_output_no_trailing_whitespace() {
        let db = Database::default();
        test_pretty_output_no_trailing_whitespace_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_pretty_output_no_trailing_whitespace_inner<'db>(db: &'db dyn salsa::Database) {
        let _ = db;
        let types = [
            TypeHint::Bool,
            TypeHint::U32,
            TypeHint::String,
        ];

        for ty_hint in types {
            let output = pretty_type_hint(db, ty_hint);
            assert!(!output.ends_with(' '), "Output has trailing space: '{}'", output);
            assert!(!output.ends_with('\t'), "Output has trailing tab: '{}'", output);
        }
    }
}
