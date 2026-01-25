//! Frame layout computation.
//!
//! Computes byte offsets for values and slots within a frame buffer,
//! respecting alignment requirements from type descriptors.

use datalove_rtdt::TyDesc;
use datalove_datafun_ir::IrType;
use crate::tydesc::IrTyDescTable;

/// Align a value up to the given alignment.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Layout information for a function/unit frame.
///
/// Maps ValueId/SlotId to byte offsets within the frame.
pub struct IrLayout {
    /// Offset for each ValueId.
    pub value_offsets: Vec<u32>,
    /// TyDesc for each ValueId.
    pub value_tydescs: Vec<*const TyDesc>,
    /// Which values are references (statically known from IrType::Ref).
    pub value_is_ref: Vec<bool>,
    /// Offset for each SlotId.
    pub slot_offsets: Vec<u32>,
    /// TyDesc for each SlotId.
    pub slot_tydescs: Vec<*const TyDesc>,
    /// Total frame size.
    pub frame_size: u32,
    /// Frame alignment.
    pub frame_align: u32,
}

impl IrLayout {
    /// Compute layout from IrType arrays.
    pub fn compute(
        value_types: &[IrType],
        slot_types: &[IrType],
        tydesc_table: &mut IrTyDescTable,
    ) -> Self {
        let mut value_offsets = Vec::with_capacity(value_types.len());
        let mut value_tydescs = Vec::with_capacity(value_types.len());
        let mut value_is_ref = Vec::with_capacity(value_types.len());
        let mut slot_offsets = Vec::with_capacity(slot_types.len());
        let mut slot_tydescs = Vec::with_capacity(slot_types.len());

        let mut offset: u32 = 0;
        let mut max_align: u32 = 1;

        // Layout values first.
        for ty in value_types {
            let tydesc = tydesc_table.get_or_create(ty);
            let size = unsafe { (*tydesc).size };
            let align = unsafe { (*tydesc).align };

            offset = align_up(offset, align);
            value_offsets.push(offset);
            value_tydescs.push(tydesc);
            value_is_ref.push(matches!(ty, IrType::Ref(_)));
            offset += size;
            max_align = max_align.max(align);
        }

        // Then slots.
        for ty in slot_types {
            let tydesc = tydesc_table.get_or_create(ty);
            let size = unsafe { (*tydesc).size };
            let align = unsafe { (*tydesc).align };

            offset = align_up(offset, align);
            slot_offsets.push(offset);
            slot_tydescs.push(tydesc);
            offset += size;
            max_align = max_align.max(align);
        }

        // Final alignment for frame size.
        let frame_size = align_up(offset, max_align);

        Self {
            value_offsets,
            value_tydescs,
            value_is_ref,
            slot_offsets,
            slot_tydescs,
            frame_size,
            frame_align: max_align,
        }
    }
}
