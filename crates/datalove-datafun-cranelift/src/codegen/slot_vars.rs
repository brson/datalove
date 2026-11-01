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
//! through it, or keeps it past its instruction, rules the slot out: a `mut`
//! or `out` argument, a field write or drop, an update of a collection in
//! it, and a `GetFieldRef`, whose reference is used later. Writing through
//! the home and reading the slot back after was tried, and was slower than
//! memory: a narrow store into the home and the wide loads after it are a
//! store the processor cannot forward. Two writes are done in registers
//! instead. An update of a scalar (`set i +!= 1`) defines the slot's variable
//! anew. A reference to a field handed only to an open-coded `mem_swap` or
//! `mem_replace` is a place in the slot's words, and the swap or replace
//! exchanges the words of it.
//!
//! A tracked slot keeps its tracking byte in memory and stays there, and so
//! does every slot of an OSR entry, which is the interpreter's.

use std::collections::{HashMap, HashSet};

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData};
use cranelift_frontend::{FunctionBuilder, Variable};
use cranelift_module::Module;

use datalove_datafun_intrinsics::NativeIntrinsic;
use datalove_datafun_ir::{CodeRef, CodeUnitId, Instruction, IrType, Operand, ParamMode, SlotDest, SlotId, ValueId};

use crate::types::{self, CraneliftRepr};
use crate::CraneliftError;

use super::words::word_pieces;
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

/// How each word of a value of `ty` is held in a register: its offset and
/// type, one for a scalar.
fn reg_pieces(ty: &IrType) -> Option<Vec<(i32, cl_ir::Type)>> {
    if ty.is_dynamic() {
        return None;
    }
    match types::ir_type_to_cranelift(ty) {
        CraneliftRepr::Scalar(t) => Some(vec![(0, t)]),
        CraneliftRepr::Aggregate(_) => word_pieces(ty),
    }
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// The slots to hold in registers, and the field references to keep as
    /// places in them.
    pub(super) fn find_register_slots(&self) -> (HashSet<SlotId>, HashMap<ValueId, FieldPlace>) {
        if self.osr_spec.is_some() {
            return (HashSet::new(), HashMap::new());
        }
        let mut chosen: HashSet<SlotId> = self.func.slot_types.iter().enumerate()
            .map(|(i, ty)| (SlotId(i as u32), ty))
            .filter(|(id, ty)| !self.func.tracked_slots.contains(id)
                && !matches!(ty, IrType::Cursor(_))
                && reg_pieces(ty).is_some())
            .map(|(id, _)| id)
            .collect();

        // A slot written through its address, or handed where it may be.
        let mut pin = |op: &Operand| {
            if let Operand::Slot(s) = op {
                chosen.remove(s);
            }
        };
        for inst in self.func.blocks.iter().flat_map(|b| &b.instructions) {
            match inst {
                Instruction::Call { func, args, .. } | Instruction::ComptimeCall { func, args, .. } => {
                    let modes = self.callee_param_modes(func);
                    for (i, arg) in args.iter().enumerate() {
                        if !matches!(modes.get(i), Some(ParamMode::In | ParamMode::Ref)) {
                            pin(arg);
                        }
                    }
                }
                Instruction::OpAssign { place, .. } | Instruction::OpAssignChecked { place, .. } => {
                    let scalar = self.get_operand_type(place).is_ok_and(|ty| types::ir_type_to_cranelift(&ty).is_scalar());
                    if !scalar {
                        pin(place);
                    }
                }
                Instruction::SetField { slot: SlotDest::Local(s), .. }
                | Instruction::SetFieldTracked { slot: SlotDest::Local(s), .. } => pin(&Operand::Slot(*s)),
                Instruction::DropField { base, .. } => pin(base),
                Instruction::ListSet { list, .. } => pin(list),
                Instruction::MapSetValue { map, .. } | Instruction::MapUpsert { map, .. } => pin(map),
                Instruction::TensorSet { tensor, .. } => pin(tensor),
                _ => {}
            }
        }

        // How each value is used, for the field references.
        let mut uses: HashMap<ValueId, Vec<(usize, usize, usize)>> = HashMap::new();
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

        let mut places = HashMap::new();
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
        places.retain(|_, place| chosen.contains(&place.slot));
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
        let cut = reg_pieces(&self.func.slot_types[s.0 as usize]).expect("held in registers");
        for ((at, _), r) in cut.iter().zip(regs) {
            builder.ins().store(MemFlagsData::new(), r, home, *at);
        }
        Some(home)
    }

    /// Declare the variables of the slots held in registers.
    pub(super) fn declare_slot_vars(&mut self, builder: &mut FunctionBuilder, slots: &HashSet<SlotId>) {
        for &s in slots {
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
