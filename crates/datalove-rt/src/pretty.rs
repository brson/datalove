//! Pretty-printing for runtime values.
//!
//! Produces valid datalit syntax that can be parsed back.

use rmx::prelude::*;
use crate::rtdt;
use crate::{LocalRtHandle, RtStatus};

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
        let ty = &*tydesc_ref;
        let string_ty = &*string_tydesc;

        if string_ty.type_tag != rtdt::TyTag::String {
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
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        match tydesc.type_tag {
            rtdt::TyTag::Bool => pretty_bool(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::U32 => pretty_u32(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::F32 => pretty_f32(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::Int => pretty_int(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::String => pretty_string(rt, value_ref, string_mut, string_tydesc),

            rtdt::TyTag::Tuple => pretty_tuple(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Struct => pretty_struct(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Enum => pretty_enum(rt, value_ref, tydesc, string_mut, string_tydesc),

            rtdt::TyTag::List => pretty_list(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Map => pretty_map(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Set => pretty_set(rt, value_ref, tydesc, string_mut, string_tydesc),

            rtdt::TyTag::Option => pretty_option(rt, value_ref, tydesc, string_mut, string_tydesc),
            rtdt::TyTag::Result => pretty_result(rt, value_ref, tydesc, string_mut, string_tydesc),

            rtdt::TyTag::Data => pretty_data(rt, value_ref, string_mut, string_tydesc),
            rtdt::TyTag::Error => pretty_error(rt, value_ref, string_mut, string_tydesc),

            _ => {
                push_str(rt, string_mut, string_tydesc, b"<unsupported-type>")?;
                Ok(())
            }
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
            push_str(rt, string_mut, string_tydesc, b"@true")
        } else {
            push_str(rt, string_mut, string_tydesc, b"@false")
        }
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
        push_str(rt, string_mut, string_tydesc, b"@")?;
        let s = n.0.to_string();
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
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
        push_str(rt, string_mut, string_tydesc, b"@")?;
        let s = if f.0.is_nan() {
            "nan".to_string()
        } else if f.0.is_infinite() {
            if f.0.is_sign_positive() {
                "inf".to_string()
            } else {
                "-inf".to_string()
            }
        } else {
            f.0.to_string()
        };
        push_str(rt, string_mut, string_tydesc, s.as_bytes())
    }
}

unsafe fn pretty_int(
    rt: LocalRtHandle,
    _value_ref: *const u8,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    // TODO: Implement bigint pretty printing.
    unsafe {
        push_str(rt, string_mut, string_tydesc, b"<bigint>")
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
        push_str(rt, string_mut, string_tydesc, b"@\"")?;
        if !s.data.is_null() && s.size > 0 {
            let bytes = std::slice::from_raw_parts(s.data, s.size as usize);
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
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let info = &tydesc.type_info.tuple;
        push_str(rt, string_mut, string_tydesc, b"@(")?;

        for i in 0..info.num_fields {
            if i > 0 {
                push_str(rt, string_mut, string_tydesc, b", ")?;
            }

            let field = &*info.fields.add(i as usize);
            let field_value = value_ref.add(field.offset as usize);
            let field_tydesc = &*field.tydesc;

            pretty_value(rt, field_value, field_tydesc, string_mut, string_tydesc)?;
        }

        push_str(rt, string_mut, string_tydesc, b")")
    }
}

unsafe fn pretty_struct(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let info = &tydesc.type_info.struct_;
        push_str(rt, string_mut, string_tydesc, b"@{")?;

        for i in 0..info.num_fields {
            if i > 0 {
                push_str(rt, string_mut, string_tydesc, b", ")?;
            }

            let field = &*info.fields.add(i as usize);
            let field_name = std::slice::from_raw_parts(field.name, field.name_len as usize);
            push_str(rt, string_mut, string_tydesc, field_name)?;
            push_str(rt, string_mut, string_tydesc, b" = ")?;

            let field_value = value_ref.add(field.offset as usize);
            let field_tydesc = &*field.tydesc;

            pretty_value(rt, field_value, field_tydesc, string_mut, string_tydesc)?;
        }

        push_str(rt, string_mut, string_tydesc, b"}")
    }
}

unsafe fn pretty_enum(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let info = &tydesc.type_info.enum_;

        // Read discriminant (u8).
        let discriminant = *value_ref;

        if (discriminant as u32) >= info.num_variants {
            push_str(rt, string_mut, string_tydesc, b"<invalid-enum>")?;
            return Ok(());
        }

        let variant = &*info.variants.add(discriminant as usize);
        let variant_name = std::slice::from_raw_parts(variant.name, variant.name_len as usize);

        push_str(rt, string_mut, string_tydesc, b"@enum ")?;
        push_str(rt, string_mut, string_tydesc, variant_name)?;

        if !variant.payload.is_null() {
            push_str(rt, string_mut, string_tydesc, b"(")?;
            let payload_value = value_ref.add(variant.offset as usize);
            let payload_tydesc = &*variant.payload;
            pretty_value(rt, payload_value, payload_tydesc, string_mut, string_tydesc)?;
            push_str(rt, string_mut, string_tydesc, b")")?;
        }

        Ok(())
    }
}

unsafe fn pretty_list(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let list = &*(value_ref as *const rtdt::List);
        let info = &tydesc.type_info.list;
        let elem_tydesc = &*info.element_tydesc;

        push_str(rt, string_mut, string_tydesc, b"@[")?;

        if !list.data.is_null() && list.size > 0 {
            for i in 0..list.size {
                if i > 0 {
                    push_str(rt, string_mut, string_tydesc, b", ")?;
                }

                let elem_value = list.data.add((i * elem_tydesc.size) as usize);
                pretty_value(rt, elem_value, elem_tydesc, string_mut, string_tydesc)?;
            }
        }

        push_str(rt, string_mut, string_tydesc, b"]")
    }
}

unsafe fn pretty_map(
    rt: LocalRtHandle,
    _value_ref: *const u8,
    _tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    // TODO: Implement map pretty printing.
    unsafe {
        push_str(rt, string_mut, string_tydesc, b"@map {}")
    }
}

unsafe fn pretty_set(
    rt: LocalRtHandle,
    _value_ref: *const u8,
    _tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    // TODO: Implement set pretty printing.
    unsafe {
        push_str(rt, string_mut, string_tydesc, b"@set {}")
    }
}

unsafe fn pretty_option(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let option = &*(value_ref as *const rtdt::Option);
        match option.tag {
            rtdt::OptionTag::None => {
                push_str(rt, string_mut, string_tydesc, b"@none")
            }
            rtdt::OptionTag::Some => {
                let info = &tydesc.type_info.option;
                let inner_tydesc = &*info.inner_tydesc;

                // Calculate payload offset.
                let payload_offset = align_up(1, inner_tydesc.align as usize);
                let payload_value = value_ref.add(payload_offset);

                pretty_value(rt, payload_value, inner_tydesc, string_mut, string_tydesc)
            }
        }
    }
}

unsafe fn pretty_result(
    rt: LocalRtHandle,
    value_ref: *const u8,
    tydesc: &rtdt::TyDesc,
    string_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> Result<(), ()> {
    unsafe {
        let result = &*(value_ref as *const rtdt::Result);
        let info = &tydesc.type_info.result;
        let ok_tydesc = &*info.ok_tydesc;

        // Calculate payload offset (need to account for both ok and error types).
        let payload_offset = align_up(1, ok_tydesc.align as usize);
        let payload_value = value_ref.add(payload_offset);

        match result.tag {
            rtdt::ResultTag::Ok => {
                pretty_value(rt, payload_value, ok_tydesc, string_mut, string_tydesc)
            }
            rtdt::ResultTag::Err => {
                push_str(rt, string_mut, string_tydesc, b"error ")?;
                // Error is always a dynamic type.
                pretty_error(rt, payload_value, string_mut, string_tydesc)
            }
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

        let tydesc_ptr = data.tydesc();
        if tydesc_ptr.is_null() {
            push_str(rt, string_mut, string_tydesc, b"<null>")?;
            return Ok(());
        }

        let inner_tydesc = &*tydesc_ptr;
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

        let tydesc_ptr = error.tydesc();
        if tydesc_ptr.is_null() {
            push_str(rt, string_mut, string_tydesc, b"<null>")?;
            return Ok(());
        }

        let inner_tydesc = &*tydesc_ptr;
        let inner_value = error.value_ptr();
        pretty_value(rt, inner_value, inner_tydesc, string_mut, string_tydesc)
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
        let status = crate::string::string_push_bytes_local(
            rt,
            string_mut,
            string_tydesc,
            bytes.as_ptr(),
            bytes.len() as u32,
        );

        if status == RtStatus::Ok {
            Ok(())
        } else {
            Err(())
        }
    }
}

fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::LocalRt;

    unsafe fn create_string_tydesc() -> rtdt::TyDesc {
        rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo {
                nothing: rtdt::TyInfoNothing,
            },
        }
    }

    unsafe fn create_output_string(rt_handle: LocalRtHandle) -> (rtdt::String, rtdt::TyDesc) {
        unsafe {
            let tydesc = create_string_tydesc();
            let mut string = std::mem::MaybeUninit::<rtdt::String>::uninit();
            crate::string::string_create_local(
                rt_handle,
                string.as_mut_ptr() as *mut u8,
                &tydesc,
            );
            (string.assume_init(), tydesc)
        }
    }

    unsafe fn get_string_contents(string: &rtdt::String) -> String {
        unsafe {
            if string.data.is_null() || string.size == 0 {
                return String::new();
            }
            let bytes = std::slice::from_raw_parts(string.data, string.size as usize);
            String::from_utf8_lossy(bytes).to_string()
        }
    }

    #[test]
    fn test_pretty_print_bool() {
        let rt = LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let true_val = rtdt::Bool(1);
            let true_tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::Bool,
                size: 1,
                align: 1,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            };

            let status = pretty_print_local(
                rt_handle,
                &true_val as *const rtdt::Bool as *const u8,
                &true_tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "@true");

            crate::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut LocalRt);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_u32() {
        let rt = LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let val = rtdt::U32(42);
            let tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::U32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            };

            let status = pretty_print_local(
                rt_handle,
                &val as *const rtdt::U32 as *const u8,
                &tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "@42");

            crate::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut LocalRt);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_f32() {
        let rt = LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let val = rtdt::F32(3.14);
            let tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::F32,
                size: 4,
                align: 4,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            };

            let status = pretty_print_local(
                rt_handle,
                &val as *const rtdt::F32 as *const u8,
                &tydesc,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            assert_eq!(status, RtStatus::Ok);
            assert_eq!(get_string_contents(&output_string), "@3.14");

            crate::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut LocalRt);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_string() {
        let rt = LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            // Create a string value.
            let mut string_val = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let string_tydesc = create_string_tydesc();
            crate::string::string_create_local(
                rt_handle,
                string_val.as_mut_ptr() as *mut u8,
                &string_tydesc,
            );
            let mut string_val = string_val.assume_init();

            crate::string::string_push_bytes_local(
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
            assert_eq!(get_string_contents(&output_string), "@\"hello\"");

            crate::string::string_destroy_local(
                rt_handle,
                &mut string_val as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            crate::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut LocalRt);
            rt.shutdown();
        }
    }

    #[test]
    fn test_pretty_print_string_with_escapes() {
        let rt = LocalRt::new();
        let rt_handle = Box::into_raw(rt) as LocalRtHandle;

        unsafe {
            let (mut output_string, output_tydesc) = create_output_string(rt_handle);

            let mut string_val = std::mem::MaybeUninit::<rtdt::String>::uninit();
            let string_tydesc = create_string_tydesc();
            crate::string::string_create_local(
                rt_handle,
                string_val.as_mut_ptr() as *mut u8,
                &string_tydesc,
            );
            let mut string_val = string_val.assume_init();

            crate::string::string_push_bytes_local(
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
            assert_eq!(get_string_contents(&output_string), "@\"hello\\nworld\"");

            crate::string::string_destroy_local(
                rt_handle,
                &mut string_val as *mut rtdt::String as *mut u8,
                &string_tydesc,
            );

            crate::string::string_destroy_local(
                rt_handle,
                &mut output_string as *mut rtdt::String as *mut u8,
                &output_tydesc,
            );

            let rt = Box::from_raw(rt_handle as *mut LocalRt);
            rt.shutdown();
        }
    }
}
