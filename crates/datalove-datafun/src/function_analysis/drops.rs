//! Drop point insertion for resource management.

use rmx::prelude::*;
use super::{SlotId, ProgramPoint};

/// Drop points for all slots.
#[salsa::tracked]
pub struct DropPoints<'db> {
    #[returns(ref)]
    pub drops: Vec<DropPoint<'db>>,
}

/// A single drop point.
#[salsa::tracked]
pub struct DropPoint<'db> {
    pub slot_id: SlotId,
    pub location: ProgramPoint,
    pub reason: DropReason,
}

/// Reason for dropping a value.
#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug)]
pub enum DropReason {
    EndOfScope,
    EarlyReturn,
    Moved,           // slot was moved, no drop needed
    Uninitialized,   // slot never initialized, no drop needed
}

impl<'db> DropPoints<'db> {
    /// Get all drop points for a specific slot.
    pub fn drops_for_slot(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Vec<DropPoint<'db>> {
        self.drops(db).iter().filter(|d| d.slot_id(db) == slot_id).copied().collect()
    }
}
