//! Native rider for sys/std: string operations.
//!
//! Each function follows the runtime C ABI convention:
//! `extern "C-unwind" fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> u8`
//!
//! For `ref string` parameters, the value pointer points to an `rtdt::String` struct.
//! Return values are written to the `result_out` pointer.
//! Returns 1 (Ok) on success, 2 (Error) on failure.

use datalove_rtdt as rtdt;
use datalove_rti::rider_helpers;

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
unsafe fn write_string_result(rt: *mut u8, out: *mut u8, out_td: *const u8, s: &str) -> u8 {
    let status = unsafe {
        datalove_rti::call::dtlv_rti_string_from_bytes(
            rt,
            s.as_ptr(),
            s.len() as rtdt::IndexRepr,
            out,
            out_td as *const rtdt::TyDesc,
        )
    };
    status as u8
}

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

// --- Character predicates ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_is_ascii_alphabetic(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic());
    unsafe { write_result(out, result as u8) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_is_ascii_digit(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    unsafe { write_result(out, result as u8) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_is_ascii_alphanumeric(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
    unsafe { write_result(out, result as u8) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_is_ascii_whitespace(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = !s.is_empty() && s.bytes().all(|b| b.is_ascii_whitespace());
    unsafe { write_result(out, result as u8) };
    OK
}

// --- Trimming ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_trim(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    unsafe { write_string_result(rt, out, out_td, s.trim()) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_trim_start(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    unsafe { write_string_result(rt, out, out_td, s.trim_start()) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_trim_end(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    unsafe { write_string_result(rt, out, out_td, s.trim_end()) }
}

// --- Case conversion ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_to_ascii_lowercase(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = s.to_ascii_lowercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_to_ascii_uppercase(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = s.to_ascii_uppercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Construction ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_repeat(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let n = unsafe { *(n_ptr as *const rtdt::Index) };
    let result = s.repeat(n.as_usize());
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Replacement ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_replace(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    pat_ptr: *const u8, _pat_td: *const u8,
    rep_ptr: *const u8, _rep_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let pat = unsafe { as_str(pat_ptr) };
    let rep = unsafe { as_str(rep_ptr) };
    let result = s.replace(pat, rep);
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Concatenation ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_concat(
    rt: *mut u8,
    a_ptr: *const u8, _a_td: *const u8,
    b_ptr: *const u8, _b_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let a = unsafe { as_str(a_ptr) };
    let b = unsafe { as_str(b_ptr) };
    let result = format!("{}{}", a, b);
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Option-returning functions ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_get_byte(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    idx_ptr: *const u8, _idx_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let idx = unsafe { *(idx_ptr as *const rtdt::Index) };
    let i = idx.as_usize();
    if i < s.len() {
        unsafe { rider_helpers::write_option_some(out, s.as_bytes()[i]) };
    } else {
        unsafe { rider_helpers::write_option_none(out) };
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_find(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    pat_ptr: *const u8, _pat_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let pat = unsafe { as_str(pat_ptr) };
    match s.find(pat) {
        Some(pos) => unsafe { rider_helpers::write_option_some(out, rtdt::Index::new(pos as rtdt::IndexRepr)) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_rfind(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    pat_ptr: *const u8, _pat_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let pat = unsafe { as_str(pat_ptr) };
    match s.rfind(pat) {
        Some(pos) => unsafe { rider_helpers::write_option_some(out, rtdt::Index::new(pos as rtdt::IndexRepr)) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_strip_prefix(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    prefix_ptr: *const u8, _prefix_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let prefix = unsafe { as_str(prefix_ptr) };
    match s.strip_prefix(prefix) {
        Some(rest) => unsafe { rider_helpers::write_option_some_string(rt, out, rest) as u8 },
        None => { unsafe { rider_helpers::write_option_none(out) }; OK },
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_strip_suffix(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    suffix_ptr: *const u8, _suffix_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let suffix = unsafe { as_str(suffix_ptr) };
    match s.strip_suffix(suffix) {
        Some(rest) => unsafe { rider_helpers::write_option_some_string(rt, out, rest) as u8 },
        None => { unsafe { rider_helpers::write_option_none(out) }; OK },
    }
}

// --- Unicode case conversion ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_to_lowercase(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = s.to_lowercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_to_uppercase(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let result = s.to_uppercase();
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Mutation ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_push_str(
    rt: *mut u8,
    self_ptr: *mut u8, self_td: *const u8,
    other_ptr: *const u8, _other_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let other = unsafe { as_str(other_ptr) };
    if other.is_empty() {
        return OK;
    }
    let status = unsafe {
        datalove_rti::call::dtlv_rti_string_push_bytes_local(
            rt,
            self_ptr,
            self_td as *const rtdt::TyDesc,
            other.as_ptr(),
            other.len() as rtdt::IndexRepr,
        )
    };
    status as u8
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_clear(
    _rt: *mut u8,
    self_ptr: *mut u8, _self_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        let s = &mut *(self_ptr as *mut rtdt::String);
        s.size = rtdt::Index::ZERO;
    }
    OK
}

/// Helper: convert an rtdt::String to a mutable byte slice.
///
/// # Safety
///
/// The pointer must be valid and point to a live, heap-allocated `rtdt::String`.
unsafe fn as_mut_bytes<'a>(ptr: *mut u8) -> &'a mut [u8] {
    let s = &mut *(ptr as *mut rtdt::String);
    if s.data.is_null() || s.size.0 == 0 {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(s.data as *mut u8, s.size.as_usize())
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_push_char(
    rt: *mut u8,
    self_ptr: *mut u8, self_td: *const u8,
    ch_ptr: *const u8, _ch_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let codepoint = unsafe { *(ch_ptr as *const u32) };
    let ch = match char::from_u32(codepoint) {
        Some(c) => c,
        None => return OK, // Invalid codepoint: no-op.
    };
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    let status = unsafe {
        datalove_rti::call::dtlv_rti_string_push_bytes_local(
            rt,
            self_ptr,
            self_td as *const rtdt::TyDesc,
            encoded.as_ptr(),
            encoded.len() as rtdt::IndexRepr,
        )
    };
    status as u8
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_pop(
    _rt: *mut u8,
    self_ptr: *mut u8, _self_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(self_ptr) };
    match s.chars().next_back() {
        Some(ch) => {
            let new_len = s.len() - ch.len_utf8();
            let s_mut = unsafe { &mut *(self_ptr as *mut rtdt::String) };
            s_mut.size = rtdt::Index::new(new_len as rtdt::IndexRepr);
            unsafe { rider_helpers::write_option_some(out, ch as u32) };
        }
        None => {
            unsafe { rider_helpers::write_option_none(out) };
        }
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_truncate(
    _rt: *mut u8,
    self_ptr: *mut u8, _self_td: *const u8,
    len_ptr: *const u8, _len_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let new_len = unsafe { *(len_ptr as *const rtdt::Index) }.as_usize();
    let s = unsafe { as_str(self_ptr) };
    if new_len >= s.len() {
        return OK; // No-op if new_len >= current length.
    }
    assert!(s.is_char_boundary(new_len), "truncate: not a char boundary");
    let s_mut = unsafe { &mut *(self_ptr as *mut rtdt::String) };
    s_mut.size = rtdt::Index::new(new_len as rtdt::IndexRepr);
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_remove(
    _rt: *mut u8,
    self_ptr: *mut u8, _self_td: *const u8,
    idx_ptr: *const u8, _idx_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let idx = unsafe { *(idx_ptr as *const rtdt::Index) }.as_usize();
    let s = unsafe { as_str(self_ptr) };
    if idx >= s.len() || !s.is_char_boundary(idx) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let ch = s[idx..].chars().next().unwrap();
    let ch_len = ch.len_utf8();
    // Shift bytes after the removed character.
    let bytes = unsafe { as_mut_bytes(self_ptr) };
    let remaining = bytes.len() - idx - ch_len;
    if remaining > 0 {
        unsafe {
            std::ptr::copy(
                bytes.as_ptr().add(idx + ch_len),
                bytes.as_mut_ptr().add(idx),
                remaining,
            );
        }
    }
    let s_mut = unsafe { &mut *(self_ptr as *mut rtdt::String) };
    s_mut.size = rtdt::Index::new((s_mut.size.as_usize() - ch_len) as rtdt::IndexRepr);
    unsafe { rider_helpers::write_option_some(out, ch as u32) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_insert_char(
    rt: *mut u8,
    self_ptr: *mut u8, self_td: *const u8,
    idx_ptr: *const u8, _idx_td: *const u8,
    ch_ptr: *const u8, _ch_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let idx = unsafe { *(idx_ptr as *const rtdt::Index) }.as_usize();
    let codepoint = unsafe { *(ch_ptr as *const u32) };
    let ch = match char::from_u32(codepoint) {
        Some(c) => c,
        None => return OK, // Invalid codepoint: no-op.
    };
    let s = unsafe { as_str(self_ptr) };
    if idx > s.len() || !s.is_char_boundary(idx) {
        return OK; // Out of bounds or not char boundary: no-op.
    }
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    let encoded_len = encoded.len();

    // First, grow the buffer by pushing the encoded bytes at the end.
    let status = unsafe {
        datalove_rti::call::dtlv_rti_string_push_bytes_local(
            rt,
            self_ptr,
            self_td as *const rtdt::TyDesc,
            encoded.as_ptr(),
            encoded_len as rtdt::IndexRepr,
        )
    };
    if status as u8 != OK {
        return status as u8;
    }

    // Now shift existing bytes after idx to make room.
    let bytes = unsafe { as_mut_bytes(self_ptr) };
    let total_len = bytes.len();
    let old_len = total_len - encoded_len;
    // Move bytes [idx..old_len] to [idx+encoded_len..total_len].
    if idx < old_len {
        unsafe {
            std::ptr::copy(
                bytes.as_ptr().add(idx),
                bytes.as_mut_ptr().add(idx + encoded_len),
                old_len - idx,
            );
        }
    }
    // Copy encoded char into the gap.
    bytes[idx..idx + encoded_len].copy_from_slice(encoded.as_bytes());
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_insert_str(
    rt: *mut u8,
    self_ptr: *mut u8, self_td: *const u8,
    idx_ptr: *const u8, _idx_td: *const u8,
    other_ptr: *const u8, _other_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let idx = unsafe { *(idx_ptr as *const rtdt::Index) }.as_usize();
    let other = unsafe { as_str(other_ptr) };
    if other.is_empty() {
        return OK;
    }
    let s = unsafe { as_str(self_ptr) };
    if idx > s.len() || !s.is_char_boundary(idx) {
        return OK; // Out of bounds or not char boundary: no-op.
    }
    let insert_len = other.len();
    // Grab the other bytes before mutating self (other_ptr could alias).
    let other_bytes: Vec<u8> = other.as_bytes().to_vec();

    // Grow the buffer by pushing the insert bytes at the end.
    let status = unsafe {
        datalove_rti::call::dtlv_rti_string_push_bytes_local(
            rt,
            self_ptr,
            self_td as *const rtdt::TyDesc,
            other_bytes.as_ptr(),
            insert_len as rtdt::IndexRepr,
        )
    };
    if status as u8 != OK {
        return status as u8;
    }

    // Now shift existing bytes after idx to make room.
    let bytes = unsafe { as_mut_bytes(self_ptr) };
    let total_len = bytes.len();
    let old_len = total_len - insert_len;
    if idx < old_len {
        unsafe {
            std::ptr::copy(
                bytes.as_ptr().add(idx),
                bytes.as_mut_ptr().add(idx + insert_len),
                old_len - idx,
            );
        }
    }
    // Copy inserted string into the gap.
    bytes[idx..idx + insert_len].copy_from_slice(&other_bytes);
    OK
}

// --- Splitting (option-of-tuple) ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_split_once(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    delim_ptr: *const u8, _delim_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let delim = unsafe { as_str(delim_ptr) };
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
pub extern "C-unwind" fn dlr_std__string_rsplit_once(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    delim_ptr: *const u8, _delim_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let delim = unsafe { as_str(delim_ptr) };
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
pub extern "C-unwind" fn dlr_std__string_char_at(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    idx_ptr: *const u8, _idx_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let idx = unsafe { *(idx_ptr as *const rtdt::Index) }.as_usize();
    match s.chars().nth(idx) {
        Some(ch) => unsafe { rider_helpers::write_option_some(out, ch as u32) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_char_to_byte_index(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    idx_ptr: *const u8, _idx_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let char_idx = unsafe { *(idx_ptr as *const rtdt::Index) }.as_usize();
    match s.char_indices().nth(char_idx) {
        Some((byte_idx, _)) => unsafe { rider_helpers::write_option_some(out, rtdt::Index::new(byte_idx as rtdt::IndexRepr)) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

// --- Slicing ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_slice(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    start_ptr: *const u8, _start_td: *const u8,
    end_ptr: *const u8, _end_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let start = unsafe { *(start_ptr as *const rtdt::Index) }.as_usize();
    let end = unsafe { *(end_ptr as *const rtdt::Index) }.as_usize();
    if start > end || end > s.len() || !s.is_char_boundary(start) || !s.is_char_boundary(end) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let status = unsafe { rider_helpers::write_option_some_string(rt, out, &s[start..end]) };
    status as u8
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_slice_from(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    start_ptr: *const u8, _start_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let start = unsafe { *(start_ptr as *const rtdt::Index) }.as_usize();
    if start > s.len() || !s.is_char_boundary(start) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let status = unsafe { rider_helpers::write_option_some_string(rt, out, &s[start..]) };
    status as u8
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_slice_to(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    end_ptr: *const u8, _end_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let end = unsafe { *(end_ptr as *const rtdt::Index) }.as_usize();
    if end > s.len() || !s.is_char_boundary(end) {
        unsafe { rider_helpers::write_option_none(out) };
        return OK;
    }
    let status = unsafe { rider_helpers::write_option_some_string(rt, out, &s[..end]) };
    status as u8
}

// --- Parsing ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_parse_u32(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    match s.parse::<u32>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_parse_i32(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    match s.parse::<i32>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_parse_f32(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    match s.parse::<f32>() {
        Ok(val) => unsafe { rider_helpers::write_option_some(out, val) },
        Err(_) => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

// --- Formatting ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_from_u32(
    rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let n = unsafe { *(n_ptr as *const u32) };
    let s = n.to_string();
    unsafe { write_string_result(rt, out, out_td, &s) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_from_i32(
    rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let n = unsafe { *(n_ptr as *const i32) };
    let s = n.to_string();
    unsafe { write_string_result(rt, out, out_td, &s) }
}

// --- Construction ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_from_char(
    rt: *mut u8,
    ch_ptr: *const u8, _ch_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let codepoint = unsafe { *(ch_ptr as *const u32) };
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
pub extern "C-unwind" fn dlr_std__string_replacen(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    pat_ptr: *const u8, _pat_td: *const u8,
    rep_ptr: *const u8, _rep_td: *const u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let pat = unsafe { as_str(pat_ptr) };
    let rep = unsafe { as_str(rep_ptr) };
    let n = unsafe { *(n_ptr as *const rtdt::Index) };
    let result = s.replacen(pat, rep, n.as_usize());
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Splitting (list returns) ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_split(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    delim_ptr: *const u8, _delim_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let delim = unsafe { as_str(delim_ptr) };
    let parts: Vec<&str> = s.split(delim).collect();
    let status = unsafe {
        rider_helpers::write_string_list(rt, out, out_td as *const rtdt::TyDesc, &parts)
    };
    status as u8
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_lines(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let parts: Vec<&str> = s.lines().collect();
    let status = unsafe {
        rider_helpers::write_string_list(rt, out, out_td as *const rtdt::TyDesc, &parts)
    };
    status as u8
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_split_whitespace(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let parts: Vec<&str> = s.split_whitespace().collect();
    let status = unsafe {
        rider_helpers::write_string_list(rt, out, out_td as *const rtdt::TyDesc, &parts)
    };
    status as u8
}

// --- Joining ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_join(
    rt: *mut u8,
    parts_ptr: *const u8, _parts_td: *const u8,
    sep_ptr: *const u8, _sep_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let parts = unsafe { rider_helpers::read_string_list(parts_ptr) };
    let sep = unsafe { as_str(sep_ptr) };
    let result = parts.join(sep);
    unsafe { write_string_result(rt, out, out_td, &result) }
}

// --- Bigint conversion ---

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_from_int(
    rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let s = unsafe { rider_helpers::int_to_string(n_ptr) };
    unsafe { write_string_result(rt, out, out_td, &s) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_parse_int(
    rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
    let status = unsafe { rider_helpers::write_option_int_from_str(rt, out, s) };
    status as u8
}

// --- Lists ---
//
// These are generic over the element type. Each parameter arrives as a pointer
// and a descriptor, so the element type is read off the list at runtime rather
// than being known when this is compiled.
//
// The descriptor also says which side of the boundary the caller is on. A call
// site that knows the element type asks for the element itself, and its
// descriptor names that type. A generic function calling one of these has no
// such type, so its slot is a `data` and its descriptor says so; the element
// then has to be packed into one on the way out and unpacked on the way in. A
// list of `data` is the case where the two readings coincide, so the element
// type is compared as well: a position is boxed only when the caller asks for
// a `data` and the element is not one already.

/// Whether an element has to be packed into a `data` to reach `slot_ty`.
fn boxes_the_element(slot_ty: rtdt::TyDescRef, element_ty: rtdt::TyDescRef) -> bool {
    slot_ty.type_tag() == rtdt::TyTag::Data && element_ty.type_tag() != rtdt::TyTag::Data
}

/// Report whether an element-taking operation succeeded, destroying the
/// element if it did not.
///
/// These take ownership of the element whatever happens, because the call site
/// gave it up to make the call. An index past the end is a `false` rather than
/// a failed call, so the element it could not place is dropped here rather
/// than left to no one.
unsafe fn placed_or_dropped(
    rt: *mut u8,
    status: datalove_rti::RtStatus,
    elem_ptr: *mut u8,
    elem_td: *const u8,
) -> bool {
    if status == datalove_rti::RtStatus::Ok {
        return true;
    }
    unsafe {
        datalove_rti::call::dtlv_rti_any_destroy_local(
            rt, elem_ptr, elem_td as *const rtdt::TyDesc,
        );
    }
    false
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_len(
    rt: *mut u8,
    list_ptr: *const u8, list_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_list_len_local(
            rt, list_ptr, list_td as *const rtdt::TyDesc, out,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__tensor_len(
    _rt: *mut u8,
    tensor_ptr: *const u8, _tensor_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    // A tensor's length along axis 0 is the first entry of its shape. Nothing
    // in the header says it: the header is a pointer, an offset, a capacity
    // and the two shape arrays.
    unsafe {
        let tensor = &*(tensor_ptr as *const rtdt::Tensor);
        let len = if tensor.shape.is_null() {
            rtdt::Index::ZERO
        } else {
            *tensor.shape
        };
        *(out as *mut rtdt::Index) = len;
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__tensor_rank(
    _rt: *mut u8,
    _tensor_ptr: *const u8, tensor_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        let ty = rtdt::TyDescRef::from_ptr(tensor_td as *const rtdt::TyDesc);
        let rank = (*ty.as_ptr()).type_info.tensor.rank;
        *(out as *mut rtdt::Index) = rtdt::Index(rank as rtdt::IndexRepr);
    }
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_get(
    rt: *mut u8,
    list_ptr: *const u8, list_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let list_ty = unsafe { rtdt::TyDescRef::from_ptr(list_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_get_erased_local(
            rt, list_ptr, list_ty.as_ptr(), index.0, out, out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_push(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    elem_ptr: *mut u8, elem_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let list_ty = unsafe { rtdt::TyDescRef::from_ptr(list_td as *const rtdt::TyDesc) };
    let elem_ty = unsafe { rtdt::TyDescRef::from_ptr(elem_td as *const rtdt::TyDesc) };

    unsafe {
        // The erased entry rather than the two branches, because there are
        // three cases and not two: an element wrapped whole, one already in
        // the slot's shape, and one that is the slot's shape with `data` at
        // some position inside it -- a `(A, B)` pushed into a `[(string,
        // u32)]` -- which is converted position by position.
        datalove_rti::call::dtlv_rti_list_push_erased_local(
            rt, list_ptr, list_ty.as_ptr(), elem_ptr, elem_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_pop(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let list_ty = unsafe { rtdt::TyDescRef::from_ptr(list_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_pop_erased_local(
            rt, list_ptr, list_ty.as_ptr(), out, out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_clear(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_list_clear_local(
            rt, list_ptr, list_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_set(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    elem_ptr: *mut u8, elem_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let list_ty = unsafe { rtdt::TyDescRef::from_ptr(list_td as *const rtdt::TyDesc) };
    let elem_ty = unsafe { rtdt::TyDescRef::from_ptr(elem_td as *const rtdt::TyDesc) };

    let status = unsafe {
        datalove_rti::call::dtlv_rti_list_set_erased_local(
            rt, list_ptr, list_ty.as_ptr(), index.0, elem_ptr, elem_ty.as_ptr(),
        )
    };
    let placed = unsafe { placed_or_dropped(rt, status, elem_ptr, elem_td) };
    unsafe { write_result(_out, placed) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_insert(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    elem_ptr: *mut u8, elem_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let list_ty = unsafe { rtdt::TyDescRef::from_ptr(list_td as *const rtdt::TyDesc) };
    let elem_ty = unsafe { rtdt::TyDescRef::from_ptr(elem_td as *const rtdt::TyDesc) };

    let status = unsafe {
        datalove_rti::call::dtlv_rti_list_insert_erased_local(
            rt, list_ptr, list_ty.as_ptr(), index.0, elem_ptr, elem_ty.as_ptr(),
        )
    };
    let placed = unsafe { placed_or_dropped(rt, status, elem_ptr, elem_td) };
    unsafe { write_result(_out, placed) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_remove(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let list_ty = unsafe { rtdt::TyDescRef::from_ptr(list_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };

    unsafe {
        datalove_rti::call::dtlv_rti_list_remove_erased_local(
            rt, list_ptr, list_ty.as_ptr(), index.0, out, out_ty.as_ptr(),
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__list_reserve(
    rt: *mut u8,
    list_ptr: *mut u8, list_td: *const u8,
    n_ptr: *const u8, _n_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let n = unsafe { *(n_ptr as *const rtdt::Index) };
    unsafe {
        datalove_rti::call::dtlv_rti_list_reserve_local(
            rt, list_ptr, list_td as *const rtdt::TyDesc, n.0,
        ) as u8
    }
}

// --- Maps and sets ---
//
// Same shape as the lists above: the container comes with a descriptor naming
// its key, value or element type, and a position the caller has no static type
// for is carried as a `data`. A key being looked up is borrowed rather than
// given away, so it arrives as itself and needs none of this.

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_len(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_len_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc, out,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_clear(
    rt: *mut u8,
    map_ptr: *mut u8, map_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_clear_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_contains_key(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    key_ptr: *const u8, key_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_contains_key_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
            key_ptr, key_td as *const rtdt::TyDesc,
            out as *mut bool,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_get(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    key_ptr: *const u8, key_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let map_ty = unsafe { rtdt::TyDescRef::from_ptr(map_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };

    unsafe {
        if boxes_the_element(out_ty.option_inner_ty(), map_ty.map_value_ty()) {
            datalove_rti::call::dtlv_rti_btreemap_get_as_data_local(
                rt, map_ptr, map_ty.as_ptr(), key_ptr, key_td as *const rtdt::TyDesc,
                out, out_ty.as_ptr(),
            ) as u8
        } else {
            datalove_rti::call::dtlv_rti_btreemap_get_local(
                rt, map_ptr, map_ty.as_ptr(), key_ptr, key_td as *const rtdt::TyDesc,
                out, out_ty.as_ptr(),
            ) as u8
        }
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_insert(
    rt: *mut u8,
    map_ptr: *mut u8, map_td: *const u8,
    key_ptr: *mut u8, key_td: *const u8,
    value_ptr: *mut u8, value_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    let map_ty = unsafe { rtdt::TyDescRef::from_ptr(map_td as *const rtdt::TyDesc) };
    let key_ty = unsafe { rtdt::TyDescRef::from_ptr(key_td as *const rtdt::TyDesc) };
    let value_ty = unsafe { rtdt::TyDescRef::from_ptr(value_td as *const rtdt::TyDesc) };

    unsafe {
        // Both positions are boxed together or neither is, since a generic
        // caller has a static type for neither.
        if boxes_the_element(key_ty, map_ty.map_key_ty())
            || boxes_the_element(value_ty, map_ty.map_value_ty())
        {
            datalove_rti::call::dtlv_rti_btreemap_insert_data_local(
                rt, map_ptr, map_ty.as_ptr(), key_ptr, value_ptr,
            ) as u8
        } else {
            datalove_rti::call::dtlv_rti_btreemap_insert_local(
                rt, map_ptr, map_ty.as_ptr(),
                key_ptr, key_ty.as_ptr(), value_ptr, value_ty.as_ptr(),
            ) as u8
        }
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_add(
    rt: *mut u8,
    map_ptr: *mut u8, map_td: *const u8,
    key_ptr: *const u8, key_td: *const u8,
    amount_ptr: *mut u8, amount_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_add_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
            key_ptr, key_td as *const rtdt::TyDesc,
            amount_ptr, amount_td as *const rtdt::TyDesc,
            out as *mut bool,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_remove(
    rt: *mut u8,
    map_ptr: *mut u8, map_td: *const u8,
    key_ptr: *const u8, key_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_remove_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
            key_ptr, key_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_len(
    rt: *mut u8,
    set_ptr: *const u8, set_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_len_local(
            rt, set_ptr, set_td as *const rtdt::TyDesc, out,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_clear(
    rt: *mut u8,
    set_ptr: *mut u8, set_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_clear_local(
            rt, set_ptr, set_td as *const rtdt::TyDesc,
        ) as u8
    }
}

/// The element at a position in sort order. See `dlr_std__list_get`: the same
/// choice of packing, for the same reason.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_get(
    rt: *mut u8,
    set_ptr: *const u8, set_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let set_ty = unsafe { rtdt::TyDescRef::from_ptr(set_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };
    let as_data = boxes_the_element(out_ty.option_inner_ty(), set_ty.set_element_ty());
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_get_at_local(
            rt, set_ptr, set_ty.as_ptr(), index.0, out, out_ty.as_ptr(), as_data,
        ) as u8
    }
}

/// The key of the entry at a position in sort order.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_key_at(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let map_ty = unsafe { rtdt::TyDescRef::from_ptr(map_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };
    let as_data = boxes_the_element(out_ty.option_inner_ty(), map_ty.map_key_ty());
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_key_at_local(
            rt, map_ptr, map_ty.as_ptr(), index.0, out, out_ty.as_ptr(), as_data,
        ) as u8
    }
}

/// The value of the entry at a position in sort order.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_value_at(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    index_ptr: *const u8, _index_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let index = unsafe { *(index_ptr as *const rtdt::Index) };
    let map_ty = unsafe { rtdt::TyDescRef::from_ptr(map_td as *const rtdt::TyDesc) };
    let out_ty = unsafe { rtdt::TyDescRef::from_ptr(out_td as *const rtdt::TyDesc) };
    let as_data = boxes_the_element(out_ty.option_inner_ty(), map_ty.map_value_ty());
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_value_at_local(
            rt, map_ptr, map_ty.as_ptr(), index.0, out, out_ty.as_ptr(), as_data,
        ) as u8
    }
}

/// Every key, value or entry of a map, or element of a set, read out in one
/// walk onto the end of the caller's list. See `dtlv_rti_btreemap_keys_into_local`.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_keys_into(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    list_ptr: *const u8, list_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_keys_into_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
            list_ptr as *mut u8, list_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_values_into(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    list_ptr: *const u8, list_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_values_into_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
            list_ptr as *mut u8, list_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__map_entries_into(
    rt: *mut u8,
    map_ptr: *const u8, map_td: *const u8,
    list_ptr: *const u8, list_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreemap_entries_into_local(
            rt, map_ptr, map_td as *const rtdt::TyDesc,
            list_ptr as *mut u8, list_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_into_list(
    rt: *mut u8,
    set_ptr: *const u8, set_td: *const u8,
    list_ptr: *const u8, list_td: *const u8,
    _out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_into_list_local(
            rt, set_ptr, set_td as *const rtdt::TyDesc,
            list_ptr as *mut u8, list_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_contains(
    rt: *mut u8,
    set_ptr: *const u8, set_td: *const u8,
    elem_ptr: *const u8, elem_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_contains_local(
            rt, set_ptr, set_td as *const rtdt::TyDesc,
            elem_ptr, elem_td as *const rtdt::TyDesc, out,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_remove(
    rt: *mut u8,
    set_ptr: *mut u8, set_td: *const u8,
    elem_ptr: *const u8, elem_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_btreeset_remove_local(
            rt, set_ptr, set_td as *const rtdt::TyDesc,
            elem_ptr, elem_td as *const rtdt::TyDesc, out,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__set_insert(
    rt: *mut u8,
    set_ptr: *mut u8, set_td: *const u8,
    elem_ptr: *mut u8, elem_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let set_ty = unsafe { rtdt::TyDescRef::from_ptr(set_td as *const rtdt::TyDesc) };
    let elem_ty = unsafe { rtdt::TyDescRef::from_ptr(elem_td as *const rtdt::TyDesc) };

    unsafe {
        if boxes_the_element(elem_ty, set_ty.set_element_ty()) {
            datalove_rti::call::dtlv_rti_btreeset_insert_data_local(
                rt, set_ptr, set_ty.as_ptr(), elem_ptr, out,
            ) as u8
        } else {
            datalove_rti::call::dtlv_rti_btreeset_insert_local(
                rt, set_ptr, set_ty.as_ptr(), elem_ptr, elem_ty.as_ptr(), out,
            ) as u8
        }
    }
}

// --- Floating point math ---
//
// Every one of these is a call into the platform's libm. The operations with
// a machine instruction behind them - sqrt, the roundings, abs, min and max -
// are intrinsics instead, and are not here.

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_sin(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.sin()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_cos(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.cos()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_tan(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.tan()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_asin(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.asin()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_acos(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.acos()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_atan(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.atan()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_sinh(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.sinh()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_cosh(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.cosh()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_tanh(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.tanh()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_exp(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.exp()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_exp2(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.exp2()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_ln(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.ln()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_log2(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.log2()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_log10(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.log10()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_cbrt(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_result(out, x.cbrt()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_atan2(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    let y = unsafe { *(y_ptr as *const f64) };
    unsafe { write_result(out, x.atan2(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_log(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    let y = unsafe { *(y_ptr as *const f64) };
    unsafe { write_result(out, x.log(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_pow(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    let y = unsafe { *(y_ptr as *const f64) };
    unsafe { write_result(out, x.powf(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_hypot(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    let y = unsafe { *(y_ptr as *const f64) };
    unsafe { write_result(out, x.hypot(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f64_rem(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    let y = unsafe { *(y_ptr as *const f64) };
    unsafe { write_result(out, x % y) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_sin(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.sin()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_cos(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.cos()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_tan(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.tan()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_asin(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.asin()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_acos(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.acos()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_atan(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.atan()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_sinh(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.sinh()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_cosh(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.cosh()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_tanh(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.tanh()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_exp(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.exp()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_exp2(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.exp2()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_ln(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.ln()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_log2(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.log2()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_log10(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.log10()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_cbrt(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_result(out, x.cbrt()) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_atan2(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    let y = unsafe { *(y_ptr as *const f32) };
    unsafe { write_result(out, x.atan2(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_log(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    let y = unsafe { *(y_ptr as *const f32) };
    unsafe { write_result(out, x.log(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_pow(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    let y = unsafe { *(y_ptr as *const f32) };
    unsafe { write_result(out, x.powf(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_hypot(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    let y = unsafe { *(y_ptr as *const f32) };
    unsafe { write_result(out, x.hypot(y)) };
    OK
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__f32_rem(
    _rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    y_ptr: *const u8, _y_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    let y = unsafe { *(y_ptr as *const f32) };
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
pub extern "C-unwind" fn dlr_std__f64_from_int(
    _rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, _out_td: *const u8,
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
pub extern "C-unwind" fn dlr_std__f32_from_int(
    _rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, _out_td: *const u8,
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
pub extern "C-unwind" fn dlr_std__int_to_u64(
    _rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let n = unsafe { (*(n_ptr as *const rtdt::Int)).to_i128() };
    match n.and_then(|n| u64::try_from(n).ok()) {
        Some(value) => unsafe { rider_helpers::write_option_some(out, value) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

/// Convert a bigint to an `i64`, or none if it is out of range.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__int_to_i64(
    _rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let n = unsafe { (*(n_ptr as *const rtdt::Int)).to_i128() };
    match n.and_then(|n| i64::try_from(n).ok()) {
        Some(value) => unsafe { rider_helpers::write_option_some(out, value) },
        None => unsafe { rider_helpers::write_option_none(out) },
    }
    OK
}

/// The low 64 bits of a bigint in two's complement, as a `u64`.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__int_low_bits_u64(
    _rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let bits = unsafe { (*(n_ptr as *const rtdt::Int)).low_u64() };
    unsafe { write_result(out, bits) };
    OK
}

/// The low 64 bits of a bigint in two's complement, as an `i64`.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__int_low_bits_i64(
    _rt: *mut u8,
    n_ptr: *const u8, _n_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let bits = unsafe { (*(n_ptr as *const rtdt::Int)).low_u64() };
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
pub extern "C-unwind" fn dlr_std__int_from_f64(
    rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
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
pub extern "C-unwind" fn dlr_std__int_from_f32(
    rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
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
pub extern "C-unwind" fn dlr_std__string_from_f64(
    rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f64) };
    unsafe { write_string_result(rt, out, out_td, &x.to_string()) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_from_f32(
    rt: *mut u8,
    x_ptr: *const u8, _x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    let x = unsafe { *(x_ptr as *const f32) };
    unsafe { write_string_result(rt, out, out_td, &x.to_string()) }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__string_parse_f64(
    _rt: *mut u8,
    s_ptr: *const u8, _s_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    let s = unsafe { as_str(s_ptr) };
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
pub extern "C-unwind" fn dlr_std__float_abs(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 0, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_sqrt(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 1, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_floor(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 2, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_ceil(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 3, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_round(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 4, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_trunc(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 5, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_fract(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 6, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_recip(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 7, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_signum(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 8, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_neg(
    rt: *mut u8,
    x: *const u8, x_td: *const u8,
    out: *mut u8, out_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_unop(
            rt, 9, x, x_td as *const rtdt::TyDesc,
            out, out_td as *const rtdt::TyDesc,
        ) as u8
    }
}

// Constants of a type parameter bounded to `fixedint`. The type parameter
// appears only in the return, so no argument brings its descriptor and the
// call site hands one over after the out parameter.

#[no_mangle]
pub extern "C-unwind" fn dlr_std__fixedint_zero(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 0, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__fixedint_one(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 1, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__fixedint_min_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 2, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__fixedint_max_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_const(
            rt, 3, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

/// The total order every value has, as a three-way comparison.
///
/// The operands are taken by value, so inside a generic each arrives as the
/// `data` wrapping it rather than as the thing itself. The runtime's own
/// comparison reads through a wrapper, so both readings work; what would not
/// work is borrowing through one, since a value packed into a `data`'s own
/// words has no address.
///
/// Ownership passes with them, and comparing does not consume: both are
/// destroyed here, which is what taking them by value promised.
#[no_mangle]
pub extern "C-unwind" fn dlr_std__ord_compare(
    rt: *mut u8,
    a: *mut u8, a_td: *const u8,
    b: *mut u8, b_td: *const u8,
    out: *mut u8, _out_td: *const u8,
) -> u8 {
    unsafe {
        let ordering = datalove_rti::call::dtlv_rti_cmp_total_local(
            rt,
            a, a_td as *const rtdt::TyDesc,
            b, b_td as *const rtdt::TyDesc,
        );
        let answer: i8 = match ordering {
            datalove_rti::RtOrdering::Less => -1,
            datalove_rti::RtOrdering::Equal => 0,
            datalove_rti::RtOrdering::Greater => 1,
            // The two descriptors disagreed, which a call site binding one
            // type parameter cannot arrange.
            datalove_rti::RtOrdering::Error => return 2,
        };
        *(out as *mut i8) = answer;
        datalove_rti::call::dtlv_rti_any_destroy_local(rt, a as *mut u8, a_td as *const rtdt::TyDesc);
        datalove_rti::call::dtlv_rti_any_destroy_local(rt, b as *mut u8, b_td as *const rtdt::TyDesc);
    }
    OK
}

// Constants of a type parameter bounded to `float`. Each takes its width from
// the descriptor the call site hands over, the type parameter appearing only
// in the return.

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_zero(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 0, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_one(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 1, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_nan(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 2, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_infinity(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 3, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_neg_infinity(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 4, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_min_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 5, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_max_value(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 6, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_min_positive(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 7, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_epsilon(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 8, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_pi(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 9, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}

#[no_mangle]
pub extern "C-unwind" fn dlr_std__float_e(
    rt: *mut u8,
    out: *mut u8, out_td: *const u8,
    value_td: *const u8,
) -> u8 {
    unsafe {
        datalove_rti::call::dtlv_rti_dyn_float_const(
            rt, 10, out, out_td as *const rtdt::TyDesc,
            value_td as *const rtdt::TyDesc,
        ) as u8
    }
}
