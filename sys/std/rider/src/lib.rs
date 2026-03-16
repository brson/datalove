//! Native rider for sys/std: string operations.
//!
//! Each function follows the rider C ABI convention:
//! `extern "C-unwind" fn(rt: *mut u8, args...: i64) -> i64`
//!
//! For `ref string` parameters, the i64 value is a pointer to an `rtdt::String` struct.
//! The runtime handle is unused for pure read-only string operations.

use datalove_rtdt as rtdt;

/// Convert a rider i64 arg (pointer to rtdt::String) to a Rust &str.
///
/// # Safety
///
/// The pointer must be valid and point to a live `rtdt::String`.
unsafe fn as_str<'a>(ptr: i64) -> &'a str {
    let s = &*(ptr as *const rtdt::String);
    if s.data.is_null() || s.size.0 == 0 {
        ""
    } else {
        let bytes = std::slice::from_raw_parts(s.data, s.size.as_usize());
        std::str::from_utf8_unchecked(bytes)
    }
}

// --- Basic properties ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_len(_rt: *mut u8, s: i64) -> i64 {
    let s = unsafe { &*(s as *const rtdt::String) };
    s.size.0 as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_char_count(_rt: *mut u8, s: i64) -> i64 {
    let s = unsafe { as_str(s) };
    s.chars().count() as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_is_ascii(_rt: *mut u8, s: i64) -> i64 {
    let s = unsafe { as_str(s) };
    s.is_ascii() as i64
}

// --- Searching ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_contains(_rt: *mut u8, s: i64, pattern: i64) -> i64 {
    let s = unsafe { as_str(s) };
    let pattern = unsafe { as_str(pattern) };
    s.contains(pattern) as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_starts_with(_rt: *mut u8, s: i64, prefix: i64) -> i64 {
    let s = unsafe { as_str(s) };
    let prefix = unsafe { as_str(prefix) };
    s.starts_with(prefix) as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_ends_with(_rt: *mut u8, s: i64, suffix: i64) -> i64 {
    let s = unsafe { as_str(s) };
    let suffix = unsafe { as_str(suffix) };
    s.ends_with(suffix) as i64
}

// --- Comparison ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_eq(_rt: *mut u8, a: i64, b: i64) -> i64 {
    let a = unsafe { as_str(a) };
    let b = unsafe { as_str(b) };
    (a == b) as i64
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_cmp(_rt: *mut u8, a: i64, b: i64) -> i64 {
    let a = unsafe { as_str(a) };
    let b = unsafe { as_str(b) };
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1i64,
        std::cmp::Ordering::Equal => 0i64,
        std::cmp::Ordering::Greater => 1i64,
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_eq_ignore_ascii_case(_rt: *mut u8, a: i64, b: i64) -> i64 {
    let a = unsafe { as_str(a) };
    let b = unsafe { as_str(b) };
    a.eq_ignore_ascii_case(b) as i64
}
