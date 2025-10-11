//! Datalove runtime.

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
