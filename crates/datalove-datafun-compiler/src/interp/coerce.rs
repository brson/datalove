//! Type coercion and narrowing operations.
//!
//! Handles automatic coercion between compatible types:
//! - data → Option<data> (wrap in Some)
//! - data → Result<data> (wrap in Ok)

use super::{InterpContext, InterpError, Value, Destination, ValueLocation};
use super::memory::destroy_value;

/// Coerce a value to a destination type.
///
/// Handles data → Option<data> (wrap in Some) and data → Result<data> (wrap in Ok).
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

    // Coerce data → Option<data> (wrap Data value in Some)
    if value_tag == TyTag::Data && dest_tag == TyTag::Option {
        let dest_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let option_inner_tydesc = dest_ref.option_inner_ty();

        // Only allow if inner type is also Data.
        if unsafe { (*option_inner_tydesc.as_ptr()).type_tag } == TyTag::Data {
            let layout = compute_option_layout(dest_ref);

            // Write Some tag.
            unsafe { *(dest.ptr as *mut u8) = OptionTag::Some as u8; }

            // Copy the Data struct (16 bytes) to payload.
            let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
            let data_size = unsafe { (*value.tydesc).size as usize };
            unsafe { std::ptr::copy_nonoverlapping(value.ptr, payload_ptr, data_size); }

            // Free the value container (contents moved to Option).
            // Note: We do NOT destroy_value here because the Data's internal pointers
            // are now owned by the Option payload. We only free the container.
            if value.location == ValueLocation::TempOwned {
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        ctx.runtime.handle(), value.tydesc, 1, value.ptr
                    );
                }
            }
            return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
        }
    }

    // Coerce data → Result<data> (wrap Data value in Ok)
    if value_tag == TyTag::Data && dest_tag == TyTag::Result {
        let dest_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };
        let result_inner_tydesc = dest_ref.result_ok_ty();

        // Only allow if inner type is also Data.
        if unsafe { (*result_inner_tydesc.as_ptr()).type_tag } == TyTag::Data {
            let layout = compute_result_layout(dest_ref);

            // Write Ok tag.
            unsafe { *(dest.ptr as *mut u8) = ResultTag::Ok as u8; }

            // Copy the Data struct (16 bytes) to payload.
            let payload_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
            let data_size = unsafe { (*value.tydesc).size as usize };
            unsafe { std::ptr::copy_nonoverlapping(value.ptr, payload_ptr, data_size); }

            // Free the value container (contents moved to Result).
            if value.location == ValueLocation::TempOwned {
                unsafe {
                    datalove_rt::c::dtlv_rti_mem_free_local(
                        ctx.runtime.handle(), value.tydesc, 1, value.ptr
                    );
                }
            }
            return Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::TempOwned });
        }
    }

    // No coercion available - type mismatch.
    destroy_value(ctx, value);
    Err(InterpError::RuntimeError(
        format!("Type mismatch: cannot coerce {:?} to {:?}", value_tag, dest_tag)
    ))
}
