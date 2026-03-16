//! Native rider for sys/std: u32 bitwise operations.
//!
//! Each function follows the rider C ABI convention:
//! `extern "C-unwind" fn(rt: *mut u8, args...: i64) -> i64`
//!
//! The runtime handle is unused for pure scalar operations.
//! Arguments and return values are i64-widened scalars.

#[no_mangle]
pub extern "C-unwind" fn dlr_std__bitnot_u32(_rt: *mut u8, a: i64) -> i64 {
    let a = a as u32;
    (!a) as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__bitand_u32(_rt: *mut u8, a: i64, b: i64) -> i64 {
    let a = a as u32;
    let b = b as u32;
    (a & b) as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__bitor_u32(_rt: *mut u8, a: i64, b: i64) -> i64 {
    let a = a as u32;
    let b = b as u32;
    (a | b) as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__bitxor_u32(_rt: *mut u8, a: i64, b: i64) -> i64 {
    let a = a as u32;
    let b = b as u32;
    (a ^ b) as i64
}
