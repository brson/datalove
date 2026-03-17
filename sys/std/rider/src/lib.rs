//! Native rider for sys/std: string operations.
//!
//! Each function follows the runtime C ABI convention:
//! `extern "C-unwind" fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> u8`
//!
//! For `ref string` parameters, the value pointer points to an `rtdt::String` struct.
//! Return values are written to the `result_out` pointer.
//! Returns 1 (Ok) on success, 2 (Error) on failure.

use datalove_rtdt as rtdt;

const OK: u8 = 1;

/// Convert a value pointer to `rtdt::String` into a Rust `&str`.
///
/// # Safety
///
/// The pointer must be valid and point to a live `rtdt::String`.
unsafe fn as_str<'a>(ptr: *const u8) -> &'a str {
    let s = &*(ptr as *const rtdt::String);
    if s.data.is_null() || s.size.0 == 0 {
        ""
    } else {
        let bytes = std::slice::from_raw_parts(s.data, s.size.as_usize());
        std::str::from_utf8_unchecked(bytes)
    }
}

/// Write a scalar result to the out pointer.
///
/// # Safety
///
/// The out pointer must be valid and have enough space for `size_of::<T>()` bytes.
unsafe fn write_result<T: Copy>(out: *mut u8, val: T) {
    std::ptr::write(out as *mut T, val);
}

// --- Basic properties ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_len(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { &*(s_ptr as *const rtdt::String) };
    unsafe { write_result(out, s.size) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_char_count(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let count = rtdt::Index::new(s.chars().count() as rtdt::IndexRepr);
    unsafe { write_result(out, count) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_is_ascii(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    unsafe { write_result(out, s.is_ascii() as u8) };
    OK
}

// --- Searching ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_contains(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    pat_ptr: *const u8, _pat_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let pat = unsafe { as_str(pat_ptr) };
    unsafe { write_result(out, s.contains(pat) as u8) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_starts_with(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    prefix_ptr: *const u8, _prefix_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let prefix = unsafe { as_str(prefix_ptr) };
    unsafe { write_result(out, s.starts_with(prefix) as u8) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_ends_with(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    suffix_ptr: *const u8, _suffix_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let suffix = unsafe { as_str(suffix_ptr) };
    unsafe { write_result(out, s.ends_with(suffix) as u8) };
    OK
}

// --- Comparison ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_eq(
    _rt: *mut u8,
    a_ptr: *const u8, _a_td: *const u8,
    b_ptr: *const u8, _b_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let a = unsafe { as_str(a_ptr) };
    let b = unsafe { as_str(b_ptr) };
    unsafe { write_result(out, (a == b) as u8) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_cmp(
    _rt: *mut u8,
    a_ptr: *const u8, _a_td: *const u8,
    b_ptr: *const u8, _b_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let a = unsafe { as_str(a_ptr) };
    let b = unsafe { as_str(b_ptr) };
    let result: i32 = match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    unsafe { write_result(out, result) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_eq_ignore_ascii_case(
    _rt: *mut u8,
    a_ptr: *const u8, _a_td: *const u8,
    b_ptr: *const u8, _b_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let a = unsafe { as_str(a_ptr) };
    let b = unsafe { as_str(b_ptr) };
    unsafe { write_result(out, a.eq_ignore_ascii_case(b) as u8) };
    OK
}
