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
/// - a copy of each small owned parameter of a copied type, which its `Value`
///   points at, so that reading it is one load;
/// - a descriptor per shape the function declares;
/// - under generics by reference, the descriptor handed over for each type
///   parameter, the descriptor table, and the shadow stack's position on
///   entry;
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
    /// Under generics by reference, where the descriptors handed over for
    /// the function's type parameters go, and how many.
    pub type_param_offset: u32,
    pub type_param_count: u32,
    /// Where the descriptor table's words start, and what fills each.
    pub table_offset: u32,
    pub table: Vec<TableEntry>,
    /// The type each entry of the table is the descriptor of.
    pub table_types: Vec<IrType>,
    /// The type parameter descriptors of the last call, and the table they
    /// filled, so that a call at the same binding copies the table rather
    /// than building it: a cache of one, as compiled code keeps.
    pub table_cache: std::cell::RefCell<(Vec<*const TyDesc>, Vec<*const TyDesc>)>,
    /// Where the shadow stack's top and limit on entry are kept, for a
    /// function that takes space on it.
    pub shadow_mark_offset: Option<u32>,
    /// Each value and slot of a dynamic type, as its frame offset and its
    /// type's descriptor table entry. Its frame bytes are a fat pointer, set
    /// on entry to space on the shadow stack its descriptor sizes.
    pub dynamic_places: Vec<(u32, u32)>,
    /// The descriptor word of each reference value that has one: those
    /// `resolve_ref_descriptors` names. Any other reference points at what its
    /// static type says.
    pub ref_desc_offsets: Vec<Option<u32>>,
    /// Where the view a `TensorIndexRef` makes of a rank 2 or higher tensor's
    /// row is built, for each value one is a reference to: in the frame, so it
    /// lasts as long as a reference can, which is the frame, and costs nothing
    /// to make again.
    pub tensor_view_offsets: Vec<Option<u32>>,
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

/// What fills one entry of a descriptor table on entry.
#[derive(Clone, Copy, Debug)]
pub enum TableEntry {
    /// The descriptor handed over for this type parameter.
    Param(u32),
    /// This template, instantiated against those.
    Template(*const TyDesc),
}

/// What a layout is for, beyond its types: what the extension holds.
pub struct LayoutExtras<'a> {
    /// How many type parameters the function was declared with, under
    /// generics by reference.
    pub type_param_count: u32,
    /// The function's descriptor table; see `FunctionContext::descriptor_table`.
    pub descriptor_table: &'a [IrType],
    /// How many shapes the function declares.
    pub shape_count: usize,
    /// The reference values that get a descriptor word.
    pub described_refs: &'a [ValueId],
    /// The reference values that point at a tensor view the frame holds.
    pub tensor_views: &'a [ValueId],
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

        // A reference to a dynamic type is a fat pointer, and is moved as one:
        // two words, plainly copied. Its second word is its descriptor word.
        let fat_reference = tydesc_table.get_or_create(&IrType::Tuple(vec![IrType::U64, IrType::U64]));

        // The interpreter reads and writes a value by its descriptor's size, so
        // the descriptor has to agree with the layout the offsets came from.
        //
        // A value of a dynamic type has none: its frame bytes are a fat
        // pointer, and the descriptor is the one in it. Null says so to the
        // frame's accessors.
        let mut described = |types: &[IrType], places: &[SlotLayout]| -> Vec<*const TyDesc> {
            types.iter().zip(places).map(|(ty, place)| {
                if ty.is_dynamic() {
                    return match ty {
                        IrType::Ref(_) => fat_reference,
                        _ => std::ptr::null(),
                    };
                }
                let tydesc = tydesc_table.get_or_create(ty);
                let (size, align) = unsafe { ((*tydesc).size, (*tydesc).align) };
                assert_eq!((size, align), (place.size, place.align),
                    "the descriptor of {:?} disagrees with its frame layout", ty);
                tydesc
            }).collect()
        };
        let value_tydescs = described(value_types, &frame.values);
        let slot_tydescs = described(slot_types, &frame.slots);

        // A dynamic parameter keeps the descriptor the caller handed over with
        // it, which is the only one that says what it is; null says so.
        let static_tydesc = |table: &mut IrTyDescTable, ty: &IrType| {
            if ty.is_dynamic() { std::ptr::null() } else { table.get_or_create(ty) }
        };
        let param_tydescs = param_types.iter()
            .map(|ty| static_tydesc(tydesc_table, ty))
            .collect();
        let return_tydesc = return_type.map(|ty| static_tydesc(tydesc_table, ty));

        let entry_of = |ty: &IrType| -> u32 {
            match extras.descriptor_table.iter().position(|t| t == ty) {
                Some(i) => i as u32,
                None => panic!("{ty} is not in the descriptor table"),
            }
        };
        let dynamic_places: Vec<(u32, u32)> = value_types.iter().zip(&frame.values)
            .chain(slot_types.iter().zip(&frame.slots))
            .filter(|(ty, _)| ty.is_dynamic() && !matches!(ty, IrType::Ref(_)))
            .map(|(ty, place)| (place.offset, entry_of(ty)))
            .collect();
        let table: Vec<TableEntry> = extras.descriptor_table.iter()
            .map(|ty| match ty {
                IrType::Param(i) => TableEntry::Param(*i),
                _ => TableEntry::Template(tydesc_table.get_or_create(ty)),
            })
            .collect();

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
        let type_param_offset = offset;
        offset += extras.type_param_count * WORD;
        let table_offset = offset;
        offset += table.len() as u32 * WORD;
        let shadow_mark_offset = (!dynamic_places.is_empty()).then(|| {
            let at = offset;
            offset += 2 * WORD;
            at
        });
        let mut ref_desc_offsets = vec![None; value_types.len()];
        for id in extras.described_refs {
            ref_desc_offsets[id.0 as usize] = Some(offset);
            offset += WORD;
        }
        // A reference to a dynamic type carries its descriptor in its own
        // second word.
        for (i, ty) in value_types.iter().enumerate() {
            if matches!(ty, IrType::Ref(_)) && ty.is_dynamic() {
                ref_desc_offsets[i] = Some(frame.values[i].offset + WORD);
            }
        }
        let mut tensor_view_offsets = vec![None; value_types.len()];
        for id in extras.tensor_views {
            tensor_view_offsets[id.0 as usize] = Some(offset);
            offset += align_up(std::mem::size_of::<datalove_rtdt::Tensor>() as u32, WORD);
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
            type_param_offset,
            type_param_count: extras.type_param_count,
            table_offset,
            table,
            table_types: extras.descriptor_table.to_vec(),
            table_cache: Default::default(),
            shadow_mark_offset,
            dynamic_places,
            ref_desc_offsets,
            tensor_view_offsets,
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
        let tensor_views: Vec<ValueId> = unit.blocks.iter()
            .flat_map(|b| &b.instructions)
            .filter_map(|instr| match instr {
                datalove_datafun_ir::Instruction::TensorIndexRef { dest, .. }
                    if matches!(&unit.value_types[dest.0 as usize],
                        IrType::Ref(inner) if matches!(**inner, IrType::Tensor(..))) => Some(*dest),
                _ => None,
            })
            .collect();
        let Some(ctx) = unit.function_context() else {
            // A script frame keeps liveness always: the REPL frees what a unit
            // that failed part way had bound, and later units read and move
            // its bindings.
            let extras = LayoutExtras {
                type_param_count: 0, descriptor_table: &[],
                shape_count: 0, described_refs: &described_refs, tensor_views: &tensor_views,
                keep_liveness: true, is_script: true,
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
            type_param_count: ctx.type_param_count,
            descriptor_table: &ctx.descriptor_table,
            shape_count: ctx.descriptor_shapes.len(),
            described_refs: &described_refs,
            tensor_views: &tensor_views,
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
/// Keyed by the function rather than by the body's address, because a
/// `CodeRef::Local` is not a name on its own; this is the same key the JIT
/// remembers a compiled body under. The body a key maps to changes when the
/// interpreter's bodies are all replaced, which empties the cache, and when a
/// unit's index is given to another unit -- every constant evaluation runs as
/// unit 0, and a unit that fails leaves its index to the next -- which forgets
/// that unit's entries (`IrInterpreter::forget_unit_bodies`).
#[derive(Default)]
pub struct LayoutCache {
    entries: FxHashMap<FuncIdentity, CachedLayout>,
}

struct CachedLayout {
    /// What the layout was computed for, to check that the body has not changed.
    value_count: u32,
    slot_count: u32,
    layout: Rc<IrLayout>,
}

impl LayoutCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the layouts of the functions of script unit `unit`.
    pub fn forget_unit(&mut self, unit: u32) {
        self.entries.retain(|key, _| !matches!(key, FuncIdentity::Unit { unit: u, .. } if *u == unit));
    }

    /// The layout for `unit`, computing it if this is the first call to it.
    pub fn get_or_compute(
        &mut self,
        key: FuncIdentity,
        unit: &IrCodeUnit,
        tydesc_table: &mut IrTyDescTable,
    ) -> Rc<IrLayout> {
        let value_count = unit.value_types.len() as u32;
        let slot_count = unit.slot_types.len() as u32;

        if let Some(entry) = self.entries.get(&key) {
            debug_assert!(
                entry.value_count == value_count && entry.slot_count == slot_count,
                "the body for {key:?} changed under its cached layout");
            return Rc::clone(&entry.layout);
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
