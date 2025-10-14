//! Datalove runtime.
//!
//! - "rt" calls are called by the language and have a restricted ABI.
//! - "rti" calls are emitted only by the compiler and have whatever ABI is needed.

#![allow(unused)]

use rmx::prelude::*;

pub use datalove_rtdt as rtdt;

mod cmp;

/// A runtime handle. Needed for all calls.
///
/// This is the only native type used in the ABI directly;
/// everything else is an rtdt argument type.
pub type LocalRtHandle = *mut u8;

/// A simple status code.
#[repr(u8)]
pub enum RtStatus {
    Ok = 1,
    Error = 2,
}

/// May return null.
#[unsafe(no_mangle)]
pub extern "C" fn dtlv_rti_init() -> LocalRtHandle {
    todo!()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_shutdown(
    rt: LocalRtHandle,
) -> RtStatus {
    todo!()
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_mem_alloc(
    rt: LocalRtHandle,
    tydesc: *const rtdt::TyDesc,
    count: u32,
) -> *mut u8 {
    todo!()
}

/// Low-level allocator access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_mem_free(
    rt: LocalRtHandle,
    tydesc: *const rtdt::TyDesc,
    count: u32,
    ptr: *mut u8
) -> RtStatus {
    todo!()
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
