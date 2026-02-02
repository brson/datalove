//! Frame layout computation for C code generation.

use datalove_datafun_ir::{IrType, ParamId, SlotId};
use crate::types::{self, align_up, CRepr, TypeLayout};

/// Layout information for a single value or slot.
#[derive(Debug, Clone)]
pub struct SlotLayout {
    /// Offset from frame base.
    pub offset: u32,
    /// Size in bytes.
    pub size: u32,
    /// Alignment requirement.
    pub align: u32,
    /// C representation.
    pub repr: CRepr,
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
    /// Layout order: params, values, slots, tracking bytes.
    pub fn compute(
        param_types: &[IrType],
        value_types: &[IrType],
        slot_types: &[IrType],
        tracked_slots: &[SlotId],
        tracked_params: &[ParamId],
    ) -> Self {
        let mut params = Vec::with_capacity(param_types.len());
        let mut values = Vec::with_capacity(value_types.len());
        let mut slots = Vec::with_capacity(slot_types.len());

        let mut offset = 0u32;
        let mut max_align = 1u32;

        // Layout params first (pointers to caller's data).
        for ty in param_types {
            let repr = types::ir_type_to_crepr(ty);

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

        // Layout values.
        for ty in value_types {
            let repr = types::ir_type_to_crepr(ty);
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
            let repr = types::ir_type_to_crepr(ty);
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
        let tracking_offset = offset;
        let slot_tracking_count = tracked_slots.len() as u32;
        let param_tracking_count = tracked_params.len() as u32;
        let tracking_count = slot_tracking_count + param_tracking_count;

        // Assign tracking byte offsets to tracked slots.
        for (i, &sid) in tracked_slots.iter().enumerate() {
            slots[sid.0 as usize].tracking_byte = Some(tracking_offset + i as u32);
        }

        // Assign tracking byte offsets to tracked params (after slots).
        let param_tracking_base = tracking_offset + slot_tracking_count;
        for (i, &pid) in tracked_params.iter().enumerate() {
            params[pid.0 as usize].tracking_byte = Some(param_tracking_base + i as u32);
        }

        offset += tracking_count;

        // Ensure frame_size is at least 1.
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
