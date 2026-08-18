//! Type descriptor emission for C code generation.

use std::collections::HashSet;
use std::fmt::Write;

use datalove_datafun_ir::{IrCodeUnit, IrType};
use crate::types::{self, compute_option_layout, compute_result_layout, compute_tuple_field_offsets};
use crate::CAotCompiler;

/// Collect all types used in a script unit.
pub fn collect_types_from_script_unit(unit: &IrCodeUnit, types: &mut HashSet<IrType>) {
    collect_types_from_code_unit(unit, types);
    for nested in &unit.nested_units {
        collect_types_from_code_unit(nested, types);
    }
}

/// Collect all types used in a code unit.
pub fn collect_types_from_code_unit(unit: &IrCodeUnit, types: &mut HashSet<IrType>) {
    // Collect from value types.
    for ty in &unit.value_types {
        collect_type_recursive(ty, types);
    }

    // Collect from slot types.
    for ty in &unit.slot_types {
        collect_type_recursive(ty, types);
    }

    // Collect from function context if present.
    if let Some(func_ctx) = unit.function_context() {
        for ty in &func_ctx.param_types {
            collect_type_recursive(ty, types);
        }
        collect_type_recursive(&func_ctx.return_type, types);
    }
}

/// Collect a type and all its nested types.
fn collect_type_recursive(ty: &IrType, types: &mut HashSet<IrType>) {
    types.insert(ty.clone());

    match ty {
        IrType::List(elem) | IrType::Set(elem) => {
            collect_type_recursive(elem, types);
        }
        IrType::Map(key, val) => {
            collect_type_recursive(key, types);
            collect_type_recursive(val, types);
        }
        IrType::Tuple(fields) => {
            for field in fields {
                collect_type_recursive(field, types);
            }
        }
        IrType::Struct(fields) => {
            for (_, field_ty) in fields {
                collect_type_recursive(field_ty, types);
            }
        }
        IrType::Enum(variants) => {
            for (_, payload) in variants {
                if let Some(payload_ty) = payload {
                    collect_type_recursive(payload_ty, types);
                }
            }
        }
        IrType::Atom(_) => {}
        IrType::Term(_, payload) => {
            collect_type_recursive(payload, types);
        }
        IrType::Option(inner) => {
            collect_type_recursive(inner, types);
        }
        IrType::Result(ok) => {
            collect_type_recursive(ok, types);
            // Also need Error type.
            types.insert(IrType::Error);
        }
        IrType::Tensor(elem, _) => {
            collect_type_recursive(elem, types);
        }
        IrType::Table(columns) => {
            for (_, col_ty) in columns {
                collect_type_recursive(col_ty, types);
            }
        }
        IrType::Ref(inner) => {
            collect_type_recursive(inner, types);
        }
        _ => {}
    }
}

/// Compute the "depth" of a type for dependency ordering.
/// Primitive types have depth 0, composite types have depth = 1 + max(child depths).
pub fn type_depth(ty: &IrType) -> u32 {
    match ty {
        IrType::Unit | IrType::Bool |
        IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64 |
        IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 |
        IrType::Index | IrType::Offset |
        IrType::F32 | IrType::F64 |
        IrType::Int | IrType::String | IrType::Data | IrType::Error => 0,

        IrType::List(elem) | IrType::Set(elem) => 1 + type_depth(elem),
        IrType::Map(key, val) => 1 + type_depth(key).max(type_depth(val)),
        IrType::Tuple(fields) => {
            1 + fields.iter().map(type_depth).max().unwrap_or(0)
        }
        IrType::Struct(fields) => {
            1 + fields.iter().map(|(_, ty)| type_depth(ty)).max().unwrap_or(0)
        }
        IrType::Enum(variants) => {
            1 + variants.iter()
                .filter_map(|(_, p)| p.as_ref().map(type_depth))
                .max()
                .unwrap_or(0)
        }
        IrType::Atom(_) => 0,
        IrType::Term(_, payload) => 1 + type_depth(payload),
        IrType::Option(inner) => 1 + type_depth(inner),
        IrType::Result(ok) => 1 + type_depth(ok),
        IrType::Tensor(elem, _) => 1 + type_depth(elem),
        IrType::Table(columns) => {
            1 + columns.iter().map(|(_, ty)| type_depth(ty)).max().unwrap_or(0)
        }
        IrType::Ref(inner) => 1 + type_depth(inner),
    }
}

/// Get the type tag for a type.
fn type_tag(ty: &IrType) -> u8 {
    match ty {
        IrType::Bool => 0x01,
        IrType::U8 => 0x10,
        IrType::I8 => 0x11,
        IrType::U16 => 0x12,
        IrType::I16 => 0x13,
        IrType::U32 => 0x14,
        IrType::I32 => 0x15,
        IrType::U64 => 0x16,
        IrType::I64 => 0x17,
        IrType::Index => 0x18,
        IrType::Offset => 0x19,
        IrType::F32 => 0x20,
        IrType::F64 => 0x21,
        IrType::Int => 0x30,
        IrType::Unit => 0x40, // Use Tuple tag for Unit
        IrType::Tuple(_) => 0x40,
        IrType::Struct(_) => 0x41,
        IrType::Enum(_) => 0x42,
        IrType::Atom(_) => 0x43,
        IrType::Term(_, _) => 0x44,
        IrType::List(_) => 0x50,
        IrType::String => 0x51,
        IrType::Map(_, _) => 0x52,
        IrType::Set(_) => 0x53,
        IrType::Tensor(_, _) => 0x54,
        IrType::Table(_) => 0x55,
        IrType::Option(_) => 0x60,
        IrType::Result(_) => 0x61,
        IrType::Data => 0x70,
        IrType::Error => 0x71,
        IrType::Ref(_) => 0x10, // Treat as pointer (U8 tag, but size is 8)
    }
}

/// Emit a type descriptor.
pub fn emit_tydesc(
    out: &mut String,
    name: &str,
    ty: &IrType,
    compiler: &mut CAotCompiler,
) -> Result<(), crate::CAotError> {
    let layout = types::ir_type_to_crepr(ty).layout();
    let tag = type_tag(ty);

    match ty {
        IrType::Unit => {
            // Unit is a zero-field tuple.
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x40, .size = 0, .align = 1, .type_info = {{ .tuple = {{ .num_fields = 0, .fields = DANGLING(dtlv_tuple_field_t) }} }} }};", name).unwrap();
        }

        // Primitive types with no type_info.
        IrType::Bool | IrType::U8 | IrType::I8 | IrType::U16 | IrType::I16 |
        IrType::U32 | IrType::I32 | IrType::U64 | IrType::I64 |
        IrType::Index | IrType::Offset | IrType::F32 | IrType::F64 |
        IrType::Int | IrType::String | IrType::Data | IrType::Error => {
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x{:02x}, .size = {}, .align = {}, .type_info = {{ .nothing = {{}} }} }};",
                name, tag, layout.size, layout.align).unwrap();
        }

        IrType::List(elem) => {
            let elem_tydesc = compiler.get_tydesc_name(elem);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x50, .size = {}, .align = {}, .type_info = {{ .list = {{ .element_tydesc = &{} }} }} }};",
                name, layout.size, layout.align, elem_tydesc).unwrap();
        }

        IrType::Set(elem) => {
            let elem_tydesc = compiler.get_tydesc_name(elem);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x53, .size = {}, .align = {}, .type_info = {{ .set = {{ .element_tydesc = &{} }} }} }};",
                name, layout.size, layout.align, elem_tydesc).unwrap();
        }

        IrType::Map(key, val) => {
            let key_tydesc = compiler.get_tydesc_name(key);
            let val_tydesc = compiler.get_tydesc_name(val);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x52, .size = {}, .align = {}, .type_info = {{ .map = {{ .key_tydesc = &{}, .value_tydesc = &{} }} }} }};",
                name, layout.size, layout.align, key_tydesc, val_tydesc).unwrap();
        }

        IrType::Tuple(fields) => {
            let offsets = compute_tuple_field_offsets(fields);
            let fields_name = format!("{}_fields", name);

            if fields.is_empty() {
                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x40, .size = {}, .align = {}, .type_info = {{ .tuple = {{ .num_fields = 0, .fields = DANGLING(dtlv_tuple_field_t) }} }} }};",
                    name, layout.size, layout.align).unwrap();
            } else {
                // Emit field array.
                write!(out, "static const dtlv_tuple_field_t {}[] = {{ ", fields_name).unwrap();
                for (i, (field_ty, offset)) in fields.iter().zip(offsets.iter()).enumerate() {
                    if i > 0 { write!(out, ", ").unwrap(); }
                    let field_tydesc = compiler.get_tydesc_name(field_ty);
                    write!(out, "{{ .offset = {}, .tydesc = &{} }}", offset, field_tydesc).unwrap();
                }
                writeln!(out, " }};").unwrap();

                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x40, .size = {}, .align = {}, .type_info = {{ .tuple = {{ .num_fields = {}, .fields = {} }} }} }};",
                    name, layout.size, layout.align, fields.len(), fields_name).unwrap();
            }
        }

        IrType::Struct(fields) => {
            let field_types: Vec<_> = fields.iter().map(|(_, ty)| ty.clone()).collect();
            let offsets = compute_tuple_field_offsets(&field_types);
            let fields_name = format!("{}_fields", name);

            if fields.is_empty() {
                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x41, .size = {}, .align = {}, .type_info = {{ .struct_ = {{ .fields = DANGLING(dtlv_struct_field_t), .num_fields = 0 }} }} }};",
                    name, layout.size, layout.align).unwrap();
            } else {
                // Emit field array.
                write!(out, "static const dtlv_struct_field_t {}[] = {{ ", fields_name).unwrap();
                for (i, ((field_name, field_ty), offset)) in fields.iter().zip(offsets.iter()).enumerate() {
                    if i > 0 { write!(out, ", ").unwrap(); }
                    let field_tydesc = compiler.get_tydesc_name(field_ty);
                    write!(out, "{{ .name = \"{}\", .name_len = {}, .offset = {}, .tydesc = &{} }}",
                        field_name, field_name.len(), offset, field_tydesc).unwrap();
                }
                writeln!(out, " }};").unwrap();

                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x41, .size = {}, .align = {}, .type_info = {{ .struct_ = {{ .fields = {}, .num_fields = {} }} }} }};",
                    name, layout.size, layout.align, fields_name, fields.len()).unwrap();
            }
        }

        IrType::Enum(variants) => {
            let variants_name = format!("{}_variants", name);

            if variants.is_empty() {
                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x42, .size = {}, .align = {}, .type_info = {{ .enum_ = {{ .variants = DANGLING(dtlv_enum_variant_t), .num_variants = 0 }} }} }};",
                    name, layout.size, layout.align).unwrap();
            } else {
                // Each variant's payload is placed at its own alignment, matching
                // what codegen writes and what the runtime recomputes.
                let variant_offsets = types::compute_enum_variant_offsets(variants);

                // Emit variant array.
                write!(out, "static const dtlv_enum_variant_t {}[] = {{ ", variants_name).unwrap();
                for (i, (variant_name, payload)) in variants.iter().enumerate() {
                    if i > 0 { write!(out, ", ").unwrap(); }
                    let payload_ptr = if let Some(payload_ty) = payload {
                        let payload_tydesc = compiler.get_tydesc_name(payload_ty);
                        format!("&{}", payload_tydesc)
                    } else {
                        "NULL".to_string()
                    };
                    write!(out, "{{ .name = \"{}\", .name_len = {}, .offset = {}, .payload = {} }}",
                        variant_name, variant_name.len(), variant_offsets[i], payload_ptr).unwrap();
                }
                writeln!(out, " }};").unwrap();

                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x42, .size = {}, .align = {}, .type_info = {{ .enum_ = {{ .variants = {}, .num_variants = {} }} }} }};",
                    name, layout.size, layout.align, variants_name, variants.len()).unwrap();
            }
        }

        IrType::Atom(atom_name) => {
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x43, .size = 0, .align = 1, .type_info = {{ .atom = {{ .name = \"{}\", .name_len = {} }} }} }};",
                name, atom_name, atom_name.len()).unwrap();
        }

        IrType::Term(term_name, payload_ty) => {
            let payload_tydesc = compiler.get_tydesc_name(payload_ty);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x44, .size = {}, .align = {}, .type_info = {{ .term = {{ .name = \"{}\", .name_len = {}, .payload = &{} }} }} }};",
                name, layout.size, layout.align, term_name, term_name.len(), payload_tydesc).unwrap();
        }

        IrType::Option(inner) => {
            let inner_tydesc = compiler.get_tydesc_name(inner);
            let opt_layout = compute_option_layout(inner);
            let inner_layout = types::ir_type_to_crepr(inner).layout();
            let payload_offset = types::align_up(1, inner_layout.align);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x60, .size = {}, .align = {}, .type_info = {{ .option = {{ .inner_tydesc = &{}, .payload_offset = {} }} }} }};",
                name, opt_layout.size, opt_layout.align, inner_tydesc, payload_offset).unwrap();
        }

        IrType::Result(ok) => {
            let ok_tydesc = compiler.get_tydesc_name(ok);
            let res_layout = compute_result_layout(ok);
            let ok_layout = types::ir_type_to_crepr(ok).layout();
            let error_align = std::mem::align_of::<datalove_rtdt::Error>() as u32;
            let max_align = ok_layout.align.max(error_align);
            let payload_offset = types::align_up(1, max_align);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x61, .size = {}, .align = {}, .type_info = {{ .result = {{ .ok_tydesc = &{}, .payload_offset = {} }} }} }};",
                name, res_layout.size, res_layout.align, ok_tydesc, payload_offset).unwrap();
        }

        IrType::Tensor(elem, rank) => {
            let elem_tydesc = compiler.get_tydesc_name(elem);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x54, .size = {}, .align = {}, .type_info = {{ .tensor = {{ .element_tydesc = &{}, .rank = {} }} }} }};",
                name, layout.size, layout.align, elem_tydesc, rank).unwrap();
        }

        IrType::Table(columns) => {
            let cols_name = format!("{}_cols", name);
            let num_cols = columns.len();

            if columns.is_empty() {
                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x55, .size = {}, .align = {}, .type_info = {{ .table = {{ .num_columns = 0, .columns = DANGLING(dtlv_table_column_t) }} }} }};",
                    name, layout.size, layout.align).unwrap();
            } else {
                // Emit column info array (TyInfoTableColumn: name, name_len, tydesc).
                writeln!(out, "static const dtlv_table_column_t {}[] = {{", cols_name).unwrap();
                for (i, (col_name, col_ty)) in columns.iter().enumerate() {
                    let col_tydesc = compiler.get_tydesc_name(col_ty);
                    let comma = if i + 1 < columns.len() { "," } else { "" };
                    writeln!(out, "    {{ .name = \"{}\", .name_len = {}, .tydesc = &{} }}{}",
                        col_name, col_name.len(), col_tydesc, comma).unwrap();
                }
                writeln!(out, "}};").unwrap();

                writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x55, .size = {}, .align = {}, .type_info = {{ .table = {{ .num_columns = {}, .columns = {} }} }} }};",
                    name, layout.size, layout.align, num_cols, cols_name).unwrap();
            }
        }

        IrType::Ref(inner) => {
            // Ref is just a pointer - emit as a primitive.
            let _inner_tydesc = compiler.get_tydesc_name(inner);
            writeln!(out, "static const dtlv_tydesc_t {} = {{ .type_tag = 0x10, .size = 8, .align = 8, .type_info = {{ .nothing = {{}} }} }};", name).unwrap();
        }
    }

    Ok(())
}
