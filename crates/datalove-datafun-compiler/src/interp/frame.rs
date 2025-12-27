//! Stack frame for function execution.
//!
//! Each function call creates a `StackFrame` with a byte buffer for slot storage.
//! Slots are laid out by `function_analysis::FrameLayout` with computed offsets.

use crate::function_analysis::{ControlFlowGraph, DropPoints, FrameLayout, SlotId};
use crate::tycheck::TypeAndHeap;
use super::{Destination, Value};

/// Slot lifecycle state for cleanup decisions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// Release-only: slot doesn't need state tracking.
    Untracked,
    /// Not yet written; skip cleanup.
    Uninitialized,
    /// Valid data; needs cleanup if not moved.
    Available,
    /// Moved or destroyed; skip cleanup.
    Moved,
}

/// CFG statement result.
pub(super) enum CfgControl {
    Continue,
    Return(Value),
}

/// Context for what's being executed in this frame.
///
/// Abstracts away function-specific details so the interpreter can later
/// support script/REPL execution with the same frame-based model.
pub struct FrameContext<'db> {
    /// Name for error messages (function name, script unit name, etc).
    pub context_name: bct::text::InternedText<'db>,
    /// Return type (unit `()` for void functions, never None).
    pub return_type: TypeAndHeap<'db>,
}

/// Stack frame for function execution.
pub struct StackFrame<'db> {
    /// Packed slot data at computed offsets.
    pub frame_data: Vec<u8>,
    /// Per-slot state for move semantics. Use accessor methods for read/write.
    pub(super) slot_states: Vec<SlotState>,
    /// Cached tydescs for each slot, indexed by SlotId.
    pub slot_tydescs: Vec<*const datalove_datalit::rtdt::TyDesc>,
    /// Context for what's being executed (function name, return type, etc).
    pub context: FrameContext<'db>,
    /// Slot offsets and types.
    pub layout: FrameLayout<'db>,
    /// Control flow graph.
    pub cfg: ControlFlowGraph<'db>,
    /// Analysis-computed cleanup points.
    pub drop_points: DropPoints<'db>,
    /// Caller's return destination (all functions return a value).
    pub return_dest: Destination,
}

/// Set slot state in a slot_states vector (for use during frame construction).
///
/// Debug: always writes, asserts slot is not Untracked.
/// Release: skips write if slot is Untracked.
#[inline]
pub fn set_slot_state_vec(slot_states: &mut [SlotState], slot_id: SlotId, state: SlotState) {
    let idx = slot_id.0 as usize;
    #[cfg(debug_assertions)]
    {
        debug_assert_ne!(slot_states[idx], SlotState::Untracked,
            "Debug mode: slot {:?} should not be Untracked", slot_id);
        slot_states[idx] = state;
    }
    #[cfg(not(debug_assertions))]
    {
        if slot_states[idx] != SlotState::Untracked {
            slot_states[idx] = state;
        }
    }
}

/// Get slot state from a slot_states vector (for use after frame is popped).
///
/// Debug: returns actual state, asserts not Untracked.
/// Release: returns state (caller should only call for tracked slots).
#[inline]
pub fn get_slot_state_vec(slot_states: &[SlotState], slot_id: SlotId) -> SlotState {
    let state = slot_states[slot_id.0 as usize];
    debug_assert_ne!(state, SlotState::Untracked,
        "Attempted to read state of untracked slot {:?}", slot_id);
    state
}

impl<'db> StackFrame<'db> {
    /// Create slot_states vector based on tracking needs.
    ///
    /// Debug: all slots are Uninitialized (full tracking).
    /// Release: tracked slots are Uninitialized, untracked are Untracked.
    pub fn init_slot_states(slots: &[crate::function_analysis::SlotInfo<'db>], db: &'db dyn crate::Db) -> Vec<SlotState> {
        let mut slot_states = Vec::with_capacity(slots.len());
        for slot in slots {
            #[cfg(debug_assertions)]
            {
                let _ = slot.needs_state_tracking(db); // Suppress unused warning.
                slot_states.push(SlotState::Uninitialized);
            }
            #[cfg(not(debug_assertions))]
            if slot.needs_state_tracking(db) {
                slot_states.push(SlotState::Uninitialized);
            } else {
                slot_states.push(SlotState::Untracked);
            }
        }
        slot_states
    }

    /// Set slot state.
    ///
    /// Debug: always writes, asserts slot is not Untracked.
    /// Release: skips write if slot is Untracked.
    #[inline]
    pub fn set_slot_state(&mut self, slot_id: SlotId, state: SlotState) {
        let idx = slot_id.0 as usize;
        #[cfg(debug_assertions)]
        {
            debug_assert_ne!(self.slot_states[idx], SlotState::Untracked,
                "Debug mode: slot {:?} should not be Untracked", slot_id);
            self.slot_states[idx] = state;
        }
        #[cfg(not(debug_assertions))]
        {
            if self.slot_states[idx] != SlotState::Untracked {
                self.slot_states[idx] = state;
            }
        }
    }

    /// Get slot state.
    ///
    /// Debug: returns actual state, asserts not Untracked.
    /// Release: returns state (caller should only call for tracked slots).
    #[inline]
    pub fn get_slot_state(&self, slot_id: SlotId) -> SlotState {
        let state = self.slot_states[slot_id.0 as usize];
        debug_assert_ne!(state, SlotState::Untracked,
            "Attempted to read state of untracked slot {:?}", slot_id);
        state
    }
}
