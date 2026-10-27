//! Native rider for sys/std: string operations.
//!
//! Each function follows the runtime C ABI convention:
//! `unsafe extern "C-unwind" fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> u8`
//!
//! Unsafe to call, every one: the caller promises that each pointer is what
//! the function's `rider.dli` signature says it is, and the function takes it
//! at its word, reading its arguments once at the top through
//! `rider_helpers`.
//!
//! For `ref string` parameters, the value pointer points to an `rtdt::String` struct.
//! Return values are written to the `result_out` pointer.
//! Returns 1 (Ok) on success, 2 (Error) on failure.

use datalove_rtdt as rtdt;
use datalove_rti::rider_helpers::{self, arg, ref_arg, str_arg, td_arg, write_result, StringMut};

// The runtime functions this crate calls are declared, not defined, so a
// library built from it leaves them for whoever loads it to supply. The test
// harness is an executable and has nobody to ask, so it links a runtime of its
// own; nothing here calls through this name, the `extern crate` is what brings
// the definitions in.
#[cfg(test)]
extern crate datalove_rt;

/// The interface this rider implements.
///
/// The declarations and the implementations have to agree, so they travel
/// together: this crate is where `rider.dli` lives and is published, and the
/// compiler reads the interface from here rather than from a path. A datalove
/// package whose rider is a Rust crate cannot carry the file itself anyway --
/// cargo excludes a directory holding a `Cargo.toml` from the package around
/// it, silently, whatever its `include` says.
pub const INTERFACE: &str = include_str!("../rider.dli");

// The `symbols` table, generated from `rider.dli`. A binary that links this
// crate uses it to find these functions without a shared library.
include!(concat!(env!("OUT_DIR"), "/symbols.rs"));

const OK: u8 = 1;

/// Write a new string result by allocating through the runtime.
///
/// # Safety
///
/// rt, out, and out_td must be valid pointers.
unsafe fn write_string_result(rt: *mut u8, out: *mut rtdt::String, out_td: *const rtdt::TyDesc, s: &str) -> u8 {
    let status = unsafe {
        datalove_rti::call::dtlv_rti_string_from_bytes(
            rt,
            s.as_ptr(),
            s.len() as rtdt::IndexRepr,
            out.cast(),
            out_td,
        )
    };
    status as u8
}

// --- Basic properties ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_len(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { ref_arg::<rtdt::String>(s_ptr) };
    unsafe { write_result(out, s.size) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_char_count(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let count = rtdt::Index::new(s.chars().count() as rtdt::IndexRepr);
    unsafe { write_result(out, count) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_is_ascii(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    unsafe { write_result(out, s.is_ascii() as u8) };
    OK
}

// --- Searching ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_contains(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    pat_ptr: *const rtdt::String, _pat_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let pat = unsafe { str_arg(pat_ptr) };
    unsafe { write_result(out, s.contains(pat) as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_starts_with(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    prefix_ptr: *const rtdt::String, _prefix_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let prefix = unsafe { str_arg(prefix_ptr) };
    unsafe { write_result(out, s.starts_with(prefix) as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_ends_with(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    suffix_ptr: *const rtdt::String, _suffix_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let suffix = unsafe { str_arg(suffix_ptr) };
    unsafe { write_result(out, s.ends_with(suffix) as u8) };
    OK
}

// --- Comparison ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_eq(
    _rt: *mut u8,
    a_ptr: *const rtdt::String, _a_td: *const rtdt::TyDesc,
    b_ptr: *const rtdt::String, _b_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let a = unsafe { str_arg(a_ptr) };
    let b = unsafe { str_arg(b_ptr) };
    unsafe { write_result(out, (a == b) as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_cmp(
    _rt: *mut u8,
    a_ptr: *const rtdt::String, _a_td: *const rtdt::TyDesc,
    b_ptr: *const rtdt::String, _b_td: *const rtdt::TyDesc,
    out: *mut i32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let a = unsafe { str_arg(a_ptr) };
    let b = unsafe { str_arg(b_ptr) };
    let result: i32 = match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    unsafe { write_result(out, result) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_eq_ignore_ascii_case(
    _rt: *mut u8,
    a_ptr: *const rtdt::String, _a_td: *const rtdt::TyDesc,
    b_ptr: *const rtdt::String, _b_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let a = unsafe { str_arg(a_ptr) };
    let b = unsafe { str_arg(b_ptr) };
    unsafe { write_result(out, a.eq_ignore_ascii_case(b) as u8) };
    OK
}

// --- Character predicates ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_is_ascii_alphabetic(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
    unsafe { write_result(out, result as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_is_ascii_digit(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    unsafe { write_result(out, result as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_is_ascii_alphanumeric(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
    unsafe { write_result(out, result as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_is_ascii_whitespace(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_whitespace());
    unsafe { write_result(out, result as u8) };
    OK
}

// --- Trimming ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_trim(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    unsafe { write_string_result(rt, out, out_td, s.trim()) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_trim_start(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    unsafe { write_string_result(rt, out, out_td, s.trim_start()) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_trim_end(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    unsafe { write_string_result(rt, out, out_td, s.trim_end()) }
}

// --- Case conversion ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_to_ascii_lowercase(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = s.to_ascii_lowercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_to_ascii_uppercase(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = s.to_ascii_uppercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Construction ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_repeat(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    n_ptr: *const rtdt::Index, _n_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let n = unsafe { arg::<rtdt::Index>(n_ptr) };
    let result = s.repeat(n.as_usize());
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Replacement ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_replace(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    pat_ptr: *const rtdt::String, _pat_td: *const rtdt::TyDesc,
    rep_ptr: *const rtdt::String, _rep_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let pat = unsafe { str_arg(pat_ptr) };
    let rep = unsafe { str_arg(rep_ptr) };
    let result = s.replace(pat, rep);
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Concatenation ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_concat(
    rt: *mut u8,
    a_ptr: *const rtdt::String, _a_td: *const rtdt::TyDesc,
    b_ptr: *const rtdt::String, _b_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let a = unsafe { str_arg(a_ptr) };
    let b = unsafe { str_arg(b_ptr) };
    let result = format!("{}{}", a, b);
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Option-returning functions ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_get_byte(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    idx_ptr: *const rtdt::Index, _idx_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let idx = unsafe { arg::<rtdt::Index>(idx_ptr) };
    let i = idx.as_usize();
    if i < s.len() {
        unsafe { rider_helpers::write_option_some(out, s.as_bytes()[i]) };
    } else {
        unsafe { rider_helpers::write_option_none(out) };
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_find(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    pat_ptr: *const rtdt::String, _pat_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let pat = unsafe { str_arg(pat_ptr) };
    match s.find(pat) {
        Some(pos) => unsafe { rider_helpers::write_option_some(out, rtdt::Index::new(pos as rtdt::IndexRepr)) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_rfind(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    pat_ptr: *const rtdt::String, _pat_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let pat = unsafe { str_arg(pat_ptr) };
    match s.rfind(pat) {
        Some(pos) => unsafe { rider_helpers::write_option_some(out, rtdt::Index::new(pos as rtdt::IndexRepr)) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_strip_prefix(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    prefix_ptr: *const rtdt::String, _prefix_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let prefix = unsafe { str_arg(prefix_ptr) };
    match s.strip_prefix(prefix) {
        Some(rest) => unsafe { rider_helpers::write_option_some_string(rt, out, rest) as u8 },
        None => { unsafe { rider_helpers::write_option_none(out) }; OK },
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_strip_suffix(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    suffix_ptr: *const rtdt::String, _suffix_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let suffix = unsafe { str_arg(suffix_ptr) };
    match s.strip_suffix(suffix) {
        Some(rest) => unsafe { rider_helpers::write_option_some_string(rt, out, rest) as u8 },
        None => { unsafe { rider_helpers::write_option_none(out) }; OK },
    }
}

// --- Unicode case conversion ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_to_lowercase(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = s.to_lowercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_to_uppercase(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let result = s.to_uppercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Mutation ---
//
// A `mut string` is changed through one `StringMut`, so that reading it and
// writing it cannot overlap.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_push_str(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    other_ptr: *const rtdt::String, _other_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    // Never the same string: the ownership checker rejects passing one
    // binding as both `mut` and `ref` (D010).
    assert!(!std::ptr::eq(self_ptr.cast_const(), other_ptr), "a string added to itself");
    let other = unsafe { str_arg(other_ptr) };
    let mut s = unsafe { StringMut::new(rt, self_ptr, self_td) };
    s.push_str(other) as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_clear(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe { StringMut::new(rt, self_ptr, self_td) }.clear();
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_push_char(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    ch_ptr: *const u32, _ch_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let Some(ch) = char::from_u32(unsafe { arg::<u32>(ch_ptr) }) else {
        return OK; // Invalid codepoint: no-op.
    };
    let mut s = unsafe { StringMut::new(rt, self_ptr, self_td) };
    s.push_str(ch.encode_utf8(&mut [0u8; 4])) as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_pop(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    match unsafe { StringMut::new(rt, self_ptr, self_td) }.pop() {
        Some(ch) => unsafe { rider_helpers::write_option_some(out, ch as u32) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_truncate(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    len_ptr: *const rtdt::Index, _len_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let len = unsafe { arg::<rtdt::Index>(len_ptr) }.as_usize();
    unsafe { StringMut::new(rt, self_ptr, self_td) }.truncate(len);
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_remove(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    idx_ptr: *const rtdt::Index, _idx_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let idx = unsafe { arg::<rtdt::Index>(idx_ptr) }.as_usize();
    match unsafe { StringMut::new(rt, self_ptr, self_td) }.remove(idx) {
        Some(ch) => unsafe { rider_helpers::write_option_some(out, ch as u32) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_insert_char(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    idx_ptr: *const rtdt::Index, _idx_td: *const rtdt::TyDesc,
    ch_ptr: *const u32, _ch_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let idx = unsafe { arg::<rtdt::Index>(idx_ptr) }.as_usize();
    let Some(ch) = char::from_u32(unsafe { arg::<u32>(ch_ptr) }) else {
        return OK; // Invalid codepoint: no-op.
    };
    let mut s = unsafe { StringMut::new(rt, self_ptr, self_td) };
    s.insert_str(idx, ch.encode_utf8(&mut [0u8; 4])) as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_insert_str(
    rt: *mut u8,
    self_ptr: *mut rtdt::String, self_td: *const rtdt::TyDesc,
    idx_ptr: *const rtdt::Index, _idx_td: *const rtdt::TyDesc,
    other_ptr: *const rtdt::String, _other_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let idx = unsafe { arg::<rtdt::Index>(idx_ptr) }.as_usize();
    // Never the same string: the ownership checker rejects passing one
    // binding as both `mut` and `ref` (D010).
    assert!(!std::ptr::eq(self_ptr.cast_const(), other_ptr), "a string added to itself");
    let other = unsafe { str_arg(other_ptr) };
    let mut s = unsafe { StringMut::new(rt, self_ptr, self_td) };
    s.insert_str(idx, other) as u8
}

// --- Splitting (option-of-tuple) ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_split_once(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    delim_ptr: *const rtdt::String, _delim_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let delim = unsafe { str_arg(delim_ptr) };
    match s.split_once(delim) {
        Some((before, after)) => {
            let status = unsafe { rider_helpers::write_option_some_string_pair(rt, out, before, after) };
            status as u8
        }
        None => {
            unsafe { rider_helpers::write_option_none(out) };
            OK
        }
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_rsplit_once(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    delim_ptr: *const rtdt::String, _delim_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let delim = unsafe { str_arg(delim_ptr) };
    match s.rsplit_once(delim) {
        Some((before, after)) => {
            let status = unsafe { rider_helpers::write_option_some_string_pair(rt, out, before, after) };
            status as u8
        }
        None => {
            unsafe { rider_helpers::write_option_none(out) };
            OK
        }
    }
}

// --- Character access ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_char_at(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    idx_ptr: *const rtdt::Index, _idx_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let idx = unsafe { arg::<rtdt::Index>(idx_ptr) }.as_usize();
    match s.chars().nth(idx) {
        Some(ch) => unsafe { rider_helpers::write_option_some(out, ch as u32) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_char_to_byte_index(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    idx_ptr: *const rtdt::Index, _idx_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let char_idx = unsafe { arg::<rtdt::Index>(idx_ptr) }.as_usize();
    match s.char_indices().nth(char_idx) {
        Some((byte_idx, _)) => unsafe { rider_helpers::write_option_some(out, rtdt::Index::new(byte_idx as rtdt::IndexRepr)) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

// --- Slicing ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_slice(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    start_ptr: *const rtdt::Index, _start_td: *const rtdt::TyDesc,
    end_ptr: *const rtdt::Index, _end_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let start = unsafe { arg::<rtdt::Index>(start_ptr) }.as_usize();
    let end = unsafe { arg::<rtdt::Index>(end_ptr) }.as_usize();
    if start > end || end > s.len() || !s.is_char_boundary(start) || !s.is_char_boundary(end) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let status = unsafe { rider_helpers::write_option_some_string(rt, out, &s[start..end]) };
    status as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_slice_from(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    start_ptr: *const rtdt::Index, _start_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let start = unsafe { arg::<rtdt::Index>(start_ptr) }.as_usize();
    if start > s.len() || !s.is_char_boundary(start) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let status = unsafe { rider_helpers::write_option_some_string(rt, out, &s[start..]) };
    status as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_slice_to(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    end_ptr: *const rtdt::Index, _end_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let end = unsafe { arg::<rtdt::Index>(end_ptr) }.as_usize();
    if end > s.len() || !s.is_char_boundary(end) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let status = unsafe { rider_helpers::write_option_some_string(rt, out, &s[..end]) };
    status as u8
}

// --- Parsing ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_parse_u32(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    match s.parse::<u32>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_parse_i32(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    match s.parse::<i32>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_parse_f32(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    match s.parse::<f32>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

// --- Formatting ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_from_u32(
    rt: *mut u8,
    n_ptr: *const u32, _n_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let n = unsafe { arg::<u32>(n_ptr) };
    let s = n.to_string();
    unsafe { write_string_result(rt, out, out_td, &s) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_from_i32(
    rt: *mut u8,
    n_ptr: *const i32, _n_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let n = unsafe { arg::<i32>(n_ptr) };
    let s = n.to_string();
    unsafe { write_string_result(rt, out, out_td, &s) }
}

// --- Construction ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_from_char(
    rt: *mut u8,
    ch_ptr: *const u32, _ch_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let codepoint = unsafe { arg::<u32>(ch_ptr) };
    match char::from_u32(codepoint) {
        Some(ch) => {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            unsafe { write_string_result(rt, out, out_td, s) }
        }
        None => {
            // Invalid codepoint: return empty string.
            unsafe { write_string_result(rt, out, out_td, "") }
        }
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_replacen(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    pat_ptr: *const rtdt::String, _pat_td: *const rtdt::TyDesc,
    rep_ptr: *const rtdt::String, _rep_td: *const rtdt::TyDesc,
    n_ptr: *const rtdt::Index, _n_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let pat = unsafe { str_arg(pat_ptr) };
    let rep = unsafe { str_arg(rep_ptr) };
    let n = unsafe { arg::<rtdt::Index>(n_ptr) };
    let result = s.replacen(pat, rep, n.as_usize());
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Splitting (list returns) ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_split(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    delim_ptr: *const rtdt::String, _delim_td: *const rtdt::TyDesc,
    out: *mut rtdt::List, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let delim = unsafe { str_arg(delim_ptr) };
    let parts: Vec<&str> = s.split(delim).collect();
    let status = unsafe {
        rider_helpers::write_string_list(rt, out, out_td, &parts)
    };
    status as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_lines(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::List, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let parts: Vec<&str> = s.lines().collect();
    let status = unsafe {
        rider_helpers::write_string_list(rt, out, out_td, &parts)
    };
    status as u8
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_split_whitespace(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut rtdt::List, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let parts: Vec<&str> = s.split_whitespace().collect();
    let status = unsafe {
        rider_helpers::write_string_list(rt, out, out_td, &parts)
    };
    status as u8
}

// --- Joining ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_join(
    rt: *mut u8,
    parts_ptr: *const rtdt::List, _parts_td: *const rtdt::TyDesc,
    sep_ptr: *const rtdt::String, _sep_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let parts = unsafe { rider_helpers::read_string_list(parts_ptr) };
    let sep = unsafe { str_arg(sep_ptr) };
    let result = parts.join(sep);
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Bigint conversion ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_from_int(
    rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { rider_helpers::int_to_string(n_ptr) };
    unsafe { write_string_result(rt, out, out_td, &s) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_parse_int(
    rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    let status = unsafe { rider_helpers::write_option_int_from_str(rt, out, s) };
    status as u8
}

// --- Lists ---
//
// These are generic over the element type. Each parameter arrives as a pointer
// and a descriptor, so the element type is read off the list at runtime rather
// than being known when this is compiled.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_len(
    rt: *mut u8,
    list_ptr: *const rtdt::List, list_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_list_len_local(
            rt, list_ptr.cast(), list_td, out.cast(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__tensor_len(
    _rt: *mut u8,
    tensor_ptr: *const rtdt::Tensor, _tensor_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    // A tensor's length along axis 0 is the first entry of its shape. Nothing
    // in the header says it: the header is a pointer, an offset, a capacity
    // and the two shape arrays.
    unsafe {
        let tensor = ref_arg::<rtdt::Tensor>(tensor_ptr);
        let len = if tensor.shape.is_null() {
            rtdt::Index::ZERO
        } else {
            *tensor.shape
        };
        write_result(out, len);
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__tensor_rank(
    _rt: *mut u8,
    _tensor_ptr: *const rtdt::Tensor, tensor_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        let ty = td_arg(tensor_td);
        let rank = (*ty.as_ptr()).type_info.tensor.rank;
        write_result(out, rtdt::Index(rank as rtdt::IndexRepr));
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_get(
    rt: *mut u8,
    list_ptr: *const rtdt::List, list_td: *const rtdt::TyDesc,
    index_ptr: *const rtdt::Index, _index_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    let index = unsafe { arg::<rtdt::Index>(index_ptr) };
    let list_ty = unsafe { td_arg(list_td) };
    let out_ty = unsafe { td_arg(out_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_get_local(
            rt, list_ptr.cast(), list_ty.as_ptr(), index.0, out.cast(), out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_push(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    elem_ptr: *mut u8, elem_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let list_ty = unsafe { td_arg(list_td) };
    let elem_ty = unsafe { td_arg(elem_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_push_local(
            rt, list_ptr.cast(), list_ty.as_ptr(), elem_ptr.cast(), elem_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_pop(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    let list_ty = unsafe { td_arg(list_td) };
    let out_ty = unsafe { td_arg(out_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_pop_local(
            rt, list_ptr.cast(), list_ty.as_ptr(), out.cast(), out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_clear(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_list_clear_local(
            rt, list_ptr.cast(), list_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_set(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    index_ptr: *const rtdt::Index, _index_td: *const rtdt::TyDesc,
    elem_ptr: *mut u8, elem_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let index = unsafe { arg::<rtdt::Index>(index_ptr) };
    let list_ty = unsafe { td_arg(list_td) };
    let elem_ty = unsafe { td_arg(elem_td) };

    let status = unsafe {
        datalove_rti::call::dtlv_rti_list_set_local(
            rt, list_ptr.cast(), list_ty.as_ptr(), index.0, elem_ptr.cast(), elem_ty.as_ptr(),
        )
    };
    // The runtime has taken the element either way.
    unsafe { write_result(_out, (status == datalove_rti::RtStatus::Ok) as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_insert(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    index_ptr: *const rtdt::Index, _index_td: *const rtdt::TyDesc,
    elem_ptr: *mut u8, elem_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let index = unsafe { arg::<rtdt::Index>(index_ptr) };
    let list_ty = unsafe { td_arg(list_td) };
    let elem_ty = unsafe { td_arg(elem_td) };

    let status = unsafe {
        datalove_rti::call::dtlv_rti_list_insert_local(
            rt, list_ptr.cast(), list_ty.as_ptr(), index.0, elem_ptr.cast(), elem_ty.as_ptr(),
        )
    };
    // The runtime has taken the element either way.
    unsafe { write_result(_out, (status == datalove_rti::RtStatus::Ok) as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_remove(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    index_ptr: *const rtdt::Index, _index_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    let index = unsafe { arg::<rtdt::Index>(index_ptr) };
    let list_ty = unsafe { td_arg(list_td) };
    let out_ty = unsafe { td_arg(out_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_remove_local(
            rt, list_ptr.cast(), list_ty.as_ptr(), index.0, out.cast(), out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_swap(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    i_ptr: *const rtdt::Index, _i_td: *const rtdt::TyDesc,
    j_ptr: *const rtdt::Index, _j_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let i = unsafe { arg::<rtdt::Index>(i_ptr) };
    let j = unsafe { arg::<rtdt::Index>(j_ptr) };
    let status = unsafe {
        datalove_rti::call::dtlv_rti_list_swap_local(rt, list_ptr.cast(), list_td, i.0, j.0)
    };
    unsafe { write_result(out, (status == datalove_rti::RtStatus::Ok) as u8) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__list_reserve(
    rt: *mut u8,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    n_ptr: *const rtdt::Index, _n_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let n = unsafe { arg::<rtdt::Index>(n_ptr) };
    unsafe {
        datalove_rti::call::dtlv_rti_list_reserve_local(
            rt, list_ptr.cast(), list_td, n.0,
        ) as u8
    }
}

// --- Maps and sets ---
//
// Same shape as the lists above: the container comes with a descriptor naming
// its key, value or element type, and a key or value comes with its own.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_len(
    rt: *mut u8,
    map_ptr: *const rtdt::Map, map_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_len_local(
            rt, map_ptr.cast(), map_td, out.cast(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_clear(
    rt: *mut u8,
    map_ptr: *mut rtdt::Map, map_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_clear_local(
            rt, map_ptr.cast(), map_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_contains_key(
    rt: *mut u8,
    map_ptr: *const rtdt::Map, map_td: *const rtdt::TyDesc,
    key_ptr: *const u8, key_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_contains_key_local(
            rt, map_ptr.cast(), map_td,
            key_ptr.cast(), key_td,
            out as *mut bool,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_get(
    rt: *mut u8,
    map_ptr: *const rtdt::Map, map_td: *const rtdt::TyDesc,
    key_ptr: *const u8, key_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    let map_ty = unsafe { td_arg(map_td) };
    let out_ty = unsafe { td_arg(out_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_get_local(
            rt, map_ptr.cast(), map_ty.as_ptr(), key_ptr.cast(), key_td,
            out.cast(), out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_insert(
    rt: *mut u8,
    map_ptr: *mut rtdt::Map, map_td: *const rtdt::TyDesc,
    key_ptr: *mut u8, key_td: *const rtdt::TyDesc,
    value_ptr: *mut u8, value_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let map_ty = unsafe { td_arg(map_td) };
    let key_ty = unsafe { td_arg(key_td) };
    let value_ty = unsafe { td_arg(value_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_insert_local(
            rt, map_ptr.cast(), map_ty.as_ptr(),
            key_ptr.cast(), key_ty.as_ptr(), value_ptr.cast(), value_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_add(
    rt: *mut u8,
    map_ptr: *mut rtdt::Map, map_td: *const rtdt::TyDesc,
    key_ptr: *const u8, key_td: *const rtdt::TyDesc,
    amount_ptr: *mut u8, amount_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_add_local(
            rt, map_ptr.cast(), map_td,
            key_ptr.cast(), key_td,
            amount_ptr.cast(), amount_td,
            out as *mut bool,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_remove(
    rt: *mut u8,
    map_ptr: *mut rtdt::Map, map_td: *const rtdt::TyDesc,
    key_ptr: *const u8, key_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_remove_local(
            rt, map_ptr.cast(), map_td,
            key_ptr.cast(), key_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__set_len(
    rt: *mut u8,
    set_ptr: *const rtdt::Set, set_td: *const rtdt::TyDesc,
    out: *mut rtdt::Index, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_len_local(
            rt, set_ptr.cast(), set_td, out.cast(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__set_clear(
    rt: *mut u8,
    set_ptr: *mut rtdt::Set, set_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_clear_local(
            rt, set_ptr.cast(), set_td,
        ) as u8
    }
}

/// Every key, value or entry of a map, or element of a set, read out in one
/// walk onto the end of the caller's list. See `dtlv_rti_btreemap_keys_into_local`.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_keys_into(
    rt: *mut u8,
    map_ptr: *const rtdt::Map, map_td: *const rtdt::TyDesc,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_keys_into_local(
            rt, map_ptr.cast(), map_td,
            list_ptr as *mut u8, list_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_values_into(
    rt: *mut u8,
    map_ptr: *const rtdt::Map, map_td: *const rtdt::TyDesc,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_values_into_local(
            rt, map_ptr.cast(), map_td,
            list_ptr as *mut u8, list_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__map_entries_into(
    rt: *mut u8,
    map_ptr: *const rtdt::Map, map_td: *const rtdt::TyDesc,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_entries_into_local(
            rt, map_ptr.cast(), map_td,
            list_ptr as *mut u8, list_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__set_into_list(
    rt: *mut u8,
    set_ptr: *const rtdt::Set, set_td: *const rtdt::TyDesc,
    list_ptr: *mut rtdt::List, list_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_into_list_local(
            rt, set_ptr.cast(), set_td,
            list_ptr as *mut u8, list_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__set_contains(
    rt: *mut u8,
    set_ptr: *const rtdt::Set, set_td: *const rtdt::TyDesc,
    elem_ptr: *const u8, elem_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_contains_local(
            rt, set_ptr.cast(), set_td,
            elem_ptr.cast(), elem_td, out.cast(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__set_remove(
    rt: *mut u8,
    set_ptr: *mut rtdt::Set, set_td: *const rtdt::TyDesc,
    elem_ptr: *const u8, elem_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_remove_local(
            rt, set_ptr.cast(), set_td,
            elem_ptr.cast(), elem_td, out.cast(),
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__set_insert(
    rt: *mut u8,
    set_ptr: *mut rtdt::Set, set_td: *const rtdt::TyDesc,
    elem_ptr: *mut u8, elem_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let set_ty = unsafe { td_arg(set_td) };
    let elem_ty = unsafe { td_arg(elem_td) };

    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_insert_local(
            rt, set_ptr.cast(), set_ty.as_ptr(), elem_ptr.cast(), elem_ty.as_ptr(), out.cast(),
        ) as u8
    }
}

// --- Floating point math ---
//
// Every one of these is a call into the platform's libm. The operations with
// a machine instruction behind them - sqrt, the roundings, abs, min and max -
// are intrinsics instead, and are not here.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_sin(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.sin()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_cos(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.cos()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_tan(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.tan()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_asin(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.asin()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_acos(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.acos()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_atan(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.atan()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_sinh(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.sinh()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_cosh(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.cosh()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_tanh(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.tanh()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_exp(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.exp()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_exp2(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.exp2()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_ln(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.ln()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_log2(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.log2()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_log10(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.log10()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_cbrt(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_result(out, x.cbrt()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_atan2(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f64, _y_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    let y = unsafe { arg::<f64>(y_ptr) };
    unsafe { write_result(out, x.atan2(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_log(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f64, _y_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    let y = unsafe { arg::<f64>(y_ptr) };
    unsafe { write_result(out, x.log(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_pow(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f64, _y_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    let y = unsafe { arg::<f64>(y_ptr) };
    unsafe { write_result(out, x.powf(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_hypot(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f64, _y_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    let y = unsafe { arg::<f64>(y_ptr) };
    unsafe { write_result(out, x.hypot(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_rem(
    _rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f64, _y_td: *const rtdt::TyDesc,
    out: *mut f64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    let y = unsafe { arg::<f64>(y_ptr) };
    unsafe { write_result(out, x % y) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_sin(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.sin()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_cos(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.cos()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_tan(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.tan()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_asin(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.asin()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_acos(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.acos()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_atan(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.atan()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_sinh(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.sinh()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_cosh(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.cosh()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_tanh(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.tanh()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_exp(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.exp()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_exp2(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.exp2()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_ln(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.ln()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_log2(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.log2()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_log10(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.log10()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_cbrt(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_result(out, x.cbrt()) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_atan2(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f32, _y_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    let y = unsafe { arg::<f32>(y_ptr) };
    unsafe { write_result(out, x.atan2(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_log(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f32, _y_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    let y = unsafe { arg::<f32>(y_ptr) };
    unsafe { write_result(out, x.log(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_pow(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f32, _y_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    let y = unsafe { arg::<f32>(y_ptr) };
    unsafe { write_result(out, x.powf(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_hypot(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f32, _y_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    let y = unsafe { arg::<f32>(y_ptr) };
    unsafe { write_result(out, x.hypot(y)) };
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_rem(
    _rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    y_ptr: *const f32, _y_td: *const rtdt::TyDesc,
    out: *mut f32, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    let y = unsafe { arg::<f32>(y_ptr) };
    unsafe { write_result(out, x % y) };
    OK
}

// --- Conversion between the integers and the floats ---
//
// A bigint travels as decimal text in both directions. The digits are exact
// either way, so the only rounding is the one the float itself imposes, and
// an integer too large to represent parses as infinity, which is the case
// these report as none.

/// Convert a bigint to the nearest `f64`, or none if it is out of range.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f64_from_int(
    _rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let text = unsafe { rider_helpers::int_to_string(n_ptr) };
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => unsafe { rider_helpers::write_option_some(out, value) },
        _ => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

/// Convert a bigint to the nearest `f32`, or none if it is out of range.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__f32_from_int(
    _rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let text = unsafe { rider_helpers::int_to_string(n_ptr) };
    match text.parse::<f32>() {
        Ok(value) if value.is_finite() => unsafe { rider_helpers::write_option_some(out, value) },
        _ => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

// --- Conversion from the integers to the fixed-width integers ---

/// Convert a bigint to a `u64`, or none if it is out of range.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__int_to_u64(
    _rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let n = unsafe { ref_arg::<rtdt::Int>(n_ptr).to_i128() };
    match n.and_then(|n| u64::try_from(n).ok()) {
        Some(value) => unsafe { rider_helpers::write_option_some(out, value) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

/// Convert a bigint to an `i64`, or none if it is out of range.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__int_to_i64(
    _rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let n = unsafe { ref_arg::<rtdt::Int>(n_ptr).to_i128() };
    match n.and_then(|n| i64::try_from(n).ok()) {
        Some(value) => unsafe { rider_helpers::write_option_some(out, value) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

/// The low 64 bits of a bigint in two's complement, as a `u64`.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__int_low_bits_u64(
    _rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut u64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let bits = unsafe { ref_arg::<rtdt::Int>(n_ptr).low_u64() };
    unsafe { write_result(out, bits) };
    OK
}

/// The low 64 bits of a bigint in two's complement, as an `i64`.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__int_low_bits_i64(
    _rt: *mut u8,
    n_ptr: *const rtdt::Int, _n_td: *const rtdt::TyDesc,
    out: *mut i64, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let bits = unsafe { ref_arg::<rtdt::Int>(n_ptr).low_u64() };
    unsafe { write_result(out, bits as i64) };
    OK
}

/// The decimal digits of a float's integer part, truncated toward zero.
///
/// `None` for a value with no integer part to name, which is a nan or an
/// infinity. Every finite float is an exact decimal, so no digits are lost.
fn truncated_digits(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let truncated = value.trunc();
    if truncated == 0.0 {
        // Covers -0.0, which would otherwise print as "-0".
        Some("0".to_string())
    } else {
        Some(format!("{truncated:.0}"))
    }
}

/// Convert an `f64` to a bigint, truncating toward zero.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__int_from_f64(
    rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    match truncated_digits(x) {
        Some(digits) => unsafe { rider_helpers::write_option_int_from_str(rt, out, &digits) as u8 },
        None => {
            unsafe { rider_helpers::write_option_none(out) };
            OK
        }
    }
}

/// Convert an `f32` to a bigint, truncating toward zero.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__int_from_f32(
    rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    match truncated_digits(x as f64) {
        Some(digits) => unsafe { rider_helpers::write_option_int_from_str(rt, out, &digits) as u8 },
        None => {
            unsafe { rider_helpers::write_option_none(out) };
            OK
        }
    }
}

// --- Float formatting and parsing ---

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_from_f64(
    rt: *mut u8,
    x_ptr: *const f64, _x_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f64>(x_ptr) };
    unsafe { write_string_result(rt, out, out_td, &x.to_string()) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_from_f32(
    rt: *mut u8,
    x_ptr: *const f32, _x_td: *const rtdt::TyDesc,
    out: *mut rtdt::String, out_td: *const rtdt::TyDesc,
) -> u8 {
    let x = unsafe { arg::<f32>(x_ptr) };
    unsafe { write_string_result(rt, out, out_td, &x.to_string()) }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__string_parse_f64(
    _rt: *mut u8,
    s_ptr: *const rtdt::String, _s_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    let s = unsafe { str_arg(s_ptr) };
    match s.parse::<f64>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

// One-operand operations on a type parameter bounded to `float`. Each is the
// same shim: the runtime reads the width off the descriptor, because the one
// compiled body of a generic cannot hold the instruction for both.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_abs(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 0, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_sqrt(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 1, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_floor(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 2, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_ceil(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 3, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_round(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 4, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_trunc(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 5, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_fract(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 6, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_recip(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 7, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_signum(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 8, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_neg(
    rt: *mut u8,
    x: *mut u8, x_td: *const rtdt::TyDesc,
    out: *mut u8, out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 9, x, x_td,
            out.cast(), out_td,
        ) as u8
    }
}

// Constants of a type parameter bounded to `fixedint`. The type parameter
// appears only in the return, so no argument brings its descriptor and the
// call site hands one over after the out parameter.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__fixedint_zero(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 0, out.cast(), out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__fixedint_one(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 1, out.cast(), out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__fixedint_min_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 2, out.cast(), out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__fixedint_max_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 3, out.cast(), out_td,
            value_td,
        ) as u8
    }
}

/// The total order every value has, as a three-way comparison.
///
/// The operands are borrowed, each with its own descriptor.
#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__ord_compare(
    rt: *mut u8,
    a: *const u8, a_td: *const rtdt::TyDesc,
    b: *const u8, b_td: *const rtdt::TyDesc,
    out: *mut i8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        let ordering = datalove_rti::call::dtlv_rti_cmp_total_local(
            rt,
            a, a_td,
            b, b_td,
        );
        let answer: i8 = match ordering {
            datalove_rti::RtOrdering::Less => -1,
            datalove_rti::RtOrdering::Equal => 0,
            datalove_rti::RtOrdering::Greater => 1,
            // The two descriptors disagreed, which a call site binding one
            // type parameter cannot arrange.
            datalove_rti::RtOrdering::Error => return 2,
        };
        write_result(out, answer);
    }
    OK
}

// Constants of a type parameter bounded to `float`. Each takes its width from
// the descriptor the call site hands over, the type parameter appearing only
// in the return.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_zero(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 0, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_one(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 1, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_nan(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 2, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_infinity(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 3, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_neg_infinity(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 4, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_min_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 5, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_max_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 6, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_min_positive(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 7, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_epsilon(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 8, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_pi(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 9, out, out_td,
            value_td,
        ) as u8
    }
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__float_e(
    rt: *mut u8,
    out: *mut u8, out_td: *const rtdt::TyDesc,
    value_td: *const rtdt::TyDesc,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 10, out, out_td,
            value_td,
        ) as u8
    }
}

// --- Moving values in and out of places ---
//
// Generic over the type moved, whose size is read off the descriptor. The
// value's bytes are the value: whatever it owns comes along with them, so a
// move is a copy of the bytes and a forgetting of where they were.

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__mem_replace(
    _rt: *mut u8,
    dest_ptr: *mut u8, dest_td: *const rtdt::TyDesc,
    src_ptr: *mut u8, _src_td: *const rtdt::TyDesc,
    out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    // The caller gave `src` up, so its bytes move into `dest` and are not
    // destroyed there. Its slot is not written, since a copy value handed over
    // at a type parameter may still be the caller's.
    unsafe {
        let size = ref_arg::<rtdt::TyDesc>(dest_td).size as usize;
        std::ptr::copy(dest_ptr, out, size);
        std::ptr::copy(src_ptr, dest_ptr, size);
    }
    OK
}

#[no_mangle]
pub unsafe extern "C-unwind" fn dlr_std__mem_swap(
    _rt: *mut u8,
    a_ptr: *mut u8, a_td: *const rtdt::TyDesc,
    b_ptr: *mut u8, _b_td: *const rtdt::TyDesc,
    _out: *mut u8, _out_td: *const rtdt::TyDesc,
) -> u8 {
    // The two cannot overlap: two `mut` arguments rooted at one binding are
    // refused (D010).
    unsafe {
        let size = ref_arg::<rtdt::TyDesc>(a_td).size as usize;
        std::ptr::swap_nonoverlapping(a_ptr, b_ptr, size);
    }
    OK
}
