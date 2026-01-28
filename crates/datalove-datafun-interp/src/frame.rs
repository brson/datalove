//! Execution frames and frame storage.
//!
//! A `Frame` holds all values and slots for a single function/unit execution.
//! `FrameStore` accumulates frames from script units for cross-unit value access.

use std::collections::HashSet;
use datalove_rt::rust::AlignedBuffer;
use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{ValueId, SlotId, ParamId};
use crate::layout::IrLayout;
use crate::value::{Value, Destination};

/// Execution frame for a function call.
pub struct Frame {
    /// Raw frame data with proper alignment (values and slots).
    data: AlignedBuffer,
    /// Layout information.
    layout: IrLayout,
    /// Track which values are live (written but not dropped).
    ///
    /// Only used for script frames that need cleanup tracking.
    /// Function frames set this to None to avoid hashing overhead.
    live_values: Option<HashSet<ValueId>>,
    /// Track which slots are initialized.
    slot_initialized: Vec<bool>,
    /// Pointers to caller's data for each parameter.
    param_ptrs: Vec<*mut u8>,
    /// Type descriptors for each parameter.
    param_tydescs: Vec<*const TyDesc>,
    /// Track which params are initialized (Out params start uninitialized).
    param_initialized: Vec<bool>,
}

impl Frame {
    /// Create a new frame for function execution (no live value tracking).
    pub fn new_function(layout: IrLayout, param_count: usize) -> Self {
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );

        Self {
            data,
            layout,
            live_values: None,
            slot_initialized: vec![false; slot_count],
            param_ptrs: vec![std::ptr::null_mut(); param_count],
            param_tydescs: vec![std::ptr::null(); param_count],
            param_initialized: vec![false; param_count],
        }
    }

    /// Create a new frame for script execution (with live value tracking).
    pub fn new_script(layout: IrLayout) -> Self {
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );

        Self {
            data,
            layout,
            live_values: Some(HashSet::new()),
            slot_initialized: vec![false; slot_count],
            param_ptrs: vec![std::ptr::null_mut(); 0],
            param_tydescs: vec![std::ptr::null(); 0],
            param_initialized: vec![false; 0],
        }
    }

    /// Mark a value as live (written).
    ///
    /// Called when a value is written to. Used to track which unit_end
    /// values need cleanup in destroy_live_values.
    /// No-op for function frames (live_values is None).
    pub fn mark_value_live(&mut self, id: ValueId) {
        if let Some(ref mut live_values) = self.live_values {
            live_values.insert(id);
        }
    }

    /// Get destination for a value.
    ///
    /// Panics if the value ID is out of bounds.
    pub fn value_dest(&mut self, id: ValueId) -> Destination {
        let idx = id.0 as usize;
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Destination { ptr, tydesc }
    }

    /// Get value (for reading).
    ///
    /// Returns the raw value without dereferencing. For ref values, this
    /// returns the stored pointer. Use `value_deref` to dereference refs.
    ///
    /// Returns None if value is not live (moved or not written).
    /// For function frames (no tracking), always returns the value.
    /// Panics if value ID is out of bounds (compiler bug).
    pub fn value(&self, id: ValueId) -> Option<Value> {
        let idx = id.0 as usize;
        if let Some(ref live_values) = self.live_values {
            if !live_values.contains(&id) {
                return None;
            }
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Some(Value { ptr, tydesc })
    }

    /// Dereference a ref value to get the pointed-to data.
    ///
    /// For values produced by GetFieldRef, reads the stored pointer and
    /// returns the data it points to with the inner type's tydesc.
    ///
    /// Returns None if value is not live (moved or not written).
    /// For function frames (no tracking), always returns the value.
    /// Panics if value ID is out of bounds (compiler bug).
    pub fn value_deref(&self, id: ValueId) -> Option<Value> {
        let idx = id.0 as usize;
        if let Some(ref live_values) = self.live_values {
            if !live_values.contains(&id) {
                return None;
            }
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };

        // Read the stored pointer.
        let stored_ptr = unsafe { *(ptr as *const *mut u8) };
        // Get inner tydesc from the ref's tydesc (stored as 1-element tuple).
        let inner_tydesc = unsafe {
            let tuple_info = (*tydesc).type_info.tuple;
            (*tuple_info.fields).tydesc
        };
        Some(Value { ptr: stored_ptr, tydesc: inner_tydesc })
    }

    /// Get destination for a slot.
    ///
    /// Panics if slot ID is out of bounds (compiler bug).
    pub fn slot_dest(&mut self, id: SlotId) -> Destination {
        let idx = id.0 as usize;
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Destination { ptr, tydesc }
    }

    /// Get slot value (for reading).
    ///
    /// Returns None if slot is not initialized (moved or not written).
    /// Panics if slot ID is out of bounds (compiler bug).
    pub fn slot(&self, id: SlotId) -> Option<Value> {
        let idx = id.0 as usize;
        if !self.slot_initialized[idx] {
            return None;
        }
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Some(Value { ptr, tydesc })
    }

    /// Mark slot as initialized.
    pub fn mark_slot_initialized(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if idx < self.slot_initialized.len() {
            self.slot_initialized[idx] = true;
        }
    }

    /// Check if slot is initialized.
    pub fn is_slot_initialized(&self, id: SlotId) -> bool {
        let idx = id.0 as usize;
        idx < self.slot_initialized.len() && self.slot_initialized[idx]
    }

    /// Check if value is initialized (live).
    ///
    /// Returns true if the value has been written and not yet dropped.
    /// For function frames (no tracking), always returns true.
    pub fn is_value_initialized(&self, id: ValueId) -> bool {
        match &self.live_values {
            Some(live_values) => live_values.contains(&id),
            None => true,
        }
    }

    /// Mark value as dropped to prevent double-destroy.
    ///
    /// No-op for function frames (live_values is None).
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        if let Some(ref mut live_values) = self.live_values {
            live_values.remove(&id);
        }
    }

    /// Mark slot as dropped to prevent double-destroy.
    pub fn mark_slot_dropped(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if idx < self.slot_initialized.len() {
            self.slot_initialized[idx] = false;
        }
    }

    /// Set a parameter with pointer to caller's data.
    ///
    /// `initialized`: true for In/Ref/Mut (data exists), false for Out (callee must write first).
    pub fn set_param(&mut self, id: ParamId, ptr: *mut u8, tydesc: *const TyDesc, initialized: bool) {
        let idx = id.0 as usize;
        if idx < self.param_ptrs.len() {
            self.param_ptrs[idx] = ptr;
            self.param_tydescs[idx] = tydesc;
            self.param_initialized[idx] = initialized;
        }
    }

    /// Read param (dereferences pointer to caller's data).
    ///
    /// Returns None if param is not initialized.
    /// Panics if param ID is out of bounds (compiler bug).
    pub fn param(&self, id: ParamId) -> Option<Value> {
        let idx = id.0 as usize;
        let ptr = self.param_ptrs[idx];
        if ptr.is_null() || !self.param_initialized[idx] {
            return None;
        }
        let tydesc = self.param_tydescs[idx];
        Some(Value { ptr, tydesc })
    }

    /// Get mutable destination for Mut/Out params.
    ///
    /// Panics if param ID is out of bounds or param not set up (compiler bug).
    pub fn param_dest(&self, id: ParamId) -> Destination {
        let idx = id.0 as usize;
        let ptr = self.param_ptrs[idx];
        assert!(!ptr.is_null(), "param_dest called on null param {:?}", id);
        let tydesc = self.param_tydescs[idx];
        Destination { ptr, tydesc }
    }

    /// Mark param as dropped (for In params after consuming).
    pub fn mark_param_dropped(&mut self, id: ParamId) {
        let idx = id.0 as usize;
        if idx < self.param_ptrs.len() {
            self.param_ptrs[idx] = std::ptr::null_mut();
            self.param_initialized[idx] = false;
        }
    }

    /// Check if param is initialized (for ParamStore to decide whether to destroy old value).
    pub fn is_param_initialized(&self, id: ParamId) -> bool {
        let idx = id.0 as usize;
        idx < self.param_initialized.len() && self.param_initialized[idx]
    }

    /// Mark param as initialized (after first write to Out param).
    pub fn mark_param_initialized(&mut self, id: ParamId) {
        let idx = id.0 as usize;
        if idx < self.param_initialized.len() {
            self.param_initialized[idx] = true;
        }
    }

    /// Destroy live unit_end bindings (values/slots) in this frame.
    ///
    /// Called during REPL cleanup to free persistent bindings.
    /// Only destroys values/slots listed in unit_end_values/slots - these are
    /// the persistent bindings that UnitEndDrop would have cleaned up.
    /// Intermediate values are explicitly dropped via Drop instructions during
    /// execution and don't need cleanup here.
    ///
    /// Skips:
    /// - Borrowed values (not owned by this frame)
    /// - Values not in live_values (not written or already dropped)
    pub fn destroy_live_values(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit_end_values: &[ValueId],
        unit_end_slots: &[SlotId],
    ) {
        // Destroy unit_end values (persistent let bindings).
        if let Some(ref mut live_values) = self.live_values {
            for &vid in unit_end_values {
                // Skip if not live (not written or already dropped).
                if !live_values.contains(&vid) {
                    continue;
                }
                let idx = vid.0 as usize;
                let offset = self.layout.value_offsets[idx] as usize;
                let tydesc = self.layout.value_tydescs[idx];
                let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr, tydesc);
                }
                live_values.remove(&vid);
            }
        }

        // Destroy unit_end slots (persistent var bindings).
        for &sid in unit_end_slots {
            let idx = sid.0 as usize;
            if idx < self.slot_initialized.len() && self.slot_initialized[idx] {
                let offset = self.layout.slot_offsets[idx] as usize;
                let tydesc = self.layout.slot_tydescs[idx];
                let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr, tydesc);
                }
                self.slot_initialized[idx] = false;
            }
        }
    }
}

/// Mutable frame storage for script unit execution.
///
/// Stores frames from previously executed units for external value/slot access.
pub struct FrameStore {
    /// Frames from executed units, indexed by unit number.
    frames: Vec<Frame>,
    /// Unit-end values for each unit (persistent bindings to destroy).
    unit_end_values: Vec<Vec<ValueId>>,
    /// Unit-end slots for each unit (persistent bindings to destroy).
    unit_end_slots: Vec<Vec<SlotId>>,
}

impl FrameStore {
    /// Create a new empty frame store.
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
        }
    }

    /// Add a completed unit's frame with its persistent bindings.
    pub fn add_frame(
        &mut self,
        frame: Frame,
        unit_end_values: Vec<ValueId>,
        unit_end_slots: Vec<SlotId>,
    ) {
        self.frames.push(frame);
        self.unit_end_values.push(unit_end_values);
        self.unit_end_slots.push(unit_end_slots);
    }

    /// Read a value from a previous unit.
    ///
    /// Returns None if value is not live (moved or not written).
    /// Panics if unit not found (compiler bug).
    pub fn external_value(&self, unit: u32, value: ValueId) -> Option<Value> {
        let frame = self.frames.get(unit as usize)
            .unwrap_or_else(|| panic!("external unit {} not found", unit));
        frame.value(value)
    }

    /// Read a slot from a previous unit.
    ///
    /// Returns None if slot is not initialized (moved or not written).
    /// Panics if unit not found (compiler bug).
    pub fn external_slot(&self, unit: u32, slot: SlotId) -> Option<Value> {
        let frame = self.frames.get(unit as usize)
            .unwrap_or_else(|| panic!("external unit {} not found", unit));
        frame.slot(slot)
    }

    /// Write a value to a slot in a previous unit.
    ///
    /// If the slot already contains a value, destroys it before writing.
    /// Panics if unit not found (compiler bug).
    pub fn write_external_slot(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit: u32,
        slot: SlotId,
        value: &Value,
    ) {
        let frame = self.frames.get_mut(unit as usize)
            .unwrap_or_else(|| panic!("external unit {} not found", unit));

        // Destroy old value if slot was already initialized.
        if let Some(old_val) = frame.slot(slot) {
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(
                    rt_handle,
                    old_val.ptr,
                    old_val.tydesc,
                );
            }
        }

        let dest = frame.slot_dest(slot);
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, (*value.tydesc).size as usize);
        }
        frame.mark_slot_initialized(slot);
    }

    /// Mark an external value as dropped (moved out).
    pub fn mark_external_value_dropped(&mut self, unit: u32, value: ValueId) {
        if let Some(frame) = self.frames.get_mut(unit as usize) {
            frame.mark_value_dropped(value);
        }
    }

    /// Mark an external slot as dropped (moved out).
    pub fn mark_external_slot_dropped(&mut self, unit: u32, slot: SlotId) {
        if let Some(frame) = self.frames.get_mut(unit as usize) {
            frame.mark_slot_dropped(slot);
        }
    }

    /// Check if an external value is initialized.
    pub fn is_external_value_initialized(&self, unit: u32, value: ValueId) -> bool {
        self.frames.get(unit as usize)
            .map(|frame| frame.is_value_initialized(value))
            .unwrap_or(false)
    }

    /// Check if an external slot is initialized.
    pub fn is_external_slot_initialized(&self, unit: u32, slot: SlotId) -> bool {
        self.frames.get(unit as usize)
            .map(|frame| frame.is_slot_initialized(slot))
            .unwrap_or(false)
    }

    /// Destroy live bindings in all frames.
    pub fn destroy_live_values(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        for i in 0..self.frames.len() {
            let values = &self.unit_end_values[i];
            let slots = &self.unit_end_slots[i];
            self.frames[i].destroy_live_values(rt_handle, values, slots);
        }
    }
}

impl Default for FrameStore {
    fn default() -> Self {
        Self::new()
    }
}
