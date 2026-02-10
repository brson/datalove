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
/// output in the format `: type / @value`.
pub fn pretty_print_runtime_value<'db>(
    db: &'db dyn crate::Db,
    ty: &Type<'db>,
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
        let value_str = if output_string.data.is_null() || output_string.size == datalove_rtdt::Index::ZERO {
            String::new()
        } else {
            let bytes = std::slice::from_raw_parts(output_string.data, output_string.size.as_usize());
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
        pretty_type(db, ty, &mut type_str);
        type_str.push_str(" / ");
        type_str.push_str(&value_str);

        Ok(type_str)
    }
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
        Type::Index => out.push_str("index"),
        Type::Offset => out.push_str("offset"),
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
                pretty_type(db, field, out);
            }
            if fields.len() == 1 {
                out.push(',');
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
                pretty_type(db, &field.ty, out);
            }
            out.push('}');
        }


        Type::List(l) => {
            out.push('[');
            pretty_type(db, &*l.element_type.clone(), out);
            out.push(']');
        }

        Type::Map(m) => {
            out.push_str("%{");
            pretty_type(db, &*m.key_type.clone(), out);
            out.push_str(" = ");
            pretty_type(db, &*m.value_type.clone(), out);
            out.push('}');
        }

        Type::Set(s) => {
            out.push_str("#{");
            pretty_type(db, &*s.element_type.clone(), out);
            out.push('}');
        }

        Type::Option(o) => {
            out.push('?');
            pretty_type(db, &*o.inner_type.clone(), out);
        }

        Type::Result(r) => {
            out.push('!');
            pretty_type(db, &*r.inner_type.clone(), out);
        }

        Type::Tensor(t) => {
            out.push_str("[|");
            pretty_type(db, &*t.element_type.clone(), out);
            out.push_str(", ");
            out.push_str(&t.rank.S());
            out.push_str("|]");
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
                pretty_type(db, &col.ty, out);
            }
            out.push_str(" |}");
        }

        Type::Atom(a) => {
            out.push_str("atom ");
            out.push_str(a.name.as_str(db));
        }

        Type::Term(t) => {
            out.push_str("term ");
            out.push_str(t.name.as_str(db));
            out.push(' ');
            pretty_type(db, &t.payload, out);
        }

        Type::Enum(e) => {
            out.push_str("enum{");
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                if let Some(payload) = &v.payload {
                    out.push_str("term ");
                    out.push_str(v.name.as_str(db));
                    out.push(' ');
                    pretty_type(db, payload, out);
                } else {
                    out.push_str("atom ");
                    out.push_str(v.name.as_str(db));
                }
            }
            out.push('}');
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
    if let Some(type_hint) = expr.type_hint(db) {
        out.push_str(": ");
        pretty_type_hint(db, type_hint.clone(), out);
        out.push_str(" / ");
    }

    // Print expression.
    let expr_inner = expr.expr(db);
    pretty_expr(db, expr_inner, out, indent);
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
            let fields = &t.fields;
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pretty_type_hint(db, field.clone(), out);
            }
            if fields.len() == 1 {
                out.push(',');
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

fn pretty_expr<'db>(
    db: &'db dyn crate::Db,
    expr: &Expr<'db>,
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
            // Trailing comma for 1-tuples to distinguish from grouping parens.
            if elements.len() == 1 {
                out.push(',');
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
            out.push_str("%{");
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
            out.push_str("#{");
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
            out.push_str("[| ");
            let shape = &t.shape;
            let elements = &t.elements;
            pretty_tensor_elements(db, shape, elements, out, indent);
            out.push_str(" |]");
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

/// Pretty-print tensor elements with multi-comma layout derived from shape.
///
/// 1D: space-separated elements.
/// 2D: single-comma between rows, space-separated within rows.
/// 3D: double-comma between slabs, single-comma between rows.
/// etc.
fn pretty_tensor_elements<'db>(
    db: &'db dyn crate::Db,
    shape: &[u32],
    elements: &[ExprFull<'db>],
    out: &mut String,
    indent: usize,
) {
    if elements.is_empty() {
        return;
    }

    let rank = shape.len();
    if rank == 0 {
        return;
    }

    // Recursively print groups with appropriate comma separators.
    pretty_tensor_group(db, shape, elements, 0, out, indent);

    // When the outermost dimension is 1, the highest comma level (rank - 1)
    // never appears as a separator. Emit trailing commas so the parser can
    // infer the correct rank.
    if rank > 1 && shape[0] == 1 {
        for _ in 0..(rank - 1) {
            out.push(',');
        }
    }
}

/// Recursively print a tensor group at the given dimension level.
///
/// `dim` is the current dimension index (0 = outermost).
fn pretty_tensor_group<'db>(
    db: &'db dyn crate::Db,
    shape: &[u32],
    elements: &[ExprFull<'db>],
    dim: usize,
    out: &mut String,
    indent: usize,
) {
    let rank = shape.len();

    if dim == rank - 1 {
        // Innermost dimension: space-separated elements.
        for (i, elem) in elements.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            pretty_expr_full(db, *elem, out, indent);
        }
        return;
    }

    // Calculate the number of elements per group at this level.
    let group_size: usize = shape[dim + 1..].iter().map(|&d| d as usize).product();
    let num_groups = shape[dim] as usize;
    // Comma count for this level: rank - dim - 1 commas.
    let comma_count = rank - dim - 1;

    for (i, chunk) in elements.chunks(group_size).enumerate().take(num_groups) {
        if i > 0 {
            for _ in 0..comma_count {
                out.push(',');
            }
            out.push(' ');
        }
        pretty_tensor_group(db, shape, chunk, dim + 1, out, indent);
    }
}
