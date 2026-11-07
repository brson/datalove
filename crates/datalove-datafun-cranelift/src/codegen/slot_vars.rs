//! Slots in registers: a `var` nothing keeps the address of, as Cranelift
//! variables rather than at its home in the frame.
//!
//! A slot is otherwise read and written in memory at every use, which for a
//! loop counter is a store and a load each time round. A slot held in
//! registers is one variable per word of it -- one for a scalar -- defined by
//! each store and read by each load, and Cranelift threads them through the
//! blocks.
//!
//! A use that reads through the slot's address gets its home with the slot
//! just written there, as a value held as words does. A use that writes
//! through it, or keeps it past its instruction, rules the slot out unless it
//! is one compiled against the registers: a `mut` or `out` argument, a field
//! write or drop, an update of a collection in it, and a `GetFieldRef`, whose
//! reference is used later. What each instruction does to the places it names
//! the IR says (`Instruction::for_each_place_use`), for every instruction. Writing through
//! the home and reading the slot back after was tried, and was slower than
//! memory: a narrow store into the home and the wide loads after it are a
//! store the processor cannot forward. Two writes are done in registers
//! instead. An update of a scalar (`set i +!= 1`) defines the slot's variable
//! anew. A reference to a field handed only to an open-coded `mem_swap` or
//! `mem_replace` is a place in the slot's words, and the swap or replace
//! exchanges the words of it.
//!
//! A tracked slot keeps its tracking byte in memory and stays there.
//!
//! An OSR entry carries on in the interpreter's frame, and its slots are held
//! in registers as a function's are, read from the frame on entry. Nothing
//! reads them back: the code runs the rest of the call, and a call it abandons
//! is dropped by the interpreter without looking in the frame. A reference
//! the interpreter made to a slot before the entry is one the IR made, and
//! the analysis, which reads the whole function, has ruled that slot out.

use bct::lookup::{LookupMap, LookupSet};

use cranelift_codegen::ir as cl_ir;
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::Module;

use datalove_datafun_intrinsics::NativeIntrinsic;
use datalove_datafun_ir::{BlockId, CodeRef, CodeUnitId, Instruction, IrType, Operand, PlaceUse, SlotId, ValueId};

use crate::types::{self, CraneliftRepr};
use crate::CraneliftError;

use super::words::{load_regs, reads_argument, reg_pieces, store_regs, word_pieces};
use super::FunctionCompiler;

/// A reference to a field of a slot held in registers, kept as where the
/// field is rather than as an address.
#[derive(Clone)]
pub(super) struct FieldPlace {
    pub slot: SlotId,
    /// The field's offset in the slot.
    pub at: u32,
    pub ty: IrType,
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// The slots to hold in registers, and the field references to keep as
    /// places in them.
    pub(super) fn find_register_slots(&self) -> (LookupSet<SlotId>, LookupMap<ValueId, FieldPlace>) {
        let mut chosen: LookupSet<SlotId> = self.func.slot_types.iter().enumerate()
            .map(|(i, ty)| (SlotId(i as u32), ty))
            .filter(|(id, ty)| !self.func.tracked_slots.contains(id)
                && !matches!(ty, IrType::Cursor(_))
                && reg_pieces(ty).is_some())
            .map(|(id, _)| id)
            .collect();

        // A slot written through its address, or handed where it may be: any
        // use but those compiled against the registers.
        for inst in self.func.blocks.iter().flat_map(|b| &b.instructions) {
            let modes = inst.call_target().map(|func| self.callee_param_modes(func)).unwrap_or_default();
            inst.for_each_place_use(|op, place_use| {
                let Operand::Slot(s) = op else { return };
                let in_registers = match place_use {
                    PlaceUse::Read | PlaceUse::Dropped => true,
                    PlaceUse::Replaced => matches!(inst,
                        Instruction::SlotStoreCopy { .. } | Instruction::SlotStoreCopyTracked { .. }
                        | Instruction::SlotStoreMove { .. } | Instruction::SlotStoreMoveTracked { .. }),
                    PlaceUse::Changed => matches!(inst,
                        Instruction::OpAssign { .. } | Instruction::OpAssignChecked { .. })
                        && types::ir_type_to_cranelift(&self.func.slot_types[s.0 as usize]).is_scalar(),
                    // Decided below, with the field references.
                    PlaceUse::Lent => matches!(inst, Instruction::GetFieldRef { .. }),
                    PlaceUse::Passed(i) => reads_argument(&modes, i),
                };
                if !in_registers {
                    chosen.remove(&s);
                }
            });
        }

        // How each value is used, for the field references.
        let mut uses: LookupMap<ValueId, Vec<(usize, usize, usize)>> = LookupMap::default();
        for (b, block) in self.func.blocks.iter().enumerate() {
            for (i, inst) in block.instructions.iter().enumerate() {
                let mut at = 0;
                inst.for_each_operand(|op| {
                    if let Operand::Value(v) | Operand::ValueRef(v) = op {
                        uses.entry(*v).or_default().push((b, i, at));
                    }
                    at += 1;
                });
                if let Instruction::DropViaRef { ref_value } = inst {
                    uses.entry(*ref_value).or_default().push((b, i, usize::MAX));
                }
            }
            block.terminator.for_each_operand(|op| {
                if let Operand::Value(v) | Operand::ValueRef(v) = op {
                    uses.entry(*v).or_default().push((b, usize::MAX, 0));
                }
            });
        }

        let mut places = LookupMap::default();
        for block in &self.func.blocks {
            for inst in &block.instructions {
                let Instruction::GetFieldRef { dest, src: Operand::Slot(s), field_index } = inst else { continue };
                match self.slot_field_place(*s, *field_index)
                    .filter(|_| chosen.contains(s))
                    .filter(|_| uses.get(dest).is_some_and(|u| u.iter().all(|&use_at| self.swaps_through(use_at, *dest))))
                {
                    Some(place) => {
                        places.insert(*dest, place);
                    }
                    None => {
                        chosen.remove(s);
                    }
                }
            }
        }
        places.retain_unordered(|_, place| chosen.contains(&place.slot));
        (chosen, places)
    }

    /// Field `index` of slot `s`, if its words are whole words of the slot's.
    fn slot_field_place(&self, s: SlotId, index: u32) -> Option<FieldPlace> {
        let slot_ty = &self.func.slot_types[s.0 as usize];
        let fields: Vec<IrType> = match slot_ty {
            IrType::Tuple(tys) => tys.clone(),
            IrType::Struct(flds) => flds.iter().map(|(_, t)| t.clone()).collect(),
            _ => return None,
        };
        let ty = fields.get(index as usize)?.clone();
        let at = types::compute_tuple_field_offsets(&fields)[index as usize];
        let slot_cut = word_pieces(slot_ty)?;
        let field_cut = word_pieces(&ty)?;
        let whole = field_cut.iter()
            .all(|&(o, t)| slot_cut.contains(&(at as i32 + o, t)));
        whole.then_some(FieldPlace { slot: s, at, ty })
    }

    /// Whether the use of reference `r` at `(block, instruction, operand)` is
    /// as the place a `mem_swap` or `mem_replace` open-codes, its first
    /// argument, beside an argument that is not such a place itself.
    fn swaps_through(&self, (b, i, at): (usize, usize, usize), r: ValueId) -> bool {
        let Some(Instruction::Call { func, args, .. }) = self.func.blocks[b].instructions.get(i) else {
            return false;
        };
        if args.get(at) != Some(&Operand::ValueRef(r)) {
            return false;
        }
        // A swap exchanges one such place with memory; of two, the other is
        // given an address.
        let other_is_a_place = |v: &ValueId| *v != r && self.is_field_ref_of_slot(*v);
        match self.callee_intrinsic(func) {
            Some(NativeIntrinsic::MemSwap) => !args.iter().any(|a| matches!(a, Operand::ValueRef(v) if other_is_a_place(v))),
            Some(NativeIntrinsic::MemReplace) => at == 0,
            _ => false,
        }
    }

    /// Whether `v` is a reference to a field of a slot.
    fn is_field_ref_of_slot(&self, v: ValueId) -> bool {
        self.func.blocks.iter().flat_map(|b| &b.instructions)
            .any(|i| matches!(i, Instruction::GetFieldRef { dest, src: Operand::Slot(_), .. } if *dest == v))
    }

    /// What intrinsic native `code_ref` is, if it is one.
    fn callee_intrinsic(&self, code_ref: &CodeRef) -> Option<NativeIntrinsic> {
        let CodeRef::Module { module, id } = code_ref else { return None };
        self.registry?
            .get_module_function_as_unit(*module, CodeUnitId(id.0))?
            .native_context()?
            .intrinsic
    }

    /// The registers of slot `s`, if it is held in them.
    pub(super) fn slot_regs(&self, builder: &mut FunctionBuilder, s: SlotId) -> Option<Vec<cl_ir::Value>> {
        let vars = self.slot_vars.get(&s)?;
        Some(vars.iter().map(|&v| builder.use_var(v)).collect())
    }

    /// Give slot `s`, held in registers, the value in `regs`.
    pub(super) fn set_slot_regs(&self, builder: &mut FunctionBuilder, s: SlotId, regs: &[cl_ir::Value]) {
        for (&var, &r) in self.slot_vars[&s].iter().zip(regs) {
            builder.def_var(var, r);
        }
    }

    /// Where slot `s` lives in the frame.
    fn slot_home(&self, builder: &mut FunctionBuilder, s: SlotId) -> cl_ir::Value {
        let frame = self.frame_slot.expect("a slot has a home in the frame");
        frame.addr(builder, self.layout.slot_offset(s.0) as i32)
    }

    /// Write slot `s`, held in registers, to its home, and give the home's
    /// address, for a use that reads through it.
    pub(super) fn materialize_slot(&self, builder: &mut FunctionBuilder, s: SlotId) -> Option<cl_ir::Value> {
        let regs = self.slot_regs(builder, s)?;
        let home = self.slot_home(builder, s);
        store_regs(builder, home, &self.func.slot_types[s.0 as usize], &regs);
        Some(home)
    }

    /// In an OSR entry, give each slot held in registers that the compiled
    /// blocks name what the interpreter's frame holds for it.
    pub(super) fn load_slots_from_frame(&self, builder: &mut FunctionBuilder, compiled: &LookupSet<BlockId>) {
        let mut named = LookupSet::default();
        for block in self.func.blocks.iter().filter(|b| compiled.contains(&b.id)) {
            for inst in &block.instructions {
                inst.for_each_place_use(|op, _| {
                    if let Operand::Slot(s) = op {
                        named.insert(s);
                    }
                });
            }
            block.terminator.for_each_operand(|op| {
                if let Operand::Slot(s) = op {
                    named.insert(*s);
                }
            });
        }
        let mut slots: Vec<SlotId> = named.into_iter_unordered().filter(|s| self.slot_vars.contains_key(s)).collect();
        slots.sort_by_key(|s| s.0);
        for s in slots {
            let home = self.slot_home(builder, s);
            let regs = load_regs(builder, home, &self.func.slot_types[s.0 as usize]);
            self.set_slot_regs(builder, s, &regs);
        }
    }

    /// Declare the variables of the slots held in registers.
    pub(super) fn declare_slot_vars(&mut self, builder: &mut FunctionBuilder, slots: &LookupSet<SlotId>) {
        for &s in slots.sorted_by_key(|s| s.0) {
            let cut = reg_pieces(&self.func.slot_types[s.0 as usize]).expect("chosen for registers");
            let vars: Vec<Variable> = cut.iter().map(|&(_, t)| builder.declare_var(t)).collect();
            self.slot_vars.insert(s, vars);
        }
    }

    /// Store to slot `s`, held in registers: its variables given `value`.
    pub(super) fn store_slot_regs(&mut self, builder: &mut FunctionBuilder, s: SlotId, value: &Operand) -> Result<(), CraneliftError> {
        let ty = self.func.slot_types[s.0 as usize].clone();
        let regs = match types::ir_type_to_cranelift(&ty) {
            CraneliftRepr::Scalar(_) => vec![self.get_operand_value(builder, value)?],
            CraneliftRepr::Aggregate(_) => self.operand_words(builder, value, &ty)?,
        };
        self.set_slot_regs(builder, s, &regs);
        Ok(())
    }

    /// Load slot `s`, held in registers, as `dest`.
    pub(super) fn load_slot_regs(&mut self, builder: &mut FunctionBuilder, dest: ValueId, s: SlotId) -> Result<(), CraneliftError> {
        let regs = self.slot_regs(builder, s).expect("held in registers");
        match types::ir_type_to_cranelift(&self.func.slot_types[s.0 as usize]) {
            CraneliftRepr::Scalar(_) => {
                self.values.insert(dest, regs[0]);
                Ok(())
            }
            CraneliftRepr::Aggregate(_) => self.define_words(builder, dest, regs),
        }
    }

    /// The words of the field `place` names, out of its slot's registers.
    pub(super) fn field_place_words(&self, builder: &mut FunctionBuilder, place: &FieldPlace) -> Vec<cl_ir::Value> {
        let slot_ty = self.func.slot_types[place.slot.0 as usize].clone();
        let regs = self.slot_regs(builder, place.slot).expect("held in registers");
        match Self::extract_part(builder, &slot_ty, &regs, place.at, &place.ty).expect("a field of whole words") {
            Err(words) => words,
            Ok(scalar) => vec![scalar],
        }
    }

    /// Give the field `place` names the words `field`, in its slot's
    /// registers.
    pub(super) fn set_field_place_words(&self, builder: &mut FunctionBuilder, place: &FieldPlace, field: &[cl_ir::Value]) {
        let slot_ty = &self.func.slot_types[place.slot.0 as usize];
        let mut regs = self.slot_regs(builder, place.slot).expect("held in registers");
        let slot_cut = word_pieces(slot_ty).expect("a slot with fields is held as words");
        let field_cut = word_pieces(&place.ty).expect("a field of whole words");
        for (&(o, _), &w) in field_cut.iter().zip(field) {
            let i = slot_cut.iter().position(|&(at, _)| at == place.at as i32 + o).expect("a field of whole words");
            regs[i] = w;
        }
        self.set_slot_regs(builder, place.slot, &regs);
    }
}
