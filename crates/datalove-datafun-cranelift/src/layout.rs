//! Frame layout computation for Cranelift compilation.
//!
//! Computes stack slot offsets for values and mutable slots within a function frame,
//! matching the interpreter's layout for ABI compatibility.

use datalove_datafun_ir::{IrType, SlotId};
use crate::types::{self, align_up, TypeLayout, CraneliftRepr};

/// Layout information for a single value or slot.
#[derive(Debug, Clone)]
pub struct SlotLayout {
    /// Offset from frame base.
    pub offset: u32,
    /// Size in bytes.
    pub size: u32,
    /// Alignment requirement.
    pub align: u32,
    /// Cranelift representation.
    pub repr: CraneliftRepr,
    /// Offset of tracking byte, if this value/slot is tracked.
    pub tracking_byte: Option<u32>,
}

/// Sentinel values for tracking bytes.
pub mod tracking {
    /// Pre-initialized / never written.
    pub const UNINIT: u8 = 0x00;
    /// Currently holds a valid value.
    pub const LIVE: u8 = 0x01;
    /// Was moved out / dropped.
    pub const MOVED: u8 = 0x02;
}

/// Layout information for a function/unit frame.
///
/// Maps ValueId/SlotId to stack slot offsets. This matches the interpreter's
/// `IrLayout` for ABI compatibility.
#[derive(Debug)]
pub struct FrameLayout {
    /// Layout for each ValueId.
    pub values: Vec<SlotLayout>,
    /// Layout for each SlotId (mutable slots).
    pub slots: Vec<SlotLayout>,
    /// Layout for each ParamId.
    pub params: Vec<SlotLayout>,
    /// Total frame size in bytes.
    pub frame_size: u32,
    /// Frame alignment requirement.
    pub frame_align: u32,
    /// Offset of tracking bytes region within frame.
    pub tracking_offset: u32,
    /// Number of tracking bytes (for initialization).
    pub tracking_count: u32,
}

impl FrameLayout {
    /// Compute frame layout from IR type arrays.
    ///
    /// Layout order: params, values, slots, tracking bytes.
    /// Only slots (var bindings) need tracking bytes - values are always precise.
    pub fn compute(
        param_types: &[IrType],
        value_types: &[IrType],
        slot_types: &[IrType],
        tracked_slots: &[SlotId],
    ) -> Self {
        let mut params = Vec::with_capacity(param_types.len());
        let mut values = Vec::with_capacity(value_types.len());
        let mut slots = Vec::with_capacity(slot_types.len());

        let mut offset = 0u32;
        let mut max_align = 1u32;

        // Layout params first (pointers to caller's data).
        // Params are stored as pointers regardless of their underlying type.
        for ty in param_types {
            let repr = types::ir_type_to_cranelift(ty);

            offset = align_up(offset, types::PTR_ALIGN);
            params.push(SlotLayout {
                offset,
                size: types::PTR_SIZE,
                align: types::PTR_ALIGN,
                repr,
                tracking_byte: None,
            });
            offset += types::PTR_SIZE;
            max_align = max_align.max(types::PTR_ALIGN);
        }

        // Layout values (no tracking bytes - all values are precise).
        for ty in value_types {
            let repr = types::ir_type_to_cranelift(ty);
            let TypeLayout { size, align } = repr.layout();

            offset = align_up(offset, align);
            values.push(SlotLayout {
                offset,
                size,
                align,
                repr,
                tracking_byte: None,
            });
            offset += size;
            max_align = max_align.max(align);
        }

        // Layout mutable slots.
        for ty in slot_types {
            let repr = types::ir_type_to_cranelift(ty);
            let TypeLayout { size, align } = repr.layout();

            offset = align_up(offset, align);
            slots.push(SlotLayout {
                offset,
                size,
                align,
                repr,
                tracking_byte: None,
            });
            offset += size;
            max_align = max_align.max(align);
        }

        // Layout tracking bytes region.
        // Only slots (var bindings) need tracking bytes.
        let tracking_offset = offset;
        let tracking_count = tracked_slots.len() as u32;

        // Assign tracking byte offsets to tracked slots.
        for (i, &sid) in tracked_slots.iter().enumerate() {
            slots[sid.0 as usize].tracking_byte = Some(tracking_offset + i as u32);
        }

        offset += tracking_count;

        // Ensure frame_size is at least 1 so we always have a valid frame slot.
        // This is needed for zero-size types like Unit that still need a valid
        // address for debuglog.
        let frame_size = align_up(offset, max_align).max(1);

        Self {
            params,
            values,
            slots,
            frame_size,
            frame_align: max_align,
            tracking_offset,
            tracking_count,
        }
    }

    /// Get the offset for a value by index.
    pub fn value_offset(&self, idx: u32) -> u32 {
        self.values[idx as usize].offset
    }

    /// Get the offset for a slot by index.
    pub fn slot_offset(&self, idx: u32) -> u32 {
        self.slots[idx as usize].offset
    }

    /// Get the offset for a param by index.
    pub fn param_offset(&self, idx: u32) -> u32 {
        self.params[idx as usize].offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_frame() {
        let layout = FrameLayout::compute(&[], &[], &[], &[]);
        // Frame size is always at least 1 for debuglog of zero-size types.
        assert_eq!(layout.frame_size, 1);
        assert_eq!(layout.frame_align, 1);
    }

    #[test]
    fn test_single_value() {
        let layout = FrameLayout::compute(&[], &[IrType::U32], &[], &[]);
        assert_eq!(layout.values.len(), 1);
        assert_eq!(layout.values[0].offset, 0);
        assert_eq!(layout.values[0].size, 4);
        assert_eq!(layout.frame_size, 4);
    }

    #[test]
    fn test_multiple_values_alignment() {
        // u8 at 0, u64 needs alignment to 8.
        let layout = FrameLayout::compute(&[], &[IrType::U8, IrType::U64], &[], &[]);
        assert_eq!(layout.values[0].offset, 0);
        assert_eq!(layout.values[1].offset, 8); // Aligned to 8.
        assert_eq!(layout.frame_size, 16);
        assert_eq!(layout.frame_align, 8);
    }

    #[test]
    fn test_params_are_pointers() {
        let layout = FrameLayout::compute(&[IrType::U32, IrType::String], &[], &[], &[]);
        assert_eq!(layout.params.len(), 2);
        // All params are pointers (8 bytes each).
        assert_eq!(layout.params[0].size, 8);
        assert_eq!(layout.params[1].size, 8);
        assert_eq!(layout.params[0].offset, 0);
        assert_eq!(layout.params[1].offset, 8);
    }

    #[test]
    fn test_values_and_slots() {
        let layout = FrameLayout::compute(
            &[],
            &[IrType::U32],
            &[IrType::U64],
            &[],
        );
        assert_eq!(layout.values[0].offset, 0);
        assert_eq!(layout.slots[0].offset, 8); // After u32, aligned to 8.
    }

    #[test]
    fn test_frame_with_aggregate_types() {
        use datalove_rtdt::String as RtString;
        let string_size = std::mem::size_of::<RtString>() as u32;

        let layout = FrameLayout::compute(
            &[],
            &[IrType::String, IrType::U32],
            &[],
            &[],
        );
        // String size depends on index-64 feature, u32 is 4 bytes.
        assert_eq!(layout.values[0].offset, 0);
        assert_eq!(layout.values[0].size, string_size);
        assert_eq!(layout.values[1].offset, string_size);
        assert_eq!(layout.values[1].size, 4);
        // Frame size aligned to 8.
        let expected_frame_size = ((string_size + 4 + 7) / 8) * 8;
        assert_eq!(layout.frame_size, expected_frame_size);
    }

    #[test]
    fn test_tracking_bytes_layout() {
        // Frame with 2 values and 1 slot, track the slot.
        // Values are always precise - no tracking bytes for values.
        let tracked_slots = vec![SlotId(0)];
        let layout = FrameLayout::compute(
            &[],
            &[IrType::U32, IrType::U32],
            &[IrType::U64],
            &tracked_slots,
        );
        // Values: 0..4, 4..8; slot: 8..16; tracking: 16..17.
        assert_eq!(layout.tracking_offset, 16);
        assert_eq!(layout.tracking_count, 1);
        assert_eq!(layout.values[0].tracking_byte, None);
        assert_eq!(layout.values[1].tracking_byte, None);
        assert_eq!(layout.slots[0].tracking_byte, Some(16));
        // Frame size should include tracking bytes, aligned to 8.
        assert_eq!(layout.frame_size, 24);
    }
}
