//! Datalove runtime.
//!
//! - "rt" calls are called by the language and have a restricted ABI.
//! - "rti" calls are emitted only by the compiler and have whatever ABI is needed.

#![allow(unused)]

use rmx::prelude::*;

pub use datalove_rtdt as rtdt;

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
pub unsafe extern "C" fn dtlv_rti_deinit(
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
pub enum RtEq {
    Equals = 1,
    NotEquals = 2,
    Error = 3,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_td_eq(
    rt: LocalRtHandle,
    tydesc_a: *const rtdt::TyDesc,
    tydesc_b: *const rtdt::TyDesc,
) -> RtEq {
    todo!()
}

#[repr(u8)]
pub enum RtOrdering {
    Less = 1,
    Equal = 2,
    Greater = 3,
    Error = 4,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_td_cmp(
    rt: LocalRtHandle,
    tydesc_a: *const rtdt::TyDesc,
    tydesc_b: *const rtdt::TyDesc,
) -> RtOrdering {
    todo!()
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
