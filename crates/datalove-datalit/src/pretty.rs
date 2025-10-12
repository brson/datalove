use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::*;

/// Pretty print a datalove literal expression.
///
/// Returns a canonical string representation that can be parsed back.
#[salsa::tracked]
pub fn pretty_print<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
) -> String {
    let mut output = String::new();
    pretty_expr_full(db, expr, &mut output, 0);
    output
}

fn pretty_expr_full<'db>(
    db: &'db dyn crate::Db,
    expr: ExprFull<'db>,
    out: &mut String,
    indent: usize,
) {
    // Print type hint if present.
    if let Some(type_hint_and_heap) = expr.type_hint(db) {
        out.push_str(": ");
        pretty_type_hint_and_heap(db, type_hint_and_heap, out);
        out.push_str(" / ");
    }

    // Print expression.
    let expr_and_heap = expr.expr(db);
    pretty_expr_and_heap(db, *expr_and_heap, out, indent);
}

fn pretty_type_hint_and_heap<'db>(
    db: &'db dyn crate::Db,
    th: TypeHintAndHeap<'db>,
    out: &mut String,
) {
    // Print heap sigil.
    match th.heap(db) {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }

    pretty_type_hint(db, th.type_hint(db), out);
}

fn pretty_type_hint<'db>(
    db: &'db dyn crate::Db,
    th: TypeHint<'db>,
    out: &mut String,
) {
    match th {
        TypeHint::Bool => out.push_str("bool"),
        TypeHint::U32 => out.push_str("u32"),
        TypeHint::F32 => out.push_str("f32"),
        TypeHint::Int => out.push_str("int"),
        TypeHint::String => out.push_str("string"),
        TypeHint::Data => out.push_str("data"),
        TypeHint::Error => out.push_str("error"),

        TypeHint::AnonTuple(t) => {
            out.push('(');
            let fields = t.fields(db);
            for (i, field) in fields.iter().enumerate() {
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
            let fields = t.fields(db);
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_type_hint_and_heap(db, *field, out);
            }
            out.push(')');
        }

        TypeHint::AnonStruct(s) => {
            out.push('{');
            let fields = s.fields(db);
            for (i, field) in fields.iter().enumerate() {
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
            let fields = s.fields(db);
            for (i, field) in fields.iter().enumerate() {
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
            let variants = e.variants(db);
            for (i, variant) in variants.iter().enumerate() {
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
            let variants = e.variants(db);
            for (i, variant) in variants.iter().enumerate() {
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

        TypeHint::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message(db).as_str(db));
            out.push('>');
        }
    }
}

fn pretty_expr_and_heap<'db>(
    db: &'db dyn crate::Db,
    eh: ExprAndHeap<'db>,
    out: &mut String,
    indent: usize,
) {
    // Print heap sigil.
    match eh.heap(db) {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }

    pretty_expr(db, eh.expr(db), out, indent);
}

fn pretty_expr<'db>(
    db: &'db dyn crate::Db,
    expr: Expr<'db>,
    out: &mut String,
    indent: usize,
) {
    match expr {
        Expr::True => out.push_str("true"),
        Expr::False => out.push_str("false"),
        Expr::None => out.push_str("none"),

        Expr::Int(i) => {
            out.push_str(i.value(db).as_str(db));
        }

        Expr::Float(f) => {
            out.push_str(f.value(db).as_str(db));
        }

        Expr::String(s) => {
            out.push_str(s.value(db).as_str(db));
        }

        Expr::AnonTuple(t) => {
            out.push('(');
            let elements = t.elements(db);
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, *elem, out, indent);
            }
            out.push(')');
        }

        Expr::NamedTuple(t) => {
            out.push_str("tuple ");
            out.push_str(t.name(db).as_str(db));
            out.push_str(" (");
            let elements = t.elements(db);
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, *elem, out, indent);
            }
            out.push(')');
        }

        Expr::AnonStruct(s) => {
            out.push('{');
            let fields = s.fields(db);
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name(db).as_str(db));
                out.push_str(" = ");
                pretty_expr_full(db, field.value(db), out, indent);
            }
            out.push('}');
        }

        Expr::NamedStruct(s) => {
            out.push_str("struct ");
            out.push_str(s.name(db).as_str(db));
            out.push_str(" {");
            let fields = s.fields(db);
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name(db).as_str(db));
                out.push_str(" = ");
                pretty_expr_full(db, field.value(db), out, indent);
            }
            out.push('}');
        }

        Expr::AnonEnum(e) => {
            out.push_str("enum ");
            out.push_str(e.variant_name(db).as_str(db));
            if let Some(payload) = e.payload(db) {
                out.push('(');
                pretty_expr_full(db, payload, out, indent);
                out.push(')');
            }
        }

        Expr::NamedEnum(e) => {
            out.push_str("enum ");
            out.push_str(e.enum_name(db).as_str(db));
            out.push('.');
            out.push_str(e.variant_name(db).as_str(db));
            if let Some(payload) = e.payload(db) {
                out.push('(');
                pretty_expr_full(db, payload, out, indent);
                out.push(')');
            }
        }

        Expr::List(l) => {
            out.push('[');
            let elements = l.elements(db);
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, *elem, out, indent);
            }
            out.push(']');
        }

        Expr::Map(m) => {
            out.push_str("map {");
            let entries = m.entries(db);
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, entry.key(db), out, indent);
                out.push_str(" = ");
                pretty_expr_full(db, entry.value(db), out, indent);
            }
            out.push('}');
        }

        Expr::Set(s) => {
            out.push_str("set {");
            let elements = s.elements(db);
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, *elem, out, indent);
            }
            out.push('}');
        }

        Expr::Data(d) => {
            out.push_str("data ");
            pretty_expr_full(db, d.value(db), out, indent);
        }

        Expr::Err(e) => {
            out.push_str("error ");
            pretty_expr_full(db, e.value(db), out, indent);
        }

        Expr::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message(db).as_str(db));
            out.push('>');
        }
    }
}
