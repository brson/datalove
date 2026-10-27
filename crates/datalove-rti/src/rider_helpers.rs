//! Helper functions for native rider implementations.
//!
//! Provides wrappers for writing Option and other complex return values
//! from rider C ABI functions.
//!
//! These reach the runtime only through the `dtlv_rti_*` functions in
//! [`crate::c`], never through anything private to this crate. A rider calls
//! them, and a rider is code outside the runtime that has nothing but the ABI
//! and the `rtdt` data types; a helper that read `RtLocal` directly would be
//! holding a privilege it cannot pass on. Keeping to the ABI is what lets
//! these move out of this crate, and lets a rider be built against a runtime
//! it does not contain.

use datalove_rtdt as rtdt;
use crate::call;
use crate::{LocalRtHandle, RtStatus};

/// Write a None option to the result pointer.
///
/// Sets the tag byte to None (1). Works for any `?T` layout.
///
/// # Safety
///
/// `out` must point to a valid, writable option-sized region.
pub unsafe fn write_option_none(out: *mut u8) {
    unsafe {
        *out = rtdt::OptionTag::None as u8;
    }
}

/// Write a Some option with a Copy payload.
///
/// Sets the tag byte to Some (2) and writes the payload at the correct
/// offset based on the payload type's alignment.
///
/// # Safety
///
/// `out` must point to a valid, writable option-sized region large enough
/// for the tag + padding + payload.
pub unsafe fn write_option_some<T: Copy>(out: *mut u8, val: T) {
    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let payload_offset = align_up(1, std::mem::align_of::<T>());
        std::ptr::write(out.add(payload_offset) as *mut T, val);
    }
}

/// Write a Some option with a string payload, allocating through the runtime.
///
/// Sets the tag byte to Some (2), then creates the string at the payload
/// offset using the runtime allocator.
///
/// # Safety
///
/// `rt` and `out` must be valid pointers. The output region must be large
/// enough for the option-of-string layout.
pub unsafe fn write_option_some_string(
    rt: LocalRtHandle,
    out: *mut u8,
    s: &str,
) -> RtStatus {
    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let payload_offset = align_up(1, std::mem::align_of::<rtdt::String>());
        let payload_ptr = out.add(payload_offset);

        if s.is_empty() {
            std::ptr::write(payload_ptr as *mut rtdt::String, rtdt::String::new());
        } else {
            let tydesc = rtdt::TyDesc {
                type_tag: rtdt::TyTag::String,
                flags: 0,
                size: std::mem::size_of::<rtdt::String>() as u32,
                align: std::mem::align_of::<rtdt::String>() as u32,
                type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
            };
            let status = call::dtlv_rti_string_from_bytes(
                rt,
                s.as_ptr(),
                s.len() as rtdt::IndexRepr,
                payload_ptr,
                &tydesc,
            );
            if status != RtStatus::Ok {
                return status;
            }
        }
        RtStatus::Ok
    }
}

/// Write a Some option with a (string, string) tuple payload.
///
/// Layout: `[tag, padding, string_0, string_1]` where the tuple starts
/// at the option payload offset and the two strings are laid out
/// sequentially with struct-like alignment.
///
/// # Safety
///
/// `rt` and `out` must be valid pointers. The output region must be large
/// enough for the `?(string, string)` layout.
pub unsafe fn write_option_some_string_pair(
    rt: LocalRtHandle,
    out: *mut u8,
    s0: &str,
    s1: &str,
) -> RtStatus {
    let string_size = std::mem::size_of::<rtdt::String>() as u32;
    let string_align = std::mem::align_of::<rtdt::String>() as u32;

    // Tuple of two strings: both fields have the same alignment, so field 1
    // starts immediately after field 0 (no inter-field padding needed).
    let field1_offset = rtdt::layout::align_up(string_size, string_align);

    // Option payload offset for a tuple with string alignment.
    let payload_offset = rtdt::layout::option_payload_offset(string_align) as usize;

    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let tuple_ptr = out.add(payload_offset);
        let s0_ptr = tuple_ptr;
        let s1_ptr = tuple_ptr.add(field1_offset as usize);

        let status = write_string_at(rt, s0_ptr, s0);
        if status != RtStatus::Ok {
            return status;
        }
        let status = write_string_at(rt, s1_ptr, s1);
        if status != RtStatus::Ok {
            return status;
        }

        RtStatus::Ok
    }
}

/// Write a string value at the given pointer.
///
/// # Safety
///
/// `rt` and `dest` must be valid pointers. `dest` must have space for an `rtdt::String`.
unsafe fn write_string_at(rt: LocalRtHandle, dest: *mut u8, s: &str) -> RtStatus {
    if s.is_empty() {
        unsafe { std::ptr::write(dest as *mut rtdt::String, rtdt::String::new()) };
        RtStatus::Ok
    } else {
        let tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            flags: 0,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
        };
        unsafe {
            call::dtlv_rti_string_from_bytes(
                rt,
                s.as_ptr(),
                s.len() as rtdt::IndexRepr,
                dest,
                &tydesc,
            )
        }
    }
}

/// Write a list of strings to the result pointer.
///
/// Allocates each string through the runtime, then builds the list by moving
/// the string elements from a temporary buffer. The `out_td` must be a list
/// tydesc whose element type is string.
///
/// # Safety
///
/// `rt`, `out`, and `out_td` must be valid pointers. `out` must point to a
/// writable region large enough for an `rtdt::List`.
pub unsafe fn write_string_list(
    rt: LocalRtHandle,
    out: &mut std::mem::MaybeUninit<rtdt::List>,
    out_td: &rtdt::TyDesc,
    parts: &[&str],
) -> RtStatus {
    let element_td = rtdt::TyDescRef::from_ref(out_td).list_element_ty().as_ptr();

    if parts.is_empty() {
        out.write(rtdt::List { data: std::ptr::null(), size: rtdt::Index::ZERO, capacity: rtdt::Index::ZERO });
        return RtStatus::Ok;
    }

    // Allocate a buffer of rtdt::String elements, typed so that it is aligned
    // for them.
    let mut buf: Vec<std::mem::MaybeUninit<rtdt::String>> = Vec::with_capacity(parts.len());

    // Initialize each string in the buffer.
    for (i, part) in parts.iter().enumerate() {
        let dest = unsafe { buf.as_mut_ptr().add(i) as *mut u8 };
        let status = unsafe { write_string_at(rt, dest, part) };
        if status != RtStatus::Ok {
            // Destroy already-initialized strings before returning.
            for j in 0..i {
                let s = unsafe { buf.as_mut_ptr().add(j) as *mut rtdt::String };
                unsafe { destroy_string(rt, s) };
            }
            return status;
        }
    }

    // Build the list by moving elements from the buffer.
    unsafe {
        call::dtlv_rti_list_build_from_slice_local(
            rt,
            out.as_mut_ptr().cast(),
            &*element_td,
            buf.as_mut_ptr() as *mut u8,
            parts.len() as rtdt::IndexRepr,
        )
    }
}

/// Destroy a string value, freeing its backing allocation.
///
/// # Safety
///
/// `rt` must be a valid runtime handle. `s` must point to a valid `rtdt::String`.
unsafe fn destroy_string(rt: LocalRtHandle, s: *mut rtdt::String) {
    let s_ref = unsafe { &*s };
    if !s_ref.data().is_null() && s_ref.capacity() != rtdt::Index::ZERO {
        let tydesc = rtdt::TyDesc {
            type_tag: rtdt::TyTag::String,
            flags: 0,
            size: std::mem::size_of::<rtdt::String>() as u32,
            align: std::mem::align_of::<rtdt::String>() as u32,
            type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
        };
        unsafe {
            call::dtlv_rti_any_destroy_local(
                rt,
                s as *mut u8,
                &tydesc,
            );
        }
    }
}

/// Read a list-of-strings argument into a Vec of `&str`.
///
/// The `list_ptr` must point to a valid `rtdt::List` whose elements are
/// `rtdt::String` values.
///
/// Panics if an element is not valid UTF-8. Nothing currently enforces that
/// invariant on the way in - a rider can write arbitrary bytes into a string -
/// so it is checked here rather than assumed. Where the validation ought to
/// live is an open question; this at least makes a violation a diagnosable
/// panic rather than undefined behaviour.
///
/// # Safety
///
/// `list` is a list of strings the runtime made.
pub unsafe fn read_string_list(list: &rtdt::List) -> Vec<&str> {
    let count = list.size.as_usize();
    if count == 0 {
        return Vec::new();
    }
    let string_size = std::mem::size_of::<rtdt::String>();
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let elem_ptr = unsafe { list.data.add(i * string_size) as *const rtdt::String };
        result.push(unsafe { &*elem_ptr }.as_str());
    }
    result
}

/// Convert an `Int` (bigint) value to its decimal string representation.
///
/// # Safety
///
/// `int_ptr` must point to a valid `rtdt::Int`.
pub unsafe fn int_to_string(int: &rtdt::Int) -> String {
    unsafe { int.to_decimal_string() }
}

/// Parse a decimal string into an `Int` (bigint), writing the result as `?int`.
///
/// Writes `Some(int)` on successful parse, `None` on failure. Handles
/// optional leading `-` sign and rejects empty strings or non-digit characters.
///
/// # Safety
///
/// `rt` must be a valid runtime handle. `out` must point to a writable
/// region large enough for the `?int` layout.
pub unsafe fn write_option_int_from_str(
    rt: LocalRtHandle,
    out: *mut u8,
    s: &str,
) -> RtStatus {
    let s = s.trim();
    if s.is_empty() {
        unsafe { write_option_none(out) };
        return RtStatus::Ok;
    }

    // Parse sign and digits.
    let (negative, digits) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    };

    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        unsafe { write_option_none(out) };
        return RtStatus::Ok;
    }

    // Convert decimal digits to u32 limbs (little-endian base-2^32).
    // Process 9 digits at a time (fits in u32 as 10^9 < 2^32).
    let limbs = decimal_to_limbs(digits);

    // Determine actual limb count (strip trailing zero limbs).
    let limb_count = limbs.iter().rposition(|&l| l != 0).map_or(0, |i| i + 1);

    // Write the option payload.
    let int_align = std::mem::align_of::<rtdt::Int>();
    let payload_offset = align_up(1, int_align);

    let tydesc = rtdt::TyDesc {
        type_tag: rtdt::TyTag::Int,
        flags: 0,
        size: std::mem::size_of::<rtdt::Int>() as u32,
        align: std::mem::align_of::<rtdt::Int>() as u32,
        type_info: rtdt::TyInfo { nothing: rtdt::TyInfoNothing { unused: 0 } },
    };

    unsafe {
        *out = rtdt::OptionTag::Some as u8;
        let int_out = out.add(payload_offset);
        call::dtlv_rti_int_from_limbs(
            rt,
            limbs.as_ptr(),
            limb_count as u32,
            negative && limb_count > 0,
            int_out,
            &tydesc,
        )
    }
}

/// Convert a decimal digit string to a vector of u32 limbs (little-endian base-2^32).
fn decimal_to_limbs(digits: &str) -> Vec<u32> {
    if digits.is_empty() || digits == "0" {
        return vec![];
    }

    // Parse 9 digits at a time into base-10^9 chunks, then multiply-accumulate
    // into base-2^32 limbs.
    const CHUNK_BASE: u64 = 1_000_000_000;
    let mut limbs: Vec<u32> = vec![0];

    let bytes = digits.as_bytes();
    let first_chunk_len = if bytes.len() % 9 == 0 { 9 } else { bytes.len() % 9 };

    // Process first (possibly short) chunk.
    let first_val: u32 = digits[..first_chunk_len].parse().unwrap();
    limbs[0] = first_val;

    // Process remaining 9-digit chunks.
    let mut pos = first_chunk_len;
    while pos < bytes.len() {
        let chunk_val: u32 = digits[pos..pos + 9].parse().unwrap();

        // Multiply all limbs by 10^9 and add chunk_val.
        let mut carry: u64 = chunk_val as u64;
        for limb in limbs.iter_mut() {
            let prod = (*limb as u64) * CHUNK_BASE + carry;
            *limb = prod as u32;
            carry = prod >> 32;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }

        pos += 9;
    }

    limbs
}

/// Smallest value >= `offset` that is a multiple of `align`.
pub const fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

// --- Arguments ---
//
// A rider function takes its arguments as references to the types its
// declaration gives them, the caller promising each one. What a reference
// cannot promise is a string's contents: the runtime keeps `size` bytes of
// UTF-8 at `data`, and these take it at its word.

/// A `mut string` argument, changed through one reference.
///
/// The ABI hands the string over as a pointer to an `rtdt::String`. Reading
/// it as a `&str` and writing its bytes through separate borrows made from
/// that pointer is how `string_remove` came to use a view the other had ended.
/// Here there is one `&mut` to the string, reached through which are its
/// contents, so the borrow checker keeps a read and a write apart. Every
/// change goes through a method that keeps the contents UTF-8, as
/// `rtdt::String` requires, and grows the buffer through the runtime.
pub struct StringMut<'a> {
    rt: LocalRtHandle,
    td: *const rtdt::TyDesc,
    s: &'a mut rtdt::String,
}

impl<'a> StringMut<'a> {
    /// # Safety
    ///
    /// The runtime `rt` allocated `s`, which `td` describes.
    pub unsafe fn new(rt: LocalRtHandle, s: &'a mut rtdt::String, td: &rtdt::TyDesc) -> StringMut<'a> {
        StringMut { rt, td, s }
    }

    pub fn as_str(&self) -> &str {
        self.s.as_str()
    }

    pub fn len(&self) -> usize {
        self.s.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The contents as bytes, for a change that keeps them UTF-8.
    ///
    /// Every caller leaves them UTF-8: a `copy_within` of whole characters,
    /// a rotation of whole characters.
    fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the callers keep the contents UTF-8, as above.
        unsafe { self.s.as_bytes_mut() }
    }

    /// Shorten it to `len` bytes.
    fn set_len(&mut self, len: usize) {
        assert!(len <= self.len() && self.as_str().is_char_boundary(len));
        // SAFETY: shorter than it was, at a character boundary.
        unsafe { self.s.set_size(rtdt::Index::new(len as rtdt::IndexRepr)) };
    }

    /// Empty the string, keeping its buffer.
    pub fn clear(&mut self) {
        self.set_len(0);
    }

    /// Shorten the string to `len` bytes, or leave it if it is no longer.
    ///
    /// Panics if `len` is not on a character boundary.
    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            assert!(self.as_str().is_char_boundary(len), "truncate: not a char boundary");
            self.set_len(len);
        }
    }

    /// Remove the last character and return it.
    pub fn pop(&mut self) -> Option<char> {
        let ch = self.as_str().chars().next_back()?;
        self.set_len(self.len() - ch.len_utf8());
        Some(ch)
    }

    /// Remove the character at byte `idx` and return it, or nothing if
    /// `idx` is not the start of one.
    pub fn remove(&mut self, idx: usize) -> Option<char> {
        let s = self.as_str();
        if idx >= s.len() || !s.is_char_boundary(idx) {
            return None;
        }
        let ch = s[idx..].chars().next()?;
        let (len, ch_len) = (s.len(), ch.len_utf8());
        self.bytes_mut().copy_within(idx + ch_len..len, idx);
        self.set_len(len - ch_len);
        Some(ch)
    }

    /// Append `other`, growing the buffer through the runtime if it has to.
    pub fn push_str(&mut self, other: &str) -> RtStatus {
        if other.is_empty() {
            return RtStatus::Ok;
        }
        // SAFETY: the handle, the string and its descriptor are what `new`
        // was given; the pointer is made from the one reference to the string.
        unsafe {
            call::dtlv_rti_string_push_bytes_local(
                self.rt,
                self.s as *mut rtdt::String as *mut u8,
                &*self.td,
                other.as_ptr(),
                other.len() as rtdt::IndexRepr,
            )
        }
    }

    /// Insert `other` at byte `idx`, or do nothing if `idx` is not on a
    /// character boundary.
    pub fn insert_str(&mut self, idx: usize, other: &str) -> RtStatus {
        if other.is_empty() || idx > self.len() || !self.as_str().is_char_boundary(idx) {
            return RtStatus::Ok;
        }
        // Appended, then turned into place: the tail after `idx` rotates right
        // by the length appended, which brings `other` to `idx`.
        let status = self.push_str(other);
        if status == RtStatus::Ok {
            self.bytes_mut()[idx..].rotate_right(other.len());
        }
        status
    }
}
