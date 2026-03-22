//! Helper functions for native rider implementations.
//!
//! Provides wrappers for writing Option and other complex return values
//! from rider C ABI functions.

use datalove_rtdt as rtdt;
use crate::c::{LocalRtHandle, RtStatus};

/// Write a None option to the result pointer.
///
/// Sets the tag byte to None (1). Works for any `?T` layout.
///
/// # Safety
///
/// `out` must point to a valid, writable option-sized region.
pub unsafe fn write_option_none(out: *mut u8) {
    unsafe {
        *out = rtdt::OptionTag::None as u8;
    }
}

/// Write a Some option with a Copy payload.
///
/// Sets the tag byte to Some (2) and writes the payload at the correct
/// offset based on the payload type's alignment.
///
/// # Safety
///
/// `out` must point to a valid, writable option-sized region large enough
/// for the tag + padding + payload.
pub unsafe fn write_option_some<T: Copy>(out: *mut u8, val: T) {
    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let payload_offset = align_up(1, std::mem::align_of::<T>());
        std::ptr::write(out.add(payload_offset) as *mut T, val);
    }
}

/// Write a Some option with a string payload, allocating through the runtime.
///
/// Sets the tag byte to Some (2), then creates the string at the payload
/// offset using the runtime allocator.
///
/// # Safety
///
/// `rt` and `out` must be valid pointers. The output region must be large
/// enough for the option-of-string layout.
pub unsafe fn write_option_some_string(
    rt: LocalRtHandle,
    out: *mut u8,
    s: &str,
) -> RtStatus {
    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let payload_offset = align_up(1, std::mem::align_of::<rtdt::String>());
        let payload_ptr = out.add(payload_offset);

        if s.is_empty() {
            let string_ptr = payload_ptr as *mut rtdt::String;
            (*string_ptr).data = std::ptr::null();
            (*string_ptr).size = rtdt::Index::ZERO;
            (*string_ptr).capacity = rtdt::Index::ZERO;
        } else {
            let tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::String,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
            };
            let status = crate::impls::string::string_from_bytes(
                rt,
                s.as_ptr(),
                s.len() as rtdt::IndexRepr,
                payload_ptr,
                &tydesc,
            );
            if status != RtStatus::Ok {
                return status;
            }
        }
        RtStatus::Ok
    }
}

/// Write a Some option with a (string, string) tuple payload.
///
/// Layout: `[tag, padding, string_0, string_1]` where the tuple starts
/// at the option payload offset and the two strings are laid out
/// sequentially with struct-like alignment.
///
/// # Safety
///
/// `rt` and `out` must be valid pointers. The output region must be large
/// enough for the `?(string, string)` layout.
pub unsafe fn write_option_some_string_pair(
    rt: LocalRtHandle,
    out: *mut u8,
    s0: &str,
    s1: &str,
) -> RtStatus {
    let string_size = std::mem::size_of::<rtdt::String>() as u32;
    let string_align = std::mem::align_of::<rtdt::String>() as u32;

    // Tuple of two strings: both fields have the same alignment, so field 1
    // starts immediately after field 0 (no inter-field padding needed).
    let field1_offset = rtdt::layout::align_up(string_size, string_align);

    // Option payload offset for a tuple with string alignment.
    let payload_offset = rtdt::layout::option_payload_offset(string_align) as usize;

    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let tuple_ptr = out.add(payload_offset);
        let s0_ptr = tuple_ptr;
        let s1_ptr = tuple_ptr.add(field1_offset as usize);

        let status = write_string_at(rt, s0_ptr, s0);
        if status != RtStatus::Ok {
            return status;
        }
        let status = write_string_at(rt, s1_ptr, s1);
        if status != RtStatus::Ok {
            return status;
        }

        RtStatus::Ok
    }
}

/// Write a string value at the given pointer.
///
/// # Safety
///
/// `rt` and `dest` must be valid pointers. `dest` must have space for an `rtdt::String`.
unsafe fn write_string_at(rt: LocalRtHandle, dest: *mut u8, s: &str) -> RtStatus {
    if s.is_empty() {
        let string_ptr = dest as *mut rtdt::String;
        unsafe {
            (*string_ptr).data = std::ptr::null();
            (*string_ptr).size = rtdt::Index::ZERO;
            (*string_ptr).capacity = rtdt::Index::ZERO;
        }
        RtStatus::Ok
    } else {
        let tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
        };
        unsafe {
            crate::impls::string::string_from_bytes(
                rt,
                s.as_ptr(),
                s.len() as rtdt::IndexRepr,
                dest,
                &tydesc,
            )
        }
    }
}

/// Write a list of strings to the result pointer.
///
/// Allocates each string through the runtime, then builds the list by moving
/// the string elements from a temporary buffer. The `out_td` must be a list
/// tydesc whose element type is string.
///
/// # Safety
///
/// `rt`, `out`, and `out_td` must be valid pointers. `out` must point to a
/// writable region large enough for an `rtdt::List`.
pub unsafe fn write_string_list(
    rt: LocalRtHandle,
    out: *mut u8,
    out_td: *const rtdt::TyDesc,
    parts: &[&str],
) -> RtStatus {
    let list_td_ref = unsafe { rtdt::TyDescRef::from_ptr(out_td) };
    let element_td = list_td_ref.list_element_ty().as_ptr();

    if parts.is_empty() {
        // Initialize empty list directly.
        let list_ptr = out as *mut rtdt::List;
        unsafe {
            (*list_ptr).data = std::ptr::null();
            (*list_ptr).size = rtdt::Index::ZERO;
            (*list_ptr).capacity = rtdt::Index::ZERO;
        }
        return RtStatus::Ok;
    }

    // Allocate a buffer of rtdt::String elements.
    let string_size = std::mem::size_of::<rtdt::String>();
    let mut buf: Vec<u8> = vec![0u8; parts.len() * string_size];

    // Initialize each string in the buffer.
    for (i, part) in parts.iter().enumerate() {
        let dest = unsafe { buf.as_mut_ptr().add(i * string_size) };
        let status = unsafe { write_string_at(rt, dest, part) };
        if status != RtStatus::Ok {
            // Destroy already-initialized strings before returning.
            for j in 0..i {
                let s = unsafe { buf.as_mut_ptr().add(j * string_size) as *mut rtdt::String };
                unsafe { destroy_string(rt, s) };
            }
            return status;
        }
    }

    // Build the list by moving elements from the buffer.
    unsafe {
        crate::c::dtlv_rti_list_build_from_slice_local(
            rt,
            out,
            element_td,
            buf.as_mut_ptr(),
            parts.len() as rtdt::IndexRepr,
        )
    }
}

/// Destroy a string value, freeing its backing allocation.
///
/// # Safety
///
/// `rt` must be a valid runtime handle. `s` must point to a valid `rtdt::String`.
unsafe fn destroy_string(rt: LocalRtHandle, s: *mut rtdt::String) {
    let s_ref = unsafe { &*s };
    if !s_ref.data.is_null() && s_ref.capacity.0 != 0 {
        let tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing },
        };
        unsafe {
            crate::c::dtlv_rti_any_destroy_local(
                rt,
                s as *mut u8,
                &tydesc,
            );
        }
    }
}

/// Smallest value >= `offset` that is a multiple of `align`.
pub const fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}
