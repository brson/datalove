//! Type coercion and narrowing operations.
//!
//! Handles automatic coercion between compatible types:
//! - T → Option<T> (wrap in Some)
//! - T → Result<T> (wrap in Ok)
//! - Int → u32 (narrowing conversion)

use super::{InterpContext, InterpError, Value, Destination, ValueLocation};
use super::memory::destroy_value;

/// Narrow an Int value to u32.
///
/// Reads the Int value and converts it to a u32. Returns an error if the Int
/// value is too large to fit in u32 or is negative.
pub(super) fn narrow_int_to_u32<'db>(
    ctx: &mut InterpContext<'db>,
    int_value: Value,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let int_ptr = int_value.ptr as *const datalove_rt::rtdt::Int;

    let u32_value = unsafe {
        let size_and_sign = (*int_ptr).size_and_sign;
        let size = size_and_sign.unsigned_abs() as usize;
        let is_negative = size_and_sign < 0;

        if is_negative {
            return Err(InterpError::RuntimeError(
                "Cannot narrow negative Int to u32".to_string()
            ));
        }

        if size == 0 {
            0u32
        } else if size == 1 {
            let limb_ptr = (*int_ptr).data as *const u32;
            *limb_ptr
        } else {
            return Err(InterpError::RuntimeError(
                "Int value too large to fit in u32".to_string()
            ));
        }
    };

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::U32);

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };

    unsafe {
        *(ptr as *mut u32) = u32_value;
    }

    destroy_value(ctx, int_value);

    Ok(Value { ptr, tydesc: tydesc_ptr, location: ValueLocation::TempOwned })
}

/// Coerce a value to a destination type.
///
/// Handles T → Option<T> (wrap in Some) and T → Result<T> (wrap in Ok).
/// If the types are compatible, copies the value to the destination.
pub(super) fn coerce_value_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    dest: Destination,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyTag, TyDescRef, OptionTag, ResultTag};
    use datalove_rt::rtdt::layout::{compute_option_layout, compute_result_layout};

    let value_tag = unsafe { (*value.tydesc).type_tag };
    let dest_tag = unsafe { (*dest.tydesc).type_tag };

    // If types match, clone the value to dest (not shallow copy - types may have internal pointers).
    if value.tydesc == dest.tydesc {
        let clone_status = unsafe {
            datalove_rt::c::dtlv_rti_clone_local(
                ctx.runtime.handle(),
                value.ptr,
                value.tydesc,
                dest.ptr,
                dest.tydesc,
            )
        };
        if clone_status != datalove_rt::c::RtStatus::Ok {
            destroy_value(ctx, value);
            return Err(InterpError::RuntimeError("Failed to clone value in coercion".to_string()));
        }
        destroy_value(ctx, value);
        return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
    }

    // Coerce T → Option<T>
    if dest_tag == TyTag::Option {
        let dest_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let inner_tydesc = dest_ref.option_inner_ty();

        // Check if value type matches inner type.
        if value.tydesc == inner_tydesc.as_ptr() {
            // Wrap value in Some.
            let layout = unsafe { compute_option_layout(dest_ref) };

            // Write Some tag.
            unsafe { *(dest.ptr as *mut u8) = OptionTag::Some as u8; }

            // Clone payload (not shallow copy - types may have internal pointers).
            let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
            let clone_status = unsafe {
                datalove_rt::c::dtlv_rti_clone_local(
                    ctx.runtime.handle(),
                    value.ptr,
                    value.tydesc,
                    payload_ptr,
                    inner_tydesc.as_ptr(),
                )
            };
            if clone_status != datalove_rt::c::RtStatus::Ok {
                destroy_value(ctx, value);
                return Err(InterpError::RuntimeError("Failed to clone value in Option coercion".to_string()));
            }

            // Clean up the original value (we cloned it).
            destroy_value(ctx, value);

            return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
        }
    }

    // Coerce T → Result<T>
    if dest_tag == TyTag::Result {
        let dest_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let inner_tydesc = dest_ref.result_ok_ty();

        // Check if value type matches inner type.
        if value.tydesc == inner_tydesc.as_ptr() {
            // Wrap value in Ok.
            let layout = unsafe { compute_result_layout(dest_ref) };

            // Write Ok tag.
            unsafe { *(dest.ptr as *mut u8) = ResultTag::Ok as u8; }

            // Clone payload (not shallow copy - types may have internal pointers).
            let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
            let clone_status = unsafe {
                datalove_rt::c::dtlv_rti_clone_local(
                    ctx.runtime.handle(),
                    value.ptr,
                    value.tydesc,
                    payload_ptr,
                    inner_tydesc.as_ptr(),
                )
            };
            if clone_status != datalove_rt::c::RtStatus::Ok {
                destroy_value(ctx, value);
                return Err(InterpError::RuntimeError("Failed to clone value in Result coercion".to_string()));
            }

            // Clean up the original value (we cloned it).
            destroy_value(ctx, value);

            return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
        }
    }

    // No coercion available - type mismatch.
    destroy_value(ctx, value);
    Err(InterpError::RuntimeError(
        format!("Type mismatch: cannot coerce {:?} to {:?}", value_tag, dest_tag)
    ))
}
