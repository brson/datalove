//! Pretty printing for type hints and expressions in worldfile generation.

use datalove_datalit::ast::{Expr, ExprFull, Heap, TypeHint, TypeHintAndHeap};

/// Pretty print a TypeHintAndHeap to a string.
pub fn pretty_type_hint_and_heap<'db>(
    db: &'db dyn salsa::Database,
    th: TypeHintAndHeap<'db>,
) -> String {
    let mut out = String::new();
    write_type_hint_and_heap(db, th, &mut out);
    out
}

/// Pretty print an expression with its heap annotation to a string.
pub fn pretty_expr_with_heap<'db>(
    db: &'db dyn salsa::Database,
    expr: Expr<'db>,
    heap: Heap,
) -> String {
    let mut out = String::new();
    match heap {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }
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

        Expr::AnonEnum(e) => {
            out.push_str("enum ");
            out.push_str(e.variant_name.as_str(db));
            if let Some(payload) = e.payload {
                out.push('(');
                write_expr_full(db, payload, out);
                out.push(')');
            }
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
            out.push_str("map {");
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
            out.push_str("set {");
            for (i, elem) in s.elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_expr_full(db, *elem, out);
            }
            out.push('}');
        }

        Expr::Tensor(t) => {
            out.push_str("tensor [");
            for (i, &dim) in t.shape.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&dim.to_string());
            }
            out.push_str("] [");
            let rank = t.shape.len();
            if rank == 1 {
                for (i, elem) in t.elements.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    write_expr_full(db, *elem, out);
                }
            } else {
                let row_size = *t.shape.last().unwrap() as usize;
                for (row_idx, row) in t.elements.chunks(row_size).enumerate() {
                    if row_idx > 0 {
                        out.push_str(", ");
                    }
                    for (elem_idx, elem) in row.iter().enumerate() {
                        if elem_idx > 0 {
                            out.push(' ');
                        }
                        write_expr_full(db, *elem, out);
                    }
                }
            }
            out.push(']');
        }

        Expr::Data(d) => {
            out.push_str("data ");
            write_expr_full(db, d.value, out);
        }

        Expr::Error(e) => {
            out.push_str("error ");
            write_expr_full(db, e.value, out);
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
        write_type_hint_and_heap(db, th, out);
        out.push_str(" / ");
    }
    let expr_and_heap = expr.expr(db);
    match expr_and_heap.heap {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }
    write_expr(db, &expr_and_heap.expr, out);
}

fn write_type_hint_and_heap<'db>(
    db: &'db dyn salsa::Database,
    th: TypeHintAndHeap<'db>,
    out: &mut String,
) {
    match th.heap(db) {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }
    write_type_hint(db, th.type_hint(db), out);
}

fn write_type_hint<'db>(
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
        TypeHint::Usize => out.push_str("usize"),
        TypeHint::Isize => out.push_str("isize"),
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
                write_type_hint_and_heap(db, *field, out);
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
                write_type_hint_and_heap(db, field.type_hint, out);
            }
            out.push('}');
        }

        TypeHint::AnonEnum(e) => {
            out.push_str("enum {");
            for (i, variant) in e.variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(variant.name.as_str(db));
                if let Some(payload) = variant.payload {
                    out.push('(');
                    write_type_hint_and_heap(db, payload, out);
                    out.push(')');
                }
            }
            out.push('}');
        }

        TypeHint::List(l) => {
            out.push('[');
            write_type_hint_and_heap(db, l.element_type, out);
            out.push(']');
        }

        TypeHint::Map(m) => {
            out.push_str("map<");
            write_type_hint_and_heap(db, m.key_type, out);
            out.push_str(", ");
            write_type_hint_and_heap(db, m.value_type, out);
            out.push('>');
        }

        TypeHint::Set(s) => {
            out.push_str("set<");
            write_type_hint_and_heap(db, s.element_type, out);
            out.push('>');
        }

        TypeHint::Option(o) => {
            out.push('?');
            write_type_hint_and_heap(db, o.inner_type, out);
        }

        TypeHint::Result(r) => {
            out.push('!');
            write_type_hint_and_heap(db, r.inner_type, out);
        }

        TypeHint::Tensor(t) => {
            out.push_str("tensor<");
            write_type_hint_and_heap(db, t.element_type, out);
            out.push_str(", ");
            out.push_str(&t.rank.to_string());
            out.push('>');
        }

        TypeHint::Table(t) => {
            out.push_str("{| ");
            for (i, col) in t.columns.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(col.name.as_str(db));
                out.push_str(": ");
                write_type_hint_and_heap(db, col.type_hint, out);
            }
            out.push_str(" |}");
        }

        TypeHint::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message.as_str(db));
            out.push('>');
        }

        TypeHint::Alias(name) => {
            out.push_str(name.as_str(db));
        }
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

    #[salsa::tracked]
    fn test_pretty_primitive_types_inner<'db>(db: &'db dyn salsa::Database) {
        // Test local heap primitives.
        let cases = [
            (TypeHint::Bool, "@bool"),
            (TypeHint::U8, "@u8"),
            (TypeHint::I8, "@i8"),
            (TypeHint::U16, "@u16"),
            (TypeHint::I16, "@i16"),
            (TypeHint::U32, "@u32"),
            (TypeHint::I32, "@i32"),
            (TypeHint::U64, "@u64"),
            (TypeHint::I64, "@i64"),
            (TypeHint::F32, "@f32"),
            (TypeHint::F64, "@f64"),
            (TypeHint::Int, "@int"),
            (TypeHint::String, "@string"),
        ];

        for (ty_hint, expected) in cases {
            let ty = TypeHintAndHeap::new(db, Heap::Local, ty_hint);
            let output = pretty_type_hint_and_heap(db, ty);
            assert_eq!(output, expected, "Pretty print mismatch for {}", expected);
        }
    }

    #[test]
    fn test_pretty_heap_annotations() {
        let db = Database::default();
        test_pretty_heap_annotations_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_heap_annotations_inner<'db>(db: &'db dyn salsa::Database) {
        // Local heap.
        let local = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        assert_eq!(pretty_type_hint_and_heap(db, local), "@u32");

        // Global heap.
        let global = TypeHintAndHeap::new(db, Heap::Global, TypeHint::U32);
        assert_eq!(pretty_type_hint_and_heap(db, global), "#u32");

        // Omitted heap.
        let omitted = TypeHintAndHeap::new(db, Heap::Omitted, TypeHint::U32);
        assert_eq!(pretty_type_hint_and_heap(db, omitted), "u32");
    }

    #[test]
    fn test_pretty_expr_bool() {
        let db = Database::default();
        test_pretty_expr_bool_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_expr_bool_inner<'db>(db: &'db dyn salsa::Database) {
        assert_eq!(pretty_expr_with_heap(db, Expr::True, Heap::Local), "@true");
        assert_eq!(pretty_expr_with_heap(db, Expr::False, Heap::Local), "@false");
        assert_eq!(pretty_expr_with_heap(db, Expr::True, Heap::Global), "#true");
        assert_eq!(pretty_expr_with_heap(db, Expr::True, Heap::Omitted), "true");
    }

    #[test]
    fn test_pretty_expr_none() {
        let db = Database::default();
        test_pretty_expr_none_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_expr_none_inner<'db>(db: &'db dyn salsa::Database) {
        assert_eq!(pretty_expr_with_heap(db, Expr::None, Heap::Local), "@none");
    }

    #[test]
    fn test_pretty_type_list() {
        let db = Database::default();
        test_pretty_type_list_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_list_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintList;

        let elem = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let list = TypeHint::List(TypeHintList { element_type: elem });
        let list_ty = TypeHintAndHeap::new(db, Heap::Local, list);

        assert_eq!(pretty_type_hint_and_heap(db, list_ty), "@[@u32]");
    }

    #[test]
    fn test_pretty_type_option() {
        let db = Database::default();
        test_pretty_type_option_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_option_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintOption;

        let inner = TypeHintAndHeap::new(db, Heap::Local, TypeHint::String);
        let opt = TypeHint::Option(TypeHintOption { inner_type: inner });
        let opt_ty = TypeHintAndHeap::new(db, Heap::Local, opt);

        assert_eq!(pretty_type_hint_and_heap(db, opt_ty), "@?@string");
    }

    #[test]
    fn test_pretty_type_result() {
        let db = Database::default();
        test_pretty_type_result_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_result_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintResult;

        let inner = TypeHintAndHeap::new(db, Heap::Local, TypeHint::I32);
        let res = TypeHint::Result(TypeHintResult { inner_type: inner });
        let res_ty = TypeHintAndHeap::new(db, Heap::Local, res);

        assert_eq!(pretty_type_hint_and_heap(db, res_ty), "@!@i32");
    }

    #[test]
    fn test_pretty_type_map() {
        let db = Database::default();
        test_pretty_type_map_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_map_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintMap;

        let key = TypeHintAndHeap::new(db, Heap::Local, TypeHint::String);
        let value = TypeHintAndHeap::new(db, Heap::Local, TypeHint::I32);
        let map = TypeHint::Map(TypeHintMap { key_type: key, value_type: value });
        let map_ty = TypeHintAndHeap::new(db, Heap::Local, map);

        assert_eq!(pretty_type_hint_and_heap(db, map_ty), "@map<@string, @i32>");
    }

    #[test]
    fn test_pretty_type_set() {
        let db = Database::default();
        test_pretty_type_set_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_set_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintSet;

        let elem = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U64);
        let set = TypeHint::Set(TypeHintSet { element_type: elem });
        let set_ty = TypeHintAndHeap::new(db, Heap::Local, set);

        assert_eq!(pretty_type_hint_and_heap(db, set_ty), "@set<@u64>");
    }

    #[test]
    fn test_pretty_type_tuple() {
        let db = Database::default();
        test_pretty_type_tuple_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_tuple_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::TypeHintAnonTuple;

        let f1 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let f2 = TypeHintAndHeap::new(db, Heap::Local, TypeHint::Bool);
        let tuple = TypeHint::AnonTuple(TypeHintAnonTuple { fields: vec![f1, f2] });
        let tuple_ty = TypeHintAndHeap::new(db, Heap::Local, tuple);

        assert_eq!(pretty_type_hint_and_heap(db, tuple_ty), "@(@u32, @bool)");
    }

    #[test]
    fn test_pretty_type_struct() {
        let db = Database::default();
        test_pretty_type_struct_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_type_struct_inner<'db>(db: &'db dyn salsa::Database) {
        use datalove_datalit::ast::{TypeHintAnonStruct, TypeHintNamedField};
        use bct::text::InternedText;

        let f1_ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::I32);
        let f1 = TypeHintNamedField {
            name: InternedText::new(db, "x".to_string()),
            type_hint: f1_ty,
        };
        let f2_ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::I32);
        let f2 = TypeHintNamedField {
            name: InternedText::new(db, "y".to_string()),
            type_hint: f2_ty,
        };

        let struct_ty = TypeHint::AnonStruct(TypeHintAnonStruct { fields: vec![f1, f2] });
        let full_ty = TypeHintAndHeap::new(db, Heap::Local, struct_ty);

        assert_eq!(pretty_type_hint_and_heap(db, full_ty), "@{x: @i32, y: @i32}");
    }

    #[test]
    fn test_pretty_output_no_trailing_whitespace() {
        let db = Database::default();
        test_pretty_output_no_trailing_whitespace_inner(&db);
    }

    #[salsa::tracked]
    fn test_pretty_output_no_trailing_whitespace_inner<'db>(db: &'db dyn salsa::Database) {
        // Verify no trailing whitespace in output.
        let types = [
            TypeHint::Bool,
            TypeHint::U32,
            TypeHint::String,
        ];

        for ty_hint in types {
            let ty = TypeHintAndHeap::new(db, Heap::Local, ty_hint);
            let output = pretty_type_hint_and_heap(db, ty);
            assert!(!output.ends_with(' '), "Output has trailing space: '{}'", output);
            assert!(!output.ends_with('\t'), "Output has trailing tab: '{}'", output);
        }
    }
}
