//! Stack frame management for function execution.
//!
//! Each function call allocates a `StackFrame` with a byte buffer for slot storage.
//! Slots are laid out by `function_analysis::FrameLayout` with computed offsets.

use crate::function_analysis::{ControlFlowGraph, DropPoints, FrameLayout};
use crate::ast;
use super::Value;

/// Slot state for frame-based execution.
///
/// Tracks whether a slot is available for use or has been moved from.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// Slot is available for reading (either reference or owned value).
    Available,
    /// Slot has been moved from (only applicable to non-copy types).
    Moved,
}

/// Control flow result from executing a CFG statement.
pub(super) enum CfgControl {
    /// Continue to the next statement in the block.
    Continue,
    /// Return from the function with a value.
    Return(Value),
}

/// Stack frame for function execution.
///
/// Contains the packed frame data buffer and per-slot state tracking.
pub struct StackFrame<'db> {
    /// Packed frame data containing all slot values at computed offsets.
    pub frame_data: Vec<u8>,
    /// Per-slot state tracking for move semantics.
    pub slot_states: Vec<SlotState>,
    /// Function being executed (for debugging).
    pub func: ast::StmtFun<'db>,
    /// Frame layout providing slot offsets and types.
    pub layout: FrameLayout<'db>,
    /// Control flow graph for CFG-based execution.
    pub cfg: ControlFlowGraph<'db>,
    /// Analysis-computed drop points for cleanup.
    pub drop_points: DropPoints<'db>,
}
