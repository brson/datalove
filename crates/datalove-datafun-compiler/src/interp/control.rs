//! Control flow: branch conditions and try operators.
//!
//! - `evaluate_branch_condition`: If-condition on Bool, Option, or Result
//! - `eval_try_option`: The `?` operator (Option unwrap or early return)
//! - `eval_try_result`: The `!` operator (Result unwrap or early return)

use bct::text::InternedText;

use super::{InterpContext, InterpError, Value, SlotState};
use super::memory::destroy_value;

/// Find a slot by variable name.
pub(super) fn find_slot_by_name<'db>(
    db: &'db dyn crate::Db,
    layout: crate::function_analysis::FrameLayout<'db>,
    name: InternedText<'db>,
) -> Option<crate::function_analysis::SlotInfo<'db>> {
    layout.slots(db).iter()
        .find(|s| s.name(db) == Some(name))
        .copied()
}

/// Evaluate if-condition on Bool, Option, or Result.
///
/// Returns true if truthy (Bool=true, Option=Some, Result=Ok).
/// Binds payloads to then_binding/else_binding slots when present.
pub(super) fn evaluate_branch_condition<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    then_binding: Option<InternedText<'db>>,
    else_binding: Option<InternedText<'db>>,
) -> Result<bool, InterpError> {
    use datalove_rt::rtdt::{TyTag, OptionTag, ResultTag, TyDescRef};
    use datalove_rt::rtdt::layout::{compute_option_layout, compute_result_layout};

    let type_tag = unsafe { (*value.tydesc).type_tag };

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
                if let Some(binding_name) = then_binding {
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = compute_option_layout(tydesc_ref);
                    let inner_tydesc = tydesc_ref.option_inner_ty();

                    let frame_index = ctx.call_stack.len() - 1;
                    let frame_layout = ctx.call_stack[frame_index].layout;
                    if let Some(slot_info) = find_slot_by_name(ctx.db, frame_layout, binding_name) {
                        let slot_offset = slot_info.offset(ctx.db) as usize;
                        let slot_ptr = unsafe {
                            ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                        };

                        let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };
                        let inner_size = inner_tydesc.size() as usize;

                        unsafe {
                            std::ptr::copy_nonoverlapping(payload_ptr, slot_ptr, inner_size);
                        }

                        let slot_index = frame_layout.slots(ctx.db)
                            .iter()
                            .position(|s| s.slot_id(ctx.db) == slot_info.slot_id(ctx.db))
                            .unwrap_or(0);
                        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Available;
                    }
                }
            }

            Ok(is_some)
        }
        TyTag::Result => {
            let tag = unsafe { *(value.ptr as *const u8) };
            let is_ok = tag == ResultTag::Ok as u8;

            if is_ok {
                if let Some(binding_name) = then_binding {
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = compute_result_layout(tydesc_ref);
                    let ok_tydesc = tydesc_ref.result_ok_ty();

                    let frame_index = ctx.call_stack.len() - 1;
                    let frame_layout = ctx.call_stack[frame_index].layout;
                    if let Some(slot_info) = find_slot_by_name(ctx.db, frame_layout, binding_name) {
                        let slot_offset = slot_info.offset(ctx.db) as usize;
                        let slot_ptr = unsafe {
                            ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                        };

                        let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };
                        let ok_size = ok_tydesc.size() as usize;
                        unsafe {
                            std::ptr::copy_nonoverlapping(payload_ptr, slot_ptr, ok_size);
                        }

                        let slot_index = frame_layout.slots(ctx.db)
                            .iter()
                            .position(|s| s.slot_id(ctx.db) == slot_info.slot_id(ctx.db))
                            .unwrap_or(0);
                        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Available;
                    }
                }
            } else {
                if let Some(binding_name) = else_binding {
                    let tydesc_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
                    let layout = compute_result_layout(tydesc_ref);

                    let frame_index = ctx.call_stack.len() - 1;
                    let frame_layout = ctx.call_stack[frame_index].layout;
                    if let Some(slot_info) = find_slot_by_name(ctx.db, frame_layout, binding_name) {
                        let slot_offset = slot_info.offset(ctx.db) as usize;
                        let slot_ptr = unsafe {
                            ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(slot_offset)
                        };

                        let payload_ptr = unsafe { value.ptr.add(layout.payload_offset as usize) };
                        let error_size = std::mem::size_of::<datalove_rt::rtdt::Error>();
                        unsafe {
                            std::ptr::copy_nonoverlapping(payload_ptr, slot_ptr, error_size);
                        }

                        let slot_index = frame_layout.slots(ctx.db)
                            .iter()
                            .position(|s| s.slot_id(ctx.db) == slot_info.slot_id(ctx.db))
                            .unwrap_or(0);
                        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Available;
                    }
                }
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
