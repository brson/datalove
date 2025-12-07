//! Bigint arithmetic operations for the Datalove runtime.

use rmx::prelude::*;
use crate::rtdt;
use crate::impls::rt_local::RtLocal;
use crate::c::{LocalRtHandle, RtStatus};

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
        let b = &*(b_in as *const rtdt::Int);
        let b_size = b.size_and_sign;
        let b_abs_size = b_size.abs() as usize;

        // Handle b = 0 case: a - 0 = a.
        if b_abs_size == 0 {
            return int_add_impl(rt, a_in, b_in, result_out);
        }

        // Create negated b: allocate temporary Int for -b.
        let neg_b_limbs_ptr = rt.alloc.alloc(4, 4, b_abs_size as u32) as *mut u32;
        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);
        let neg_b_limbs = std::slice::from_raw_parts_mut(neg_b_limbs_ptr, b_abs_size);
        neg_b_limbs.copy_from_slice(b_limbs);

        // Create temporary rtdt::Int for -b.
        let mut neg_b = rtdt::Int {
            data: neg_b_limbs_ptr as *const u32,
            size_and_sign: -b_size,
            capacity: b_abs_size as u32,
        };

        // Compute a + (-b).
        let result = int_add_impl(rt, a_in, &neg_b as *const rtdt::Int as *const u8, result_out);

        // Free temporary allocation.
        rt.alloc.free(4, 4, b_abs_size as u32, neg_b_limbs_ptr as *mut u8);

        result
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
        if a_abs_size == 0 || b_abs_size == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = 0;
            return RtStatus::Ok;
        }

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        // Grade-school multiplication: result can have up to a_len + b_len limbs.
        let result_max_len = a_abs_size + b_abs_size;
        let mut result_limbs = vec![0u32; result_max_len];

        for i in 0..a_abs_size {
            let mut carry: u64 = 0;
            for j in 0..b_abs_size {
                let product = (a_limbs[i] as u64) * (b_limbs[j] as u64)
                    + (result_limbs[i + j] as u64)
                    + carry;
                result_limbs[i + j] = product as u32;
                carry = product >> 32;
            }
            result_limbs[i + b_abs_size] = carry as u32;
        }

        // Remove leading zeros.
        while result_limbs.len() > 1 && *result_limbs.last().unwrap() == 0 {
            result_limbs.pop();
        }

        let result_len = result_limbs.len();

        // Allocate and copy to runtime memory.
        let result_ptr = rt.alloc.alloc(4, 4, result_len as u32) as *mut u32;
        for (i, &limb) in result_limbs.iter().enumerate() {
            *result_ptr.add(i) = limb;
        }

        // Result sign: negative if exactly one operand is negative.
        let result_is_neg = a_is_neg != b_is_neg;

        result.data = result_ptr as *const u32;
        result.size_and_sign = if result_is_neg {
            -(result_len as i32)
        } else {
            result_len as i32
        };
        result.capacity = result_len as u32;

        RtStatus::Ok
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

/// Divide magnitude: returns (quotient_limbs, quotient_len).
/// Assumes divisor is not zero.
unsafe fn div_magnitude(
    rt: &mut RtLocal,
    dividend_limbs: &[u32],
    divisor_limbs: &[u32],
) -> (*const u32, usize) {
    let m = dividend_limbs.len();
    let n = divisor_limbs.len();

    // Handle dividend < divisor case.
    if m < n || (m == n && unsafe { compare_magnitude(dividend_limbs, divisor_limbs) } < 0) {
        // Quotient is zero.
        return (std::ptr::null(), 0);
    }

    // Handle single-limb divisor.
    if n == 1 {
        let divisor = divisor_limbs[0] as u64;

        // Defensive check for zero divisor (shouldn't happen if caller checks).
        if divisor == 0 {
            return (std::ptr::null(), 0);
        }

        let mut quotient = Vec::with_capacity(m);
        let mut remainder: u64 = 0;

        for i in (0..m).rev() {
            let current = (remainder << 32) | (dividend_limbs[i] as u64);
            quotient.push((current / divisor) as u32);
            remainder = current % divisor;
        }

        quotient.reverse();

        // Remove leading zeros.
        while quotient.len() > 1 && *quotient.last().unwrap() == 0 {
            quotient.pop();
        }

        let q_len = quotient.len();
        if q_len == 1 && quotient[0] == 0 {
            return (std::ptr::null(), 0);
        }

        unsafe {
            let q_ptr = rt.alloc.alloc(4, 4, q_len as u32) as *mut u32;
            for (i, &limb) in quotient.iter().enumerate() {
                *q_ptr.add(i) = limb;
            }
            (q_ptr as *const u32, q_len)
        }
    } else {
        // Multi-limb division using Knuth's Algorithm D.
        // Normalize: scale so divisor's MSB has high bit set.
        let shift = divisor_limbs[n - 1].leading_zeros();
        let mut norm_dividend = vec![0u32; m + 1];
        let mut norm_divisor = vec![0u32; n];

        // Shift divisor.
        if shift > 0 {
            let mut carry: u64 = 0;
            for i in 0..n {
                let shifted = ((divisor_limbs[i] as u64) << shift) | carry;
                norm_divisor[i] = shifted as u32;
                carry = shifted >> 32;
            }
        } else {
            norm_divisor.copy_from_slice(divisor_limbs);
        }

        // Shift dividend.
        if shift > 0 {
            let mut carry: u64 = 0;
            for i in 0..m {
                let shifted = ((dividend_limbs[i] as u64) << shift) | carry;
                norm_dividend[i] = shifted as u32;
                carry = shifted >> 32;
            }
            norm_dividend[m] = carry as u32;
        } else {
            norm_dividend[..m].copy_from_slice(dividend_limbs);
        }

        let mut quotient = vec![0u32; m - n + 1];

        // Main division loop.
        for j in (0..=m - n).rev() {
            // Estimate quotient digit.
            let dividend_high = ((norm_dividend[j + n] as u64) << 32) | (norm_dividend[j + n - 1] as u64);
            let mut q_hat = dividend_high / (norm_divisor[n - 1] as u64);
            let mut r_hat = dividend_high % (norm_divisor[n - 1] as u64);

            // Refine estimate.
            while q_hat >= (1u64 << 32)
                || (n >= 2
                    && q_hat * (norm_divisor[n - 2] as u64)
                        > ((r_hat << 32) | (norm_dividend[j + n - 2] as u64)))
            {
                q_hat -= 1;
                r_hat += norm_divisor[n - 1] as u64;
                if r_hat >= (1u64 << 32) {
                    break;
                }
            }

            // Multiply and subtract.
            let mut borrow: i64 = 0;
            for i in 0..n {
                let product = q_hat * (norm_divisor[i] as u64);
                let sub = (norm_dividend[j + i] as i64) - (product as i64) - borrow;
                norm_dividend[j + i] = sub as u32;
                borrow = if sub < 0 {
                    ((-(sub as i64)) + 0xFFFFFFFF) / 0x100000000
                } else {
                    0
                };
            }
            let sub = (norm_dividend[j + n] as i64) - borrow;
            norm_dividend[j + n] = sub as u32;

            // Store quotient digit.
            quotient[j] = q_hat as u32;

            // Add back if we subtracted too much.
            if sub < 0 {
                quotient[j] -= 1;
                let mut carry: u64 = 0;
                for i in 0..n {
                    let sum = (norm_dividend[j + i] as u64) + (norm_divisor[i] as u64) + carry;
                    norm_dividend[j + i] = sum as u32;
                    carry = sum >> 32;
                }
                norm_dividend[j + n] = ((norm_dividend[j + n] as u64) + carry) as u32;
            }
        }

        // Remove leading zeros.
        while quotient.len() > 1 && *quotient.last().unwrap() == 0 {
            quotient.pop();
        }

        let q_len = quotient.len();
        if q_len == 1 && quotient[0] == 0 {
            return (std::ptr::null(), 0);
        }

        unsafe {
            let q_ptr = rt.alloc.alloc(4, 4, q_len as u32) as *mut u32;
            for (i, &limb) in quotient.iter().enumerate() {
                *q_ptr.add(i) = limb;
            }
            (q_ptr as *const u32, q_len)
        }
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
        let a = &*(a_in as *const rtdt::Int);
        let b = &*(b_in as *const rtdt::Int);
        let result = &mut *(result_out as *mut rtdt::Int);

        let a_size = a.size_and_sign;
        let b_size = b.size_and_sign;
        let a_abs_size = a_size.abs() as usize;
        let b_abs_size = b_size.abs() as usize;
        let a_is_neg = a_size < 0;
        let b_is_neg = b_size < 0;

        // Check for division by zero.
        if b_abs_size == 0 {
            return RtStatus::Error;
        }

        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        // Additional check: if all limbs are zero, that's also division by zero.
        let b_is_zero = b_limbs.iter().all(|&limb| limb == 0);
        if b_is_zero {
            return RtStatus::Error;
        }

        // Handle dividend = 0 case.
        if a_abs_size == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = 0;
            return RtStatus::Ok;
        }

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);

        let (q_ptr, q_len) = div_magnitude(rt, a_limbs, b_limbs);

        // Handle zero quotient.
        if q_len == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = 0;
            return RtStatus::Ok;
        }

        // Result sign: negative if exactly one operand is negative.
        let result_is_neg = a_is_neg != b_is_neg;

        result.data = q_ptr;
        result.size_and_sign = if result_is_neg {
            -(q_len as i32)
        } else {
            q_len as i32
        };
        result.capacity = q_len as u32;

        RtStatus::Ok
    }
}

