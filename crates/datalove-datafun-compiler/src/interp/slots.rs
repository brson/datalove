//! Operand evaluation and slot access helpers.
//!
//! Provides building blocks for expression evaluation:
//! - `Operand`: A value for binop/unop evaluation (variable ref or temp)
//! - Temp slot management: allocation, state tracking
//! - Slot access: reading values from frame slots

use super::{
    InterpContext, InterpError, Value, Destination,
    eval_expression_frame,
};
use super::memory::destroy_value;
use super::frame::SlotState;
use crate::function_analysis::SlotId;

/// An operand value for binop/unop evaluation.
///
/// For variable references, points directly to the variable's slot (no clone).
/// For compound expressions, points to a temp slot holding the evaluated result.
pub(super) struct Operand<'db> {
    pub value: Value,
    /// Whether cleanup is needed after use (true for compound exprs, false for variable refs).
    pub needs_cleanup: bool,
    /// The expression, used for marking temp slot as moved during cleanup.
    pub expr: crate::ast::ExprFun<'db>,
}

/// Evaluate an operand for binop/unop.
///
/// For variable references, returns a Value pointing directly to the slot (no clone).
/// For compound expressions, evaluates to a temp slot and returns a Value pointing there.
pub(super) fn eval_operand<'db>(
    ctx: &mut InterpContext<'db>,
    expr: crate::ast::ExprFun<'db>,
) -> Result<Operand<'db>, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;

    match expr.expr(ctx.db) {
        crate::ast::ExprFunKind::Name(name) => {
            // Find slot by resolved slot ID.
            let layout = ctx.call_stack[frame_index].layout;
            let slot_info = layout.get_slot_for_name_expr(ctx.db, expr)
                .ok_or_else(|| InterpError::VariableNotFound(name.text(ctx.db).to_string()))?;

            let slot_id = slot_info.slot_id(ctx.db);

            // Check slot state (debug-only).
            #[cfg(debug_assertions)]
            if ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] == SlotState::Moved {
                return Err(InterpError::UseAfterMove(name.text(ctx.db).to_string()));
            }

            // Get cached tydesc for this slot.
            let tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);

            // Get pointer to value (reference or local slot).
            let ptr = if slot_info.kind(ctx.db) == crate::function_analysis::SlotKind::Reference {
                read_reference_slot(&ctx.call_stack[frame_index], slot_info, ctx.db)
            } else {
                let offset = slot_info.offset(ctx.db) as usize;
                unsafe { ctx.call_stack[frame_index].frame_data.as_ptr().add(offset) as *mut u8 }
            };

            // Return Value pointing to slot. DO NOT mark as moved.
            Ok(Operand {
                value: Value { ptr, tydesc },
                needs_cleanup: false,
                expr,
            })
        }
        _ => {
            // Compound expression: evaluate to its temp slot.
            let temp_dest = get_destination_for_expr(ctx, expr)?;
            eval_expression_frame(ctx, expr, temp_dest)?;
            mark_temp_slot_available(ctx, expr);
            Ok(Operand {
                value: temp_dest.to_value(),
                needs_cleanup: true,
                expr,
            })
        }
    }
}

/// Cleanup an operand after use if needed.
pub(super) fn cleanup_operand<'db>(ctx: &mut InterpContext<'db>, operand: Operand<'db>) {
    if operand.needs_cleanup {
        destroy_value(ctx, operand.value);
        mark_temp_slot_moved(ctx, operand.expr);
    }
}

/// Get a Destination for an expression's temporary slot.
pub(super) fn get_destination_for_expr<'db>(
    ctx: &mut InterpContext<'db>,
    expr: crate::ast::ExprFun<'db>,
) -> Result<Destination, InterpError> {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    let slot_info = layout.get_temp_slot_for_expr(ctx.db, expr)
        .ok_or_else(|| InterpError::RuntimeError(
            "No temp slot allocated for expression".to_string()
        ))?;

    let slot_id = slot_info.slot_id(ctx.db);
    let offset = slot_info.offset(ctx.db) as usize;
    let ptr = unsafe {
        ctx.call_stack[frame_index].frame_data.as_mut_ptr().add(offset)
    };
    let tydesc = get_slot_tydesc(&ctx.call_stack[frame_index], slot_id);
    Ok(Destination { ptr, tydesc })
}

/// Mark a temporary slot as Available after writing a value to it.
pub(super) fn mark_temp_slot_available<'db>(ctx: &mut InterpContext<'db>, expr: crate::ast::ExprFun<'db>) {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    if let Some(slot_info) = layout.get_temp_slot_for_expr(ctx.db, expr) {
        let slot_id = slot_info.slot_id(ctx.db);
        ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Available;
    }
}

/// Mark a temporary slot as Moved after its contents have been destroyed.
///
/// This prevents cleanup_frame from trying to destroy the slot again.
pub(super) fn mark_temp_slot_moved<'db>(ctx: &mut InterpContext<'db>, expr: crate::ast::ExprFun<'db>) {
    let frame_index = ctx.call_stack.len() - 1;
    let layout = ctx.call_stack[frame_index].layout;

    if let Some(slot_info) = layout.get_temp_slot_for_expr(ctx.db, expr) {
        let slot_id = slot_info.slot_id(ctx.db);
        ctx.call_stack[frame_index].slot_states[slot_id.0 as usize] = SlotState::Moved;
    }
}

/// Read a pointer value from a Reference slot (parameter).
pub(super) fn read_reference_slot<'db>(
    frame: &super::StackFrame<'db>,
    slot_info: crate::function_analysis::SlotInfo<'db>,
    db: &'db dyn crate::Db,
) -> *mut u8 {
    let offset = slot_info.offset(db) as usize;
    let ptr_bytes = &frame.frame_data[offset..offset + std::mem::size_of::<usize>()];
    let ptr_value = usize::from_ne_bytes(ptr_bytes.try_into().unwrap());
    ptr_value as *mut u8
}

/// Get the cached tydesc for a slot from the stack frame.
#[inline]
pub(super) fn get_slot_tydesc<'db>(
    frame: &super::StackFrame<'db>,
    slot_id: SlotId,
) -> *const datalove_datalit::rtdt::TyDesc {
    frame.slot_tydescs[slot_id.0 as usize]
}
