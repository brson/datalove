//! Value allocation functions.
//!
//! These functions allocate runtime values for primitive types and
//! compound structures like Option, Result, tuples, and collections.

use crate::datalit::tycheck::Type;
use super::{InterpContext, InterpError, Value, ValueLocation, Destination};
use super::memory::destroy_value;

/// Allocate a boolean value.
pub(super) fn allocate_bool<'db>(
    ctx: &mut InterpContext<'db>,
    value: bool,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::Bool);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *ptr = if value { 1 } else { 0 }; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate an f32 value.
pub(super) fn allocate_f32<'db>(
    ctx: &mut InterpContext<'db>,
    value: f32,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::F32);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut f32) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate a u8 value.
pub(super) fn allocate_u8<'db>(
    ctx: &mut InterpContext<'db>,
    value: u8,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U8);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *ptr = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate an i8 value.
pub(super) fn allocate_i8<'db>(
    ctx: &mut InterpContext<'db>,
    value: i8,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::I8);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut i8) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate a u16 value.
pub(super) fn allocate_u16<'db>(
    ctx: &mut InterpContext<'db>,
    value: u16,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U16);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut u16) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate an i16 value.
pub(super) fn allocate_i16<'db>(
    ctx: &mut InterpContext<'db>,
    value: i16,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::I16);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut i16) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate a u32 value.
pub(super) fn allocate_u32_raw<'db>(
    ctx: &mut InterpContext<'db>,
    value: u32,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U32);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut u32) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate an i32 value.
pub(super) fn allocate_i32<'db>(
    ctx: &mut InterpContext<'db>,
    value: i32,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::I32);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut i32) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate a u64 value.
pub(super) fn allocate_u64<'db>(
    ctx: &mut InterpContext<'db>,
    value: u64,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U64);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut u64) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate an i64 value.
pub(super) fn allocate_i64<'db>(
    ctx: &mut InterpContext<'db>,
    value: i64,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::I64);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe { *(ptr as *mut i64) = value; }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate a bigint (Int) value initialized to zero.
pub(super) fn allocate_bigint<'db>(
    ctx: &mut InterpContext<'db>,
) -> Result<Value, InterpError> {
    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::Int);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };
    unsafe {
        let int_ptr = ptr as *mut datalove_rt::rtdt::Int;
        (*int_ptr).data = std::ptr::null();
        (*int_ptr).size_and_sign = 0;
        (*int_ptr).capacity = 0;
    }
    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Allocate an Option<T> with None value.
pub(super) fn allocate_option_none<'db>(
    ctx: &mut InterpContext<'db>,
    inner_tydesc: *const datalove_rt::rtdt::TyDesc,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_tydesc);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, option_tydesc, 1)
    };
    if ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Option".to_string()));
    }
    unsafe { *ptr = rtdt::OptionTag::None as u8; }
    Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
}

/// Wrap an existing value in Some, consuming the inner value.
pub(super) fn allocate_option_some_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_value.tydesc);
    let option_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(option_tydesc) };
    let layout = rtdt::layout::compute_option_layout(option_tydesc_ref);

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, option_tydesc, 1)
    };
    if ptr.is_null() {
        destroy_value(ctx, inner_value);
        return Err(InterpError::RuntimeError("Failed to allocate Option".to_string()));
    }

    unsafe {
        *ptr = rtdt::OptionTag::Some as u8;
        let payload_ptr = ptr.add(layout.payload_offset as usize);
        let inner_size = (*inner_value.tydesc).size as usize;
        std::ptr::copy_nonoverlapping(inner_value.ptr, payload_ptr, inner_size);
    }

    // Free the inner value's container (data has been copied to Option).
    if inner_value.location == ValueLocation::TempOwned {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                inner_value.tydesc,
                1,
                inner_value.ptr,
            );
        }
    }

    Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
}

/// Wrap an existing value in Ok, consuming the inner value.
pub(super) fn allocate_result_ok_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    let result_tydesc = ctx.tydesc_table.create_result_from_inner_tydesc(inner_value.tydesc);
    let result_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(result_tydesc) };
    let layout = rtdt::layout::compute_result_layout(result_tydesc_ref);

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, result_tydesc, 1)
    };
    if ptr.is_null() {
        destroy_value(ctx, inner_value);
        return Err(InterpError::RuntimeError("Failed to allocate Result".to_string()));
    }

    unsafe {
        *ptr = rtdt::ResultTag::Ok as u8;
        let payload_ptr = ptr.add(layout.payload_offset as usize);
        let inner_size = (*inner_value.tydesc).size as usize;
        std::ptr::copy_nonoverlapping(inner_value.ptr, payload_ptr, inner_size);
    }

    // Free the inner value's container.
    if inner_value.location == ValueLocation::TempOwned {
        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                inner_value.tydesc,
                1,
                inner_value.ptr,
            );
        }
    }

    Ok(Value { ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned })
}

/// Allocate a Result<T, String> with Err(string) value.
pub(super) fn allocate_result_err<'db>(
    ctx: &mut InterpContext<'db>,
    ok_tydesc: *const datalove_rt::rtdt::TyDesc,
    err_tydesc: *const datalove_rt::rtdt::TyDesc,
    err_ptr: *mut u8,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    let result_tydesc = ctx.tydesc_table.create_result_from_inner_tydesc(ok_tydesc);
    let result_tydesc_ref = unsafe { rtdt::TyDescRef::from_ptr(result_tydesc) };
    let layout = rtdt::layout::compute_result_layout(result_tydesc_ref);

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, result_tydesc, 1)
    };
    if ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Result".to_string()));
    }

    unsafe {
        *ptr = rtdt::ResultTag::Err as u8;
        let err_payload_ptr = ptr.add(layout.payload_offset as usize);
        let err_size = (*err_tydesc).size as usize;
        std::ptr::copy_nonoverlapping(err_ptr, err_payload_ptr, err_size);
    }

    Ok(Value { ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned })
}

/// Write Result::Err to a destination (DPS).
///
/// The destination must be a Result type. Writes the Err tag and copies the error payload.
pub(super) fn write_result_err_to_dest(
    dest: Destination,
    err_tydesc: *const datalove_rt::rtdt::TyDesc,
    err_ptr: *mut u8,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{self, TyDescRef, TyTag};

    let dest_tydesc_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    if dest_tydesc_ref.type_tag() != TyTag::Result {
        return Err(InterpError::RuntimeError(
            format!("write_result_err_to_dest requires Result destination, got {:?}",
                    dest_tydesc_ref.type_tag())
        ));
    }

    let layout = rtdt::layout::compute_result_layout(dest_tydesc_ref);

    unsafe {
        *dest.ptr = rtdt::ResultTag::Err as u8;
        let err_payload_ptr = dest.ptr.add(layout.payload_offset as usize);
        let err_size = (*err_tydesc).size as usize;
        std::ptr::copy_nonoverlapping(err_ptr, err_payload_ptr, err_size);
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
}

/// Widen any fixed-width integer to an Int (bigint) value.
///
/// Handles: u8, i8, u16, i16, u32, i32, u64, i64.
/// For signed types, preserves the sign in the bigint representation.
pub(super) fn widen_fixed_int_to_int<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let type_tag = unsafe { (*value.tydesc).type_tag };

    // Extract magnitude and sign from the fixed-width integer.
    let (magnitude, is_negative): (u64, bool) = unsafe {
        match type_tag {
            TyTag::U8 => (*(value.ptr as *const u8) as u64, false),
            TyTag::U16 => (*(value.ptr as *const u16) as u64, false),
            TyTag::U32 => (*(value.ptr as *const u32) as u64, false),
            TyTag::U64 => (*(value.ptr as *const u64), false),
            TyTag::I8 => {
                let v = *(value.ptr as *const i8);
                if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
            }
            TyTag::I16 => {
                let v = *(value.ptr as *const i16);
                if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
            }
            TyTag::I32 => {
                let v = *(value.ptr as *const i32);
                if v < 0 { ((-(v as i64)) as u64, true) } else { (v as u64, false) }
            }
            TyTag::I64 => {
                let v = *(value.ptr as *const i64);
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

    let int_val = allocate_bigint(ctx)?;
    let int_ptr = int_val.ptr as *mut datalove_rt::rtdt::Int;

    unsafe {
        if magnitude == 0 {
            (*int_ptr).data = std::ptr::null();
            (*int_ptr).size_and_sign = 0;
            (*int_ptr).capacity = 0;
        } else if magnitude <= u32::MAX as u64 {
            // Fits in one limb.
            let rt_handle = ctx.runtime.handle();
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, 1
            ) as *mut u32;
            *limb_ptr = magnitude as u32;
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = if is_negative { -1 } else { 1 };
            (*int_ptr).capacity = 1;
        } else {
            // Needs two limbs (for u64/i64 values > u32::MAX).
            let rt_handle = ctx.runtime.handle();
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

    Ok(int_val)
}

/// Initialize a bigint (Int) at the given pointer from an i128 value.
///
/// This writes directly to the destination without allocating the Int struct itself.
/// The limbs are allocated via the runtime allocator.
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

/// Wrap an existing value in Data, consuming the inner value.
///
/// The Data struct takes ownership of the inner value's pointer.
/// If the inner value is borrowed, it is cloned first.
pub(super) fn allocate_data_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;
    use super::memory::clone_value;

    // If the inner value is borrowed, we need to clone it since Data will take ownership.
    let owned_inner = if inner_value.location == ValueLocation::TempOwned {
        inner_value
    } else {
        // Clone borrowed/slot-owned values so Data can own them.
        clone_value(ctx, inner_value)
    };

    let data_tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::Data);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, data_tydesc, 1)
    };
    if ptr.is_null() {
        destroy_value(ctx, owned_inner);
        return Err(InterpError::RuntimeError("Failed to allocate Data".to_string()));
    }

    unsafe {
        std::ptr::write(
            ptr as *mut rtdt::Data,
            rtdt::Data::from_pointers(owned_inner.tydesc, owned_inner.ptr)
        );
    }

    // Data now owns the pointer to inner value's allocation.
    // Don't free owned_inner - Data::from_pointers stores owned_inner.ptr directly.

    Ok(Value { ptr, tydesc: data_tydesc, location: ValueLocation::TempOwned })
}
