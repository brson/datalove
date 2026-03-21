//! Native rider for sys/std: string operations.
//!
//! Each function follows the runtime C ABI convention:
//! `extern "C-unwind" fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> u8`
//!
//! For `ref string` parameters, the value pointer points to an `rtdt::String` struct.
//! Return values are written to the `result_out` pointer.
//! Returns 1 (Ok) on success, 2 (Error) on failure.

use datalove_rtdt as rtdt;
use datalove_rt::rust::rider_helpers;

const OK: u8 = 1;

/// Write a new string result by allocating through the runtime.
///
/// # Safety
///
/// rt, out, and out_td must be valid pointers.
unsafe fn write_string_result(rt: *mut u8, out: *mut u8, out_td: *const u8, s: &str) -> u8 {
    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_from_bytes(
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
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
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
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
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
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
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
        datalove_rt::c::dtlv_rti_string_push_bytes_local(
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
