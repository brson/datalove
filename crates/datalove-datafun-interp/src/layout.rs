//! Frame layout computation.
//!
//! Computes byte offsets for values and slots within a frame buffer,
//! respecting alignment requirements from type descriptors.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use datalove_rtdt::TyDesc;
use datalove_datafun_ir::frame_layout::{FrameLayout, SlotLayout};
use datalove_datafun_ir::{IrCodeUnit, IrType, ParamId, ParamMode, SlotId};

use crate::dispatch::FuncIdentity;
use crate::tydesc::IrTyDescTable;

/// Layout information for a function/unit frame.
///
/// Maps ValueId/SlotId to byte offsets within the frame. Everything a frame can
/// be told from its code unit alone lives here, so that entering the function
/// reads it rather than working it out again.
///
/// The offsets are `ir::frame_layout::FrameLayout`'s, the layout compiled code
/// uses, flattened for the interpreter to read, with the descriptors the
/// interpreter needs beside them.
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
    /// Where the tracking bytes start, and how many there are.
    pub tracking_offset: u32,
    pub tracking_count: u32,
    /// Total frame size.
    pub frame_size: u32,
    /// Frame alignment.
    pub frame_align: u32,
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
            param_tracking: frame.params.iter().map(|p| p.tracking_byte).collect(),
            tracking_offset: frame.tracking_offset,
            tracking_count: frame.tracking_count,
            frame_size: frame.frame_size,
            frame_align: frame.frame_align,
        }
    }

    /// Compute the layout of a code unit, with what its calls need to know about
    /// its parameters.
    pub fn of_unit(unit: &IrCodeUnit, tydesc_table: &mut IrTyDescTable) -> Self {
        let Some(ctx) = unit.function_context() else {
            return Self::compute(
                &unit.value_types, &unit.slot_types, &[], &unit.tracked_slots, &[], None,
                tydesc_table);
        };
        let mut layout = Self::compute(
            &unit.value_types, &unit.slot_types, &ctx.param_types,
            &unit.tracked_slots, &ctx.tracked_params, Some(&ctx.return_type), tydesc_table);
        layout.param_modes = (0..ctx.param_types.len())
            .map(|i| ctx.param_modes.get(i).copied().unwrap_or(ParamMode::In))
            .collect();
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
