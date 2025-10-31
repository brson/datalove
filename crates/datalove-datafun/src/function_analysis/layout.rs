//! Frame layout computation.

use rmx::prelude::*;
use bct::text::InternedText;
use super::{SlotId, SlotKind};
use super::type_sizing::{compute_datafun_type_layout, TypeLayout};

/// Align a value up to the given alignment.
/// Alignment must be a power of 2.
#[inline]
fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Complete frame layout with all slots.
#[salsa::tracked]
pub struct FrameLayout<'db> {
    pub total_size: u32,
    pub total_align: u32,
    #[returns(ref)]
    pub slots: Vec<SlotInfo<'db>>,
}

/// Information about a single slot in the frame.
#[salsa::tracked]
pub struct SlotInfo<'db> {
    pub slot_id: SlotId,
    pub name: Option<InternedText<'db>>,  // None for temporaries
    pub kind: SlotKind,
    pub offset: u32,
    pub ty: crate::tycheck::TypeAndHeap<'db>,
}

impl<'db> FrameLayout<'db> {
    /// Get a slot by its ID.
    pub fn get_slot(self, db: &'db dyn crate::Db, slot_id: SlotId) -> Option<SlotInfo<'db>> {
        self.slots(db).iter().find(|s| s.slot_id(db) == slot_id).copied()
    }

    /// Compute frame layout from allocated slots with types.
    pub fn compute_layout(
        db: &'db dyn crate::Db,
        slots: Vec<(SlotId, Option<InternedText<'db>>, SlotKind, crate::tycheck::TypeAndHeap<'db>)>,
    ) -> Self {
        let mut offset = 0u32;
        let mut max_align = 1u32;
        let mut slot_infos = Vec::new();

        for (slot_id, name, kind, ty) in slots {
            // Compute size and alignment for this slot's type.
            let layout = compute_datafun_type_layout(db, ty);

            // Align offset to this slot's alignment requirement.
            offset = align_up(offset, layout.align);

            // Create slot info.
            let slot_info = SlotInfo::new(db, slot_id, name, kind, offset, ty);
            slot_infos.push(slot_info);

            // Advance offset by slot size.
            offset += layout.size;

            // Track maximum alignment.
            max_align = max_align.max(layout.align);
        }

        // Total size must be aligned to maximum alignment.
        let total_size = align_up(offset, max_align);

        FrameLayout::new(db, total_size, max_align, slot_infos)
    }
}
