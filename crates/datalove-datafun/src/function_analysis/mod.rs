//! Function analysis for linear type system.
//!
//! This module implements per-function analysis that provides:
//! - Packed frame layout with computed offsets for all slots
//! - Liveness ranges for each slot
//! - Move and borrow tracking
//! - Drop point insertion

use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::StmtFun;

mod layout;
mod liveness;
mod moves;
mod drops;
mod slot_allocation;
mod type_sizing;

pub use layout::*;
pub use liveness::*;
pub use moves::*;
pub use drops::*;
pub use slot_allocation::*;
pub use type_sizing::*;

/// Complete analysis result for a function.
#[salsa::tracked]
pub struct FunctionAnalysis<'db> {
    pub function: StmtFun<'db>,
    pub frame_layout: FrameLayout<'db>,
    pub live_ranges: LiveRanges<'db>,
    pub move_info: MoveInfo<'db>,
    pub drop_points: DropPoints<'db>,
}

/// Unique identifier for a slot in the frame.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct SlotId(pub u32);

/// Unique identifier for a statement.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct StmtId(pub u32);

/// Unique identifier for an expression.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct ExprId(pub u32);

/// Unique identifier for a basic block.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct BlockId(pub u32);

/// Kind of slot in the frame.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum SlotKind {
    Parameter,
    Local,
    Temporary,
}

/// Position within a statement (before or after).
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum Position {
    Before,
    After,
}

/// Point in the program for liveness tracking.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub struct ProgramPoint {
    pub stmt_id: StmtId,
    pub position: Position,
}

/// Initialization state for a slot.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum InitState {
    Always,      // definitely initialized
    Sometimes,   // conditionally initialized (if-branches)
    Never,       // never initialized on this path
}
