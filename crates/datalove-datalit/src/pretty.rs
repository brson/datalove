use rmx::prelude::*;
use crate::ast::*;
use crate::tycheck::*;

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

/// Pretty print a runtime value with type hint.
///
/// Combines the type from typechecking with the runtime value to produce
/// output in the format `: @type / @value`.
pub fn pretty_print_runtime_value<'db>(
    db: &'db dyn crate::Db,
    ty: &TypeAndHeap<'db>,
    value_ptr: *const u8,
    tydesc_ptr: *const datalove_rtdt::TyDesc,
) -> Result<String, String> {
    unsafe {
        // Initialize runtime for pretty printing.
        let rt_handle = datalove_rt::c::dtlv_rti_init();
        if rt_handle.is_null() {
            return Err("Failed to initialize runtime".S());
        }

        // Create string tydesc.
        let string_tydesc = datalove_rtdt::TyDesc {
            type_tag: datalove_rtdt::TyTag::String,
            size: std::mem::size_of::<datalove_rtdt::String>() as u32,
            align: std::mem::align_of::<datalove_rtdt::String>() as u32,
            type_info: datalove_rtdt::TyInfo {
                nothing: datalove_rtdt::TyInfoNothing,
            },
        };

        // Create output string.
        let mut output_string = std::mem::MaybeUninit::<datalove_rtdt::String>::uninit();
        let status = datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            output_string.as_mut_ptr() as *mut u8,
            &string_tydesc,
        );

        if status != datalove_rt::c::RtStatus::Ok {
            datalove_rt::c::dtlv_rti_shutdown(rt_handle);
            return Err("Failed to create output string".S());
        }

        let mut output_string = output_string.assume_init();

        // Pretty-print value using runtime.
        let status = datalove_rt::c::dtlv_rti_pretty_print_local(
            rt_handle,
            value_ptr,
            tydesc_ptr,
            &mut output_string as *mut datalove_rtdt::String as *mut u8,
            &string_tydesc,
        );

        if status != datalove_rt::c::RtStatus::Ok {
            datalove_rt::c::dtlv_rti_string_destroy_local(
                rt_handle,
                &mut output_string as *mut datalove_rtdt::String as *mut u8,
                &string_tydesc,
            );
            datalove_rt::c::dtlv_rti_shutdown(rt_handle);
            return Err("Failed to pretty-print value".S());
        }

        // Extract value string.
        let value_str = if output_string.data.is_null() || output_string.size == 0 {
            String::new()
        } else {
            let bytes = std::slice::from_raw_parts(output_string.data, output_string.size as usize);
            String::from_utf8_lossy(bytes).S()
        };

        // Cleanup runtime string.
        datalove_rt::c::dtlv_rti_string_destroy_local(
            rt_handle,
            &mut output_string as *mut datalove_rtdt::String as *mut u8,
            &string_tydesc,
        );
        datalove_rt::c::dtlv_rti_shutdown(rt_handle);

        // Build type hint string.
        let mut type_str = String::new();
        type_str.push_str(": ");
        pretty_type_and_heap(db, ty, &mut type_str);
        type_str.push_str(" / ");
        type_str.push_str(&value_str);

        Ok(type_str)
    }
}

fn pretty_type_and_heap<'db>(
    db: &'db dyn crate::Db,
    ty: &TypeAndHeap<'db>,
    out: &mut String,
) {
    // Print heap sigil.
    match ty.heap(db) {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }

    pretty_type(db, ty.ty(db), out);
}

fn pretty_type<'db>(
    db: &'db dyn crate::Db,
    ty: &Type<'db>,
    out: &mut String,
) {
    match ty {
        Type::Bool => out.push_str("bool"),
        Type::U8 => out.push_str("u8"),
        Type::I8 => out.push_str("i8"),
        Type::U16 => out.push_str("u16"),
        Type::I16 => out.push_str("i16"),
        Type::U32 => out.push_str("u32"),
        Type::I32 => out.push_str("i32"),
        Type::U64 => out.push_str("u64"),
        Type::I64 => out.push_str("i64"),
        Type::Usize => out.push_str("usize"),
        Type::Isize => out.push_str("isize"),
        Type::F32 => out.push_str("f32"),
        Type::F64 => out.push_str("f64"),
        Type::Int => out.push_str("int"),
        Type::String => out.push_str("string"),
        Type::Data => out.push_str("data"),
        Type::Error => out.push_str("error"),

        Type::AnonTuple(t) => {
            out.push('(');
            let fields = &t.fields;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_type_and_heap(db, field, out);
            }
            out.push(')');
        }

        Type::AnonStruct(s) => {
            out.push('{');
            let fields = &s.fields;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name.as_str(db));
                out.push_str(": ");
                pretty_type_and_heap(db, &field.ty, out);
            }
            out.push('}');
        }

        Type::AnonEnum(e) => {
            out.push_str("enum {");
            let variants = &e.variants;
            for (i, variant) in variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(variant.name.as_str(db));
                if let Some(payload) = &variant.payload {
                    out.push('(');
                    pretty_type_and_heap(db, payload, out);
                    out.push(')');
                }
            }
            out.push('}');
        }

        Type::List(l) => {
            out.push('[');
            pretty_type_and_heap(db, &l.element_type, out);
            out.push(']');
        }

        Type::Map(m) => {
            out.push_str("map<");
            pretty_type_and_heap(db, &m.key_type, out);
            out.push_str(", ");
            pretty_type_and_heap(db, &m.value_type, out);
            out.push('>');
        }

        Type::Set(s) => {
            out.push_str("set<");
            pretty_type_and_heap(db, &s.element_type, out);
            out.push('>');
        }

        Type::Option(o) => {
            out.push('?');
            pretty_type_and_heap(db, &o.inner_type, out);
        }

        Type::Result(r) => {
            out.push('!');
            pretty_type_and_heap(db, &r.inner_type, out);
        }

        Type::Tensor(t) => {
            out.push_str("tensor<");
            pretty_type_and_heap(db, &t.element_type, out);
            out.push_str(", ");
            out.push_str(&t.rank.S());
            out.push('>');
        }

        Type::Table(t) => {
            out.push_str("{| ");
            let columns = &t.columns;
            for (i, col) in columns.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(col.name.as_str(db));
                out.push_str(": ");
                pretty_type_and_heap(db, &col.ty, out);
            }
            out.push_str(" |}");
        }
    }
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
    pretty_expr_and_heap(db, expr_and_heap.C(), out, indent);
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
            let fields = &t.fields;
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
            let fields = &s.fields;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name.as_str(db));
                out.push_str(": ");
                pretty_type_hint_and_heap(db, field.type_hint, out);
            }
            out.push('}');
        }

        TypeHint::AnonEnum(e) => {
            out.push_str("enum {");
            let variants = &e.variants;
            for (i, variant) in variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(variant.name.as_str(db));
                if let Some(payload) = variant.payload {
                    out.push('(');
                    pretty_type_hint_and_heap(db, payload, out);
                    out.push(')');
                }
            }
            out.push('}');
        }

        TypeHint::List(l) => {
            out.push('[');
            pretty_type_hint_and_heap(db, l.element_type, out);
            out.push(']');
        }

        TypeHint::Map(m) => {
            out.push_str("map<");
            pretty_type_hint_and_heap(db, m.key_type, out);
            out.push_str(", ");
            pretty_type_hint_and_heap(db, m.value_type, out);
            out.push('>');
        }

        TypeHint::Set(s) => {
            out.push_str("set<");
            pretty_type_hint_and_heap(db, s.element_type, out);
            out.push('>');
        }

        TypeHint::Option(o) => {
            out.push('?');
            pretty_type_hint_and_heap(db, o.inner_type, out);
        }

        TypeHint::Result(r) => {
            out.push('!');
            pretty_type_hint_and_heap(db, r.inner_type, out);
        }

        TypeHint::Tensor(t) => {
            out.push_str("tensor<");
            pretty_type_hint_and_heap(db, t.element_type, out);
            out.push_str(", ");
            out.push_str(&t.rank.S());
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
                pretty_type_hint_and_heap(db, col.type_hint, out);
            }
            out.push_str(" |}");
        }

        TypeHint::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message.as_str(db));
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
    match eh.heap {
        Heap::Local => out.push('@'),
        Heap::Global => out.push('#'),
        Heap::Omitted => {}
    }

    pretty_expr(db, eh.expr, out, indent);
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
        Expr::Some(s) => {
            out.push_str("some ");
            pretty_expr_full(db, s.payload, out, indent);
        }
        Expr::Ok(o) => {
            out.push_str("ok ");
            pretty_expr_full(db, o.payload, out, indent);
        }
        Expr::Er(e) => {
            out.push_str("er ");
            pretty_expr_full(db, e.payload, out, indent);
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
            let elements = &t.elements;
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
            let fields = &s.fields;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(field.name.as_str(db));
                out.push_str(" = ");
                pretty_expr_full(db, field.value, out, indent);
            }
            out.push('}');
        }

        Expr::AnonEnum(e) => {
            out.push_str("enum ");
            out.push_str(e.variant_name.as_str(db));
            if let Some(payload) = e.payload {
                out.push('(');
                pretty_expr_full(db, payload, out, indent);
                out.push(')');
            }
        }

        Expr::List(l) => {
            out.push('[');
            let elements = &l.elements;
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
            let entries = &m.entries;
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, entry.key, out, indent);
                out.push_str(" = ");
                pretty_expr_full(db, entry.value, out, indent);
            }
            out.push('}');
        }

        Expr::Set(s) => {
            out.push_str("set {");
            let elements = &s.elements;
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_expr_full(db, *elem, out, indent);
            }
            out.push('}');
        }

        Expr::Tensor(t) => {
            out.push_str("tensor [");
            let shape = &t.shape;
            for (i, &dim) in shape.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&dim.S());
            }
            out.push_str("] [");
            let elements = &t.elements;
            let rank = shape.len();

            if rank == 1 {
                // 1D tensor: comma-separated elements.
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    pretty_expr_full(db, *elem, out, indent);
                }
            } else {
                // 2D+ tensor: comma-separated rows, space-separated elements.
                let row_size = *shape.last().unwrap() as usize;
                for (row_idx, row) in elements.chunks(row_size).enumerate() {
                    if row_idx > 0 {
                        out.push_str(", ");
                    }
                    for (elem_idx, elem) in row.iter().enumerate() {
                        if elem_idx > 0 {
                            out.push(' ');
                        }
                        pretty_expr_full(db, *elem, out, indent);
                    }
                }
            }

            out.push(']');
        }

        Expr::Data(d) => {
            out.push_str("data ");
            pretty_expr_full(db, d.value, out, indent);
        }

        Expr::Error(e) => {
            out.push_str("error ");
            pretty_expr_full(db, e.value, out, indent);
        }

        Expr::ParseError(e) => {
            out.push_str("<parse-error: ");
            out.push_str(e.message.as_str(db));
            out.push('>');
        }

        Expr::Table(t) => {
            out.push_str("{| ");
            // Print header.
            for (i, name) in t.header.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(name.as_str(db));
            }
            // Print rows.
            for row in &t.rows {
                out.push_str("; ");
                for (i, elem) in row.elements.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    pretty_expr_full(db, *elem, out, indent);
                }
            }
            out.push_str(" |}");
        }
    }
}
