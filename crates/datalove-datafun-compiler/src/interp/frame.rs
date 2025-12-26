//! Stack frame for function execution.
//!
//! Each function call creates a `StackFrame` with a byte buffer for slot storage.
//! Slots are laid out by `function_analysis::FrameLayout` with computed offsets.

use crate::function_analysis::{ControlFlowGraph, DropPoints, FrameLayout, SlotId};
use crate::ast;
use super::{Destination, Value};

/// Slot lifecycle state for cleanup decisions.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SlotState {
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
    ReturnVoid,
}

/// Stack frame for function execution.
pub struct StackFrame<'db> {
    /// Packed slot data at computed offsets.
    pub frame_data: Vec<u8>,
    /// Per-slot state for move semantics.
    pub slot_states: Vec<SlotState>,
    /// Cached tydescs for each slot, indexed by SlotId.
    pub slot_tydescs: Vec<*const datalove_datalit::rtdt::TyDesc>,
    /// Function being executed.
    pub func: ast::StmtFun<'db>,
    /// Slot offsets and types.
    pub layout: FrameLayout<'db>,
    /// Control flow graph.
    pub cfg: ControlFlowGraph<'db>,
    /// Analysis-computed cleanup points.
    pub drop_points: DropPoints<'db>,
    /// Slots needing runtime state tracking.
    pub tracked_slots: Vec<SlotId>,
    /// Caller's return destination (None for void functions).
    pub return_dest: Option<Destination>,
}
