//! Memory operations: clone, move, destroy, and cleanup.
//!
//! All values live in caller-owned memory (frame slots). These functions
//! handle cloning for copy semantics, moving for linear semantics, and
//! destroying heap-owned data (strings, collections) without freeing the
//! slot memory itself.
//!
//! Also includes frame cleanup logic using analysis-computed drop points.

use super::{InterpContext, InterpError, Value, Destination};
use super::frame::{StackFrame, SlotState};
use super::slots::get_slot_tydesc;
use crate::function_analysis::BlockId;

/// Deep-clone a value to a destination.
///
/// Copies the value and any heap-allocated data it owns. For compatible
/// type descriptors, uses the runtime clone. For structurally compatible
/// types with different tydesc pointers, uses memcpy.
pub(super) fn clone_value_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
    dest: Destination,
) -> Value {
    use datalove_rt::rtdt::TyDescRef;

    if value.tydesc == dest.tydesc {
        let rt_handle = ctx.runtime.handle();
        unsafe {
            datalove_rt::c::dtlv_rti_clone_local(rt_handle, value.ptr, value.tydesc, dest.ptr, dest.tydesc);
        }
    } else {
        let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
        let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

        if src_ref.type_tag() == dst_ref.type_tag() && src_ref.size() == dst_ref.size() {
            unsafe {
                std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, src_ref.size() as usize);
            }
        } else {
            panic!(
                "clone_value_to_dest: incompatible types {:?} (size {}) vs {:?} (size {})",
                src_ref.type_tag(), src_ref.size(),
                dst_ref.type_tag(), dst_ref.size()
            );
        }
    }
    Value { ptr: dest.ptr, tydesc: dest.tydesc }
}

/// Shallow-move a value to a destination.
///
/// Copies the slot bytes (including any heap pointers) without cloning
/// heap data. The source must be marked `Moved` afterward to prevent
/// double-free.
pub(super) fn move_value_to_dest(
    value: Value,
    dest: Destination,
) -> Value {
    use datalove_rt::rtdt::TyDescRef;

    let src_ref = unsafe { TyDescRef::from_ptr(value.tydesc) };
    let dst_ref = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    if src_ref.size() == dst_ref.size() {
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, src_ref.size() as usize);
        }
    } else {
        panic!(
            "move_value_to_dest: incompatible sizes {} vs {}",
            src_ref.size(), dst_ref.size()
        );
    }

    Value { ptr: dest.ptr, tydesc: dest.tydesc }
}

/// Destroy heap-owned data without freeing slot memory.
///
/// Frees strings, collections, and other heap data owned by the value.
/// The slot memory itself (in the frame buffer) is not freed.
pub fn destroy_value_contents_only<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );
    }
}

/// Destroy a value's heap-owned data.
///
/// Equivalent to `destroy_value_contents_only`. The slot memory is owned
/// by the frame and freed when the frame is dropped.
pub fn destroy_value<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
    unsafe {
        let rt_handle = ctx.runtime.handle();
        datalove_rt::c::dtlv_rti_any_destroy_local(
            rt_handle,
            value.ptr,
            value.tydesc,
        );
    }
}

/// No-op: slot memory is owned by the frame.
///
/// Retained for API compatibility. All values live in frame slots;
/// the frame buffer is freed when the frame is dropped.
#[allow(unused_variables)]
pub fn free_value_structure<'db>(
    ctx: &mut InterpContext<'db>,
    value: Value,
) {
}

// ============================================================================
// Frame Cleanup
// ============================================================================

/// Process block-exit drops when leaving a block via Goto.
///
/// This handles branch convergence: when a slot is moved in one branch but not another,
/// we drop it at the exit of the branch where it's not moved. This ensures the slot
/// is "consumed" on all paths to the join point.
pub(super) fn process_block_exit_drops<'db>(
    ctx: &mut InterpContext<'db>,
    block_id: BlockId,
) -> Result<(), InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let drop_points = ctx.call_stack[frame_index].drop_points;
    let layout = ctx.call_stack[frame_index].layout;
    let slots = layout.slots(ctx.db);

    // Use pre-indexed BranchExit drops for this block (O(1) lookup).
    for drop_point in drop_points.get_branch_exit_drops(ctx.db, block_id) {
        let slot_id = drop_point.slot_id(ctx.db);

        // Use slot_id.0 as the index into slots and slot_states.
        // Slots are allocated sequentially, so slot_id.0 equals position.
        let slot_index = slot_id.0 as usize;
        if slot_index >= slots.len() {
            continue;
        }
        let slot_info = &slots[slot_index];

        // For tracked slots (conditional init or move), check runtime state.
        // For non-tracked slots, static analysis guarantees correctness.
        if slot_info.needs_state_tracking(ctx.db) {
            if ctx.call_stack[frame_index].slot_states[slot_index] != SlotState::Available {
                continue;
            }
        }

        // Destroy the slot contents.
        let frame_data = &ctx.call_stack[frame_index].frame_data;
        let offset = slot_info.offset(ctx.db) as usize;
        let tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);
        let slot_ptr = unsafe { frame_data.as_ptr().add(offset) as *mut u8 };

        let value = Value { ptr: slot_ptr, tydesc };
        destroy_value_contents_only(ctx, value);

        // Mark slot as Moved so cleanup_frame doesn't try to drop it again.
        ctx.call_stack[frame_index].slot_states[slot_index] = SlotState::Moved;
    }

    Ok(())
}

/// Clean up a stack frame using analysis-computed drop points.
///
/// Uses drop_points as source of truth, combined with runtime slot_states
/// to handle conditional moves. Drop points identify non-copy, initialized, owned
/// slots that need cleanup. Runtime slot_states filter out slots that were actually
/// moved at runtime (handling conditional branches).
///
/// Temporaries are handled inline during evaluation:
/// - BinOp/UnaryOp operands: marked Moved after destroy_value
/// - If-condition temps: marked Moved after evaluate_branch_condition
/// - Data/Error wrapper temps: marked Moved after cloning to wrapper
pub(super) fn cleanup_frame<'db>(
    ctx: &mut InterpContext<'db>,
    frame: StackFrame<'db>,
    exit_block_id: BlockId,
) {
    let layout = frame.layout;
    let slots = layout.slots(ctx.db);
    let drop_points = frame.drop_points;

    // Use pre-indexed function exit drops for this specific return block.
    for drop_point in drop_points.get_function_exit_drops(ctx.db, exit_block_id) {
        let slot_id = drop_point.slot_id(ctx.db);

        // Use slot_id.0 as the index into slots and slot_states.
        // Slots are allocated sequentially, so slot_id.0 equals position.
        let slot_index = slot_id.0 as usize;
        if slot_index >= slots.len() {
            continue;
        }
        let slot_info = &slots[slot_index];

        // Verify slot_id matches position (debug check).
        debug_assert_eq!(
            slot_info.slot_id(ctx.db),
            slot_id,
            "Slot at position {} has id {:?}, expected {:?}",
            slot_index, slot_info.slot_id(ctx.db), slot_id
        );

        // For tracked slots (conditional init or move), check runtime state.
        // For non-tracked slots, static analysis guarantees correctness.
        if slot_info.needs_state_tracking(ctx.db) {
            if frame.slot_states[slot_index] != SlotState::Available {
                continue;
            }
        }

        destroy_slot_contents(ctx, &frame, slot_info, slot_id);
    }
}

/// Destroy the contents of a slot (helper for cleanup_frame).
fn destroy_slot_contents<'db>(
    ctx: &mut InterpContext<'db>,
    frame: &StackFrame<'db>,
    slot_info: &crate::function_analysis::SlotInfo<'db>,
    slot_id: crate::function_analysis::SlotId,
) {
    let offset = slot_info.offset(ctx.db) as usize;
    let tydesc = get_slot_tydesc(frame, slot_id);
    let slot_ptr = unsafe { frame.frame_data.as_ptr().add(offset) as *mut u8 };

    // Destroy the value contents only (not the structure itself).
    // The memory is part of the frame buffer and will be freed with the frame.
    let value = Value { ptr: slot_ptr, tydesc };
    destroy_value_contents_only(ctx, value);
}
