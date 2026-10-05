//! Execution frames and frame storage.
//!
//! A `Frame` holds all values and slots for a single function/unit execution.
//! `FrameStore` accumulates frames from script units for cross-unit value access.

use std::rc::Rc;

use datalove_rt::rust::AlignedBuffer;
use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{CodeUnitContext, IrCodeUnit, ParamMode, ValueId, SlotId, ParamId};
use datalove_datafun_ir::frame_layout::tracking;

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
    /// Boxed, so that taking one and handing it back moves a pointer rather
    /// than the frame: by value it was two `memcpy`s of the whole struct per
    /// call.
    free: Vec<Box<Frame>>,
}

impl FramePool {
    /// Create an empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// A function frame for the body `layout` describes, reusing one if the
    /// pool has any, ready to have its arguments pushed.
    ///
    /// Nothing else about it is ready until `Frame::enter`. The arguments come
    /// first because a dispatcher is offered them before anything is decided
    /// about running the body here, and one that takes the call leaves the rest
    /// of the frame unused.
    pub fn take(&mut self, layout: Rc<IrLayout>) -> Box<Frame> {
        match self.free.pop() {
            Some(mut frame) => {
                frame.params.clear();
                frame.shape_descriptors.clear();
                frame.layout = layout;
                frame
            }
            None => Box::new(Frame {
                data: AlignedBuffer::uninit(0, 1),
                layout,
                params: Vec::new(),
                shape_descriptors: Vec::new(),
                value_tydescs: Vec::new(),
                is_script: false,
                liveness: Liveness { kept: cfg!(debug_assertions), ..Liveness::default() },
            }),
        }
    }

    /// Hand a frame back for the next call to use.
    pub fn give_back(&mut self, frame: Box<Frame>) {
        self.free.push(frame);
    }
}

/// Which of a frame's bindings hold something, kept for every one of them.
///
/// The frame does not need this to run. What the ownership analysis cannot
/// know statically -- a tracked slot, an `out` parameter -- has a tracking byte
/// in the frame data, as in compiled code, and everything else is precise:
/// whether it holds a value is fixed by where the program is. A script frame
/// keeps this anyway, because the REPL recovers from a unit that fails part way
/// and has to free what the unit had bound by then, and because later units
/// read and move its bindings. A function frame keeps it in debug builds only,
/// as a check on the analysis: reading a binding the flags say is empty, or
/// passing on a destination the analysis says is full when it is not, panics
/// there, while a release build pays nothing for it.
#[derive(Default)]
struct Liveness {
    /// Whether these are kept at all; when not, they stay empty.
    kept: bool,
    values: Vec<bool>,
    slots: Vec<bool>,
    params: Vec<bool>,
}

/// Execution frame for a function or script unit.
///
/// Laid out by `ir::frame_layout::FrameLayout`, as compiled code's frames are:
/// values, slots and tracking bytes at the same offsets. The parameter
/// pointers that layout puts first are kept in `params` instead, so that they
/// are a slice of `Value`s to hand a dispatcher, and their space is unused.
pub struct Frame {
    /// Raw frame data with proper alignment.
    data: AlignedBuffer,
    /// Layout information.
    layout: Rc<IrLayout>,
    /// Each parameter: a pointer to the caller's data and its descriptor.
    ///
    /// One `Value` apiece, so that the arguments as a caller pushed them are a
    /// slice a dispatcher can be handed without building another.
    params: Vec<Value>,
    /// Descriptors handed over for this function's declared shapes, in order.
    ///
    /// A shape has no value to carry a descriptor with, so unlike everything
    /// else here it arrives on its own.
    shape_descriptors: Vec<*const TyDesc>,
    /// What a reference points at, where the layout's descriptor for it is a
    /// lie.
    ///
    /// `value_tydescs` lives in the layout and is computed from static types,
    /// which inside a generic say `data` where a type parameter stood. A
    /// projection of a borrowed parameter knows better -- it read the field's
    /// own descriptor off the one the caller supplied -- and this is where it
    /// puts it. Null means the layout's answer was right, which is every value
    /// outside a generic, so it is left empty until one is recorded rather
    /// than filled with nulls on every call.
    value_tydescs: Vec<*const TyDesc>,
    /// Whether this is a script unit's frame, whose untracked bindings are
    /// answered for by `liveness` rather than assumed to hold something.
    is_script: bool,
    /// Every binding's liveness, where it is kept; see `Liveness`.
    liveness: Liveness,
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

    /// The layout this frame was taken for.
    pub fn layout(&self) -> &IrLayout {
        &self.layout
    }

    /// The layout this frame was taken for, shared.
    pub(crate) fn layout_rc(&self) -> Rc<IrLayout> {
        Rc::clone(&self.layout)
    }

    /// Where the frame's data starts.
    pub(crate) fn base_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    /// Stop keeping liveness flags for a frame the bytecode runs.
    ///
    /// The bytecode keeps no flags for values and untracked slots -- a release
    /// build keeps none either -- so the debug checks that read them would
    /// fire on what it never wrote. The IR walker remains the checked engine.
    pub(crate) fn stop_keeping_liveness(&mut self) {
        self.liveness.kept = false;
    }

    /// Hand over the next argument, as the caller has it.
    pub fn push_param(&mut self, value: Value) {
        self.params.push(value);
    }

    /// Hand over the descriptor for the next shape the function declared.
    pub fn push_shape_descriptor(&mut self, tydesc: *const TyDesc) {
        self.shape_descriptors.push(tydesc);
    }

    /// The arguments as pushed, before `enter` gives owned ones the callee's
    /// own descriptors.
    pub fn params(&self) -> &[Value] {
        &self.params
    }

    /// The shape descriptors as pushed.
    pub fn shape_descriptors(&self) -> &[*const TyDesc] {
        &self.shape_descriptors
    }

    /// Make a frame taken from the pool ready to run its body, once every
    /// argument has been pushed.
    ///
    /// A parameter the callee owns gets the callee's own descriptor: the caller
    /// has already converted it into the shape the callee was compiled for. A
    /// borrowed one keeps the descriptor it came with, because a generic
    /// function was compiled with `data` where its type parameter stood and the
    /// value at that pointer is whatever the caller actually has; for a function
    /// that is not generic the two agree anyway. An `out` parameter starts
    /// uninitialized, since the callee writes it first.
    pub fn enter(&mut self) {
        let layout = &*self.layout;
        assert_eq!(self.params.len(), layout.param_modes.len(),
            "a call handed over {} arguments for {} parameters",
            self.params.len(), layout.param_modes.len());

        let size = layout.frame_size as usize;
        let align = layout.frame_align as usize;
        if self.data.fits(size, align) {
            self.data.reset_prefix(size);
        } else {
            self.data = AlignedBuffer::uninit(size, align);
        }
        self.clear_tracking();
        self.value_tydescs.clear();

        let layout = &*self.layout;
        for (i, mode) in layout.param_modes.iter().enumerate() {
            if matches!(mode, ParamMode::In | ParamMode::Out) {
                self.params[i].tydesc = layout.param_tydescs[i];
            }
            // The pointer goes where compiled code keeps it too, which is
            // where the bytecode reads a parameter through.
            let at = layout.param_offsets[i] as usize;
            // SAFETY: the parameter region is inside the frame, by the layout.
            unsafe { (self.data.as_mut_ptr().add(at) as *mut *mut u8).write_unaligned(self.params[i].ptr) };
        }
        if self.liveness.kept {
            refill(&mut self.liveness.values, layout.value_offsets.len(), false);
            refill(&mut self.liveness.slots, layout.slot_offsets.len(), false);
            self.liveness.params.clear();
            self.liveness.params.extend(layout.param_modes.iter().map(|m| *m != ParamMode::Out));
        }
    }

    /// Create the frame for a script unit.
    ///
    /// A script unit has no parameters, and its frame outlives it in the
    /// `FrameStore` rather than going back to a pool. Function frames come from
    /// `FramePool::take`.
    pub fn new(unit: &IrCodeUnit, layout: Rc<IrLayout>) -> Self {
        if let CodeUnitContext::Native(ctx) = &unit.context {
            panic!("native functions are dispatched directly, not via Frame: {}", ctx.symbol)
        }
        assert!(unit.function_context().is_none(), "function frames come from the pool");
        // Uninitialized, not zeroed: what has been written is tracked, and
        // reading what has not is a bug rather than a value worth defining.
        let data = AlignedBuffer::uninit(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );
        let mut frame = Self {
            data,
            params: Vec::new(),
            shape_descriptors: Vec::new(),
            value_tydescs: Vec::new(),
            is_script: true,
            liveness: Liveness {
                kept: true,
                values: vec![false; layout.value_offsets.len()],
                slots: vec![false; layout.slot_offsets.len()],
                params: Vec::new(),
            },
            layout,
        };
        frame.clear_tracking();
        frame
    }

    /// Set every tracking byte to say its binding has never been written.
    fn clear_tracking(&mut self) {
        let offset = self.layout.tracking_offset as usize;
        let count = self.layout.tracking_count as usize;
        // A byte at a time: there are a handful at most, and `write_bytes` of a
        // length only known at run time is a call into `memset`, which was most
        // of what entering a function cost.
        for i in 0..count {
            // SAFETY: the tracking bytes are inside the frame, by the layout.
            unsafe { *self.data.as_mut_ptr().add(offset + i) = tracking::UNINIT };
        }
    }

    /// The tracking byte at `offset`.
    #[inline(always)]
    fn tracking_byte(&self, offset: u32) -> u8 {
        // SAFETY: a tracking byte offset is inside the frame, by the layout.
        unsafe { *self.data.as_ptr().add(offset as usize) }
    }

    #[inline(always)]
    fn set_tracking_byte(&mut self, offset: u32, state: u8) {
        // SAFETY: a tracking byte offset is inside the frame, by the layout.
        unsafe { *self.data.as_mut_ptr().add(offset as usize) = state };
    }

    /// Whether a value holds something.
    ///
    /// A value is precise, so in a function it holds something wherever the
    /// program asks. A script frame says from its flags, since a unit that
    /// failed part way may not have reached it.
    #[inline]
    pub fn value_is_live(&self, id: ValueId) -> bool {
        let idx = id.0 as usize;
        if self.is_script {
            return self.liveness.values[idx];
        }
        debug_assert!(!self.liveness.kept || self.liveness.values[idx],
            "value {:?} was taken to hold something and does not", id);
        true
    }

    /// Mark a value as holding something.
    #[inline(always)]
    pub fn mark_value_live(&mut self, id: ValueId) {
        if self.liveness.kept {
            self.liveness.values[id.0 as usize] = true;
        }
    }

    /// Mark a value as holding nothing, having been moved or destroyed.
    #[inline(always)]
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        if self.liveness.kept {
            self.liveness.values[id.0 as usize] = false;
        }
    }

    /// Get destination for a value.
    ///
    /// Panics if the value ID is out of bounds.
    #[inline(always)]
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
    #[inline(always)]
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
    #[inline]
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
        if let Some(&found) = self.value_tydescs.get(idx) && !found.is_null() {
            return Value { ptr: stored_ptr, tydesc: found };
        }
        let inner_tydesc = unsafe {
            let tuple_info = (*tydesc).type_info.tuple;
            (*tuple_info.fields).tydesc
        };
        Value { ptr: stored_ptr, tydesc: inner_tydesc }
    }

    /// Record what a reference points at, where the layout does not say.
    pub fn set_value_tydesc(&mut self, id: ValueId, tydesc: *const TyDesc) {
        let idx = id.0 as usize;
        if self.value_tydescs.len() <= idx {
            self.value_tydescs.resize(self.layout.value_offsets.len(), std::ptr::null());
        }
        self.value_tydescs[idx] = tydesc;
    }

    /// Get destination for a slot.
    ///
    /// Panics if slot ID is out of bounds (compiler bug).
    #[inline(always)]
    pub fn slot_dest(&mut self, id: SlotId) -> Destination {
        let idx = id.0 as usize;
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { self.data.as_mut_ptr().add(offset) };
        Destination { ptr, tydesc }
    }

    /// Get slot value (for reading).
    ///
    /// Panics if slot ID is out of bounds (compiler bug), and in a debug build
    /// where liveness is kept, if the slot holds nothing.
    #[inline(always)]
    pub fn slot(&self, id: SlotId) -> Value {
        let idx = id.0 as usize;
        debug_assert!(!self.liveness.kept || self.liveness.slots[idx], "read of an empty slot {:?}", id);
        let offset = self.layout.slot_offsets[idx] as usize;
        let tydesc = self.layout.slot_tydescs[idx];
        let ptr = unsafe { (self.data.as_ptr() as *mut u8).add(offset) };
        Value { ptr, tydesc }
    }

    /// Whether a slot holds something to destroy.
    ///
    /// A tracked slot says from its tracking byte, and a script frame's
    /// untracked slot from its flags. An untracked slot in a function is taken
    /// to hold something, as compiled code takes it: the analysis tracks every
    /// slot whose state it cannot fix and whose type owns something, so an
    /// untracked slot either holds a value or is of a type that owns nothing,
    /// and destroying it is right either way.
    #[inline]
    pub fn slot_is_live(&self, id: SlotId) -> bool {
        let idx = id.0 as usize;
        match self.layout.slot_tracking[idx] {
            Some(offset) => {
                let live = self.tracking_byte(offset) == tracking::LIVE;
                debug_assert!(!self.liveness.kept || self.liveness.slots[idx] == live,
                    "slot {:?} is {} by its tracking byte and not by its flags",
                    id, if live { "live" } else { "empty" });
                live
            }
            None if self.is_script => self.liveness.slots[idx],
            None => {
                debug_assert!(
                    !self.liveness.kept || self.liveness.slots[idx] || self.layout.slot_is_copy[idx],
                    "untracked slot {:?} owns something and holds nothing", id);
                true
            }
        }
    }

    /// Check, in a debug build where liveness is kept, that a slot about to be
    /// moved into holds nothing, since overwriting what it held would leak it.
    #[inline]
    pub fn check_slot_empty(&self, id: SlotId) {
        let idx = id.0 as usize;
        debug_assert!(!self.liveness.kept || !self.liveness.slots[idx],
            "move into occupied slot {:?}", id);
    }

    /// Mark slot as holding something.
    #[inline(always)]
    pub fn mark_slot_live(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if let Some(offset) = self.layout.slot_tracking[idx] {
            self.set_tracking_byte(offset, tracking::LIVE);
        }
        if self.liveness.kept {
            self.liveness.slots[idx] = true;
        }
    }

    /// Mark slot as holding nothing, having been moved or destroyed.
    #[inline]
    pub fn mark_slot_dropped(&mut self, id: SlotId) {
        let idx = id.0 as usize;
        if let Some(offset) = self.layout.slot_tracking[idx] {
            self.set_tracking_byte(offset, tracking::MOVED);
        }
        if self.liveness.kept {
            self.liveness.slots[idx] = false;
        }
    }

    /// Read param (dereferences pointer to caller's data).
    ///
    /// Panics if param ID is out of bounds (compiler bug), and in a debug build
    /// where liveness is kept, if the parameter holds nothing.
    #[inline(always)]
    pub fn param(&self, id: ParamId) -> Value {
        let idx = id.0 as usize;
        debug_assert!(!self.liveness.kept || self.liveness.params[idx], "read of an absent param {:?}", id);
        self.params[idx]
    }

    /// Get mutable destination for Mut/Out params.
    ///
    /// Panics if param ID is out of bounds (compiler bug).
    pub fn param_dest(&self, id: ParamId) -> Destination {
        let param = self.params[id.0 as usize];
        Destination { ptr: param.ptr, tydesc: param.tydesc }
    }

    /// Whether a parameter holds something.
    ///
    /// An `out` parameter says from its tracking byte. Any other holds what
    /// the caller passed, wherever the program asks.
    #[inline]
    pub fn param_is_live(&self, id: ParamId) -> bool {
        let idx = id.0 as usize;
        let live = match self.layout.param_tracking[idx] {
            Some(offset) => self.tracking_byte(offset) == tracking::LIVE,
            None => true,
        };
        debug_assert!(!self.liveness.kept || self.liveness.params[idx] == live,
            "param {:?} is {} by the frame and not by its flags",
            id, if live { "live" } else { "empty" });
        live
    }

    /// Mark param as holding something (after the first write to an `out` one).
    #[inline]
    pub fn mark_param_live(&mut self, id: ParamId) {
        let idx = id.0 as usize;
        if let Some(offset) = self.layout.param_tracking[idx] {
            self.set_tracking_byte(offset, tracking::LIVE);
        }
        if self.liveness.kept {
            self.liveness.params[idx] = true;
        }
    }

    /// Mark param as holding nothing, having been moved or destroyed.
    #[inline]
    pub fn mark_param_dropped(&mut self, id: ParamId) {
        let idx = id.0 as usize;
        if let Some(offset) = self.layout.param_tracking[idx] {
            self.set_tracking_byte(offset, tracking::MOVED);
        }
        if self.liveness.kept {
            self.liveness.params[idx] = false;
        }
    }

    /// Destroy initialized unit_end bindings on error cleanup.
    ///
    /// Used when a script unit errors out before being added to FrameStore.
    pub fn destroy_on_error(
        &mut self,
        rt_handle: datalove_rt::c::LocalRtHandle,
        unit_end_values: &[ValueId],
        unit_end_slots: &[SlotId],
    ) {
        self.destroy_unit_end_bindings(rt_handle, unit_end_values, unit_end_slots);
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
        for &vid in unit_end_values {
            if !self.value_is_live(vid) {
                continue;
            }
            let val = self.value(vid);
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, val.ptr, val.tydesc);
            }
            self.mark_value_dropped(vid);
        }

        for &sid in unit_end_slots {
            if !self.slot_is_live(sid) {
                continue;
            }
            let val = self.slot(sid);
            unsafe {
                datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, val.ptr, val.tydesc);
            }
            self.mark_slot_dropped(sid);
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

    /// Drop the frames from `len` on, destroying what they owned.
    ///
    /// What truncating a session needs, and what splicing its unit list needs
    /// for the suffix it is about to run again. **The destroy is the whole of
    /// why this is not three `Vec::truncate` calls**, for the reason
    /// [`Self::replace_frame`] destroys: dropping a `Frame` frees its buffer
    /// without destroying the values in it, and the `let` and `var` bindings a
    /// unit left behind live in that buffer.
    ///
    /// Nothing that survives points at what is destroyed. A unit copies out of
    /// an earlier unit's bindings rather than referring to them, so a unit
    /// before `len` that read one of these holds its own copy, and a binding a
    /// dropped unit moved out of is no longer initialized and so is skipped
    /// rather than destroyed twice.
    pub fn truncate_units(&mut self, rt_handle: datalove_rt::c::LocalRtHandle, len: usize) {
        assert!(
            len <= self.frames.len(),
            "cannot truncate to {len} units; the store holds {}",
            self.frames.len(),
        );
        for unit in len..self.frames.len() {
            let values = &self.unit_end_values[unit];
            let slots = &self.unit_end_slots[unit];
            self.frames[unit].destroy_unit_end_bindings(rt_handle, values, slots);
        }
        self.frames.truncate(len);
        self.unit_end_values.truncate(len);
        self.unit_end_slots.truncate(len);
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
        if !frame.value_is_live(value) {
            return None;
        }
        Some(frame.value(value))
    }

    /// Read a slot from a previous unit.
    ///
    /// Returns None if the slot has been moved out or was never written.
    /// Panics if unit not found (compiler bug).
    pub fn external_slot(&self, unit: u32, slot: SlotId) -> Option<Value> {
        let frame = self.frame(unit);
        if !frame.slot_is_live(slot) {
            return None;
        }
        Some(frame.slot(slot))
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
        if frame.slot_is_live(slot) {
            let old_val = frame.slot(slot);
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
        frame.mark_slot_live(slot);
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
        self.frame(unit).slot_is_live(slot)
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
