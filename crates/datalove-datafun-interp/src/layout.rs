//! Frame layout computation.
//!
//! Computes byte offsets for values and slots within a frame buffer,
//! respecting alignment requirements from type descriptors.

use std::rc::Rc;

use rustc_hash::FxHashMap;

use datalove_rtdt::TyDesc;
use datalove_datafun_ir::{IrCodeUnit, IrType};

use crate::dispatch::FuncIdentity;
use crate::tydesc::IrTyDescTable;

/// Align a value up to the given alignment.
#[inline]
pub fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}

/// Layout information for a function/unit frame.
///
/// Maps ValueId/SlotId to byte offsets within the frame. Everything a frame can
/// be told from its code unit alone lives here, so that entering the function
/// reads it rather than working it out again.
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
        return_type: Option<&IrType>,
        tydesc_table: &mut IrTyDescTable,
    ) -> Self {
        let mut value_offsets = Vec::with_capacity(value_types.len());
        let mut value_tydescs = Vec::with_capacity(value_types.len());
        let mut slot_offsets = Vec::with_capacity(slot_types.len());
        let mut slot_tydescs = Vec::with_capacity(slot_types.len());

        let mut offset: u32 = 0;
        let mut max_align: u32 = 1;

        // Layout values first.
        for ty in value_types {
            let tydesc = tydesc_table.get_or_create(ty);
            let size = unsafe { (*tydesc).size };
            let align = unsafe { (*tydesc).align };

            offset = align_up(offset, align);
            value_offsets.push(offset);
            value_tydescs.push(tydesc);
            offset += size;
            max_align = max_align.max(align);
        }

        // Then slots.
        for ty in slot_types {
            let tydesc = tydesc_table.get_or_create(ty);
            let size = unsafe { (*tydesc).size };
            let align = unsafe { (*tydesc).align };

            offset = align_up(offset, align);
            slot_offsets.push(offset);
            slot_tydescs.push(tydesc);
            offset += size;
            max_align = max_align.max(align);
        }

        // Parameters live in the caller's frame, so they take no space here and
        // only their descriptors are wanted.
        let param_tydescs = param_types.iter()
            .map(|ty| tydesc_table.get_or_create(ty))
            .collect();
        let return_tydesc = return_type.map(|ty| tydesc_table.get_or_create(ty));

        // Final alignment for frame size.
        let frame_size = align_up(offset, max_align);

        Self {
            value_offsets,
            value_tydescs,
            slot_offsets,
            slot_tydescs,
            param_tydescs,
            return_tydesc,
            frame_size,
            frame_align: max_align,
        }
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

        let (param_types, return_type) = match unit.function_context() {
            Some(ctx) => (&ctx.param_types[..], Some(&ctx.return_type)),
            None => (&[][..], None),
        };
        let layout = Rc::new(IrLayout::compute(
            &unit.value_types, &unit.slot_types, param_types, return_type, tydesc_table));
        self.entries.insert(key, CachedLayout {
            value_count,
            slot_count,
            layout: Rc::clone(&layout),
        });
        layout
    }
}
