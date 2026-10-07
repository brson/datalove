//! Pretty-printing for runtime values.
//!
//! Produces valid datalit syntax that can be parsed back.

use datalove_rtdt as rtdt;
use crate::c::{LocalRtHandle, RtStatus};

/// Pretty-prints a runtime value into a string.
///
/// The string must be pre-allocated using string_create_local.
pub unsafe fn pretty_print_local(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc_ref: *const rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || value_ref.is_null() || tydesc_ref.is_null() {
        return RtStatus::Error;
    }
    if string_mut.is_null() || string_tydesc.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let ty = rtdt::TyDescRef::from_ptr(tydesc_ref);
        let string_ty = rtdt::TyDescRef::from_ptr(string_tydesc);

        if string_ty.as_ref().type_tag != rtdt::TyTag::String {
            return RtStatus::Error;
        }

        match pretty_value(rt, value_ref, ty, string_mut, string_tydesc) {
            Ok(()) => RtStatus::Ok,
            Err(()) => RtStatus::Error,
        }
    }
}

unsafe fn pretty_value(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        match tydesc.as_ref().type_tag {
            rtdt::TyTag::Bool => pretty_bool(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::U8 => pretty_u8(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::I8 => pretty_i8(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::U16 => pretty_u16(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::I16 => pretty_i16(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::U32 => pretty_u32(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::I32 => pretty_i32(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::U64 => pretty_u64(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::I64 => pretty_i64(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::Index => pretty_usize(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::Offset => pretty_isize(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::F32 => pretty_f32(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::F64 => pretty_f64(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::Int => pretty_int(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::String => pretty_string(rt, value_ref, string_mut, string_tydesc),

            rtdt::TyTag::Tuple => pretty_tuple(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Struct => pretty_struct(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Enum => pretty_enum(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Atom => pretty_atom(rt, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Term => pretty_term(rt, value_ref, tydesc, string_mut, string_tydesc),

            rtdt::TyTag::List => pretty_list(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Map => pretty_map(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Set => pretty_set(rt, value_ref, tydesc, string_mut, string_tydesc),

            rtdt::TyTag::Option => pretty_option(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Result => pretty_result(rt, value_ref, tydesc, string_mut, string_tydesc),

            rtdt::TyTag::Data => pretty_data(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::Error => pretty_error(rt, value_ref, string_mut, string_tydesc),

            rtdt::TyTag::Tensor => pretty_tensor(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Table => pretty_table(rt, value_ref, tydesc, string_mut, string_tydesc),
        }
    }
}

unsafe fn pretty_bool(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let b = &*(value_ref as *const rtdt::Bool);
        if b.0 != 0 {
            push_str(rt, string_mut, string_tydesc, b"true")
        } else {
            push_str(rt, string_mut, string_tydesc, b"false")
        }
    }
}

unsafe fn pretty_u8(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::U8);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_i8(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::I8);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_u16(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::U16);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_i16(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::I16);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_u32(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::U32);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_i32(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::I32);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_u64(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::U64);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_i64(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::I64);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_usize(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::Index);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_isize(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let n = &*(value_ref as *const rtdt::Offset);
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}


/// The magnitudes that print in positional notation.
///
/// Outside this an exponent is shorter as well as clearer: `1e16` is the
/// first magnitude past the range where a float counts whole numbers
/// exactly, and `1e-5` the first with more leading zeros than digits.
const POSITIONAL: std::ops::Range<f64> = 1e-5..1e16;

/// Give a float's text a point or an exponent, so that it reads as a float.
///
/// Rust's own formatting drops the point on a whole number, which leaves a
/// float indistinguishable from an integer, and writes an exponent's
/// mantissa the same way. Every finite float gets one or the other here, so
/// that what is printed is also what can be typed back in.
fn float_text(text: String) -> String {
    let Some(marker) = text.find('e') else {
        return if text.contains('.') { text } else { format!("{text}.0") };
    };
    if text[..marker].contains('.') {
        text
    } else {
        format!("{}.0{}", &text[..marker], &text[marker..])
    }
}

unsafe fn pretty_f32(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let f = &*(value_ref as *const rtdt::F32);
        let s = if f.0.is_nan() {
            "nan".to_string()
        } else if f.0.is_infinite() {
            if f.0.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else if POSITIONAL.contains(&(f.0.abs() as f64)) || f.0 == 0.0 {
            float_text(f.0.to_string())
        } else {
            float_text(format!("{:e}", f.0))
        };
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_f64(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let f = &*(value_ref as *const rtdt::F64);
        let s = if f.0.is_nan() {
            "nan".to_string()
        } else if f.0.is_infinite() {
            if f.0.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else if POSITIONAL.contains(&f.0.abs()) || f.0 == 0.0 {
            float_text(f.0.to_string())
        } else {
            float_text(format!("{:e}", f.0))
        };
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_int(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let int_ptr = value_ref as *const rtdt::Int;
        let s = (*int_ptr).to_decimal_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_string(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let s = &*(value_ref as *const rtdt::String);
        push_str(rt, string_mut, string_tydesc, b"\"")?;
        if !s.data.is_null() && s.size > rtdt::Index::ZERO {
            let bytes = std::slice::from_raw_parts(s.data, s.size.as_usize());
            for &byte in bytes {
                match byte {
                    b'"' => push_str(rt, string_mut, string_tydesc, b"\\\"")?,
                    b'\\' => push_str(rt, string_mut, string_tydesc, b"\\\\")?,
                    b'\n' => push_str(rt, string_mut, string_tydesc, b"\\n")?,
                    b'\r' => push_str(rt, string_mut, string_tydesc, b"\\r")?,
                    b'\t' => push_str(rt, string_mut, string_tydesc, b"\\t")?,
                    _ => push_str(rt, string_mut, string_tydesc, &[byte])?,
                }
            }
        }
        push_str(rt, string_mut, string_tydesc, b"\"")
    }
}

unsafe fn pretty_tuple(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        push_str(rt, string_mut, string_tydesc, b"(")?;

        for (i, field) in tydesc.iter_tuple_fields().enumerate() {
            if i > 0 {
                push_str(rt, string_mut, string_tydesc, b", ")?;
            }

            let field_value = value_ref.add(field.offset() as usize);
            pretty_value(rt, field_value, field.tydesc(), string_mut, string_tydesc)?;
        }

        push_str(rt, string_mut, string_tydesc, b")")
    }
}

unsafe fn pretty_struct(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        push_str(rt, string_mut, string_tydesc, b"{")?;

        for (i, field) in tydesc.iter_struct_fields().enumerate() {
            if i > 0 {
                push_str(rt, string_mut, string_tydesc, b", ")?;
            }

            push_str(rt, string_mut, string_tydesc, field.name().as_bytes())?;
            push_str(rt, string_mut, string_tydesc, b" = ")?;

            let field_value = value_ref.add(field.offset() as usize);
            pretty_value(rt, field_value, field.tydesc(), string_mut, string_tydesc)?;
        }

        push_str(rt, string_mut, string_tydesc, b"}")
    }
}

unsafe fn pretty_enum(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let enum_info = tydesc.enum_info();

        let discriminant = *(value_ref as *const u32);

        if discriminant >= enum_info.num_variants() {
            push_str(rt, string_mut, string_tydesc, b"<invalid-enum>")?;
            return Ok(());
        }

        if let core::option::Option::Some(variant) = enum_info.variant(discriminant as usize) {
            // Written as the literal is, so that what is printed reads back.
            match variant.payload() {
                core::option::Option::Some(payload_ty) => {
                    push_str(rt, string_mut, string_tydesc, b"enum { term ")?;
                    push_str(rt, string_mut, string_tydesc, variant.name().as_bytes())?;
                    push_str(rt, string_mut, string_tydesc, b" ")?;
                    let payload_value = value_ref.add(variant.offset() as usize);
                    pretty_value(rt, payload_value, payload_ty, string_mut, string_tydesc)?;
                }
                core::option::Option::None => {
                    push_str(rt, string_mut, string_tydesc, b"enum { atom ")?;
                    push_str(rt, string_mut, string_tydesc, variant.name().as_bytes())?;
                }
            }
            push_str(rt, string_mut, string_tydesc, b" }")?;
        }

        Ok(())
    }
}

unsafe fn pretty_atom(
    rt: LocalRtHandle,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let (name, _) = tydesc.atom_info();
        push_str(rt, string_mut, string_tydesc, b"atom ")?;
        push_str(rt, string_mut, string_tydesc, name.as_bytes())
    }
}

unsafe fn pretty_term(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let (name, payload_ty) = tydesc.term_info();
        push_str(rt, string_mut, string_tydesc, b"term ")?;
        push_str(rt, string_mut, string_tydesc, name.as_bytes())?;
        push_str(rt, string_mut, string_tydesc, b" ")?;
        pretty_value(rt, value_ref, payload_ty, string_mut, string_tydesc)
    }
}

unsafe fn pretty_list(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let list = &*(value_ref as *const rtdt::List);
        let elem_ty = tydesc.list_element_ty();

        push_str(rt, string_mut, string_tydesc, b"[")?;

        if !list.data.is_null() && list.size > rtdt::Index::ZERO {
            for i in 0..list.size.0 {
                if i > 0 {
                    push_str(rt, string_mut, string_tydesc, b", ")?;
                }

                let elem_value = list.data.add((i as usize) * (elem_ty.size() as usize));
                pretty_value(rt, elem_value, elem_ty, string_mut, string_tydesc)?;
            }
        }

        push_str(rt, string_mut, string_tydesc, b"]")
    }
}

unsafe fn pretty_map(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let map = &*(value_ref as *const rtdt::Map);
        let key_ty = tydesc.map_key_ty();
        let value_ty = tydesc.map_value_ty();

        push_str(rt, string_mut, string_tydesc, b"%{")?;

        if !map.root.is_null() && map.len > rtdt::Index::ZERO {
            let key_size = key_ty.size() as usize;
            let value_size = value_ty.size() as usize;

            let map_ty = crate::impls::btreemap::MapTy::of(tydesc);
            let mut current_leaf = crate::impls::btreemap::leftmost_leaf(
                map.root as *mut rtdt::MapNode, map_ty);
            let mut entry_count = 0u32;

            while !current_leaf.is_null() {
                let node_len = (*current_leaf).len;

                let layout = map_ty.leaf;
                let keys_array = (current_leaf as *const u8).add(layout.keys_offset as usize);
                let values_array = (current_leaf as *const u8).add(layout.values_offset as usize);

                for i in 0..node_len {
                    if entry_count > 0 {
                        push_str(rt, string_mut, string_tydesc, b", ")?;
                    }

                    let key_ptr = keys_array.add(i as usize * key_size);
                    let value_ptr = values_array.add(i as usize * value_size);

                    pretty_value(rt, key_ptr, key_ty, string_mut, string_tydesc)?;
                    push_str(rt, string_mut, string_tydesc, b" = ")?;
                    pretty_value(rt, value_ptr, value_ty, string_mut, string_tydesc)?;

                    entry_count += 1;
                }

                // Move to next leaf.
                let next_leaf_ptr = (current_leaf as *const u8).add(layout.next_leaf_offset as usize) as *const *mut rtdt::MapNode;
                current_leaf = *next_leaf_ptr;
            }
        }

        push_str(rt, string_mut, string_tydesc, b"}")
    }
}

unsafe fn pretty_set(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let set = &*(value_ref as *const rtdt::Set);
        let elem_ty = tydesc.set_element_ty();

        push_str(rt, string_mut, string_tydesc, b"#{")?;

        if !set.root.is_null() && set.len > rtdt::Index::ZERO {
            let elem_size = elem_ty.size() as usize;

            let set_ty = crate::impls::set::SetTy::of(tydesc);
            let mut current_leaf = crate::impls::set::leftmost_leaf(
                set.root as *mut rtdt::SetNode, set_ty);
            let mut elem_count = 0u32;

            while !current_leaf.is_null() {
                let node_len = (*current_leaf).len;

                let layout = set_ty.leaf;
                let keys_array = (current_leaf as *const u8).add(layout.keys_offset as usize);

                for i in 0..node_len {
                    if elem_count > 0 {
                        push_str(rt, string_mut, string_tydesc, b", ")?;
                    }

                    let elem_ptr = keys_array.add(i as usize * elem_size);
                    pretty_value(rt, elem_ptr, elem_ty, string_mut, string_tydesc)?;

                    elem_count += 1;
                }

                // Move to next leaf.
                let next_leaf_ptr = (current_leaf as *const u8).add(layout.next_leaf_offset as usize) as *const *mut rtdt::SetNode;
                current_leaf = *next_leaf_ptr;
            }
        }

        push_str(rt, string_mut, string_tydesc, b"}")
    }
}

unsafe fn pretty_option(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let option = &*(value_ref as *const rtdt::Option);
        match option.tag {
            rtdt::OptionTag::None => {
                push_str(rt, string_mut, string_tydesc, b"none")
            }
            rtdt::OptionTag::Some => {
                push_str(rt, string_mut, string_tydesc, b"some ")?;
                let inner_ty = tydesc.option_inner_ty();
                let payload_offset = rtdt::layout::option_payload_offset(inner_ty.align());
                let payload_value = value_ref.add(payload_offset as usize);

                pretty_value(rt, payload_value, inner_ty, string_mut, string_tydesc)
            }
        }
    }
}

unsafe fn pretty_result(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let result = &*(value_ref as *const rtdt::Result);
        let ok_ty = tydesc.result_ok_ty();

        // Calculate payload offset using the correct layout that accounts for both ok and error types.
        let layout = rtdt::layout::compute_result_layout(tydesc);
        let payload_value = value_ref.add(layout.payload_offset as usize);

        match result.tag {
            rtdt::ResultTag::Ok => {
                push_str(rt, string_mut, string_tydesc, b"ok ")?;
                pretty_value(rt, payload_value, ok_ty, string_mut, string_tydesc)
            }
            rtdt::ResultTag::Err => {
                // The error is written under `er`, as the literal is, since an
                // `error` alone is no result and would not read back as one.
                push_str(rt, string_mut, string_tydesc, b"er ")?;
                pretty_error(rt, payload_value, string_mut, string_tydesc)
            }
        }
    }
}

/// Print a scalar held in the two words rather than behind a pointer.
///
/// A small immediate carries no tydesc, so the type comes from the tag and the
/// value from the accessor for it. The value is materialized into a local of the
/// right type so the ordinary printers can read it, which keeps this independent
/// of byte order.
unsafe fn pretty_packed(
    rt: LocalRtHandle,
    data: &rtdt::Data,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        macro_rules! print_as {
            ($accessor:ident, $printer:ident, $ty:ty) => {{
                let v: $ty = data.$accessor().ok_or(())?;
                $printer(rt, &v as *const $ty as *const u8, string_mut, string_tydesc)
            }};
        }
        match data.tytag() {
            rtdt::TyTag::Bool => print_as!(as_bool, pretty_bool, bool),
            rtdt::TyTag::U8 => print_as!(as_u8, pretty_u8, u8),
            rtdt::TyTag::I8 => print_as!(as_i8, pretty_i8, i8),
            rtdt::TyTag::U16 => print_as!(as_u16, pretty_u16, u16),
            rtdt::TyTag::I16 => print_as!(as_i16, pretty_i16, i16),
            rtdt::TyTag::U32 => print_as!(as_u32, pretty_u32, u32),
            rtdt::TyTag::I32 => print_as!(as_i32, pretty_i32, i32),
            rtdt::TyTag::U64 => print_as!(as_u64, pretty_u64, u64),
            rtdt::TyTag::I64 => print_as!(as_i64, pretty_i64, i64),
            rtdt::TyTag::F32 => print_as!(as_f32, pretty_f32, f32),
            rtdt::TyTag::F64 => print_as!(as_f64, pretty_f64, f64),
            _ => Err(()),
        }
    }
}

unsafe fn pretty_data(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let data = &*(value_ref as *const rtdt::Data);
        push_str(rt, string_mut, string_tydesc, b"data ")?;

        // A scalar small enough to pack sits in the two words. Only the pointer
        // form has a value to follow, and only it has a tydesc to follow it with.
        if data.tag() != rtdt::anypack::Tag::TwoPointers {
            return pretty_packed(rt, data, string_mut, string_tydesc);
        }

        let tydesc_ptr = data.tydesc();
        if tydesc_ptr.is_null() {
            push_str(rt, string_mut, string_tydesc, b"<null>")?;
            return Ok(());
        }

        let inner_tydesc = rtdt::TyDescRef::from_ptr(tydesc_ptr);
        let inner_value = data.value_ptr();
        pretty_value(rt, inner_value, inner_tydesc, string_mut, string_tydesc)
    }
}

unsafe fn pretty_error(
    rt: LocalRtHandle,
    value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let error = &*(value_ref as *const rtdt::Error);
        push_str(rt, string_mut, string_tydesc, b"error ")?;

        // Error uses the same encoding as Data, so a packed scalar reads the
        // same way. The transmute is what `Error::tydesc` does internally.
        let as_data = &*(error as *const rtdt::Error as *const rtdt::Data);
        if as_data.tag() != rtdt::anypack::Tag::TwoPointers {
            return pretty_packed(rt, as_data, string_mut, string_tydesc);
        }

        let tydesc_ptr = error.tydesc();
        if tydesc_ptr.is_null() {
            push_str(rt, string_mut, string_tydesc, b"<null>")?;
            return Ok(());
        }

        let inner_tydesc = rtdt::TyDescRef::from_ptr(tydesc_ptr);
        let inner_value = error.value_ptr();
        pretty_value(rt, inner_value, inner_tydesc, string_mut, string_tydesc)
    }
}

unsafe fn pretty_tensor(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let tensor = &*(value_ref as *const rtdt::Tensor);
        let elem_ty = tydesc.tensor_element_ty();
        let rank = tydesc.tensor_rank() as usize;
        let elem_size = elem_ty.size() as usize;

        // Every tensor has at least one axis, and keeps its shape even when
        // it holds nothing.
        let shape: Vec<usize> = (0..rank).map(|i| (*tensor.shape.add(i)).as_usize()).collect();
        let total_elems = rtdt::tensor_element_count(&shape);

        // A shape the separators cannot show -- a zero extent above the
        // innermost axis, or a leading extent of one -- goes in a header,
        // and the body after it flat.
        if rank > 1 && (shape[0] == 1 || shape.contains(&0)) {
            push_str(rt, string_mut, string_tydesc, b"[| ")?;
            for extent in &shape {
                push_str(rt, string_mut, string_tydesc, extent.to_string().as_bytes())?;
                push_str(rt, string_mut, string_tydesc, b" ")?;
            }
            push_str(rt, string_mut, string_tydesc, b"|")?;
            for i in 0..total_elems {
                push_str(rt, string_mut, string_tydesc, b" ")?;
                let elem = tensor_elem_ptr(tensor, &shape, i, elem_size);
                pretty_value(rt, elem, elem_ty, string_mut, string_tydesc)?;
            }
            return push_str(rt, string_mut, string_tydesc, b" |]");
        }

        if total_elems == 0 {
            return push_str(rt, string_mut, string_tydesc, b"[| |]");
        }
        push_str(rt, string_mut, string_tydesc, b"[| ")?;
        pretty_tensor_group(
            rt, tensor, elem_ty, elem_size,
            &shape, 0, 0, total_elems,
            string_mut, string_tydesc,
        )?;
        push_str(rt, string_mut, string_tydesc, b" |]")
    }
}

/// The element at row-major position `flat` of a tensor of this shape.
///
/// Found through the tensor's offset and strides rather than by position in
/// its buffer, since a view into another tensor starts partway through that
/// tensor's buffer.
unsafe fn tensor_elem_ptr(
    tensor: &rtdt::Tensor,
    shape: &[usize],
    flat: usize,
    elem_size: usize,
) -> *const u8 {
    unsafe {
        let mut rest = flat;
        let mut linear = tensor.offset_elems.as_usize();
        for dim in (0..shape.len()).rev() {
            let index = rest % shape[dim];
            rest /= shape[dim];
            linear += index * (*tensor.strides.add(dim)).as_usize();
        }
        tensor.ptr_base.add(linear * elem_size)
    }
}

/// Recursively print tensor elements with multi-comma separators.
///
/// `shape` has at least one extent, none of them zero, and `dim` indexes one
/// of them.
unsafe fn pretty_tensor_group(
    rt: LocalRtHandle,
    tensor: &rtdt::Tensor,
    elem_ty: rtdt::TyDescRef,
    elem_size: usize,
    shape: &[usize],
    dim: usize,
    offset: usize,
    count: usize,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let rank = shape.len();

        if dim == rank - 1 {
            // Innermost dimension: space-separated elements.
            for i in 0..count {
                if i > 0 {
                    push_str(rt, string_mut, string_tydesc, b" ")?;
                }
                let elem_ptr = tensor_elem_ptr(tensor, shape, offset + i, elem_size);
                pretty_value(rt, elem_ptr, elem_ty, string_mut, string_tydesc)?;
            }
            return Ok(());
        }

        let group_size: usize = shape[dim + 1..].iter().product();
        let num_groups = shape[dim];
        let comma_count = rank - dim - 1;

        for i in 0..num_groups {
            if i > 0 {
                // Write comma_count commas followed by a space.
                let commas: Vec<u8> = core::iter::repeat(b',').take(comma_count).chain(core::iter::once(b' ')).collect();
                push_str(rt, string_mut, string_tydesc, &commas)?;
            }
            pretty_tensor_group(
                rt, tensor, elem_ty, elem_size,
                shape, dim + 1, offset + i * group_size, group_size,
                string_mut, string_tydesc,
            )?;
        }

        Ok(())
    }
}

unsafe fn pretty_table(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: rtdt::TyDescRef,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let table = &*(value_ref as *const rtdt::Table);
        let column_tydescs = crate::impls::table::collect_column_tydescs(tydesc);

        // Format: {| col1, col2; val1, val2; val3, val4 |}
        // Header row with column names, then data rows separated by ";".
        push_str(rt, string_mut, string_tydesc, b"{| ")?;

        // Print column names as header.
        for (i, col) in tydesc.table_column_tydescs().enumerate() {
            if i > 0 {
                push_str(rt, string_mut, string_tydesc, b", ")?;
            }
            push_str(rt, string_mut, string_tydesc, col.name().as_bytes())?;
        }

        // Print data rows.
        if !table.data.is_null() && table.len > rtdt::Index::ZERO {
            for row in 0..table.len.0 {
                push_str(rt, string_mut, string_tydesc, b"; ")?;

                for (col, col_info) in tydesc.table_column_tydescs().enumerate() {
                    if col > 0 {
                        push_str(rt, string_mut, string_tydesc, b", ")?;
                    }

                    let elem_ptr = crate::impls::table::element_ptr(
                        table.data,
                        &column_tydescs,
                        row,
                        col,
                        table.capacity.0,
                    );
                    pretty_value(rt, elem_ptr, col_info.tydesc(), string_mut, string_tydesc)?;
                }
            }
        }

        push_str(rt, string_mut, string_tydesc, b" |}")
    }
}

unsafe fn push_str(
    rt: LocalRtHandle,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
    bytes: &[u8],
) -> Result<(), ()> {
    if bytes.is_empty() {
        return Ok(());
    }

    unsafe {
        let status = crate::impls::string::string_push_bytes_local(
            rt,
            string_mut,
            string_tydesc,
            bytes.as_ptr(),
            bytes.len() as rtdt::IndexRepr,
        );

        if status == RtStatus::Ok {
            Ok(())
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impls::rt_local;

    unsafe fn create_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing { unused: 0 },
            },
        }
    }

    unsafe fn create_output_string(rt_handle: LocalRtHandle) -> (rtdt::String, rtdt::TyDesc) {
        unsafe {
            let tydesc = create_string_tydesc();
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            crate::impls::string::string_create_local(
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            (string.assume_init(), tydesc)
        }
    }

    unsafe fn get_string_contents(string: &rtdt::String) -> String {
        unsafe {
            if string.data.is_null() || string.size == rtdt::Index::ZERO {
                return String::new();
            }
            let bytes = std::slice::from_raw_parts(string.data, string.size.as_usize());
            String::from_utf8_lossy(bytes).to_string()
        }
    }

    #[test]
    fn test_pretty_print_bool() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let true_val = rtdt::Bool(1);
            let true_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Bool,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            };

            let status = pretty_print_local(
                rt_handle,
                &true_val as *const rtdt::Bool as *const u8,
                &true_tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "true");

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_u32() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let val = rtdt::U32(42);
            let tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::U32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            };

            let status = pretty_print_local(
                rt_handle,
                &val as *const rtdt::U32 as *const u8,
                &tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "42");

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_f32() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let val = rtdt::F32(3.14);
            let tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::F32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            };

            let status = pretty_print_local(
                rt_handle,
                &val as *const rtdt::F32 as *const u8,
                &tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "3.14");

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_string() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            // Create a string value.
            let mut string_val = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let string_tydesc = create_string_tydesc();
            crate::impls::string::string_create_local(
                rt_handle,
                string_val.as_mut_ptr() as *mut u8,
                &string_tydesc,
            );
            let mut string_val = string_val.assume_init();

            crate::impls::string::string_push_bytes_local(
                rt_handle,
                &mut string_val as *mut rtdt::String as *mut u8,
                &string_tydesc,
                b"hello".as_ptr(),
                5,
            );

            let status = pretty_print_local(
                rt_handle,
                &string_val as *const rtdt::String as *const u8,
                &string_tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "\"hello\"");

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut string_val as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_string_with_escapes() {
        let rt = rt_local::RtLocal::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let mut string_val = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let string_tydesc = create_string_tydesc();
            crate::impls::string::string_create_local(
                rt_handle,
                string_val.as_mut_ptr() as *mut u8,
                &string_tydesc,
            );
            let mut string_val = string_val.assume_init();

            crate::impls::string::string_push_bytes_local(
                rt_handle,
                &mut string_val as *mut rtdt::String as *mut u8,
                &string_tydesc,
                b"hello\nworld".as_ptr(),
                11,
            );

            let status = pretty_print_local(
                rt_handle,
                &string_val as *const rtdt::String as *const u8,
                &string_tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "\"hello\\nworld\"");

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut string_val as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            crate::impls::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut rt_local::RtLocal);
            rt.shutdown();
        }
    }
}
