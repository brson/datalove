//! Execution frames and frame storage.
//!
//! A `Frame` holds all values and slots for a single function/unit execution.
//! `FrameStore` accumulates frames from script units for cross-unit value access.

use datalove_rt::rust::AlignedBuffer;
use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{CodeUnitContext, IrCodeUnit, ValueId, SlotId, ParamId};
use crate::layout::IrLayout;
use crate::value::{Value, Destination};

/// Execution frame for a function or script unit.
pub struct Frame {
    /// Raw frame data with proper alignment (values and slots).
    data: AlignedBuffer,
    /// Layout information.
    layout: IrLayout,
    /// Track which values are initialized (for DropTracked).
    ///
    /// Only used for script frames. Function frames don't need this tracking
    /// since they use precise Drop instructions and are discarded on return.
    value_initialized: Option<Vec<bool>>,
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
    /// Create a new frame for code unit execution.
    ///
    /// Dispatches on the unit's context:
    /// - Function frames don't track value initialization (precise Drop instructions)
    /// - Script frames track value initialization for DropTracked cleanup
    pub fn new(unit: &IrCodeUnit, layout: IrLayout) -> Self {
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );

        match &unit.context {
            CodeUnitContext::Function(ctx) => {
                let param_count = ctx.params.len();
                Self {
                    data,
                    layout,
                    value_initialized: None,
                    slot_initialized: vec![false; slot_count],
                    param_ptrs: vec![std::ptr::null_mut(); param_count],
                    param_tydescs: vec![std::ptr::null(); param_count],
                    param_initialized: vec![false; param_count],
                }
            }
            CodeUnitContext::Script(_) => {
                let value_count = layout.value_offsets.len();
                Self {
                    data,
                    layout,
                    value_initialized: Some(vec![false; value_count]),
                    slot_initialized: vec![false; slot_count],
                    param_ptrs: Vec::new(),
                    param_tydescs: Vec::new(),
                    param_initialized: Vec::new(),
                }
            }
            CodeUnitContext::Native(ctx) => {
                panic!("native functions are dispatched directly, not via Frame: {}", ctx.symbol)
            }
        }
    }

    /// Mark a value as initialized.
    ///
    /// No-op for function frames (which don't track value initialization).
    pub fn mark_value_live(&mut self, id: ValueId) {
        if let Some(ref mut initialized) = self.value_initialized {
            let idx = id.0 as usize;
            if idx < initialized.len() {
                initialized[idx] = true;
            }
        }
    }

    /// Mark a value as dropped.
    ///
    /// No-op for function frames (which don't track value initialization).
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        if let Some(ref mut initialized) = self.value_initialized {
            let idx = id.0 as usize;
            if idx < initialized.len() {
                initialized[idx] = false;
            }
        }
    }

    /// Check if a value is initialized.
    ///
    /// Always true for function frames, which don't track value initialization.
    pub fn is_value_initialized(&self, id: ValueId) -> bool {
        match &self.value_initialized {
            Some(initialized) => initialized[id.0 as usize],
            None => true,
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
    /// Panics if value ID is out of bounds (compiler bug).
    pub fn value(&self, id: ValueId) -> Value {
        let idx = id.0 as usize;
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Value { ptr, tydesc }
    }

    /// Dereference a ref value to get the pointed-to data.
    ///
    /// For values produced by GetFieldRef, reads the stored pointer and
    /// returns the data it points to with the inner type's tydesc.
    ///
    /// Panics if value ID is out of bounds (compiler bug).
    pub fn value_deref(&self, id: ValueId) -> Value {
        let idx = id.0 as usize;
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
        Value { ptr: stored_ptr, tydesc: inner_tydesc }
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

    /// Destroy initialized unit_end bindings on error cleanup.
    ///
    /// Used when a script unit errors out before being added to FrameStore.
    /// Uses value_initialized to determine what to destroy.
    ///
    /// Only valid for script frames (panics if value_initialized is None).
    pub fn destroy_on_error(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit_end_values: &[ValueId],
        unit_end_slots: &[SlotId],
    ) {
        let initialized = self.value_initialized.as_mut()
            .expect("destroy_on_error called on function frame");

        // Destroy unit_end values that were initialized.
        for &vid in unit_end_values {
            let idx = vid.0 as usize;
            if !initialized[idx] {
                continue;
            }
            let offset = self.layout.value_offsets[idx] as usize;
            let tydesc = self.layout.value_tydescs[idx];
            let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr, tydesc);
            }
            initialized[idx] = false;
        }

        // Destroy unit_end slots that were initialized.
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

    /// Destroy unit_end bindings (values/slots) in this frame.
    ///
    /// Called during REPL cleanup to free persistent bindings. Bindings that
    /// were moved out of are no longer initialized, so they are skipped.
    ///
    /// Only valid for script frames (panics if value_initialized is None).
    pub fn destroy_unit_end_bindings(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit_end_values: &[ValueId],
        unit_end_slots: &[SlotId],
    ) {
        let initialized = self.value_initialized.as_mut()
            .expect("destroy_unit_end_bindings called on function frame");

        // Destroy unit_end values (persistent let bindings).
        for &vid in unit_end_values {
            let idx = vid.0 as usize;
            if !initialized[idx] {
                continue;
            }
            let offset = self.layout.value_offsets[idx] as usize;
            let tydesc = self.layout.value_tydescs[idx];
            let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, ptr, tydesc);
            }
            initialized[idx] = false;
        }

        // Destroy unit_end slots (persistent var bindings).
        for &sid in unit_end_slots {
            let idx = sid.0 as usize;
            if idx >= self.slot_initialized.len() || !self.slot_initialized[idx] {
                continue;
            }
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

/// Mutable frame storage for script unit execution.
///
/// Stores frames from previously executed units for external value/slot access.
/// A binding that is moved out of is marked uninitialized in the frame that
/// owns it, which is what makes it unreadable afterwards.
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
    /// Returns None if the value has been moved out.
    /// Panics if unit not found (compiler bug).
    pub fn external_value(&self, unit: u32, value: ValueId) -> Option<Value> {
        let frame = self.frame(unit);
        if !frame.is_value_initialized(value) {
            return None;
        }
        Some(frame.value(value))
    }

    /// Read a slot from a previous unit.
    ///
    /// Returns None if the slot has been moved out or was never written.
    /// Panics if unit not found (compiler bug).
    pub fn external_slot(&self, unit: u32, slot: SlotId) -> Option<Value> {
        self.frame(unit).slot(slot)
    }

    /// Write a value to a slot in a previous unit.
    ///
    /// Destroys whatever the slot still owns before writing, and leaves it
    /// initialized, so a slot that had been moved out of is readable again.
    ///
    /// Panics if unit not found (compiler bug).
    pub fn write_external_slot(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit: u32,
        slot: SlotId,
        value: &Value,
    ) {
        let frame = self.frame_mut(unit);

        // A slot that was moved out of is uninitialized and owns nothing.
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

    /// Mark an external value as moved out.
    pub fn mark_external_value_dropped(&mut self, unit: u32, value: ValueId) {
        self.frame_mut(unit).mark_value_dropped(value);
    }

    /// Mark an external slot as moved out.
    pub fn mark_external_slot_dropped(&mut self, unit: u32, slot: SlotId) {
        self.frame_mut(unit).mark_slot_dropped(slot);
    }

    /// Check if an external slot is initialized (not moved).
    pub fn is_external_slot_initialized(&self, unit: u32, slot: SlotId) -> bool {
        self.frame(unit).is_slot_initialized(slot)
    }

    /// Destroy live bindings in all frames.
    pub fn destroy_live_values(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        for i in 0..self.frames.len() {
            let values = &self.unit_end_values[i];
            let slots = &self.unit_end_slots[i];
            self.frames[i].destroy_unit_end_bindings(rt_handle, values, slots);
        }
    }

    /// The frame of a previous unit.
    ///
    /// Panics if the unit is not in the store (compiler bug).
    fn frame(&self, unit: u32) -> &Frame {
        self.frames.get(unit as usize)
            .unwrap_or_else(|| panic!("external unit {} not found", unit))
    }

    fn frame_mut(&mut self, unit: u32) -> &mut Frame {
        self.frames.get_mut(unit as usize)
            .unwrap_or_else(|| panic!("external unit {} not found", unit))
    }
}

impl Default for FrameStore {
    fn default() -> Self {
        Self::new()
    }
}
