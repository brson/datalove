//! Execution frame for function calls and script units.

use super::super::{ValueId, SlotId};
use super::error::InterpError;
use super::layout::IrLayout;
use super::value::{Value, Destination};

/// Execution frame for a function call.
pub struct Frame {
    /// Raw frame data.
    data: Vec<u8>,
    /// Layout information.
    layout: IrLayout,
    /// Track which values are initialized.
    value_initialized: Vec<bool>,
    /// Track which slots are initialized.
    slot_initialized: Vec<bool>,
}

impl Frame {
    /// Create a new frame from layout.
    pub fn new(layout: IrLayout) -> Self {
        let value_count = layout.value_offsets.len();
        let slot_count = layout.slot_offsets.len();
        let data = vec![0u8; layout.frame_size as usize];

        Self {
            data,
            layout,
            value_initialized: vec![false; value_count],
            slot_initialized: vec![false; slot_count],
        }
    }

    /// Get destination for a value.
    pub fn value_dest(&mut self, id: ValueId) -> Result<Destination, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.value_offsets.len() {
            return Err(InterpError::MissingType(id));
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Ok(Destination { ptr, tydesc })
    }

    /// Get value (for reading).
    pub fn value(&self, id: ValueId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.value_offsets.len() {
            return Err(InterpError::MissingType(id));
        }
        if !self.value_initialized[idx] {
            return Err(InterpError::UninitializedValue(id));
        }
        let offset = self.layout.value_offsets[idx] as usize;
        let tydesc = self.layout.value_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Ok(Value { ptr, tydesc })
    }

    /// Mark value as initialized.
    pub fn mark_value_initialized(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = true;
        }
    }

    /// Get destination for a slot.
    pub fn slot_dest(&mut self, id: SlotId) -> Result<Destination, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.slot_offsets.len() {
            return Err(InterpError::MissingSlotType(id));
        }
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Ok(Destination { ptr, tydesc })
    }

    /// Get slot value (for reading).
    pub fn slot(&self, id: SlotId) -> Result<Value, InterpError> {
        let idx = id.0 as usize;
        if idx >= self.layout.slot_offsets.len() {
            return Err(InterpError::MissingSlotType(id));
        }
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

    /// Mark value as dropped (uninitialized).
    ///
    /// Called after Drop instruction to prevent double-destroy.
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = false;
        }
    }

    /// Mark slot as dropped (uninitialized).
    ///
    /// Called after Drop instruction to prevent double-destroy.
    pub fn mark_slot_dropped(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if idx < self.slot_initialized.len() {
            self.slot_initialized[idx] = false;
        }
    }

    /// Destroy all initialized values and slots.
    ///
    /// Calls the runtime destructor for each initialized value/slot.
    pub fn destroy_all(&mut self, rt_handle: datalove_rt::c::LocalRtHandle) {
        // Destroy initialized values.
        for idx in 0..self.value_initialized.len() {
            if self.value_initialized[idx] {
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
    pub fn write_external_slot(&mut self, unit: u32, slot: SlotId, value: &Value) -> Result<(), InterpError> {
        let frame = self.frames.get_mut(unit as usize)
            .ok_or(InterpError::ExternalUnitNotFound(unit))?;
        let dest = frame.slot_dest(slot)?;
        unsafe {
            std::ptr::copy_nonoverlapping(value.ptr, dest.ptr, (*value.tydesc).size as usize);
        }
        Ok(())
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
