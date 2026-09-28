//! Execution frames and frame storage.
//!
//! A `Frame` holds all values and slots for a single function/unit execution.
//! `FrameStore` accumulates frames from script units for cross-unit value access.

use std::rc::Rc;

use datalove_rt::rust::AlignedBuffer;
use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{CodeUnitContext, IrCodeUnit, ValueId, SlotId, ParamId};
use crate::layout::IrLayout;
use crate::value::{Value, Destination};

/// Overwrite `v` with `len` copies of `value`, keeping the allocation it has.
fn refill<T: Clone>(v: &mut Vec<T>, len: usize, value: T) {
    v.clear();
    v.resize(len, value);
}

/// A supply of frames to reuse, so that a call is not an allocation.
///
/// Function frames are strictly nested -- a call returns before its caller goes
/// on -- so one frame per level of call depth is all this ever holds, and a
/// frame handed back is free for the next call at that depth.
#[derive(Default)]
pub struct FramePool {
    free: Vec<Frame>,
}

impl FramePool {
    /// Create an empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// A frame for `unit`, reusing one if the pool has any.
    pub fn take(&mut self, unit: &IrCodeUnit, layout: Rc<IrLayout>) -> Frame {
        match self.free.pop() {
            Some(mut frame) => {
                frame.reset(unit, layout);
                frame
            }
            None => Frame::new(unit, layout),
        }
    }

    /// Hand a frame back for the next call to use.
    pub fn give_back(&mut self, frame: Frame) {
        self.free.push(frame);
    }
}

/// Execution frame for a function or script unit.
pub struct Frame {
    /// Raw frame data with proper alignment (values and slots).
    data: AlignedBuffer,
    /// Layout information.
    layout: Rc<IrLayout>,
    /// Track which values hold something.
    ///
    /// A script frame needs this for `DropTracked`. A function frame needs it
    /// because `prepare_call_args` destroys an `out` argument's destination
    /// before the call, and a destination that has never been written must not
    /// be destroyed. That used to rest on the frame being zeroed, so that the
    /// destroy read a null pointer and did nothing -- which made "uninitialized"
    /// and "empty" the same thing, and is the one invariant the compiled
    /// backends carry a tracking byte for rather than assume.
    value_initialized: Vec<bool>,
    /// Track which slots are initialized.
    slot_initialized: Vec<bool>,
    /// Pointers to caller's data for each parameter.
    param_ptrs: Vec<*mut u8>,
    /// Type descriptors for each parameter.
    param_tydescs: Vec<*const TyDesc>,
    /// Track which params are initialized (Out params start uninitialized).
    param_initialized: Vec<bool>,
    /// Descriptors handed over for this function's declared shapes, in order.
    ///
    /// A shape has no value to carry a descriptor with, so unlike everything
    /// else here it arrives on its own.
    shape_descriptors: Vec<*const TyDesc>,
    /// The shapes this function declared, so a call site inside it can tell
    /// which of them a callee's need matches.
    own_shapes: Vec<datalove_datafun_ir::DescriptorShape>,
    /// What a reference points at, where the layout's descriptor for it is a
    /// lie.
    ///
    /// `value_tydescs` lives in the layout and is computed from static types,
    /// which inside a generic say `data` where a type parameter stood. A
    /// projection of a borrowed parameter knows better -- it read the field's
    /// own descriptor off the one the caller supplied -- and this is where it
    /// puts it. Null means the layout's answer was right, which is every value
    /// outside a generic.
    value_tydescs: Vec<*const TyDesc>,
}

impl Frame {
    /// Hand this frame the descriptors for the function's declared shapes.
    pub fn set_shape_descriptors(&mut self, descriptors: Vec<*const TyDesc>) {
        self.shape_descriptors = descriptors;
    }

    /// The descriptor handed over for the shape declared at `index`.
    pub fn shape_descriptor(&self, index: u32) -> Option<*const TyDesc> {
        self.shape_descriptors.get(index as usize).copied()
    }

    /// The shapes this function declared.
    pub fn own_shapes(&self) -> &[datalove_datafun_ir::DescriptorShape] {
        &self.own_shapes
    }

    /// The descriptor this function's own signature gives for parameter `i`.
    pub fn param_tydesc(&self, i: usize) -> *const TyDesc {
        self.layout.param_tydescs[i]
    }

    /// Create a new frame for code unit execution.
    ///
    /// A function frame and a script frame differ only in their parameters: a
    /// script unit has none and a function's are pointers into its caller.
    /// Both track which values hold something.
    pub fn new(unit: &IrCodeUnit, layout: Rc<IrLayout>) -> Self {
        let slot_count = layout.slot_offsets.len();
        // Uninitialized, not zeroed: what has been written is tracked, and
        // reading what has not is a bug rather than a value worth defining.
        let data = AlignedBuffer::uninit(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );

        let own_shapes = match &unit.context {
            CodeUnitContext::Function(ctx) => ctx.descriptor_shapes.clone(),
            _ => Vec::new(),
        };

        let value_count = layout.value_offsets.len();
        match &unit.context {
            CodeUnitContext::Function(ctx) => {
                let param_count = ctx.params.len();
                Self {
                    data,
                    layout,
                    value_initialized: vec![false; value_count],
                    slot_initialized: vec![false; slot_count],
                    param_ptrs: vec![std::ptr::null_mut(); param_count],
                    param_tydescs: vec![std::ptr::null(); param_count],
                    param_initialized: vec![false; param_count],
                    shape_descriptors: Vec::new(),
                    own_shapes,
                    value_tydescs: vec![std::ptr::null(); value_count],
                }
            }
            CodeUnitContext::Script(_) => {
                Self {
                    data,
                    layout,
                    value_initialized: vec![false; value_count],
                    slot_initialized: vec![false; slot_count],
                    param_ptrs: Vec::new(),
                    param_tydescs: Vec::new(),
                    param_initialized: Vec::new(),
                    shape_descriptors: Vec::new(),
                    own_shapes: own_shapes.clone(),
                    value_tydescs: vec![std::ptr::null(); value_count],
                }
            }
            CodeUnitContext::Native(ctx) => {
                panic!("native functions are dispatched directly, not via Frame: {}", ctx.symbol)
            }
        }
    }

    /// Point a frame that has been returned to the pool at a new body.
    ///
    /// Every buffer it holds is reused: the data block if it is large enough and
    /// aligned well enough, and the capacity of each side table either way. A
    /// function frame is five to seven separate allocations, and allocating them
    /// per call was the largest thing in the interpreter's profile after the
    /// instruction loop.
    ///
    /// Only function frames are pooled. A script unit's frame outlives its unit,
    /// because a later unit reads the bindings it holds, so it goes to the
    /// `FrameStore` rather than coming back here.
    fn reset(&mut self, unit: &IrCodeUnit, layout: Rc<IrLayout>) {
        let ctx = match &unit.context {
            CodeUnitContext::Function(ctx) => ctx,
            CodeUnitContext::Script(_) => panic!("script frames are not pooled"),
            CodeUnitContext::Native(ctx) => {
                panic!("native functions are dispatched directly, not via Frame: {}", ctx.symbol)
            }
        };

        let size = layout.frame_size as usize;
        let align = layout.frame_align as usize;
        if self.data.fits(size, align) {
            self.data.reset_prefix(size);
        } else {
            self.data = AlignedBuffer::uninit(size, align);
        }

        let param_count = ctx.params.len();
        refill(&mut self.slot_initialized, layout.slot_offsets.len(), false);
        refill(&mut self.param_ptrs, param_count, std::ptr::null_mut());
        refill(&mut self.param_tydescs, param_count, std::ptr::null());
        refill(&mut self.param_initialized, param_count, false);
        refill(&mut self.value_tydescs, layout.value_offsets.len(), std::ptr::null());
        refill(&mut self.value_initialized, layout.value_offsets.len(), false);

        self.shape_descriptors.clear();
        self.own_shapes.clear();
        self.own_shapes.extend_from_slice(&ctx.descriptor_shapes);
        self.layout = layout;
    }

    /// Mark a value as holding something.
    pub fn mark_value_live(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = true;
        }
    }

    /// Mark a value as holding nothing, having been moved or destroyed.
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        let idx = id.0 as usize;
        if idx < self.value_initialized.len() {
            self.value_initialized[idx] = false;
        }
    }

    /// Whether a value holds something.
    pub fn is_value_initialized(&self, id: ValueId) -> bool {
        self.value_initialized[id.0 as usize]
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
        // What it points at is whatever the projection that made it found, when
        // it found out; otherwise the ref's own tydesc, which wraps the inner
        // type as a one-field tuple.
        if !self.value_tydescs[idx].is_null() {
            return Value { ptr: stored_ptr, tydesc: self.value_tydescs[idx] };
        }
        let inner_tydesc = unsafe {
            let tuple_info = (*tydesc).type_info.tuple;
            (*tuple_info.fields).tydesc
        };
        Value { ptr: stored_ptr, tydesc: inner_tydesc }
    }

    /// Record what a reference points at, where the layout does not say.
    pub fn set_value_tydesc(&mut self, id: ValueId, tydesc: *const TyDesc) {
        self.value_tydescs[id.0 as usize] = tydesc;
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
    pub fn destroy_on_error(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit_end_values: &[ValueId],
        unit_end_slots: &[SlotId],
    ) {
        let initialized = &mut self.value_initialized;

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
    pub fn destroy_unit_end_bindings(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit_end_values: &[ValueId],
        unit_end_slots: &[SlotId],
    ) {
        let initialized = &mut self.value_initialized;

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

    /// Put a new frame in place of unit `unit`'s, destroying what the old one
    /// owned.
    ///
    /// This is what re-executing a unit needs, and the destroy is the whole of
    /// why it is not just an assignment: the old frame owns the unit's
    /// persistent `let` and `var` bindings, and dropping the frame frees its
    /// buffer without destroying the values in it.
    ///
    /// **Nothing else in the store points at what is destroyed.** A unit
    /// copies out of an earlier unit's bindings rather than referring to them
    /// (`compiler-guide.md`, "Ownership across units"), so a later unit that
    /// read one of these holds its own copy. A binding a later unit *moved*
    /// out of is no longer initialized, so it is skipped rather than destroyed
    /// twice.
    pub fn replace_frame(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit: usize,
        frame: Frame,
        unit_end_values: Vec<ValueId>,
        unit_end_slots: Vec<SlotId>,
    ) {
        assert!(
            unit < self.frames.len(),
            "unit {unit} has no frame to replace; the store holds {}",
            self.frames.len(),
        );
        let old_values = std::mem::take(&mut self.unit_end_values[unit]);
        let old_slots = std::mem::take(&mut self.unit_end_slots[unit]);
        self.frames[unit].destroy_unit_end_bindings(rt_handle, &old_values, &old_slots);

        self.frames[unit] = frame;
        self.unit_end_values[unit] = unit_end_values;
        self.unit_end_slots[unit] = unit_end_slots;
    }

    /// How many units the store holds frames for.
    ///
    /// External operands index frames by unit, so this is also the index the
    /// next unit to execute will be given.
    pub fn unit_count(&self) -> usize {
        self.frames.len()
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
