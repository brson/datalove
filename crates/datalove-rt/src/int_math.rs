//! Bigint arithmetic operations for the Datalove runtime.

use rmx::prelude::*;
use crate::rtdt;
use crate::rt_local::RtLocal;
use crate::{LocalRtHandle, RtStatus};
use ibig::IBig;

/// Convert rtdt::Int to ibig::IBig.
pub unsafe fn rtdt_int_to_ibig(int_ptr: *const rtdt::Int) -> IBig {
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

/// Add two bigints: a + b.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_int_add(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut RtLocal);
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);
        let result = a + b;
        ibig_to_rtdt_int(rt_ref, result, result_out as *mut rtdt::Int)
    }
}

/// Subtract two bigints: a - b.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_int_sub(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut RtLocal);
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);
        let result = a - b;
        ibig_to_rtdt_int(rt_ref, result, result_out as *mut rtdt::Int)
    }
}

/// Multiply two bigints: a * b.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_int_mul(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut RtLocal);
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);
        let result = a * b;
        ibig_to_rtdt_int(rt_ref, result, result_out as *mut rtdt::Int)
    }
}

/// Negate a bigint: -a.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_int_neg(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut RtLocal);
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let result = -a;
        ibig_to_rtdt_int(rt_ref, result, result_out as *mut rtdt::Int)
    }
}

/// Division of bigints: a / b.
/// Caller must ensure b is not zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dtlv_rti_int_div_checked(
    rt: LocalRtHandle,
    a_in: *const u8,
    a_tydesc: *const rtdt::TyDesc,
    b_in: *const u8,
    b_tydesc: *const rtdt::TyDesc,
    result_out: *mut u8,
    result_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    if rt.is_null() || a_in.is_null() || b_in.is_null() || result_out.is_null() {
        return RtStatus::Error;
    }

    unsafe {
        let rt_ref = &mut *(rt as *mut RtLocal);
        let a = rtdt_int_to_ibig(a_in as *const rtdt::Int);
        let b = rtdt_int_to_ibig(b_in as *const rtdt::Int);
        let result = a / b;
        ibig_to_rtdt_int(rt_ref, result, result_out as *mut rtdt::Int)
    }
}
