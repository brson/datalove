//! Frame layout computation.
//!
//! Computes byte offsets for values and slots within a frame buffer,
//! respecting alignment requirements from type descriptors, and for what only
//! the interpreter keeps in a frame.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use datalove_rtdt::TyDesc;
use datalove_datafun_ir::frame_layout::{FrameLayout, SlotLayout};
use datalove_datafun_ir::layout::align_up;
use datalove_datafun_ir::{IrCodeUnit, IrType, ParamId, ParamMode, SlotId, ValueId};

use crate::bytecode::BcFunction;
use crate::dispatch::FuncIdentity;
use crate::tydesc::IrTyDescTable;

/// Layout information for a function/unit frame.
///
/// Maps ValueId/SlotId to byte offsets within the frame. Everything a frame can
/// be told from its code unit alone lives here, so that entering the function
/// reads it rather than working it out again.
///
/// The offsets of values, slots and tracking bytes are
/// `ir::frame_layout::FrameLayout`'s, the layout compiled code uses, flattened
/// for the interpreter to read, with the descriptors the interpreter needs
/// beside them. After that layout's bytes comes an extension that is the
/// interpreter's alone, which compiled code neither has nor sees:
///
/// - the parameters, a `Value` each: a pointer to the caller's data and its
///   descriptor, so that the arguments are a slice of the frame;
/// - a descriptor per shape the function declares;
/// - a descriptor word for each reference whose referent's static type does
///   not describe it, as `resolve_ref_descriptors` says, the answer every
///   backend reads;
/// - where liveness is kept, a byte per value, slot and parameter.
///
/// So a frame is its bytes, and nothing about one lives anywhere else.
pub struct IrLayout {
    /// Offset for each ValueId.
    pub value_offsets: Vec<u32>,
    /// TyDesc for each ValueId.
    pub value_tydescs: Vec<*const TyDesc>,
    /// Offset for each SlotId.
    pub slot_offsets: Vec<u32>,
    /// TyDesc for each SlotId.
    pub slot_tydescs: Vec<*const TyDesc>,
    /// TyDesc for each parameter, as the callee's own signature describes it.
    ///
    /// What an owned parameter holds, since the caller converted it into the
    /// shape the callee was compiled for before handing it over. A borrowed one
    /// keeps the descriptor it arrived with instead and this is not read for it.
    pub param_tydescs: Vec<*const TyDesc>,
    /// TyDesc for what the function returns. `None` for a script unit.
    pub return_tydesc: Option<*const TyDesc>,
    /// How each parameter is passed. Empty for a script unit.
    pub param_modes: Vec<ParamMode>,
    /// Whether each argument is moved into the call, so that the caller has to
    /// stop owning it: an `in` parameter of a type that is not copied.
    ///
    /// Asked at every call, and walking the type to find out was a visible part
    /// of a call's cost, so it is answered once per body here.
    pub param_moves: Vec<bool>,
    /// The tracking byte of each slot the analysis could not make precise.
    pub slot_tracking: Vec<Option<u32>>,
    /// Whether each slot's type is copied, and so owns nothing to destroy.
    ///
    /// An untracked slot of such a type may hold nothing at all: the analysis
    /// does not track it because destroying it does nothing either way. The
    /// frame's debug checks need to know which those are.
    pub slot_is_copy: Vec<bool>,
    /// The tracking byte of each `out` parameter.
    pub param_tracking: Vec<Option<u32>>,
    /// Where each parameter's `Value` is: its pointer, then its descriptor.
    /// Consecutive, so that the parameters are a `[Value]` from the first.
    pub param_offsets: Vec<u32>,
    /// For each parameter the frame keeps a copy of, where the copy goes and
    /// its size: an owned parameter of a small copied type, whose `Value`
    /// points at the copy rather than into the caller's frame, so that a read
    /// of it is one load rather than two. Nothing can tell: the callee owns
    /// the argument, and the caller's original, being a copied type, is
    /// untouched either way.
    pub param_copies: Vec<Option<(u32, u32)>>,
    /// Where the descriptors for the declared shapes start, and how many.
    pub shape_offset: u32,
    pub shape_count: u32,
    /// The descriptor word of each reference value that has one: those
    /// `resolve_ref_descriptors` names. Any other reference points at what its
    /// static type says.
    pub ref_desc_offsets: Vec<Option<u32>>,
    /// Where the liveness bytes start, if this frame keeps them: one per value,
    /// then one per slot, then one per parameter.
    pub liveness_offset: Option<u32>,
    /// Whether this is a script unit's frame, whose untracked bindings say
    /// from their liveness bytes whether they hold anything.
    pub is_script: bool,
    /// The body lowered to bytecode, once something has asked for it, with
    /// the address of the body it was lowered from.
    ///
    /// The address is checked on the way out, so that a body replaced under
    /// the same layout -- which the cache keys by identity and value counts,
    /// not by body -- is lowered again rather than run as the old one.
    pub(crate) bytecode: std::cell::OnceCell<(usize, Rc<BcFunction>)>,
    /// Where the tracking bytes start, and how many there are.
    pub tracking_offset: u32,
    pub tracking_count: u32,
    /// Total frame size, the extension included.
    pub frame_size: u32,
    /// Frame alignment.
    pub frame_align: u32,
}

/// What a layout is for, beyond its types: what the extension holds.
pub struct LayoutExtras<'a> {
    /// How many shapes the function declares.
    pub shape_count: usize,
    /// The reference values that get a descriptor word.
    pub described_refs: &'a [ValueId],
    /// Whether the frame keeps liveness bytes.
    pub keep_liveness: bool,
    /// Whether the frame is a script unit's.
    pub is_script: bool,
    /// How each parameter is passed; empty if not known, as for a script.
    pub param_modes: &'a [ParamMode],
}

impl IrLayout {
    /// Compute layout from IrType arrays.
    pub fn compute(
        value_types: &[IrType],
        slot_types: &[IrType],
        param_types: &[IrType],
        tracked_slots: &[SlotId],
        tracked_params: &[ParamId],
        return_type: Option<&IrType>,
        extras: LayoutExtras,
        tydesc_table: &mut IrTyDescTable,
    ) -> Self {
        let frame = FrameLayout::compute(
            param_types, value_types, slot_types, tracked_slots, tracked_params);

        // The interpreter reads and writes a value by its descriptor's size, so
        // the descriptor has to agree with the layout the offsets came from.
        let mut described = |types: &[IrType], places: &[SlotLayout]| -> Vec<*const TyDesc> {
            types.iter().zip(places).map(|(ty, place)| {
                let tydesc = tydesc_table.get_or_create(ty);
                let (size, align) = unsafe { ((*tydesc).size, (*tydesc).align) };
                assert_eq!((size, align), (place.size, place.align),
                    "the descriptor of {:?} disagrees with its frame layout", ty);
                tydesc
            }).collect()
        };
        let value_tydescs = described(value_types, &frame.values);
        let slot_tydescs = described(slot_types, &frame.slots);

        let param_tydescs = param_types.iter()
            .map(|ty| tydesc_table.get_or_create(ty))
            .collect();
        let return_tydesc = return_type.map(|ty| tydesc_table.get_or_create(ty));

        // The extension, after the shared layout: the words first, then the
        // liveness bytes.
        const WORD: u32 = std::mem::size_of::<usize>() as u32;
        const VALUE: u32 = std::mem::size_of::<crate::value::Value>() as u32;
        let mut offset = align_up(frame.frame_size, WORD);
        let param_offsets: Vec<u32> = (0..param_types.len() as u32).map(|i| offset + i * VALUE).collect();
        offset += param_types.len() as u32 * VALUE;
        let param_copies: Vec<Option<(u32, u32)>> = param_types.iter().enumerate().map(|(i, ty)| {
            let size = datalove_datafun_ir::layout::layout_of(ty).size;
            let copied = extras.param_modes.get(i) == Some(&ParamMode::In) && ty.is_copy()
                && size > 0 && size <= WORD;
            copied.then(|| {
                let at = offset;
                offset += WORD;
                (at, size)
            })
        }).collect();
        let shape_offset = offset;
        offset += extras.shape_count as u32 * WORD;
        let mut ref_desc_offsets = vec![None; value_types.len()];
        for id in extras.described_refs {
            ref_desc_offsets[id.0 as usize] = Some(offset);
            offset += WORD;
        }
        let liveness_offset = extras.keep_liveness.then_some(offset);
        if extras.keep_liveness {
            offset += (value_types.len() + slot_types.len() + param_types.len()) as u32;
        }
        let frame_align = frame.frame_align.max(WORD);
        let frame_size = align_up(offset, frame_align);

        Self {
            value_offsets: frame.values.iter().map(|v| v.offset).collect(),
            value_tydescs,
            slot_offsets: frame.slots.iter().map(|s| s.offset).collect(),
            slot_tydescs,
            param_tydescs,
            return_tydesc,
            param_modes: Vec::new(),
            param_moves: Vec::new(),
            slot_tracking: frame.slots.iter().map(|s| s.tracking_byte).collect(),
            slot_is_copy: slot_types.iter().map(|ty| ty.is_copy()).collect(),
            param_tracking: frame.param_tracking.clone(),
            param_offsets,
            param_copies,
            shape_offset,
            shape_count: extras.shape_count as u32,
            ref_desc_offsets,
            liveness_offset,
            is_script: extras.is_script,
            bytecode: std::cell::OnceCell::new(),
            tracking_offset: frame.tracking_offset,
            tracking_count: frame.tracking_count,
            frame_size,
            frame_align,
        }
    }

    /// Compute the layout of a code unit, with what its calls need to know about
    /// its parameters.
    pub fn of_unit(unit: &IrCodeUnit, tydesc_table: &mut IrTyDescTable) -> Self {
        let described_refs: Vec<ValueId> =
            datalove_datafun_ir::resolve_ref_descriptors(unit).into_keys().collect();
        let Some(ctx) = unit.function_context() else {
            // A script frame keeps liveness always: the REPL frees what a unit
            // that failed part way had bound, and later units read and move
            // its bindings.
            let extras = LayoutExtras {
                shape_count: 0, described_refs: &described_refs, keep_liveness: true, is_script: true,
                param_modes: &[],
            };
            return Self::compute(
                &unit.value_types, &unit.slot_types, &[], &unit.tracked_slots, &[], None,
                extras, tydesc_table);
        };
        // A function frame keeps liveness in debug builds only, as a check on
        // the ownership analysis.
        let param_modes: Vec<ParamMode> = (0..ctx.param_types.len())
            .map(|i| ctx.param_modes.get(i).copied().unwrap_or(ParamMode::In))
            .collect();
        let extras = LayoutExtras {
            shape_count: ctx.descriptor_shapes.len(),
            described_refs: &described_refs,
            keep_liveness: cfg!(debug_assertions),
            is_script: false,
            param_modes: &param_modes,
        };
        let mut layout = Self::compute(
            &unit.value_types, &unit.slot_types, &ctx.param_types,
            &unit.tracked_slots, &ctx.tracked_params, Some(&ctx.return_type), extras, tydesc_table);
        layout.param_modes = param_modes;
        layout.param_moves = layout.param_modes.iter().zip(&ctx.param_types)
            .map(|(mode, ty)| *mode == ParamMode::In && !ty.is_copy())
            .collect();
        layout
    }
}

/// Frame layouts, computed once per body rather than once per call.
///
/// A layout is a function of a code unit's value and slot types alone, so
/// computing one per call recomputed an identical answer -- four allocations and
/// a type descriptor lookup per value and per slot, in the innermost loop there
/// is.
///
/// Keyed by the function rather than by the body's address, because an address
/// is not stable across an inlining and a `CodeRef::Local` is not a name on its
/// own; this is the same key the dispatcher remembers an optimized body under.
/// The body a key maps to does change, when the inliner replaces one, and
/// `value_count` is how that is noticed: inlining only ever adds values, so a
/// body with the count the entry was computed from is the body it was computed
/// from.
#[derive(Default)]
pub struct LayoutCache {
    entries: FxHashMap<FuncIdentity, CachedLayout>,
}

struct CachedLayout {
    /// What the layout was computed for, and the version number of the body.
    value_count: u32,
    slot_count: u32,
    layout: Rc<IrLayout>,
}

impl LayoutCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// The layout for `unit`, computing it if this is the first call to it or
    /// the first since the inliner replaced its body.
    pub fn get_or_compute(
        &mut self,
        key: FuncIdentity,
        unit: &IrCodeUnit,
        tydesc_table: &mut IrTyDescTable,
    ) -> Rc<IrLayout> {
        let value_count = unit.value_types.len() as u32;
        let slot_count = unit.slot_types.len() as u32;

        if let Some(entry) = self.entries.get(&key) {
            if entry.value_count == value_count && entry.slot_count == slot_count {
                return Rc::clone(&entry.layout);
            }
        }

        let layout = Rc::new(IrLayout::of_unit(unit, tydesc_table));
        self.entries.insert(key, CachedLayout {
            value_count,
            slot_count,
            layout: Rc::clone(&layout),
        });
        layout
    }
}
