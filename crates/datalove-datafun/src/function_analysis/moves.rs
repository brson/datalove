//! Move tracking for linear type system.

use rmx::prelude::*;
use super::{SlotId, ExprId};

/// Move information for all operations.
#[salsa::tracked]
pub struct MoveInfo<'db> {
    #[returns(ref)]
    pub moves: Vec<MoveOp<'db>>,
    #[returns(ref)]
    pub last_uses: Vec<(SlotId, ExprId)>,
}

/// A single move operation.
#[salsa::tracked]
pub struct MoveOp<'db> {
    pub expr_id: ExprId,
    pub slot_id: SlotId,
    pub move_kind: MoveKind,
}

/// Kind of move operation.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum MoveKind {
    FunctionCall,      // argument moved to callee
    FunctionReturn,    // return value moved to caller
    Assignment,        // let binding consumes value
    LastUse,          // last use optimization
    Copy,             // automatic copy for scalar types (bool, u32, i32, f32)
    // Clone will be added later for explicit .clone() operations
}

impl<'db> MoveInfo<'db> {
    /// Get all moves for a specific slot.
    pub fn moves_for_slot(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Vec<MoveOp<'db>> {
        self.moves(db).iter().filter(|m| m.slot_id(db) == slot_id).copied().collect()
    }

    /// Check if an expression is a last use of a slot.
    pub fn is_last_use(self, db: &'db dyn crate::Db, slot_id: SlotId, expr_id: ExprId) -> bool {
        self.last_uses(db).contains(&(slot_id, expr_id))
    }
}
