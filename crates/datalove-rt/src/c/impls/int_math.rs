//! Bigint arithmetic operations for the Datalove runtime.

use datalove_rtdt as rtdt;
use crate::c::impls::rt_local::RtLocal;
use crate::c::RtStatus;

/// The most limbs an `Int` holds, its size being a signed 32-bit count.
const MAX_LIMBS: usize = i32::MAX as usize;

/// Write zero for a result too large for an `Int`, and fail.
///
/// The zero is so that a caller that goes on to drop the result has a well
/// formed `Int` to drop.
fn too_large(result: &mut rtdt::Int) -> RtStatus {
    result.data = std::ptr::null();
    result.size_and_sign = 0;
    result.capacity = rtdt::Index::ZERO;
    RtStatus::Error
}

// ============================================================================
// Magnitude arithmetic
// ============================================================================
//
// The arithmetic on limbs, apart from the memory it goes into. A magnitude is
// its limbs least significant first with no leading zero limb, and zero has
// none. Each operation takes the limbs to write as a slice the caller
// allocated, of the capacity the matching `*_capacity` gives or the doc says,
// and returns how many it wrote. They are pure, so that Kani holds them to
// `u128` arithmetic; see botdocs/kani.md.

/// Compare two magnitudes: -1 if a < b, 0 if a == b, 1 if a > b.
fn compare_magnitude(a_limbs: &[u32], b_limbs: &[u32]) -> i32 {
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

/// How many limbs a sum of magnitudes can take: one more than the longer, for
/// a carry out of the top, unless one is zero and there can be none.
fn sum_capacity(a_len: usize, b_len: usize) -> usize {
    let max_len = a_len.max(b_len);
    if a_len == 0 || b_len == 0 { max_len } else { max_len + 1 }
}

/// Write a + b into `out`, which holds `sum_capacity` limbs; the sum's length.
fn add_magnitude_into(out: &mut [u32], a_limbs: &[u32], b_limbs: &[u32]) -> usize {
    let max_len = a_limbs.len().max(b_limbs.len());
    let mut carry: u64 = 0;
    for i in 0..max_len {
        let a_limb = a_limbs.get(i).copied().unwrap_or(0) as u64;
        let b_limb = b_limbs.get(i).copied().unwrap_or(0) as u64;
        let sum = a_limb + b_limb + carry;
        out[i] = sum as u32;
        carry = sum >> 32;
    }
    let mut len = max_len;
    if carry > 0 {
        out[len] = carry as u32;
        len += 1;
    }
    len
}

/// Write a - b into `out`, which holds as many limbs as `a`, for a > b; the
/// difference's length once its leading zeros are dropped, at least one.
fn sub_magnitude_into(out: &mut [u32], a_limbs: &[u32], b_limbs: &[u32]) -> usize {
    let mut borrow: i64 = 0;
    for i in 0..a_limbs.len() {
        let b_limb = b_limbs.get(i).copied().unwrap_or(0) as i64;
        let diff = a_limbs[i] as i64 - b_limb - borrow;
        borrow = (diff < 0) as i64;
        out[i] = (diff + (borrow << 32)) as u32;
    }
    let mut len = a_limbs.len();
    while len > 1 && out[len - 1] == 0 {
        len -= 1;
    }
    assert!(!(len == 1 && out[0] == 0), "sub_magnitude produced zero result");
    len
}

/// Write a * b into `out`, which holds as many limbs as both together, neither
/// being zero; the product's length.
fn mul_magnitude_into(out: &mut [u32], a_limbs: &[u32], b_limbs: &[u32]) -> usize {
    out.fill(0);
    for i in 0..a_limbs.len() {
        let mut carry: u64 = 0;
        for j in 0..b_limbs.len() {
            let product = (a_limbs[i] as u64) * (b_limbs[j] as u64) + (out[i + j] as u64) + carry;
            out[i + j] = product as u32;
            carry = product >> 32;
        }
        out[i + b_limbs.len()] = carry as u32;
    }
    let mut len = out.len();
    while len > 1 && out[len - 1] == 0 {
        len -= 1;
    }
    len
}

/// Add `b` into the magnitude of `a_len` limbs at the start of `buf`, which
/// has room for the sum; the sum's length.
fn add_assign_magnitude(buf: &mut [u32], a_len: usize, b_limbs: &[u32]) -> usize {
    let max_len = a_len.max(b_limbs.len());
    let mut carry: u64 = 0;
    for i in 0..max_len {
        let a_limb = if i < a_len { buf[i] as u64 } else { 0 };
        let b_limb = b_limbs.get(i).copied().unwrap_or(0) as u64;
        let sum = a_limb + b_limb + carry;
        buf[i] = sum as u32;
        carry = sum >> 32;
    }
    let mut len = max_len;
    if carry > 0 {
        buf[len] = carry as u32;
        len += 1;
    }
    len
}

/// Subtract the smaller of two magnitudes from the larger, which
/// `a_is_larger` names: `a` is the `a_len` limbs at the start of `buf`, and
/// the difference goes there too, which has room for it. The difference's
/// length, at least one.
fn sub_assign_magnitude(buf: &mut [u32], a_len: usize, b_limbs: &[u32], a_is_larger: bool) -> usize {
    let mut len = if a_is_larger {
        let mut borrow: i64 = 0;
        for i in 0..a_len {
            let b_limb = b_limbs.get(i).copied().unwrap_or(0) as i64;
            let diff = buf[i] as i64 - b_limb - borrow;
            borrow = (diff < 0) as i64;
            buf[i] = (diff + (borrow << 32)) as u32;
        }
        a_len
    } else {
        let mut borrow: i64 = 0;
        for (i, &b_limb) in b_limbs.iter().enumerate() {
            let a_limb = if i < a_len { buf[i] as i64 } else { 0 };
            let diff = b_limb as i64 - a_limb - borrow;
            borrow = (diff < 0) as i64;
            buf[i] = (diff + (borrow << 32)) as u32;
        }
        b_limbs.len()
    };
    while buf[len - 1] == 0 {
        len -= 1;
    }
    len
}

/// How many limbs a quotient of magnitudes can take, the divisor not zero, or
/// `None` where the dividend is the smaller and the quotient zero.
fn quotient_capacity(dividend_limbs: &[u32], divisor_limbs: &[u32]) -> Option<usize> {
    let (m, n) = (dividend_limbs.len(), divisor_limbs.len());
    if m < n || (m == n && compare_magnitude(dividend_limbs, divisor_limbs) < 0) {
        None
    } else {
        Some(m - n + 1)
    }
}

/// Write dividend / divisor into `out`, which holds `quotient_capacity`
/// limbs; the quotient's length, at least one.
fn div_magnitude_into(out: &mut [u32], dividend_limbs: &[u32], divisor_limbs: &[u32]) -> usize {
    let m = dividend_limbs.len();
    let n = divisor_limbs.len();

    if n == 1 {
        let divisor = divisor_limbs[0] as u64;
        assert!(divisor != 0, "div_magnitude called with zero divisor");

        // From the top limb down.
        let mut remainder: u64 = 0;
        for i in (0..m).rev() {
            let current = (remainder << 32) | (dividend_limbs[i] as u64);
            out[i] = (current / divisor) as u32;
            remainder = current % divisor;
        }

        // Drop leading zeros.
        let mut q_len = m;
        while q_len > 1 && out[q_len - 1] == 0 {
            q_len -= 1;
        }

        // Zero quotient is handled by the dividend < divisor check.
        assert!(!(q_len == 1 && out[0] == 0), "single-limb division produced zero quotient");

        q_len
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

        // The quotient goes to `out`; the normalized operands above are only
        // working space.
        let quotient = out;
        quotient.fill(0);

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
            //
            // Each partial product is a full 64 bits, so only its low half
            // belongs at this position and the high half is carried into the
            // next one. Subtracting the whole product from a single limb
            // would need a range no 64-bit integer has.
            //
            // The carry holds that high half plus whatever the subtraction
            // borrowed, which the arithmetic shift reads off the difference's
            // sign. Both stay within a few billion of zero.
            let mut carry: i64 = 0;
            for i in 0..n {
                let product = q_hat * (norm_divisor[i] as u64);
                let difference =
                    (norm_dividend[j + i] as i64) - carry - ((product & 0xFFFF_FFFF) as i64);
                norm_dividend[j + i] = difference as u32;
                carry = ((product >> 32) as i64) - (difference >> 32);
            }
            let difference = (norm_dividend[j + n] as i64) - carry;
            norm_dividend[j + n] = difference as u32;

            // Store quotient digit.
            quotient[j] = q_hat as u32;

            // Add back if we subtracted too much.
            if difference < 0 {
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

        // Drop leading zeros.
        let mut q_len = quotient.len();
        while q_len > 1 && quotient[q_len - 1] == 0 {
            q_len -= 1;
        }

        // Zero quotient is handled by the dividend < divisor check.
        assert!(!(q_len == 1 && quotient[0] == 0), "multi-limb division produced zero quotient");

        q_len
    }
}

// ============================================================================
// Magnitudes in the runtime's memory
// ============================================================================

/// A magnitude written into the runtime's memory: `capacity` limbs allocated
/// and filled by `write`, which says how many it used. Returns the limbs, how
/// many there are, and how many were allocated, which the `Int` keeps so as
/// to free them.
///
/// Written straight into the runtime's memory, where it was built in a `Vec`
/// first and copied, which allocated twice.
unsafe fn magnitude_in_runtime(
    rt: &mut RtLocal,
    capacity: usize,
    write: impl FnOnce(&mut [u32]) -> usize,
) -> (*const u32, usize, usize) {
    let ptr = unsafe { rt.alloc.alloc(4, 4, (capacity as u32).into()) as *mut u32 };
    let len = write(unsafe { std::slice::from_raw_parts_mut(ptr, capacity) });
    (ptr as *const u32, len, capacity)
}

/// Add magnitudes: result = a + b (ignoring signs).
unsafe fn add_magnitude(
    rt: &mut RtLocal,
    a_limbs: &[u32],
    b_limbs: &[u32],
) -> (*const u32, usize, usize) {
    let capacity = sum_capacity(a_limbs.len(), b_limbs.len());
    unsafe { magnitude_in_runtime(rt, capacity, |out| add_magnitude_into(out, a_limbs, b_limbs)) }
}

/// Subtract magnitudes: result = a - b (assumes a > b, ignoring signs).
unsafe fn sub_magnitude(
    rt: &mut RtLocal,
    a_limbs: &[u32],
    b_limbs: &[u32],
) -> (*const u32, usize, usize) {
    unsafe { magnitude_in_runtime(rt, a_limbs.len(), |out| sub_magnitude_into(out, a_limbs, b_limbs)) }
}

/// Divide magnitudes: result = a / b, the divisor not zero. A zero quotient
/// is no limbs, allocated nowhere.
unsafe fn div_magnitude(
    rt: &mut RtLocal,
    dividend_limbs: &[u32],
    divisor_limbs: &[u32],
) -> (*const u32, usize, usize) {
    match quotient_capacity(dividend_limbs, divisor_limbs) {
        None => (std::ptr::null(), 0, 0),
        Some(capacity) => unsafe {
            magnitude_in_runtime(rt, capacity, |out| div_magnitude_into(out, dividend_limbs, divisor_limbs))
        },
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

        // A carry can take the sum a limb past the larger operand.
        if a_abs_size.max(b_abs_size) >= MAX_LIMBS {
            return too_large(result);
        }

        // Handle zero cases.
        if a_abs_size == 0 {
            // a is zero, return b.
            if b_abs_size == 0 {
                result.data = std::ptr::null();
                result.size_and_sign = 0;
                result.capacity = rtdt::Index::ZERO;
            } else {
                let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);
                let (ptr, len, capacity) = add_magnitude(rt, b_limbs, &[]);
                result.data = ptr;
                result.size_and_sign = if b_is_neg { -(len as i32) } else { len as i32 };
                result.capacity = rtdt::Index(capacity as rtdt::IndexRepr);
            }
            return RtStatus::Ok;
        }
        if b_abs_size == 0 {
            // b is zero, return a.
            let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
            let (ptr, len, capacity) = add_magnitude(rt, a_limbs, &[]);
            result.data = ptr;
            result.size_and_sign = if a_is_neg { -(len as i32) } else { len as i32 };
            result.capacity = rtdt::Index(capacity as rtdt::IndexRepr);
            return RtStatus::Ok;
        }

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        if a_is_neg == b_is_neg {
            // Same sign: add magnitudes.
            let (ptr, len, capacity) = add_magnitude(rt, a_limbs, b_limbs);
            result.data = ptr;
            result.size_and_sign = if a_is_neg { -(len as i32) } else { len as i32 };
            result.capacity = rtdt::Index(capacity as rtdt::IndexRepr);
        } else {
            // Different signs: subtract magnitudes.
            let cmp = compare_magnitude(a_limbs, b_limbs);
            if cmp == 0 {
                // a + (-a) = 0
                result.data = std::ptr::null();
                result.size_and_sign = 0;
                result.capacity = rtdt::Index::ZERO;
            } else if cmp > 0 {
                // |a| > |b|: result has sign of a.
                let (ptr, len, capacity) = sub_magnitude(rt, a_limbs, b_limbs);
                result.data = ptr;
                result.size_and_sign = if a_is_neg { -(len as i32) } else { len as i32 };
                result.capacity = rtdt::Index(capacity as rtdt::IndexRepr);
            } else {
                // |a| < |b|: result has sign of b.
                let (ptr, len, capacity) = sub_magnitude(rt, b_limbs, a_limbs);
                result.data = ptr;
                result.size_and_sign = if b_is_neg { -(len as i32) } else { len as i32 };
                result.capacity = rtdt::Index(capacity as rtdt::IndexRepr);
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

        // -b, as a view of b's own limbs with the sign flipped. Addition only
        // reads it, and it is never destroyed, so the limbs need no copy.
        let neg_b = rtdt::Int {
            data: b.data,
            size_and_sign: -b_size,
            capacity: b.capacity,
        };

        // Compute a + (-b).
        int_add_impl(rt, a_in, &neg_b as *const rtdt::Int as *const u8, result_out)
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

        if a_abs_size + b_abs_size > MAX_LIMBS {
            return too_large(result);
        }

        // Handle zero cases.
        if a_abs_size == 0 || b_abs_size == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = rtdt::Index::ZERO;
            return RtStatus::Ok;
        }

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        // Grade-school multiplication: the product has up to a_len + b_len
        // limbs.
        let (result_ptr, result_len, capacity) =
            magnitude_in_runtime(rt, a_abs_size + b_abs_size, |out| mul_magnitude_into(out, a_limbs, b_limbs));

        // Result sign: negative if exactly one operand is negative.
        let result_is_neg = a_is_neg != b_is_neg;

        result.data = result_ptr;
        result.size_and_sign = if result_is_neg {
            -(result_len as i32)
        } else {
            result_len as i32
        };
        result.capacity = rtdt::Index(capacity as rtdt::IndexRepr);

        RtStatus::Ok
    }
}

// ============================================================================
// Updates in place
// ============================================================================

/// Add `b` into `a`: a = a + b, in `a`'s own buffer where it has room.
///
/// What `set a += b` runs. A sum takes a limb more than the longer operand, for
/// the carry, and addition leaves that limb spare when it doesn't use it, so a
/// running total mostly fits where it is: no allocation, nothing freed.
///
/// `a` and `b` do not overlap, and so neither do their buffers: `set a += a`
/// is lowered to an addition and a store.
pub(crate) unsafe fn int_add_assign_impl(rt: &mut RtLocal, a_mut: *mut u8, b_in: *const u8) -> RtStatus {
    unsafe {
        let b = &*(b_in as *const rtdt::Int);
        add_assign_signed(rt, &mut *(a_mut as *mut rtdt::Int), int_limbs(b), b.size_and_sign < 0)
    }
}

/// Subtract `b` from `a`: a = a - b, in `a`'s own buffer where it has room.
pub(crate) unsafe fn int_sub_assign_impl(rt: &mut RtLocal, a_mut: *mut u8, b_in: *const u8) -> RtStatus {
    unsafe {
        let b = &*(b_in as *const rtdt::Int);
        add_assign_signed(rt, &mut *(a_mut as *mut rtdt::Int), int_limbs(b), b.size_and_sign > 0)
    }
}

/// The limbs of `int`'s magnitude.
///
/// # Safety
///
/// `int` must be initialized.
unsafe fn int_limbs(int: &rtdt::Int) -> &[u32] {
    let len = int.size_and_sign.unsigned_abs() as usize;
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(int.data, len) } }
}

/// Add `b_limbs`, a magnitude negated if `b_neg`, into `a`, whose buffer they
/// are not in.
///
/// `a`'s limbs go through a raw pointer, since past its size its buffer is
/// spare capacity that may never have been written.
unsafe fn add_assign_signed(rt: &mut RtLocal, a: &mut rtdt::Int, b_limbs: &[u32], b_neg: bool) -> RtStatus {
    let a_len = a.size_and_sign.unsigned_abs() as usize;
    let b_len = b_limbs.len();
    if b_len == 0 {
        return RtStatus::Ok;
    }
    // A zero holds no buffer to work in.
    if a_len == 0 {
        return unsafe { add_assign_elsewhere(rt, a, b_limbs, b_neg) };
    }
    let a_neg = a.size_and_sign < 0;
    let capacity = a.capacity.as_usize();
    // `a`'s buffer as far as it was allocated. Past `a`'s size it is spare
    // capacity that may never have been written, which nothing below reads.
    let buf = unsafe { std::slice::from_raw_parts_mut(a.data as *mut u32, capacity) };

    if a_neg == b_neg {
        // The magnitudes add, keeping the sign.
        let max_len = a_len.max(b_len);
        if max_len >= MAX_LIMBS {
            return RtStatus::Error;
        }
        if max_len + 1 > capacity {
            return unsafe { add_assign_elsewhere(rt, a, b_limbs, b_neg) };
        }
        let len = add_assign_magnitude(buf, a_len, b_limbs);
        a.size_and_sign = if a_neg { -(len as i32) } else { len as i32 };
        return RtStatus::Ok;
    }

    // The magnitudes subtract, the smaller from the larger, whose sign the
    // result takes. Equal ones cancel.
    let order = compare_magnitude(&buf[..a_len], b_limbs);
    if order == 0 {
        unsafe { set_zero(rt, a) };
        return RtStatus::Ok;
    }
    if order < 0 && b_len > capacity {
        return unsafe { add_assign_elsewhere(rt, a, b_limbs, b_neg) };
    }
    let len = sub_assign_magnitude(buf, a_len, b_limbs, order > 0);
    let negative = if order > 0 { a_neg } else { b_neg };
    a.size_and_sign = if negative { -(len as i32) } else { len as i32 };
    RtStatus::Ok
}

/// Add into `a` through a new buffer, for a sum that doesn't fit `a`'s.
unsafe fn add_assign_elsewhere(rt: &mut RtLocal, a: &mut rtdt::Int, b_limbs: &[u32], b_neg: bool) -> RtStatus {
    let b_len = b_limbs.len() as i32;
    let b = rtdt::Int {
        data: b_limbs.as_ptr(),
        size_and_sign: if b_neg { -b_len } else { b_len },
        capacity: rtdt::Index::ZERO,
    };
    unsafe {
        let mut sum = std::mem::MaybeUninit::<rtdt::Int>::uninit();
        let status = int_add_impl(
            rt,
            a as *const rtdt::Int as *const u8,
            &b as *const rtdt::Int as *const u8,
            sum.as_mut_ptr() as *mut u8,
        );
        if status != RtStatus::Ok {
            return status;
        }
        replace(rt, a, sum.assume_init());
    }
    RtStatus::Ok
}

/// Multiply `a` by `b`: a = a * b.
///
/// A product is built apart from its operands, which it reads to the end, so
/// this replaces `a`'s buffer rather than reusing it.
pub(crate) unsafe fn int_mul_assign_impl(rt: &mut RtLocal, a_mut: *mut u8, b_in: *const u8) -> RtStatus {
    unsafe {
        let mut product = std::mem::MaybeUninit::<rtdt::Int>::uninit();
        let status = int_mul_impl(rt, a_mut, b_in, product.as_mut_ptr() as *mut u8);
        if status != RtStatus::Ok {
            return status;
        }
        replace(rt, &mut *(a_mut as *mut rtdt::Int), product.assume_init());
    }
    RtStatus::Ok
}

/// Divide `a` by `b`: a = a / b, leaving `a` as it was if `b` is zero.
pub(crate) unsafe fn int_div_assign_checked_impl(rt: &mut RtLocal, a_mut: *mut u8, b_in: *const u8) -> RtStatus {
    unsafe {
        let mut quotient = std::mem::MaybeUninit::<rtdt::Int>::uninit();
        let status = int_div_checked_impl(rt, a_mut, b_in, quotient.as_mut_ptr() as *mut u8);
        if status != RtStatus::Ok {
            // The quotient is a zero, holding nothing to free.
            return status;
        }
        replace(rt, &mut *(a_mut as *mut rtdt::Int), quotient.assume_init());
    }
    RtStatus::Ok
}

/// Free `a`'s buffer and give it `value` instead.
unsafe fn replace(rt: &mut RtLocal, a: &mut rtdt::Int, value: rtdt::Int) {
    if !a.data.is_null() && a.capacity > rtdt::Index::ZERO {
        unsafe { rt.alloc.free(4, 4, a.capacity.0, a.data as *mut u8) };
    }
    *a = value;
}

/// Make `a` zero, as every other operation writes it: holding no buffer.
unsafe fn set_zero(rt: &mut RtLocal, a: &mut rtdt::Int) {
    let zero = rtdt::Int {
        data: std::ptr::null(),
        size_and_sign: 0,
        capacity: rtdt::Index::ZERO,
    };
    unsafe { replace(rt, a, zero) };
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
            result.capacity = rtdt::Index::ZERO;
            return RtStatus::Ok;
        }

        // Allocate limbs for result.
        let limbs_ptr = rt.alloc.alloc(4, 4, (abs_size as u32).into()) as *mut u32;

        // Copy limbs.
        let src_limbs = std::slice::from_raw_parts(a.data, abs_size);
        let dst_limbs = std::slice::from_raw_parts_mut(limbs_ptr, abs_size);
        dst_limbs.copy_from_slice(src_limbs);

        // Negate the sign.
        result.data = limbs_ptr as *const u32;
        result.size_and_sign = -size_and_sign;
        result.capacity = rtdt::Index(abs_size as rtdt::IndexRepr);

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
        let a = &*(a_in as *const rtdt::Int);
        let b = &*(b_in as *const rtdt::Int);
        let result = &mut *(result_out as *mut rtdt::Int);

        let a_size = a.size_and_sign;
        let b_size = b.size_and_sign;
        let a_abs_size = a_size.abs() as usize;
        let b_abs_size = b_size.abs() as usize;
        let a_is_neg = a_size < 0;
        let b_is_neg = b_size < 0;

        // Check for division by zero. The result is still written, as zero, so
        // that a caller that goes on to drop it has a well formed Int to drop.
        if b_abs_size == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = rtdt::Index::ZERO;
            return RtStatus::Error;
        }

        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        // Malformed Int: size_and_sign != 0 but all limbs are zero.
        assert!(
            !b_limbs.iter().all(|&limb| limb == 0),
            "malformed Int: non-zero size but all limbs are zero"
        );

        // Handle dividend = 0 case.
        if a_abs_size == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = rtdt::Index::ZERO;
            return RtStatus::Ok;
        }

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);

        let (q_ptr, q_len, q_capacity) = div_magnitude(rt, a_limbs, b_limbs);

        // Handle zero quotient.
        if q_len == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = rtdt::Index::ZERO;
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
        result.capacity = rtdt::Index(q_capacity as rtdt::IndexRepr);

        RtStatus::Ok
    }
}

/// Compare two bigints.
///
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
pub(crate) unsafe fn int_cmp_impl(
    a_in: *const u8,
    b_in: *const u8,
) -> i32 {
    unsafe {
        let a = &*(a_in as *const rtdt::Int);
        let b = &*(b_in as *const rtdt::Int);

        let a_size = a.size_and_sign;
        let b_size = b.size_and_sign;

        // Handle zero cases.
        if a_size == 0 && b_size == 0 {
            return 0;
        }
        if a_size == 0 {
            // a is zero, b is non-zero.
            return if b_size > 0 { -1 } else { 1 };
        }
        if b_size == 0 {
            // b is zero, a is non-zero.
            return if a_size > 0 { 1 } else { -1 };
        }

        // Both non-zero. Compare signs first.
        let a_is_neg = a_size < 0;
        let b_is_neg = b_size < 0;

        if a_is_neg && !b_is_neg {
            return -1; // negative < positive
        }
        if !a_is_neg && b_is_neg {
            return 1; // positive > negative
        }

        // Same sign. Compare magnitudes.
        let a_abs_size = a_size.abs() as usize;
        let b_abs_size = b_size.abs() as usize;

        let a_limbs = std::slice::from_raw_parts(a.data, a_abs_size);
        let b_limbs = std::slice::from_raw_parts(b.data, b_abs_size);

        let mag_cmp = compare_magnitude(a_limbs, b_limbs);

        // For negative numbers, larger magnitude means smaller value.
        if a_is_neg {
            -mag_cmp
        } else {
            mag_cmp
        }
    }
}

/// Widen a fixed-width integer to Int.
pub(crate) unsafe fn int_from_fixed_impl(
    rt: &mut RtLocal,
    src_in: *const u8,
    src_tydesc: &rtdt::TyDesc,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let type_tag = src_tydesc.type_tag;
        let result = &mut *(result_out as *mut rtdt::Int);

        // Extract magnitude and sign from the fixed-width integer.
        let (magnitude, is_negative): (u64, bool) = match type_tag {
            rtdt::TyTag::U8 => (*(src_in as *const u8) as u64, false),
            rtdt::TyTag::U16 => (*(src_in as *const u16) as u64, false),
            rtdt::TyTag::U32 => (*(src_in as *const u32) as u64, false),
            rtdt::TyTag::U64 => (*(src_in as *const u64), false),
            rtdt::TyTag::I8 => {
                let v = *(src_in as *const i8);
                (v.unsigned_abs() as u64, v < 0)
            }
            rtdt::TyTag::I16 => {
                let v = *(src_in as *const i16);
                (v.unsigned_abs() as u64, v < 0)
            }
            rtdt::TyTag::I32 => {
                let v = *(src_in as *const i32);
                (v.unsigned_abs() as u64, v < 0)
            }
            rtdt::TyTag::I64 => {
                let v = *(src_in as *const i64);
                (v.unsigned_abs(), v < 0)
            }
            // Index and offset widen to int at whatever width they are
            // configured to; both reprs cast losslessly to 64 bits.
            rtdt::TyTag::Index => (*(src_in as *const rtdt::IndexRepr) as u64, false),
            rtdt::TyTag::Offset => {
                let v = *(src_in as *const rtdt::OffsetRepr) as i64;
                (v.unsigned_abs(), v < 0)
            }
            _ => {
                // Not a fixed-width integer type - shouldn't happen.
                return RtStatus::Error;
            }
        };

        // Initialize the Int struct.
        if magnitude == 0 {
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = rtdt::Index::ZERO;
        } else if magnitude <= u32::MAX as u64 {
            // Single limb.
            let limb_ptr = rt.alloc.alloc(4, 4, 1) as *mut u32;
            *limb_ptr = magnitude as u32;
            result.data = limb_ptr as *const u32;
            result.size_and_sign = if is_negative { -1 } else { 1 };
            result.capacity = rtdt::Index(1);
        } else {
            // Two limbs.
            let limb_ptr = rt.alloc.alloc(4, 4, 2) as *mut u32;
            *limb_ptr = magnitude as u32;
            *limb_ptr.add(1) = (magnitude >> 32) as u32;
            result.data = limb_ptr as *const u32;
            result.size_and_sign = if is_negative { -2 } else { 2 };
            result.capacity = rtdt::Index(2);
        }

        RtStatus::Ok
    }
}

/// Construct an Int from a limbs array.
///
/// Takes a pointer to u32 limbs (little-endian, least significant first),
/// the count of limbs, and a sign flag. Allocates new limb memory and
/// initializes the Int struct at the destination.
pub(crate) unsafe fn int_from_limbs_impl(
    rt: &mut RtLocal,
    limbs_ptr: *const u32,
    limb_count: u32,
    negative: bool,
    result_out: *mut u8,
) -> RtStatus {
    unsafe {
        let result = &mut *(result_out as *mut rtdt::Int);

        if limb_count as usize > MAX_LIMBS {
            return too_large(result);
        }

        if limb_count == 0 {
            // Zero value.
            result.data = std::ptr::null();
            result.size_and_sign = 0;
            result.capacity = rtdt::Index::ZERO;
        } else {
            // Allocate and copy limbs.
            let count = limb_count as rtdt::IndexRepr;
            let new_limbs = rt.alloc.alloc(4, 4, count) as *mut u32;
            std::ptr::copy_nonoverlapping(limbs_ptr, new_limbs, limb_count as usize);

            result.data = new_limbs as *const u32;
            result.size_and_sign = if negative {
                -(limb_count as i32)
            } else {
                limb_count as i32
            };
            result.capacity = rtdt::Index(count);
        }

        RtStatus::Ok
    }
}


#[cfg(kani)]
mod kani_proofs {
    //! Bounded proofs of the magnitude arithmetic against `u128`; see
    //! botdocs/kani.md.
    //!
    //! Operands have up to three limbs, so that any two and any sum or
    //! difference of them fit a `u128`; products take two each, which is
    //! what fits.

    use super::*;

    const LIMBS: usize = 3;

    /// A length for a magnitude held in `limbs`: up to `max`, and such that
    /// the magnitude has no leading zero limb.
    fn any_len(limbs: &[u32], max: usize) -> usize {
        let len: usize = kani::any();
        kani::assume(len <= max && (len == 0 || limbs[len - 1] != 0));
        len
    }

    fn value(limbs: &[u32]) -> u128 {
        let mut v = 0u128;
        for &limb in limbs.iter().rev() {
            v = (v << 32) | limb as u128;
        }
        v
    }

    fn normalized(limbs: &[u32]) -> bool {
        limbs.is_empty() || limbs[limbs.len() - 1] != 0
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn compare_magnitude_orders_values() {
        let a_limbs: [u32; LIMBS] = kani::any();
        let b_limbs: [u32; LIMBS] = kani::any();
        let a = &a_limbs[..any_len(&a_limbs, LIMBS)];
        let b = &b_limbs[..any_len(&b_limbs, LIMBS)];
        let order = compare_magnitude(a, b);
        assert!(order == value(a).cmp(&value(b)) as i32);
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn add_magnitude_into_adds() {
        let a_limbs: [u32; LIMBS] = kani::any();
        let b_limbs: [u32; LIMBS] = kani::any();
        let a = &a_limbs[..any_len(&a_limbs, LIMBS)];
        let b = &b_limbs[..any_len(&b_limbs, LIMBS)];
        let mut out = [0u32; LIMBS + 1];
        let capacity = sum_capacity(a.len(), b.len());
        let len = add_magnitude_into(&mut out[..capacity], a, b);
        assert!(len <= capacity);
        assert!(value(&out[..len]) == value(a) + value(b));
        assert!(normalized(&out[..len]));
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn sub_magnitude_into_subtracts() {
        let a_limbs: [u32; LIMBS] = kani::any();
        let b_limbs: [u32; LIMBS] = kani::any();
        let a = &a_limbs[..any_len(&a_limbs, LIMBS)];
        let b = &b_limbs[..any_len(&b_limbs, LIMBS)];
        kani::assume(compare_magnitude(a, b) > 0);
        let mut out = [0u32; LIMBS];
        let len = sub_magnitude_into(&mut out[..a.len()], a, b);
        assert!(1 <= len && len <= a.len());
        assert!(value(&out[..len]) == value(a) - value(b));
        assert!(normalized(&out[..len]));
    }

    /// A one-limb product is right. A wider one is not provable this way:
    /// holding a schoolbook product to a `u128` product asks the solver for
    /// a multiplier equivalence, which ran for hours at two limbs by two and
    /// for ten minutes at two by one without finishing.
    #[kani::proof]
    #[kani::unwind(3)]
    fn mul_magnitude_into_multiplies_limbs() {
        let a: u32 = kani::any();
        let b: u32 = kani::any();
        kani::assume(a != 0 && b != 0);
        let mut out = [0u32; 2];
        let len = mul_magnitude_into(&mut out, &[a], &[b]);
        assert!(1 <= len && len <= 2);
        assert!(value(&out[..len]) == a as u128 * b as u128);
        assert!(normalized(&out[..len]));
    }

    /// A wider product stays in bounds and panics nowhere, whatever its
    /// value; the value is the tests' to check.
    #[kani::proof]
    #[kani::unwind(5)]
    fn mul_magnitude_into_stays_in_bounds() {
        let a_limbs: [u32; 2] = kani::any();
        let b_limbs: [u32; 2] = kani::any();
        let a = &a_limbs[..any_len(&a_limbs, 2)];
        let b = &b_limbs[..any_len(&b_limbs, 2)];
        kani::assume(!a.is_empty() && !b.is_empty());
        let mut out = [0u32; 4];
        let len = mul_magnitude_into(&mut out[..a.len() + b.len()], a, b);
        assert!(1 <= len && len <= a.len() + b.len());
    }

    /// Division by a limb of a wider dividend stays in bounds, panics
    /// nowhere, and gives a quotient with no leading zero.
    #[kani::proof]
    #[kani::unwind(4)]
    fn div_magnitude_into_by_a_limb_stays_in_bounds() {
        let a_limbs: [u32; LIMBS] = kani::any();
        let d: u32 = kani::any();
        let a = &a_limbs[..any_len(&a_limbs, LIMBS)];
        kani::assume(!a.is_empty() && d != 0);
        if let Some(capacity) = quotient_capacity(a, &[d]) {
            let mut out = [0u32; LIMBS];
            let len = div_magnitude_into(&mut out[..capacity], a, &[d]);
            assert!(1 <= len && len <= capacity);
            assert!(normalized(&out[..len]));
        }
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn add_assign_magnitude_adds_in_place() {
        let mut buf: [u32; LIMBS + 1] = kani::any();
        let b_limbs: [u32; LIMBS] = kani::any();
        let a_len = any_len(&buf, LIMBS);
        let b = &b_limbs[..any_len(&b_limbs, LIMBS)];
        kani::assume(a_len >= 1 && !b.is_empty());
        let (a_value, b_value) = (value(&buf[..a_len]), value(b));
        let len = add_assign_magnitude(&mut buf, a_len, b);
        assert!(len <= sum_capacity(a_len, b.len()));
        assert!(value(&buf[..len]) == a_value + b_value);
        assert!(normalized(&buf[..len]));
    }

    #[kani::proof]
    #[kani::unwind(4)]
    fn sub_assign_magnitude_subtracts_in_place() {
        let mut buf: [u32; LIMBS] = kani::any();
        let b_limbs: [u32; LIMBS] = kani::any();
        let a_len = any_len(&buf, LIMBS);
        let b = &b_limbs[..any_len(&b_limbs, LIMBS)];
        kani::assume(a_len >= 1 && !b.is_empty());
        let order = compare_magnitude(&buf[..a_len], b);
        kani::assume(order != 0);
        let (a_value, b_value) = (value(&buf[..a_len]), value(b));
        let len = sub_assign_magnitude(&mut buf, a_len, b, order > 0);
        assert!(1 <= len && len <= a_len.max(b.len()));
        assert!(value(&buf[..len]) == a_value.max(b_value) - a_value.min(b_value));
        assert!(normalized(&buf[..len]));
    }
}
