//! Bigint arithmetic operations for the Datalove runtime.

use rmx::prelude::*;
use crate::rtdt;
use crate::rt_local::RtLocal;
use crate::{LocalRtHandle, RtStatus};
use ibig::IBig;

/// Convert rtdt::Int to ibig::IBig.
unsafe fn rtdt_int_to_ibig(int_ptr: *const rtdt::Int) -> IBig {
    unsafe {
        let int = &*int_ptr;
        let size_and_sign = int.size_and_sign;
        let abs_size = size_and_sign.abs() as usize;
        let is_negative = size_and_sign < 0;

        if abs_size == 0 {
            return IBig::from(0);
        }

        // Read limbs from the data pointer.
        let limbs = std::slice::from_raw_parts(int.data, abs_size);

        // Convert limbs to IBig.
        // Limbs are stored least significant first.
        let mut result = IBig::from(0);
        for (i, &limb) in limbs.iter().enumerate() {
            result += IBig::from(limb) << (32 * i);
        }

        if is_negative {
            result = -result;
        }

        result
    }
}

/// Convert ibig::IBig to rtdt::Int, allocating space for limbs.
unsafe fn ibig_to_rtdt_int(
    rt: &mut RtLocal,
    value: IBig,
    out_ptr: *mut rtdt::Int,
) -> RtStatus {
    let is_negative = value < IBig::from(0);
    let abs_value = if is_negative { -value } else { value };

    // Convert to limbs (32-bit chunks, least significant first).
    let mut limbs = Vec::new();
    let mut remaining = abs_value;
    let mask = IBig::from(0xFFFFFFFFu32);

    while remaining > IBig::from(0) {
        let limb = (&remaining & &mask).to_string().parse::<u64>().unwrap_or(0) as u32;
        limbs.push(limb);
        remaining >>= 32;
    }

    if limbs.is_empty() {
        limbs.push(0);
    }

    unsafe {
        // Allocate limbs array.
        let limbs_ptr = if !limbs.is_empty() {
            let ptr = rt.alloc.alloc(4, 4, limbs.len() as u32) as *mut u32;
            for (i, &limb) in limbs.iter().enumerate() {
                *ptr.add(i) = limb;
            }
            ptr as *const u32
        } else {
            std::ptr::null()
        };

        // Write to output Int.
        (*out_ptr).data = limbs_ptr;
        (*out_ptr).size_and_sign = if is_negative {
            -(limbs.len() as i32)
        } else {
            limbs.len() as i32
        };
        (*out_ptr).capacity = limbs.len() as u32;

        RtStatus::Ok
    }
}

/// Convert rtdt::Int to String for printing.
pub(crate) unsafe fn int_to_string_impl(int_ptr: *const rtdt::Int) -> String {
    unsafe {
        let int = &*int_ptr;
        let size_and_sign = int.size_and_sign;
        let abs_size = size_and_sign.abs() as usize;
        let is_negative = size_and_sign < 0;

        // Handle zero.
        if abs_size == 0 {
            return "0".to_string();
        }

        // Copy limbs to working buffer.
        let limbs = std::slice::from_raw_parts(int.data, abs_size);
        let mut working = limbs.to_vec();

        // Convert to decimal by repeated division by 10^9.
        const DIVISOR: u64 = 1_000_000_000;
        let mut chunks = Vec::new();

        loop {
            // Divide working by DIVISOR, collecting remainder.
            let mut remainder: u64 = 0;
            let mut all_zero = true;

            for i in (0..working.len()).rev() {
                let current = (remainder << 32) | (working[i] as u64);
                working[i] = (current / DIVISOR) as u32;
                remainder = current % DIVISOR;

                if working[i] != 0 {
                    all_zero = false;
                }
            }

            chunks.push(remainder as u32);

            if all_zero {
                break;
            }
        }

        // Build string from chunks in reverse order.
        let mut result = String::new();

        if is_negative {
            result.push('-');
        }

        // First chunk has no leading zeros.
        result.push_str(&chunks.last().unwrap().to_string());

        // Remaining chunks are padded to 9 digits.
        for i in (0..chunks.len() - 1).rev() {
            result.push_str(&format!("{:09}", chunks[i]));
        }

        result
    }
}

/// Compare magnitudes of two limb arrays.
/// Returns: -1 if a < b, 0 if a == b, 1 if a > b.
unsafe fn compare_magnitude(a_limbs: &[u32], b_limbs: &[u32]) -> i32 {
    if a_limbs.len() != b_limbs.len() {
        return if a_limbs.len() < b_limbs.len() { -1 } else { 1 };
    }

    for i in (0..a_limbs.len()).rev() {
        if a_limbs[i] < b_limbs[i] {
            return -1;
        } else if a_limbs[i] > b_limbs[i] {
            return 1;
        }
    }

    0
}

/// Add magnitudes: result = a + b (ignoring signs).
unsafe fn add_magnitude(
    rt: &mut RtLocal,
    a_limbs: &[u32],
    b_limbs: &[u32],
) -> (*const u32, usize) {
    let max_len = a_limbs.len().max(b_limbs.len());
    let mut result_limbs = Vec::with_capacity(max_len + 1);
    let mut carry: u64 = 0;

    for i in 0..max_len {
        let a_limb = if i < a_limbs.len() { a_limbs[i] as u64 } else { 0 };
        let b_limb = if i < b_limbs.len() { b_limbs[i] as u64 } else { 0 };

        let sum = a_limb + b_limb + carry;
        result_limbs.push(sum as u32);
        carry = sum >> 32;
    }

    if carry > 0 {
        result_limbs.push(carry as u32);
    }

    // Allocate and copy to runtime memory.
    let result_len = result_limbs.len();
    unsafe {
        let result_ptr = rt.alloc.alloc(4, 4, result_len as u32) as *mut u32;
        for (i, &limb) in result_limbs.iter().enumerate() {
            *result_ptr.add(i) = limb;
        }
        (result_ptr as *const u32, result_len)
    }
}

/// Subtract magnitudes: result = a - b (assumes a >= b, ignoring signs).
unsafe fn sub_magnitude(
    rt: &mut RtLocal,
    a_limbs: &[u32],
    b_limbs: &[u32],
) -> (*const u32, usize) {
    let mut result_limbs = Vec::with_capacity(a_limbs.len());
    let mut borrow: i64 = 0;

    for i in 0..a_limbs.len() {
        let a_limb = a_limbs[i] as i64;
        let b_limb = if i < b_limbs.len() { b_limbs[i] as i64 } else { 0 };

        let diff = a_limb - b_limb - borrow;
        if diff < 0 {
            result_limbs.push((diff + (1i64 << 32)) as u32);
            borrow = 1;
        } else {
            result_limbs.push(diff as u32);
            borrow = 0;
        }
    }

    // Remove leading zeros.
    while result_limbs.len() > 1 && *result_limbs.last().unwrap() == 0 {
        result_limbs.pop();
    }

    // Allocate and copy to runtime memory.
    let result_len = result_limbs.len();
    if result_len == 1 && result_limbs[0] == 0 {
        // Result is zero.
        return (std::ptr::null(), 0);
    }

    unsafe {
        let result_ptr = rt.alloc.alloc(4, 4, result_len as u32) as *mut u32;
        for (i, &limb) in result_limbs.iter().enumerate() {
            *result_ptr.add(i) = limb;
        }
        (result_ptr as *const u32, result_len)
    }
}

/// Add two bigints: a + b.
pub(crate) unsafe fn int_add_impl(
    rt: &mut RtLocal,
    a_in: *const u8,
    b_in: *const u8,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let a = &*(a_in as *const rtdt::Int);
        let b = &*(b_in as *const rtdt::Int);
        let result = &mut *(result_out as *mut rtdt::Int);

        let a_size = a.size_and_sign;
        let b_size = b.size_and_sign;
        let a_abs_size = a_size.abs() as usize;
        let b_abs_size = b_size.abs() as usize;
        let a_is_neg = a_size < 0;
        let b_is_neg = b_size < 0;

        // Handle zero cases.
        if a_abs_size == 0 {
            // a is zero, return b.
            if b_abs_size == 0 {
                result.data = std::ptr::null();
                result.size_and_sign = 0;
                result.capacity = 0;
            } else {
                let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);
                let (ptr, len) = add_magnitude(rt, b_limbs, &[]);
                result.data = ptr;
                result.size_and_sign = if b_is_neg { -(len as i32) } else { len as i32 };
                result.capacity = len as u32;
            }
            return RtStatus::Ok;
        }
        if b_abs_size == 0 {
            // b is zero, return a.
            let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
            let (ptr, len) = add_magnitude(rt, a_limbs, &[]);
            result.data = ptr;
            result.size_and_sign = if a_is_neg { -(len as i32) } else { len as i32 };
            result.capacity = len as u32;
            return RtStatus::Ok;
        }

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        if a_is_neg == b_is_neg {
            // Same sign: add magnitudes.
            let (ptr, len) = add_magnitude(rt, a_limbs, b_limbs);
            result.data = ptr;
            result.size_and_sign = if a_is_neg { -(len as i32) } else { len as i32 };
            result.capacity = len as u32;
        } else {
            // Different signs: subtract magnitudes.
            let cmp = compare_magnitude(a_limbs, b_limbs);
            if cmp == 0 {
                // a + (-a) = 0
                result.data = std::ptr::null();
                result.size_and_sign = 0;
                result.capacity = 0;
            } else if cmp > 0 {
                // |a| > |b|: result has sign of a.
                let (ptr, len) = sub_magnitude(rt, a_limbs, b_limbs);
                result.data = ptr;
                result.size_and_sign = if a_is_neg { -(len as i32) } else { len as i32 };
                result.capacity = len as u32;
            } else {
                // |a| < |b|: result has sign of b.
                let (ptr, len) = sub_magnitude(rt, b_limbs, a_limbs);
                result.data = ptr;
                result.size_and_sign = if b_is_neg { -(len as i32) } else { len as i32 };
                result.capacity = len as u32;
            }
        }

        RtStatus::Ok
    }
}

/// Subtract two bigints: a - b.
pub(crate) unsafe fn int_sub_impl(
    rt: &mut RtLocal,
    a_in: *const u8,
    b_in: *const u8,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);
        let result = a - b;
        ibig_to_rtdt_int(rt, result, result_out as *mut rtdt::Int)
    }
}

/// Multiply two bigints: a * b.
pub(crate) unsafe fn int_mul_impl(
    rt: &mut RtLocal,
    a_in: *const u8,
    b_in: *const u8,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);
        let result = a * b;
        ibig_to_rtdt_int(rt, result, result_out as *mut rtdt::Int)
    }
}

/// Negate a bigint: -a.
pub(crate) unsafe fn int_neg_impl(
    rt: &mut RtLocal,
    a_in: *const u8,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let a = &*(a_in as *const rtdt::Int);
        let result = &mut *(result_out as *mut rtdt::Int);

        let size_and_sign = a.size_and_sign;
        let abs_size = size_and_sign.abs() as usize;

        // Handle zero.
        if abs_size == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = 0;
            return RtStatus::Ok;
        }

        // Allocate limbs for result.
        let limbs_ptr = rt.alloc.alloc(4, 4, abs_size as u32) as *mut u32;

        // Copy limbs.
        let src_limbs = std::slice::from_raw_parts(a.data, abs_size);
        let dst_limbs = std::slice::from_raw_parts_mut(limbs_ptr, abs_size);
        dst_limbs.copy_from_slice(src_limbs);

        // Negate the sign.
        result.data = limbs_ptr as *const u32;
        result.size_and_sign = -size_and_sign;
        result.capacity = abs_size as u32;

        RtStatus::Ok
    }
}

/// Division of bigints: a / b.
/// Returns RtStatus::Error if b is zero.
pub(crate) unsafe fn int_div_checked_impl(
    rt: &mut RtLocal,
    a_in: *const u8,
    b_in: *const u8,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);

        // Check for division by zero.
        if b == IBig::from(0) {
            return RtStatus::Error;
        }

        let result = a / b;
        ibig_to_rtdt_int(rt, result, result_out as *mut rtdt::Int)
    }
}
