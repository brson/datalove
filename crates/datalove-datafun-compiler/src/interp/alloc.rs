//! DPS write functions for compound types.
//!
//! Write Int (widened from fixed-width) and Data wrappers to pre-allocated destinations.

use super::{InterpContext, InterpError, Value, Destination};

/// Widen a fixed-width integer to Int at destination.
///
/// Converts u8/i8/u16/i16/u32/i32/u64/i64 to bigint representation.
/// Allocates limbs via the runtime allocator.
pub(super) fn write_widened_int_to_dest(
    rt_handle: *mut u8,
    src_ptr: *const u8,
    src_tydesc: *const datalove_rt::rtdt::TyDesc,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::TyTag;

    let type_tag = unsafe { (*src_tydesc).type_tag };

    // Extract magnitude and sign from the fixed-width integer.
    let (magnitude, is_negative): (u64, bool) = unsafe {
        match type_tag {
            TyTag::U8 => (*(src_ptr as *const u8) as u64, false),
            TyTag::U16 => (*(src_ptr as *const u16) as u64, false),
            TyTag::U32 => (*(src_ptr as *const u32) as u64, false),
            TyTag::U64 => (*(src_ptr as *const u64), false),
            TyTag::I8 => {
                let v = *(src_ptr as *const i8);
                if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
            }
            TyTag::I16 => {
                let v = *(src_ptr as *const i16);
                if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
            }
            TyTag::I32 => {
                let v = *(src_ptr as *const i32);
                if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
            }
            TyTag::I64 => {
                let v = *(src_ptr as *const i64);
                if v == i64::MIN {
                    // Special case: i64::MIN cannot be negated without overflow.
                    // Its magnitude is 2^63 = 0x8000_0000_0000_0000.
                    (0x8000_0000_0000_0000u64, true)
                } else if v < 0 {
                    ((-v) as u64, true)
                } else {
                    (v as u64, false)
                }
            }
            _ => return Err(InterpError::InvalidExpression(
                format!("Cannot widen type {:?} to int", type_tag)
            )),
        }
    };

    let int_ptr = dest.ptr as *mut datalove_rt::rtdt::Int;

    unsafe {
        if magnitude == 0 {
            (*int_ptr).data = std::ptr::null();
            (*int_ptr).size_and_sign = 0;
            (*int_ptr).capacity = 0;
        } else if magnitude <= u32::MAX as u64 {
            // Fits in one limb.
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, 1
            ) as *mut u32;
            *limb_ptr = magnitude as u32;
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = if is_negative { -1 } else { 1 };
            (*int_ptr).capacity = 1;
        } else {
            // Needs two limbs (for u64/i64 values > u32::MAX).
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, 2
            ) as *mut u32;
            // Low limb first (little-endian limb order).
            *limb_ptr = magnitude as u32;
            *limb_ptr.add(1) = (magnitude >> 32) as u32;
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = if is_negative { -2 } else { 2 };
            (*int_ptr).capacity = 2;
        }
    }

    Ok(())
}

/// Write an i128 as a bigint (Int) at the given pointer.
///
/// Allocates limbs via the runtime allocator.
pub(super) fn write_bigint_to_ptr(
    rt_handle: *mut u8,
    int_ptr: *mut datalove_rt::rtdt::Int,
    value: i128,
) {
    let (magnitude, is_negative) = if value < 0 {
        ((-value) as u128, true)
    } else {
        (value as u128, false)
    };

    unsafe {
        if magnitude == 0 {
            (*int_ptr).data = std::ptr::null();
            (*int_ptr).size_and_sign = 0;
            (*int_ptr).capacity = 0;
        } else if magnitude <= u32::MAX as u128 {
            // Fits in one limb.
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, 1
            ) as *mut u32;
            *limb_ptr = magnitude as u32;
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = if is_negative { -1 } else { 1 };
            (*int_ptr).capacity = 1;
        } else if magnitude <= u64::MAX as u128 {
            // Fits in two limbs.
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, 2
            ) as *mut u32;
            *limb_ptr = magnitude as u32;
            *limb_ptr.add(1) = (magnitude >> 32) as u32;
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = if is_negative { -2 } else { 2 };
            (*int_ptr).capacity = 2;
        } else {
            // Fits in three or four limbs.
            let num_limbs = if magnitude <= (1u128 << 96) - 1 { 3 } else { 4 };
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, num_limbs
            ) as *mut u32;
            *limb_ptr = magnitude as u32;
            *limb_ptr.add(1) = (magnitude >> 32) as u32;
            *limb_ptr.add(2) = (magnitude >> 64) as u32;
            if num_limbs == 4 {
                *limb_ptr.add(3) = (magnitude >> 96) as u32;
            }
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = if is_negative { -(num_limbs as i32) } else { num_limbs as i32 };
            (*int_ptr).capacity = num_limbs as u32;
        }
    }
}

/// Write a Data wrapper to destination.
///
/// Clones the inner value; the Data struct owns the clone.
pub(super) fn write_data_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt;

    // All values are Borrowed, so we always clone for Data to own.
    let rt_handle = ctx.runtime.handle();
    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner_value.tydesc, 1)
    };
    if cloned_ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Data inner clone".to_string()));
    }
    unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt_handle,
            inner_value.ptr,
            inner_value.tydesc,
            cloned_ptr,
            inner_value.tydesc,
        );
    }

    // Write Data struct to destination.
    unsafe {
        std::ptr::write(
            dest.ptr as *mut rtdt::Data,
            rtdt::Data::from_pointers(inner_value.tydesc, cloned_ptr)
        );
    }

    Ok(())
}

/// Write an Error wrapper to destination.
///
/// Clones the inner value; the Error struct owns the clone.
/// Error has the same layout as Data.
pub(super) fn write_error_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
    dest: Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt;

    // All values are Borrowed, so we always clone for Error to own.
    let rt_handle = ctx.runtime.handle();
    let cloned_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner_value.tydesc, 1)
    };
    if cloned_ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Error inner clone".to_string()));
    }
    unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            rt_handle,
            inner_value.ptr,
            inner_value.tydesc,
            cloned_ptr,
            inner_value.tydesc,
        );
    }

    // Write Error struct to destination. Error has same layout as Data.
    unsafe {
        let data = rtdt::Data::from_pointers(inner_value.tydesc, cloned_ptr);
        // Transmute Data to Error since they have identical layouts.
        std::ptr::write(
            dest.ptr as *mut rtdt::Error,
            std::mem::transmute(data)
        );
    }

    Ok(())
}
