//! Native functions for the `local/calc` test package.
//!
//! Each follows the runtime C ABI: the runtime handle, a value and type
//! descriptor pointer per argument, then the result's, returning 1 for success.

/// Triple an `i64`.
#[no_mangle]
pub extern "C-unwind" fn dlr_calc__triple(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const i64) };
    unsafe { std::ptr::write(out as *mut i64, x * 3) };
    1
}
