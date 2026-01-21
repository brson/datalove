//! Execution frames and frame storage.
//!
//! A `Frame` holds all values and slots for a single function/unit execution.
//! `FrameStore` accumulates frames from script units for cross-unit value access.

use datalove_rt::rust::AlignedBuffer;
use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{ValueId, SlotId, ParamId};
use crate::error::InterpError;
use crate::layout::IrLayout;
use crate::value::{Value, Destination};

/// Execution frame for a function call.
pub struct Frame {
    /// Raw frame data with proper alignment (values and slots).
    data: AlignedBuffer,
    /// Layout information.
    layout: IrLayout,
    /// Track which values are initialized.
    value_initialized: Vec<bool>,
    /// Track which slots are initialized.
    slot_initialized: Vec<bool>,
    /// Track which values are borrowed (not owned, skip destruction).
    value_borrowed: Vec<bool>,
    /// Track which values are references (store pointers, dereference on read).
    value_is_ref: Vec<bool>,
    /// Pointers to caller's data for each parameter.
    param_ptrs: Vec<*mut u8>,
    /// Type descriptors for each parameter.
    param_tydescs: Vec<*const TyDesc>,
    /// Track which params are borrowed (Ref/Mut/Out - caller retains ownership).
    param_borrowed: Vec<bool>,
    /// Track which params are initialized (Out params start uninitialized).
    param_initialized: Vec<bool>,
}

impl Frame {
    /// Create a new frame from layout.
    pub fn new(layout: IrLayout, param_count: usize) -> Self {
        let value_count = layout.value_offsets.len();
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );

        Self {
            data,
            layout,
            value_initialized: vec![false; value_count],
            slot_initialized: vec![false; slot_count],
            value_borrowed: vec![false; value_count],
            value_is_ref: vec![false; value_count],
            param_ptrs: vec![std::ptr::null_mut(); param_count],
            param_tydescs: vec![std::ptr::null(); param_count],
            param_borrowed: vec![false; param_count],
            param_initialized: vec![false; param_count],
        }
    }

    /// Mark a value as borrowed (not owned, skip destruction).
    pub fn mark_value_borrowed(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_borrowed.len() {
            self.value_borrowed[idx] = true;
        }
    }

    /// Mark a value as a reference (stores pointer, dereference on read).
    pub fn mark_value_is_ref(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_is_ref.len() {
            self.value_is_ref[idx] = true;
        }
    }

    /// Check if a value is a reference.
    pub fn is_value_ref(&self, id: ValueId) -> bool {
        let idx = id.0 as usize;
        idx < self.value_is_ref.len() && self.value_is_ref[idx]
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
    /// Panics if value ID is out of bounds (compiler bug).
    /// Returns error if value is uninitialized (may happen during Drop).
    pub fn value(&self, id: ValueId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        if !self.value_initialized[idx] {
            return Err(InterpError::UninitializedValue(id));
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };

        // Handle ref values: dereference the stored pointer and use inner tydesc.
        if self.value_is_ref[idx] {
            // Read the stored pointer.
            let stored_ptr = unsafe { *(ptr as *const *mut u8) };
            // Get inner tydesc from the ref's tydesc (stored as 1-element tuple).
            let inner_tydesc = unsafe {
                let tuple_info = (*tydesc).type_info.tuple;
                (*tuple_info.fields).tydesc
            };
            Ok(Value { ptr: stored_ptr, tydesc: inner_tydesc })
        } else {
            Ok(Value { ptr, tydesc })
        }
    }

    /// Mark value as initialized.
    pub fn mark_value_initialized(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = true;
        }
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
    /// Panics if slot ID is out of bounds (compiler bug).
    /// Returns error if slot is uninitialized (may happen during Drop).
    pub fn slot(&self, id: SlotId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        if !self.slot_initialized[idx] {
            return Err(InterpError::UninitializedSlot(id));
        }
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Ok(Value { ptr, tydesc })
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

    /// Mark value as dropped to prevent double-destroy.
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = false;
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
    pub fn set_param(&mut self, id: ParamId, ptr: *mut u8, tydesc: *const TyDesc, borrowed: bool, initialized: bool) {
        let idx = id.0 as usize;
        if idx < self.param_ptrs.len() {
            self.param_ptrs[idx] = ptr;
            self.param_tydescs[idx] = tydesc;
            self.param_borrowed[idx] = borrowed;
            self.param_initialized[idx] = initialized;
        }
    }

    /// Read param (dereferences pointer to caller's data).
    ///
    /// Panics if param ID is out of bounds (compiler bug).
    /// Returns error if param is uninitialized (may happen during Drop).
    pub fn param(&self, id: ParamId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        let ptr = self.param_ptrs[idx];
        if ptr.is_null() || !self.param_initialized[idx] {
            return Err(InterpError::UninitializedParam(id));
        }
        let tydesc = self.param_tydescs[idx];
        Ok(Value { ptr, tydesc })
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

    /// Destroy all initialized values, slots, and owned params.
    ///
    /// Calls the runtime destructor for each initialized value/slot.
    /// Skips borrowed values and borrowed params (they're not owned by this frame).
    pub fn destroy_all(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        // Destroy In params (not borrowed, and initialized - callee owns through pointer).
        for idx in 0..self.param_ptrs.len() {
            let ptr = self.param_ptrs[idx];
            if !ptr.is_null() && !self.param_borrowed[idx] && self.param_initialized[idx] {
                let tydesc = self.param_tydescs[idx];
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr, tydesc);
                }
                self.param_ptrs[idx] = std::ptr::null_mut();
                self.param_initialized[idx] = false;
            }
        }

        // Destroy initialized values (skip borrowed ones).
        for idx in 0..self.value_initialized.len() {
            if self.value_initialized[idx] && !self.value_borrowed[idx] {
                let offset = self.layout.value_offsets[idx] as usize;
                let tydesc = self.layout.value_tydescs[idx];
                let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
                unsafe {
                    datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr, tydesc);
                }
                self.value_initialized[idx] = false;
            }
        }

        // Destroy initialized slots.
        for idx in 0..self.slot_initialized.len() {
            if self.slot_initialized[idx] {
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
}

impl FrameStore {
    /// Create a new empty frame store.
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    /// Add a completed unit's frame.
    pub fn add_frame(&mut self, frame: Frame) {
        self.frames.push(frame);
    }

    /// Read a value from a previous unit.
    pub fn external_value(&self, unit: u32, value: ValueId) -> Result<Value, InterpError> {
        let frame = self.frames.get(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        frame.value(value)
    }

    /// Read a slot from a previous unit.
    pub fn external_slot(&self, unit: u32, slot: SlotId) -> Result<Value, InterpError> {
        let frame = self.frames.get(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        frame.slot(slot)
    }

    /// Write a value to a slot in a previous unit.
    ///
    /// If the slot already contains a value, destroys it before writing.
    pub fn write_external_slot(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit: u32,
        slot: SlotId,
        value: &Value,
    ) -> Result<(), InterpError> {
        let frame = self.frames.get_mut(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;

        // Destroy old value if slot was already initialized.
        if frame.is_slot_initialized(slot) {
            let old_val = frame.slot(slot)?;
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
        Ok(())
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

    /// Destroy all values in all frames.
    pub fn destroy_all(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        for frame in &mut self.frames {
            frame.destroy_all(rt_handle);
        }
    }
}

impl Default for FrameStore {
    fn default() -> Self {
        Self::new()
    }
}
