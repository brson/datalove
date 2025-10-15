//! Datalove runtime.
//!
//! - "rt" calls are called by the language and have a restricted ABI.
//! - "rti" calls are emitted only by the compiler and have whatever ABI is needed.

#![allow(unused)]

use rmx::prelude::*;

pub use datalove_rtdt as rtdt;

mod cmp;
pub mod alloc;
pub mod clone;
pub mod string;
pub mod pretty;
pub mod btreemap;

/// A runtime handle. Needed for all calls.
///
/// This is the only native type used in the ABI directly;
/// everything else is an rtdt argument type.
pub type LocalRtHandle = *mut u8;

/// A simple status code.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RtStatus {
    Ok = 1,
    Error = 2,
}

/// May return null.
#[unsafe(no_mangle)]
pub extern "C" fn dtlv_rti_init() -> LocalRtHandle {
    let rt = alloc::LocalRt::new();
    Box::into_raw(rt) as *mut u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_shutdown(
    rt: LocalRtHandle,
) -> RtStatus {
    if rt.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt = Box::from_raw(rt as *mut alloc::LocalRt);
        rt.shutdown();
    }

    RtStatus::Ok
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_mem_alloc_local(
    rt: LocalRtHandle,
    // The type of the element being allocated (not the container).
    tydesc: *const rtdt::TyDesc,
    count: u32,
) -> *mut u8 {
    if rt.is_null() || tydesc.is_null() {
        return std::ptr::null_mut();
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        let ty = &*tydesc;
        rt_ref.alloc(ty.size, ty.align, count)
    }
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_mem_free_local(
    rt: LocalRtHandle,
    tydesc: *const rtdt::TyDesc,
    count: u32,
    ptr: *mut u8
) -> RtStatus {
    if rt.is_null() || tydesc.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        let ty = &*tydesc;
        rt_ref.free(ty.size, ty.align, count, ptr);
    }

    RtStatus::Ok
}


/// Clone any type into the local heap.
///
/// Space is already allocated for the proximate type
/// at the `value_out` location - we just need to allocate
/// any needed buffers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_clone_local(
    rt: LocalRtHandle,
    value_in: *const u8,
    tydesc_in: *const rtdt::TyDesc,
    value_out: *mut u8,
) -> RtStatus {
    unsafe {
        clone::clone_value(rt, value_in, tydesc_in, value_out)
    }
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum RtEq {
    Equals = 1,
    NotEquals = 2,
    /// Type mismatch.
    Error = 3,
}

/// Standard equality.
///
/// Floats have weird cases.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_eq(
    _rt: LocalRtHandle,
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> RtEq {
    // Note that the runtime handle isn't needed
    // because we don't allocate - it's just part
    // of the ABI.
    unsafe {
        cmp::eq(value_a, tydesc_a, value_b, tydesc_b)
    }
}

/// Equality where each value has a single representation.
///
/// Floats are compared bitwise.
/// This is primarily useful for keying hash tables.
/// Not yet clear whether Datalove wants this.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_eq_unique(
    _rt: LocalRtHandle,
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> RtEq {
    unsafe {
        cmp::eq_unique(value_a, tydesc_a, value_b, tydesc_b)
    }
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum RtOrdering {
    Less = 1,
    Equal = 2,
    Greater = 3,
    /// Type mismatch.
    Error = 4,
}

/// Establish ordering using Datalove ordering.
///
/// This is probably not actually useful. Just experimenting.
/// NaN's have total order; float zeros are equal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_cmp(
    _rt: LocalRtHandle,
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> RtOrdering {
    unsafe {
        cmp::cmp(value_a, tydesc_a, value_b, tydesc_b)
    }
}

/// Establish total ordering.
///
/// Floats use the typical ordering, like Rust's `total_cmp`:
///
/// > -NaN < -Infinity < -numbers < -0.0 < +0.0 < +numbers < +Infinity < +NaN
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_cmp_total(
    _rt: LocalRtHandle,
    value_a: *const u8,
    tydesc_a: *const rtdt::TyDesc,
    value_b: *const u8,
    tydesc_b: *const rtdt::TyDesc,
) -> RtOrdering {
    unsafe {
        cmp::cmp_total(value_a, tydesc_a, value_b, tydesc_b)
    }
}





#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_string_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        string::string_create_local(rt, value_out, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_string_destroy_local(
    rt: LocalRtHandle,
    // Pointer will be freed.
    value_in: *mut u8,
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        string::string_destroy_local(rt, value_in, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_string_push_bytes_local(
    rt: LocalRtHandle,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
    bytes_ref: *const u8,
    bytes_len: u32,
) -> RtStatus {
    unsafe {
        string::string_push_bytes_local(rt, string_value_mut, string_tydesc, bytes_ref, bytes_len)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_string_clear_local(
    rt: LocalRtHandle,
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        string::string_clear_local(rt, string_value_mut, string_tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_pretty_print_local(
    rt: LocalRtHandle,
    arg_value_ref: *const u8,
    arg_tydesc_ref: *const rtdt::TyDesc,
    // String previously allocated by string_create_local
    string_value_mut: *mut u8,
    string_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        pretty::pretty_print_local(rt, arg_value_ref, arg_tydesc_ref, string_value_mut, string_tydesc)
    }
}








#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_btreemap_create_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    value_out: *mut u8,
    // BTreeMap type.
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        btreemap::btreemap_create_impl(rt_ref, value_out, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_btreemap_clone_from_slice_local(
    rt: LocalRtHandle,
    // Destination will be overwritten.
    btreemap_value_out: *mut u8,
    // BTreeMap type.
    btreemap_tydesc: *const rtdt::TyDesc,
    // Values will be cloned.
    slice_ptr_ref: *const u8,
    slice_ptr_len: u32,
    slice_element_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        btreemap::btreemap_clone_from_slice_impl(
            rt_ref,
            btreemap_value_out,
            btreemap_tydesc,
            slice_ptr_ref,
            slice_ptr_len,
            slice_element_tydesc,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_btreemap_destroy_local(
    rt: LocalRtHandle,
    // Pointer will be freed.
    value_in: *mut u8,
    // BTreMap type.
    tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        btreemap::btreemap_destroy_impl(rt_ref, value_in, tydesc)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_btreemap_insert_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    // Value is moved.
    value_in: *mut u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        btreemap::btreemap_insert_impl(
            rt_ref,
            btreemap_value_mut,
            btreemap_tydesc,
            value_in,
            value_tydesc,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_btreemap_remove_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
    value_ref: *const u8,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        btreemap::btreemap_remove_impl(
            rt_ref,
            btreemap_value_mut,
            btreemap_tydesc,
            value_ref,
            value_tydesc,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_btreemap_clear_local(
    rt: LocalRtHandle,
    btreemap_value_mut: *mut u8,
    btreemap_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if rt.is_null() {
            return RtStatus::Error;
        }

        let rt_ref = &mut *(rt as *mut alloc::LocalRt);
        btreemap::btreemap_clear_impl(rt_ref, btreemap_value_mut, btreemap_tydesc)
    }
}









/// Hypothetical language-accessible runtime call.
///
/// Note it's using ByRefArg and ReturnArg.
///
/// Probably list constructors and destructors though will
/// be emitted by the compiler and this won't actually be used
/// by the standard library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rt_list_create(
    rt: LocalRtHandle,
    tydesc: rtdt::ByRefArg,
    ret_new_list: rtdt::ReturnArg,
) -> RtStatus {
    todo!()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rt_list_destroy(
    rt: LocalRtHandle,
    tydesc: rtdt::ByRefArg,
    ret_new_list: rtdt::ReturnArg,
) -> RtStatus {
    todo!()
}
