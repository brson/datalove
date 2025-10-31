//! Liveness analysis for slot tracking.

use rmx::prelude::*;
use super::{SlotId, ProgramPoint, InitState};

/// Live ranges for all slots.
#[salsa::tracked]
pub struct LiveRanges<'db> {
    #[returns(ref)]
    pub ranges: Vec<LiveRange<'db>>,
}

/// Live range for a single slot.
#[salsa::tracked]
pub struct LiveRange<'db> {
    pub slot_id: SlotId,
    pub birth: ProgramPoint,    // where value is written
    pub death: ProgramPoint,    // last use
    pub is_initialized: InitState,
}

impl<'db> LiveRanges<'db> {
    /// Get the live range for a slot.
    pub fn get_range(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Option<LiveRange<'db>> {
        self.ranges(db).iter().find(|r| r.slot_id(db) == slot_id).copied()
    }
}
