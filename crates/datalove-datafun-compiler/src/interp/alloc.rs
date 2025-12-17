//! Value allocation functions.
//!
//! These functions allocate runtime values for primitive types and
//! compound structures like Option, Result, tuples, and collections.

use crate::datalit::tycheck::Type;
use super::{InterpContext, InterpError, Value, ValueLocation};
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

/// Widen a u32 value to an Int (bigint) value.
pub(super) fn widen_u32_to_int<'db>(
    ctx: &mut InterpContext<'db>,
    u32_value: Value,
) -> Result<Value, InterpError> {
    let value_u32 = unsafe { *(u32_value.ptr as *const u32) };
    let int_val = allocate_bigint(ctx)?;
    let int_ptr = int_val.ptr as *mut datalove_rt::rtdt::Int;

    unsafe {
        if value_u32 == 0 {
            (*int_ptr).data = std::ptr::null();
            (*int_ptr).size_and_sign = 0;
            (*int_ptr).capacity = 0;
        } else {
            let rt_handle = ctx.runtime.handle();
            let limb_ptr = datalove_rt::c::dtlv_rti_mem_alloc_raw_local(
                rt_handle, 4, 4, 1
            ) as *mut u32;
            *limb_ptr = value_u32;
            (*int_ptr).data = limb_ptr;
            (*int_ptr).size_and_sign = 1;
            (*int_ptr).capacity = 1;
        }
    }

    Ok(int_val)
}

/// Wrap an existing value in Data, consuming the inner value.
///
/// The Data struct takes ownership of the inner value's pointer.
/// The inner value's memory is NOT freed - Data now owns it.
pub(super) fn allocate_data_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    inner_value: Value,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt;

    let data_tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::Data);
    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, data_tydesc, 1)
    };
    if ptr.is_null() {
        destroy_value(ctx, inner_value);
        return Err(InterpError::RuntimeError("Failed to allocate Data".to_string()));
    }

    unsafe {
        std::ptr::write(
            ptr as *mut rtdt::Data,
            rtdt::Data::from_pointers(inner_value.tydesc, inner_value.ptr)
        );
    }

    // Data now owns the pointer to inner value's allocation.
    // Don't free inner_value - Data::from_pointers stores inner_value.ptr directly.
    // If inner was TempOwned, the ownership transfers to Data.
    // If inner was Borrowed, the caller still owns it (but Data now has a pointer to it).

    Ok(Value { ptr, tydesc: data_tydesc, location: ValueLocation::TempOwned })
}
