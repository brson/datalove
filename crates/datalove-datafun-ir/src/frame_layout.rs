//! Frame layout of a compiled function.
//!
//! Where each parameter, value and slot of a code unit lives in its frame, and
//! where the tracking bytes go. Every backend lays its frames out from this:
//! the cranelift and C backends, and the interpreter, whose `IrLayout` takes
//! its offsets from here and keeps its parameter pointers elsewhere, leaving
//! their space unused. So a frame the interpreter is running holds its values,
//! slots and tracking bytes where compiled code for the same body would look
//! for them.
//!
//! Sizes come from `layout::layout_of`, so a value occupies the same bytes here
//! as it does anywhere else.

use crate::layout::{align_up, layout_of, TypeLayout};
use crate::{IrType, ParamId, SlotId};

/// Layout information for a single parameter, value or slot.
#[derive(Debug, Clone)]
pub struct SlotLayout {
    /// Offset from frame base.
    pub offset: u32,
    /// Size in bytes.
    pub size: u32,
    /// Alignment requirement.
    pub align: u32,
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
    /// Layout order: params, values, slots, tracking bytes. Tracked slots and
    /// tracked params each get a tracking byte; values are precise and get none.
    pub fn compute(
        param_types: &[IrType],
        value_types: &[IrType],
        slot_types: &[IrType],
        tracked_slots: &[SlotId],
        tracked_params: &[ParamId],
    ) -> Self {
        let mut offset = 0u32;
        let mut max_align = 1u32;
        let mut place = |layout: TypeLayout| {
            offset = align_up(offset, layout.align);
            let slot = SlotLayout {
                offset,
                size: layout.size,
                align: layout.align,
                tracking_byte: None,
            };
            offset += layout.size;
            max_align = max_align.max(layout.align);
            slot
        };

        // A parameter is a pointer to the caller's data, whatever its type.
        let ptr = layout_of(&IrType::Ref(Box::new(IrType::Unit)));
        let mut params: Vec<SlotLayout> = param_types.iter().map(|_| place(ptr)).collect();
        let values: Vec<SlotLayout> = value_types.iter().map(|ty| place(layout_of(ty))).collect();
        let mut slots: Vec<SlotLayout> = slot_types.iter().map(|ty| place(layout_of(ty))).collect();

        let tracking_offset = offset;
        let slot_tracking_count = tracked_slots.len() as u32;
        let tracking_count = slot_tracking_count + tracked_params.len() as u32;

        for (i, &sid) in tracked_slots.iter().enumerate() {
            slots[sid.0 as usize].tracking_byte = Some(tracking_offset + i as u32);
        }

        // Tracked params' bytes come after the slots'.
        let param_tracking_base = tracking_offset + slot_tracking_count;
        for (i, &pid) in tracked_params.iter().enumerate() {
            params[pid.0 as usize].tracking_byte = Some(param_tracking_base + i as u32);
        }

        offset += tracking_count;

        // At least 1, so there is always a valid address in the frame, which
        // debuglog of a zero-size type such as unit needs.
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

    /// Get the tracking byte offset for a value, if tracked.
    pub fn value_tracking_byte(&self, idx: u32) -> Option<u32> {
        self.values[idx as usize].tracking_byte
    }

    /// Get the tracking byte offset for a slot, if tracked.
    pub fn slot_tracking_byte(&self, idx: u32) -> Option<u32> {
        self.slots[idx as usize].tracking_byte
    }

    /// Get the tracking byte offset for a param, if tracked.
    pub fn param_tracking_byte(&self, idx: u32) -> Option<u32> {
        self.params[idx as usize].tracking_byte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_frame() {
        let layout = FrameLayout::compute(&[], &[], &[], &[], &[]);
        // Frame size is always at least 1 for debuglog of zero-size types.
        assert_eq!(layout.frame_size, 1);
        assert_eq!(layout.frame_align, 1);
    }

    #[test]
    fn test_single_value() {
        let layout = FrameLayout::compute(&[], &[IrType::U32], &[], &[], &[]);
        assert_eq!(layout.values.len(), 1);
        assert_eq!(layout.values[0].offset, 0);
        assert_eq!(layout.values[0].size, 4);
        assert_eq!(layout.frame_size, 4);
    }

    #[test]
    fn test_multiple_values_alignment() {
        // u8 at 0, u64 needs alignment to 8.
        let layout = FrameLayout::compute(&[], &[IrType::U8, IrType::U64], &[], &[], &[]);
        assert_eq!(layout.values[0].offset, 0);
        assert_eq!(layout.values[1].offset, 8); // Aligned to 8.
        assert_eq!(layout.frame_size, 16);
        assert_eq!(layout.frame_align, 8);
    }

    #[test]
    fn test_params_are_pointers() {
        let layout = FrameLayout::compute(&[IrType::U32, IrType::String], &[], &[], &[], &[]);
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
            &[],
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
