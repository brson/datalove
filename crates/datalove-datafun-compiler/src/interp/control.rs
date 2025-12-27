//! Control flow: branch conditions and try operators.
//!
//! - `evaluate_branch_condition`: If-condition on Bool, Option, or Result
//! - `eval_try_option`: The `?` operator (Option unwrap or early return)
//! - `eval_try_result`: The `!` operator (Result unwrap or early return)

use super::{InterpContext, InterpError, Value, SlotState};
use super::memory::destroy_value;
use crate::ast::StmtIf;

/// Evaluate if-condition on Bool, Option, or Result.
///
/// Returns true if truthy (Bool=true, Option=Some, Result=Ok).
/// Binds payloads to then_binding/else_binding slots when present.
pub(super) fn evaluate_branch_condition<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    if_stmt: StmtIf<'db>,
) -> Result<bool, InterpError> {
    use datalove_rt::rtdt::{TyTag, OptionTag, ResultTag, TyDescRef, Data, Error};
    use datalove_rt::rtdt::layout::{compute_option_layout, compute_result_layout};

    let type_tag = unsafe { (*value.tydesc).type_tag };
    let frame_index = ctx.call_stack.len() - 1;
    let frame_layout = ctx.call_stack[frame_index].layout;

    // Look up then/else binding slots using indexed lookup.
    let (then_slot, else_slot) = frame_layout.get_if_binding_slots(ctx.db, if_stmt);

    match type_tag {
        TyTag::Bool => {
            let result = unsafe { *(value.ptr as *const bool) };
            destroy_value(ctx, value);
            Ok(result)
        }
        TyTag::Option => {
            let tag = unsafe { *(value.ptr as *const u8) };
            let is_some = tag == OptionTag::Some as u8;

            if is_some {
                if let Some(slot_info) = then_slot {
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = compute_option_layout(tydesc_ref);
                    let inner_tydesc = tydesc_ref.option_inner_ty();

                    let slot_offset = slot_info.offset(ctx.db) as usize;
                    let slot_ptr = unsafe {
                        ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                    };

                    let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };

                    // Clone inner value to slot (deep copy with heap).
                    let rt_handle = ctx.runtime.handle();
                    unsafe {
                        datalove_rt::c::dtlv_rti_clone_local(
                            rt_handle,
                            payload_ptr,
                            inner_tydesc.as_ptr(),
                            slot_ptr,
                            inner_tydesc.as_ptr(),
                        );
                    }

                    let slot_id = slot_info.slot_id(ctx.db);
                    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Available);
                }
            }

            // Destroy the original Option (frees any heap data in the payload).
            destroy_value(ctx, value);
            Ok(is_some)
        }
        TyTag::Result => {
            let tag = unsafe { *(value.ptr as *const u8) };
            let is_ok = tag == ResultTag::Ok as u8;
            // Compute layout once, reuse in both Ok and Err branches.
            let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
            let layout = compute_result_layout(tydesc_ref);

            if is_ok {
                if let Some(slot_info) = then_slot {
                    let ok_tydesc = tydesc_ref.result_ok_ty();

                    let slot_offset = slot_info.offset(ctx.db) as usize;
                    let slot_ptr = unsafe {
                        ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                    };

                    let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };

                    // Clone Ok payload to slot (deep copy with heap).
                    let rt_handle = ctx.runtime.handle();
                    unsafe {
                        datalove_rt::c::dtlv_rti_clone_local(
                            rt_handle,
                            payload_ptr,
                            ok_tydesc.as_ptr(),
                            slot_ptr,
                            ok_tydesc.as_ptr(),
                        );
                    }

                    let slot_id = slot_info.slot_id(ctx.db);
                    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Available);
                }

                // Destroy the original Result (frees any heap data in the Ok payload).
                destroy_value(ctx, value);
            } else {
                if let Some(slot_info) = else_slot {
                    let slot_offset = slot_info.offset(ctx.db) as usize;
                    let slot_ptr = unsafe {
                        ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                    };

                    let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };

                    // Read the Error from the payload. Error has same layout as Data.
                    let error_data = unsafe { std::ptr::read(payload_ptr as *const Data) };
                    let inner_tydesc = error_data.tydesc();
                    let inner_value_ptr = error_data.value_ptr();

                    // Clone the Error's inner value to new heap allocation.
                    let rt_handle = ctx.runtime.handle();
                    let cloned_ptr = unsafe {
                        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, inner_tydesc, 1)
                    };
                    if cloned_ptr.is_null() {
                        destroy_value(ctx, value);
                        return Err(InterpError::RuntimeError(
                            "Failed to allocate Error inner clone for if-result binding".to_string()
                        ));
                    }
                    unsafe {
                        datalove_rt::c::dtlv_rti_clone_local(
                            rt_handle,
                            inner_value_ptr,
                            inner_tydesc,
                            cloned_ptr,
                            inner_tydesc,
                        );
                    }

                    // Write new Error (with cloned data) to else_slot.
                    unsafe {
                        let new_data = Data::from_pointers(inner_tydesc, cloned_ptr);
                        std::ptr::write(
                            slot_ptr as *mut Error,
                            std::mem::transmute(new_data),
                        );
                    }

                    let slot_id = slot_info.slot_id(ctx.db);
                    ctx.call_stack[frame_index].set_slot_state(slot_id, SlotState::Available);
                }

                // Destroy the original Result (frees the original Error's heap data).
                destroy_value(ctx, value);
            }

            Ok(is_ok)
        }
        _ => {
            destroy_value(ctx, value);
            Err(InterpError::RuntimeError(
                format!("Expected Bool, Option, or Result in condition, got {:?}", type_tag)
            ))
        }
    }
}

/// Evaluate try-option operator (`val?`).
///
/// Some: writes inner to dest. None: returns `InterpError::OptionNone`.
pub(super) fn eval_try_option<'db>(
    ctx: &mut InterpContext<'db>,
    operand_value: Value,
    dest: super::Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, OptionTag, layout::compute_option_layout};

    let tydesc_ref = unsafe { TyDescRef::from_ptr(operand_value.tydesc) };

    if tydesc_ref.type_tag() != TyTag::Option {
        destroy_value(ctx, operand_value);
        return Err(InterpError::RuntimeError(
            format!("Try-option operator (?) requires Option type, got {:?}", tydesc_ref.type_tag())
        ));
    }

    let tag = unsafe { *(operand_value.ptr as *const u8) };

    if tag == OptionTag::None as u8 {
        destroy_value(ctx, operand_value);
        return Err(InterpError::OptionNone);
    }

    let layout = compute_option_layout(tydesc_ref);
    let payload_ptr = unsafe { operand_value.ptr.add(layout.payload_offset as usize) };

    let inner_size = unsafe { (*dest.tydesc).size as usize };

    unsafe {
        std::ptr::copy_nonoverlapping(payload_ptr, dest.ptr, inner_size);
    }

    Ok(())
}

/// Evaluate try-result operator (`val!`).
///
/// Ok: writes inner to dest. Err: returns `InterpError::ResultErr`.
pub(super) fn eval_try_result<'db>(
    ctx: &mut InterpContext<'db>,
    operand_value: Value,
    dest: super::Destination,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, ResultTag, Data, layout::compute_result_layout};

    let tydesc_ref = unsafe { TyDescRef::from_ptr(operand_value.tydesc) };

    if tydesc_ref.type_tag() != TyTag::Result {
        destroy_value(ctx, operand_value);
        return Err(InterpError::RuntimeError(
            format!("Try-result operator (!) requires Result type, got {:?}", tydesc_ref.type_tag())
        ));
    }

    let tag = unsafe { *(operand_value.ptr as *const u8) };

    let layout = compute_result_layout(tydesc_ref);
    let payload_ptr = unsafe { operand_value.ptr.add(layout.payload_offset as usize) };

    if tag == ResultTag::Err as u8 {
        let error_data = unsafe { std::ptr::read(payload_ptr as *const Data) };
        let err_tydesc = error_data.tydesc();
        let err_value_ptr = error_data.value_ptr();

        let err_tydesc_ref = unsafe { TyDescRef::from_ptr(err_tydesc) };
        let err_size = err_tydesc_ref.size() as usize;

        let rt_handle = ctx.runtime.handle();
        let cloned_err_ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, err_tydesc, 1)
        };

        if !cloned_err_ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(err_value_ptr, cloned_err_ptr, err_size);
            }
        }

        unsafe {
            datalove_rt::c::dtlv_rti_mem_free_local(
                ctx.runtime.handle(),
                err_tydesc,
                1,
                err_value_ptr as *mut u8,
            );
        }

        return Err(InterpError::ResultErr {
            tydesc: err_tydesc,
            ptr: cloned_err_ptr,
        });
    }

    let ok_size = unsafe { (*dest.tydesc).size as usize };

    unsafe {
        std::ptr::copy_nonoverlapping(payload_ptr, dest.ptr, ok_size);
    }

    Ok(())
}
