//! Execution frames and frame storage.
//!
//! A frame holds all values and slots for a single function or unit
//! execution, and everything else the interpreter keeps about it, in its bytes,
//! as `IrLayout` lays them out. Function frames come off the `FrameStack`;
//! script frames are `ScriptFrame`s, which own their bytes and outlive their
//! unit in the `FrameStore` for later units to read. Either is worked on
//! through a `Frame`, a pointer to the bytes and the layout.

use std::alloc::Layout as AllocLayout;
use std::ptr::NonNull;
use std::rc::Rc;

use datalove_rt::rust::AlignedBuffer;
use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{CodeUnitContext, IrCodeUnit, ParamMode, ValueId, SlotId, ParamId};
use datalove_datafun_ir::frame_layout::tracking;

use crate::error::InterpError;
use crate::layout::IrLayout;
use crate::value::{Value, Destination};

/// The stack function frames are pushed on.
///
/// Function frames are strictly nested -- a call returns before its caller goes
/// on -- so they come off a stack, by bumping a pointer, and a frame never
/// moves while it is live: a parameter is a pointer into its caller's frame,
/// and references point into frames anywhere up the stack.
///
/// The stack is a list of chunks that are allocated once and kept, so that
/// nothing moves when it grows; a frame never straddles two. A chunk list
/// rather than one reserved region of address space, because the interpreter
/// is the engine for targets that cannot reserve address space, wasm among
/// them.
///
/// Each frame is preceded by a header: where the top was before it, and its
/// layout, held as a strong reference until the frame is popped, so that a
/// layout the cache replaces outlives the frames still running on it.
pub struct FrameStack {
    chunks: Vec<Chunk>,
    /// The chunk in use, and the offset of the first free byte in it.
    current: usize,
    top: usize,
    /// How many bytes of chunks may be allocated in all.
    limit: usize,
    allocated: usize,
}

struct Chunk {
    ptr: NonNull<u8>,
    size: usize,
}

/// What precedes each frame on the stack.
#[repr(C)]
struct Header {
    /// The chunk and offset the top was at before the frame was pushed.
    prev_chunk: u32,
    prev_top: u32,
    /// The frame's layout: from `Rc::into_raw` if the frame holds a count of
    /// it, with the low bit set if something else keeps it alive.
    layout: *const IrLayout,
}

/// The low bit of a header's layout pointer: the frame borrows its layout.
const BORROWED: usize = 1;

const HEADER: usize = std::mem::size_of::<Header>();
/// Every chunk's alignment, and so the most a frame may ask for.
const CHUNK_ALIGN: usize = 64;
const FIRST_CHUNK: usize = 64 * 1024;
/// The default for how much frame stack there may be in all.
const DEFAULT_LIMIT: usize = 256 * 1024 * 1024;

impl FrameStack {
    /// Create an empty stack, with the default limit.
    pub fn new() -> Self {
        Self { chunks: Vec::new(), current: 0, top: 0, limit: DEFAULT_LIMIT, allocated: 0 }
    }

    /// Push a frame for the body `layout` describes, ready to have its
    /// arguments set.
    ///
    /// Nothing else about it is ready until `Frame::enter`. The arguments come
    /// first because a dispatcher is offered them before anything is decided
    /// about running the body here, and one that takes the call leaves the rest
    /// of the frame unused. Every frame pushed is popped, error or not.
    #[inline]
    pub fn push(&mut self, layout: Rc<IrLayout>) -> Result<Frame, InterpError> {
        let layout = Rc::into_raw(layout);
        // SAFETY: from `Rc::into_raw`, and released by `pop`.
        unsafe { self.push_raw(layout, layout) }
    }

    /// Push a frame whose layout something else keeps alive for as long as
    /// the frame is on the stack, sparing it a reference count.
    ///
    /// # Safety
    ///
    /// `layout` has to outlive the frame.
    #[inline(always)]
    pub(crate) unsafe fn push_borrowed(&mut self, layout: &IrLayout) -> Result<Frame, InterpError> {
        let layout = layout as *const IrLayout;
        let tagged = (layout as usize | BORROWED) as *const IrLayout;
        // SAFETY: the caller's.
        unsafe { self.push_raw(layout, tagged) }
    }

    /// Push a frame for `layout`, recording `header_layout` in its header.
    #[inline(always)]
    unsafe fn push_raw(&mut self, layout: *const IrLayout, header_layout: *const IrLayout) -> Result<Frame, InterpError> {
        // SAFETY: the callers'.
        let layout_ref = unsafe { &*layout };
        let size = layout_ref.frame_size as usize;
        let align = layout_ref.frame_align as usize;
        debug_assert!(align <= CHUNK_ALIGN, "a frame aligned to {} bytes", align);
        let (prev_chunk, prev_top) = (self.current, self.top);
        // An alignment is a power of two, so this is a mask, not a division.
        // The arithmetic wraps, so that it is not checked: release builds
        // check overflow, and nothing here comes near it, a chunk being far
        // smaller than the address space.
        let align_up = |n: usize| n.wrapping_add(align - 1) & !(align - 1);
        let mut start = align_up(self.top.wrapping_add(HEADER));
        if self.chunks.get(self.current).is_none_or(|c| start.wrapping_add(size) > c.size) {
            self.next_chunk(HEADER + align + size)?;
            start = align_up(self.top.wrapping_add(HEADER));
        }
        // SAFETY: `next_chunk` has made `current` a chunk, if it was not.
        let chunk = unsafe { self.chunks.get_unchecked(self.current) }.ptr.as_ptr();
        self.top = start.wrapping_add(size);
        // SAFETY: the header and the frame are inside the chunk, by the check
        // above, and the header is aligned, `start` being a multiple of at
        // least a word.
        unsafe {
            let base = chunk.add(start);
            (base.sub(HEADER) as *mut Header).write(Header {
                prev_chunk: prev_chunk as u32,
                prev_top: prev_top as u32,
                layout: header_layout,
            });
            if cfg!(debug_assertions) {
                // So that reading what was never written reads as garbage
                // rather than as what the last frame here left.
                std::ptr::write_bytes(base, datalove_rt::rust::POISON, size);
            }
            Ok(Frame::at(base, layout))
        }
    }

    /// Move on to a chunk with room for `needed` bytes, allocating it if the
    /// next one is missing or too small.
    #[cold]
    fn next_chunk(&mut self, needed: usize) -> Result<(), InterpError> {
        let next = if self.chunks.is_empty() { 0 } else { self.current + 1 };
        if self.chunks.get(next).is_some_and(|c| c.size < needed) {
            let chunk = self.chunks.remove(next);
            self.allocated -= chunk.size;
            // SAFETY: allocated by `allocate` with this size, and above the
            // top, so nothing is in it.
            unsafe { std::alloc::dealloc(chunk.ptr.as_ptr(), Self::alloc_layout(chunk.size)) };
        }
        if self.chunks.get(next).is_none() {
            let previous = self.chunks.last().map_or(FIRST_CHUNK / 2, |c| c.size);
            let size = (previous * 2).max(needed.next_power_of_two());
            if self.allocated + size > self.limit {
                return Err(InterpError::StackOverflow);
            }
            // SAFETY: the size is not zero.
            let ptr = unsafe { std::alloc::alloc(Self::alloc_layout(size)) };
            let ptr = NonNull::new(ptr).unwrap_or_else(|| std::alloc::handle_alloc_error(Self::alloc_layout(size)));
            self.chunks.insert(next, Chunk { ptr, size });
            self.allocated += size;
        }
        self.current = next;
        self.top = 0;
        Ok(())
    }

    fn alloc_layout(size: usize) -> AllocLayout {
        AllocLayout::from_size_align(size, CHUNK_ALIGN).expect("a frame stack chunk's layout")
    }

    /// Pop the frame on top, which has to be `frame`.
    #[inline]
    pub fn pop(&mut self, frame: Frame) {
        // SAFETY: `frame` was pushed here, so a header precedes it.
        let header = unsafe { (frame.base.sub(HEADER) as *const Header).read() };
        debug_assert_eq!(header.layout as usize & !BORROWED, frame.layout as usize,
            "popped a frame that is not on top");
        self.current = header.prev_chunk as usize;
        self.top = header.prev_top as usize;
        if header.layout as usize & BORROWED == 0 {
            // SAFETY: from `Rc::into_raw` at the push, and released once.
            drop(unsafe { Rc::from_raw(header.layout) });
        }
    }
}

impl Default for FrameStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FrameStack {
    fn drop(&mut self) {
        for chunk in &self.chunks {
            // SAFETY: allocated by `next_chunk` with this size.
            unsafe { std::alloc::dealloc(chunk.ptr.as_ptr(), Self::alloc_layout(chunk.size)) };
        }
    }
}

/// A frame: where its bytes are, and their layout.
///
/// A handle, copied freely; the bytes belong to the `FrameStack` or to a
/// `ScriptFrame`. Laid out by `IrLayout`: values, slots and tracking bytes where
/// compiled code puts them, then the parameters, the shape descriptors, the
/// descriptors of references and, where they are kept, the liveness bytes.
#[derive(Clone, Copy)]
pub struct Frame {
    base: *mut u8,
    layout: *const IrLayout,
    /// The liveness bytes, while they are being kept; null when not. See
    /// `IrLayout::liveness_offset`.
    liveness: *mut u8,
}

impl Frame {
    /// The frame whose bytes start at `base`.
    ///
    /// # Safety
    ///
    /// `base` has to be `layout.frame_size` bytes of frame, aligned as it says,
    /// and both have to outlive every use of the handle.
    unsafe fn at(base: *mut u8, layout: *const IrLayout) -> Self {
        let liveness = match unsafe { (*layout).liveness_offset } {
            Some(offset) => unsafe { base.add(offset as usize) },
            None => std::ptr::null_mut(),
        };
        Frame { base, layout, liveness }
    }

    /// A bytecode frame, keeping no liveness, at `base`.
    ///
    /// # Safety
    ///
    /// As `at`.
    #[inline(always)]
    pub(crate) unsafe fn without_liveness(base: *mut u8, layout: *const IrLayout) -> Self {
        Frame { base, layout, liveness: std::ptr::null_mut() }
    }

    /// The layout, as a pointer.
    #[inline(always)]
    pub(crate) fn layout_ptr(&self) -> *const IrLayout {
        self.layout
    }

    /// The layout this frame was made for.
    ///
    /// Not tied to the borrow of the handle: the layout lives as long as the
    /// frame, which outlives every use of the handle.
    #[inline(always)]
    pub fn layout<'a>(&self) -> &'a IrLayout {
        // SAFETY: held by the frame's header or its `ScriptFrame`.
        unsafe { &*self.layout }
    }

    /// Where the frame's data starts.
    #[inline(always)]
    pub(crate) fn base_ptr(&self) -> *mut u8 {
        self.base
    }

    /// Stop keeping liveness for a frame the bytecode runs.
    ///
    /// The bytecode keeps no flags for values and untracked slots -- a release
    /// build keeps none either -- so the debug checks that read them would
    /// fire on what it never wrote. The IR walker remains the checked engine.
    pub(crate) fn stop_keeping_liveness(&mut self) {
        self.liveness = std::ptr::null_mut();
    }

    #[inline(always)]
    fn at_offset(&self, offset: u32) -> *mut u8 {
        // SAFETY: every offset the layout gives is inside the frame.
        unsafe { self.base.add(offset as usize) }
    }

    /// Whether liveness is being kept, and the byte for binding `index` if so:
    /// values first, then slots, then parameters.
    #[inline(always)]
    fn liveness_byte(&self, index: usize) -> Option<*mut bool> {
        if self.liveness.is_null() {
            return None;
        }
        // SAFETY: the liveness bytes are inside the frame, by the layout.
        Some(unsafe { self.liveness.add(index) as *mut bool })
    }

    #[inline(always)]
    fn kept_live(&self, index: usize) -> Option<bool> {
        // SAFETY: as `liveness_byte`, and always written before it is read.
        self.liveness_byte(index).map(|b| unsafe { *b })
    }

    #[inline(always)]
    fn set_kept_live(&self, index: usize, live: bool) {
        if let Some(b) = self.liveness_byte(index) {
            // SAFETY: as `liveness_byte`.
            unsafe { *b = live };
        }
    }

    fn slot_index(&self, id: SlotId) -> usize {
        self.layout().value_offsets.len() + id.0 as usize
    }

    fn param_index(&self, id: ParamId) -> usize {
        let layout = self.layout();
        layout.value_offsets.len() + layout.slot_offsets.len() + id.0 as usize
    }

    /// Hand over argument `index`, as the caller has it.
    #[inline(always)]
    pub fn set_param(&mut self, index: usize, value: Value) {
        let offset = self.layout().param_offsets[index];
        // SAFETY: a parameter's place is inside the frame and word-aligned.
        unsafe { (self.at_offset(offset) as *mut Value).write(value) };
    }

    /// Hand over the descriptor for the shape declared at `index`.
    #[inline]
    pub fn set_shape_descriptor(&mut self, index: usize, tydesc: *const TyDesc) {
        let layout = self.layout();
        assert!(index < layout.shape_count as usize,
            "a descriptor for shape {} of {}", index, layout.shape_count);
        let offset = layout.shape_offset + (index * std::mem::size_of::<usize>()) as u32;
        // SAFETY: inside the shape region, word-aligned.
        unsafe { (self.at_offset(offset) as *mut *const TyDesc).write(tydesc) };
    }

    /// The descriptor handed over for the shape declared at `index`.
    pub fn shape_descriptor(&self, index: u32) -> Option<*const TyDesc> {
        self.shape_descriptors().get(index as usize).copied()
    }

    /// The arguments, once every one has been set.
    ///
    /// As the caller handed them over until `enter`, which gives the owned
    /// ones the callee's own descriptors.
    pub fn params(&self) -> &[Value] {
        let layout = self.layout();
        match layout.param_offsets.first() {
            // SAFETY: the parameters are consecutive `Value`s in the frame,
            // all set.
            Some(&first) => unsafe {
                std::slice::from_raw_parts(self.at_offset(first) as *const Value, layout.param_offsets.len())
            },
            None => &[],
        }
    }

    /// The shape descriptors, once every one has been set.
    pub fn shape_descriptors(&self) -> &[*const TyDesc] {
        let layout = self.layout();
        // SAFETY: the shape region is consecutive words in the frame, all set.
        unsafe {
            std::slice::from_raw_parts(
                self.at_offset(layout.shape_offset) as *const *const TyDesc,
                layout.shape_count as usize)
        }
    }

    /// Make a function frame ready to run its body, once every argument has
    /// been set.
    ///
    /// A parameter the callee owns gets the callee's own descriptor: the caller
    /// has already converted it into the shape the callee was compiled for. A
    /// borrowed one keeps the descriptor it came with, because a generic
    /// function was compiled with `data` where its type parameter stood and the
    /// value at that pointer is whatever the caller actually has; for a function
    /// that is not generic the two agree anyway. An `out` parameter starts
    /// uninitialized, since the callee writes it first.
    #[inline]
    pub fn enter(&mut self) {
        let layout = self.layout();
        for (i, mode) in layout.param_modes.iter().enumerate() {
            if matches!(mode, ParamMode::In | ParamMode::Out) {
                let offset = layout.param_offsets[i] + std::mem::size_of::<usize>() as u32;
                // SAFETY: the descriptor half of a parameter's place.
                unsafe { (self.at_offset(offset) as *mut *const TyDesc).write(layout.param_tydescs[i]) };
            }
            if let Some((at, size)) = layout.param_copies[i] {
                // SAFETY: the parameter's place holds the caller's pointer to
                // `size` bytes, and its copy's place is inside the frame.
                unsafe {
                    let place = self.at_offset(layout.param_offsets[i]) as *mut *mut u8;
                    let copy = self.at_offset(at);
                    std::ptr::copy_nonoverlapping(place.read(), copy, size as usize);
                    place.write(copy);
                }
            }
        }
        self.clear_tracking();
        if !self.liveness.is_null() {
            let (values, slots) = (layout.value_offsets.len(), layout.slot_offsets.len());
            for i in 0..values + slots {
                self.set_kept_live(i, false);
            }
            for (i, mode) in layout.param_modes.iter().enumerate() {
                self.set_kept_live(values + slots + i, *mode != ParamMode::Out);
            }
        }
    }

    /// Set every tracking byte to say its binding has never been written.
    #[inline]
    pub(crate) fn clear_tracking(&mut self) {
        let layout = self.layout();
        // A byte at a time: there are a handful at most, and `write_bytes` of a
        // length only known at run time is a call into `memset`, which was most
        // of what entering a function cost.
        for i in 0..layout.tracking_count {
            // SAFETY: the tracking bytes are inside the frame, by the layout.
            unsafe { *self.at_offset(layout.tracking_offset.wrapping_add(i)) = tracking::UNINIT };
        }
    }

    /// The tracking byte at `offset`.
    #[inline(always)]
    fn tracking_byte(&self, offset: u32) -> u8 {
        // SAFETY: a tracking byte offset is inside the frame, by the layout.
        unsafe { *self.at_offset(offset) }
    }

    #[inline(always)]
    fn set_tracking_byte(&mut self, offset: u32, state: u8) {
        // SAFETY: a tracking byte offset is inside the frame, by the layout.
        unsafe { *self.at_offset(offset) = state };
    }

    /// Whether a value holds something.
    ///
    /// A value is precise, so in a function it holds something wherever the
    /// program asks. A script frame says from its liveness bytes, since a unit
    /// that failed part way may not have reached it.
    #[inline]
    pub fn value_is_live(&self, id: ValueId) -> bool {
        let idx = id.0 as usize;
        if self.layout().is_script {
            return self.kept_live(idx).expect("a script frame keeps liveness");
        }
        debug_assert!(self.kept_live(idx) != Some(false),
            "value {:?} was taken to hold something and does not", id);
        true
    }

    /// Mark a value as holding something.
    #[inline(always)]
    pub fn mark_value_live(&mut self, id: ValueId) {
        self.set_kept_live(id.0 as usize, true);
    }

    /// Mark a value as holding nothing, having been moved or destroyed.
    #[inline(always)]
    pub fn mark_value_dropped(&mut self, id: ValueId) {
        self.set_kept_live(id.0 as usize, false);
    }

    /// Get destination for a value.
    ///
    /// Panics if the value ID is out of bounds.
    #[inline(always)]
    pub fn value_dest(&mut self, id: ValueId) -> Destination {
        let idx = id.0 as usize;
        let layout = self.layout();
        Destination { ptr: self.at_offset(layout.value_offsets[idx]), tydesc: layout.value_tydescs[idx] }
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
        let layout = self.layout();
        Value { ptr: self.at_offset(layout.value_offsets[idx]), tydesc: layout.value_tydescs[idx] }
    }

    /// Dereference a ref value to get the pointed-to data.
    ///
    /// What it points at is described by the reference's descriptor word,
    /// where it has one, and otherwise by its static type, which wraps the
    /// referent's as a one-field tuple.
    ///
    /// Panics if value ID is out of bounds (compiler bug).
    #[inline]
    pub fn value_deref(&self, id: ValueId) -> Value {
        let idx = id.0 as usize;
        let layout = self.layout();
        // SAFETY: a reference value holds a pointer.
        let ptr = unsafe { *(self.at_offset(layout.value_offsets[idx]) as *const *mut u8) };
        let tydesc = match layout.ref_desc_offsets[idx] {
            // SAFETY: written by the projection that made the reference.
            Some(offset) => unsafe { *(self.at_offset(offset) as *const *const TyDesc) },
            None => static_referent(layout.value_tydescs[idx]),
        };
        Value { ptr, tydesc }
    }

    /// Record what a reference points at, as a projection found it.
    ///
    /// Only a reference `resolve_ref_descriptors` names has a word for it;
    /// any other is described by its static type, which the projection's
    /// finding has to agree with.
    #[inline]
    pub fn set_value_tydesc(&mut self, id: ValueId, tydesc: *const TyDesc) {
        let idx = id.0 as usize;
        let layout = self.layout();
        match layout.ref_desc_offsets[idx] {
            // SAFETY: the reference's descriptor word.
            Some(offset) => unsafe { (self.at_offset(offset) as *mut *const TyDesc).write(tydesc) },
            None => debug_assert_eq!(
                unsafe { (*tydesc).size },
                unsafe { (*static_referent(layout.value_tydescs[idx])).size },
                "reference {:?} has no descriptor word, and its static type is not what it found", id),
        }
    }

    /// Get destination for a slot.
    ///
    /// Panics if slot ID is out of bounds (compiler bug).
    #[inline(always)]
    pub fn slot_dest(&mut self, id: SlotId) -> Destination {
        let idx = id.0 as usize;
        let layout = self.layout();
        Destination { ptr: self.at_offset(layout.slot_offsets[idx]), tydesc: layout.slot_tydescs[idx] }
    }

    /// Get slot value (for reading).
    ///
    /// Panics if slot ID is out of bounds (compiler bug), and in a debug build
    /// where liveness is kept, if the slot holds nothing.
    #[inline(always)]
    pub fn slot(&self, id: SlotId) -> Value {
        let idx = id.0 as usize;
        debug_assert!(self.kept_live(self.slot_index(id)) != Some(false), "read of an empty slot {:?}", id);
        let layout = self.layout();
        Value { ptr: self.at_offset(layout.slot_offsets[idx]), tydesc: layout.slot_tydescs[idx] }
    }

    /// Whether a slot holds something to destroy.
    ///
    /// A tracked slot says from its tracking byte, and a script frame's
    /// untracked slot from its liveness byte. An untracked slot in a function
    /// is taken to hold something, as compiled code takes it: the analysis
    /// tracks every slot whose state it cannot fix and whose type owns
    /// something, so an untracked slot either holds a value or is of a type
    /// that owns nothing, and destroying it is right either way.
    #[inline]
    pub fn slot_is_live(&self, id: SlotId) -> bool {
        let idx = id.0 as usize;
        let layout = self.layout();
        let kept = self.kept_live(self.slot_index(id));
        match layout.slot_tracking[idx] {
            Some(offset) => {
                let live = self.tracking_byte(offset) == tracking::LIVE;
                debug_assert!(kept.is_none_or(|k| k == live),
                    "slot {:?} is {} by its tracking byte and not by its flags",
                    id, if live { "live" } else { "empty" });
                live
            }
            None if layout.is_script => kept.expect("a script frame keeps liveness"),
            None => {
                debug_assert!(kept != Some(false) || layout.slot_is_copy[idx],
                    "untracked slot {:?} owns something and holds nothing", id);
                true
            }
        }
    }

    /// Check, in a debug build where liveness is kept, that a slot about to be
    /// moved into holds nothing, since overwriting what it held would leak it.
    #[inline]
    pub fn check_slot_empty(&self, id: SlotId) {
        debug_assert!(self.kept_live(self.slot_index(id)) != Some(true), "move into occupied slot {:?}", id);
    }

    /// Mark slot as holding something.
    #[inline(always)]
    pub fn mark_slot_live(&mut self, id: SlotId) {
        if let Some(offset) = self.layout().slot_tracking[id.0 as usize] {
            self.set_tracking_byte(offset, tracking::LIVE);
        }
        self.set_kept_live(self.slot_index(id), true);
    }

    /// Mark slot as holding nothing, having been moved or destroyed.
    #[inline]
    pub fn mark_slot_dropped(&mut self, id: SlotId) {
        if let Some(offset) = self.layout().slot_tracking[id.0 as usize] {
            self.set_tracking_byte(offset, tracking::MOVED);
        }
        self.set_kept_live(self.slot_index(id), false);
    }

    /// Read param (dereferences pointer to caller's data).
    ///
    /// Panics if param ID is out of bounds (compiler bug), and in a debug build
    /// where liveness is kept, if the parameter holds nothing.
    #[inline(always)]
    pub fn param(&self, id: ParamId) -> Value {
        debug_assert!(self.kept_live(self.param_index(id)) != Some(false), "read of an absent param {:?}", id);
        let offset = self.layout().param_offsets[id.0 as usize];
        // SAFETY: a parameter's place, set before the frame was entered.
        unsafe { (self.at_offset(offset) as *const Value).read() }
    }

    /// Get mutable destination for Mut/Out params.
    ///
    /// Panics if param ID is out of bounds (compiler bug).
    pub fn param_dest(&self, id: ParamId) -> Destination {
        let offset = self.layout().param_offsets[id.0 as usize];
        // SAFETY: a parameter's place, set before the frame was entered.
        let param = unsafe { (self.at_offset(offset) as *const Value).read() };
        Destination { ptr: param.ptr, tydesc: param.tydesc }
    }

    /// Whether a parameter holds something.
    ///
    /// An `out` parameter says from its tracking byte. Any other holds what
    /// the caller passed, wherever the program asks.
    #[inline]
    pub fn param_is_live(&self, id: ParamId) -> bool {
        let live = match self.layout().param_tracking[id.0 as usize] {
            Some(offset) => self.tracking_byte(offset) == tracking::LIVE,
            None => true,
        };
        debug_assert!(self.kept_live(self.param_index(id)).is_none_or(|k| k == live),
            "param {:?} is {} by the frame and not by its flags",
            id, if live { "live" } else { "empty" });
        live
    }

    /// Mark param as holding something (after the first write to an `out` one).
    #[inline]
    pub fn mark_param_live(&mut self, id: ParamId) {
        if let Some(offset) = self.layout().param_tracking[id.0 as usize] {
            self.set_tracking_byte(offset, tracking::LIVE);
        }
        self.set_kept_live(self.param_index(id), true);
    }

    /// Mark param as holding nothing, having been moved or destroyed.
    #[inline]
    pub fn mark_param_dropped(&mut self, id: ParamId) {
        if let Some(offset) = self.layout().param_tracking[id.0 as usize] {
            self.set_tracking_byte(offset, tracking::MOVED);
        }
        self.set_kept_live(self.param_index(id), false);
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

/// What a reference's static type says it points at: its descriptor wraps the
/// referent's as a one-field tuple.
#[inline(always)]
fn static_referent(ref_tydesc: *const TyDesc) -> *const TyDesc {
    // SAFETY: a reference's descriptor is such a tuple.
    unsafe { (*(*ref_tydesc).type_info.tuple.fields).tydesc }
}

/// A script unit's frame, which owns its bytes.
///
/// A script unit has no parameters, and its frame outlives it in the
/// `FrameStore` rather than going on the stack: later units read and move its
/// bindings, and its address has to stay put for the whole session, which the
/// buffer's does when the store's list of frames grows.
pub struct ScriptFrame {
    data: AlignedBuffer,
    layout: Rc<IrLayout>,
}

impl ScriptFrame {
    /// Create the frame for a script unit.
    pub fn new(unit: &IrCodeUnit, layout: Rc<IrLayout>) -> Self {
        if let CodeUnitContext::Native(ctx) = &unit.context {
            panic!("native functions are dispatched directly, not via Frame: {}", ctx.symbol())
        }
        assert!(unit.function_context().is_none(), "function frames come from the stack");
        assert!(layout.is_script, "a script frame on a function's layout");
        // Uninitialized, not zeroed: what has been written is tracked, and
        // reading what has not is a bug rather than a value worth defining.
        let data = AlignedBuffer::uninit(layout.frame_size as usize, layout.frame_align as usize);
        let script = Self { data, layout };
        let mut frame = script.frame();
        frame.clear_tracking();
        for i in 0..script.layout.value_offsets.len() + script.layout.slot_offsets.len() {
            frame.set_kept_live(i, false);
        }
        script
    }

    /// The frame, to work on.
    pub fn frame(&self) -> Frame {
        // SAFETY: the buffer is the layout's size and alignment, and both live
        // as long as this.
        unsafe { Frame::at(self.data.as_ptr() as *mut u8, Rc::as_ptr(&self.layout)) }
    }
}

/// Mutable frame storage for script unit execution.
///
/// Stores frames from previously executed units for external value/slot access.
/// A binding that is moved out of is marked uninitialized in the frame that
/// owns it, which is what makes it unreadable afterwards.
pub struct FrameStore {
    /// Frames from executed units, indexed by unit number.
    frames: Vec<ScriptFrame>,
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
        frame: ScriptFrame,
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
        frame: ScriptFrame,
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
        self.frames[unit].frame().destroy_unit_end_bindings(rt_handle, &old_values, &old_slots);

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
            self.frames[unit].frame().destroy_unit_end_bindings(rt_handle, values, slots);
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
        let mut frame = self.frame(unit);

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
        self.frame(unit).mark_value_dropped(value);
    }

    /// Mark an external slot as moved out.
    pub fn mark_external_slot_dropped(&mut self, unit: u32, slot: SlotId) {
        self.frame(unit).mark_slot_dropped(slot);
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
            self.frames[i].frame().destroy_unit_end_bindings(rt_handle, values, slots);
        }
    }

    /// The frame of a previous unit.
    ///
    /// Panics if the unit is not in the store (compiler bug).
    fn frame(&self, unit: u32) -> Frame {
        self.frames.get(unit as usize)
            .unwrap_or_else(|| panic!("external unit {} not found", unit))
            .frame()
    }
}

impl Default for FrameStore {
    fn default() -> Self {
        Self::new()
    }
}
