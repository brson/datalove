//! Lowering a function body's IR to ops: one instruction at a time, then the
//! optimization pass (`opt`) over the blocks, then laying them out.

use super::*;

// =============================================================================
// Lowering
// =============================================================================

/// The offset and type of a field of a tuple or struct whose layout its static
/// type says, which a type with no `data` in it does: elsewhere the runtime's
/// descriptor has the offsets.
fn static_field(ty: &IrType, index: u32) -> Option<(u32, &IrType)> {
    if has_data(ty) {
        return None;
    }
    let types: Vec<&IrType> = match ty {
        IrType::Tuple(fields) => fields.iter().collect(),
        IrType::Struct(fields) => fields.iter().map(|(_, t)| t).collect(),
        _ => return None,
    };
    let owned: Vec<IrType> = types.iter().map(|t| (*t).clone()).collect();
    let offset = *datalove_datafun_ir::layout::aggregate_field_offsets(&owned).get(index as usize)?;
    Some((offset, types[index as usize]))
}

/// Whether a type has a `data` anywhere in it: a type parameter, whose real
/// layout only its descriptor knows at run time.
fn has_data(ty: &IrType) -> bool {
    match ty {
        IrType::Data => true,
        IrType::Tuple(fs) => fs.iter().any(has_data),
        IrType::Struct(fs) => fs.iter().any(|(_, t)| has_data(t)),
        IrType::Enum(vs) => vs.iter().any(|(_, t)| t.as_ref().is_some_and(has_data)),
        IrType::Term(_, t) | IrType::List(t) | IrType::Set(t) | IrType::Option(t)
        | IrType::Result(t) | IrType::Tensor(t, _) | IrType::Ref(t) => has_data(t),
        IrType::Map(k, v) => has_data(k) || has_data(v),
        IrType::Table(cs) => cs.iter().any(|(_, t)| has_data(t)),
        _ => false,
    }
}

struct Lowering<'a> {
    func: &'a IrCodeUnit,
    layout: &'a IrLayout,
    /// Whether to run the optimization pass, `opt`, over what is lowered.
    optimize: bool,
    /// Each block's ops, the IR's blocks and then the stubs that pass an
    /// edge's arguments. Until they are assembled, a jump names a block here.
    blocks: Vec<Vec<Op>>,
    /// The stubs each IR block's edges go through, laid out after it.
    stubs: Vec<Vec<u32>>,
    /// The block ops are emitted into.
    cur: usize,
    pool: Vec<u8>,
    switches: Vec<SwitchTable>,
    calls: Vec<FastCall>,
    descs: Vec<*const rtdt::TyDesc>,
    rt: Vec<[(Loc, Desc); 3]>,
    statics: Vec<StaticSlot>,
    /// What was left to the IR walker, by name: each instruction's variant,
    /// or the edge or return that was.
    escapes: Vec<String>,
    /// How many times each value is read, for fusing an op into its only use.
    uses: Vec<u32>,
    /// The `u32` each value nothing but its constant writes holds.
    const_u32: Vec<Option<u32>>,
    /// Values something other than their defining instruction may write, an
    /// `out` argument or a reference store, which a hoisted constant would not
    /// be rewritten for.
    pinned: Vec<bool>,
    /// Whether each block is in a loop, where hoisting a constant out of it
    /// saves work rather than adding it to every call.
    in_loop: Vec<bool>,
    loops: Vec<LoopCounter>,
    /// The block being lowered, and the instruction in it.
    block: usize,
    index: usize,
}

impl<'a> Lowering<'a> {
    /// The static type of an operand, where its layout and descriptor are the
    /// truth about what is there.
    ///
    /// What the frame holds itself -- values, slots, parameters passed by
    /// value -- it holds erased, so a `data` in its type is a `data` in its
    /// bytes. What a borrowed parameter or a reference points at is the
    /// caller's, as it really is, which inside a generic the static type does
    /// not say; nor is anything that reaches another unit.
    fn typed(&self, op: &Operand) -> Option<&'a IrType> {
        let func = self.func;
        match op {
            Operand::Value(id) => Some(&func.value_types[id.0 as usize]),
            Operand::Slot(id) => Some(&func.slot_types[id.0 as usize]),
            Operand::Param(id) => {
                let ctx = func.function_context()?;
                let i = id.0 as usize;
                let ty = &ctx.param_types[i];
                let borrowed = matches!(ctx.param_modes.get(i), Some(ParamMode::Ref | ParamMode::Mut));
                (!borrowed || !has_data(ty)).then_some(ty)
            }
            Operand::ValueRef(id) => match &func.value_types[id.0 as usize] {
                IrType::Ref(inner) if !has_data(inner) => Some(inner),
                _ => None,
            },
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => None,
        }
    }

    /// Where an operand's descriptor comes from: the layout where its static
    /// type says, otherwise the frame, at run time -- a borrowed parameter's
    /// from the caller, a reference's from its descriptor word, which only
    /// the references `resolve_ref_descriptors` names have.
    fn desc(&self, op: &Operand) -> Option<Desc> {
        if self.typed(op).is_some() {
            return Some(Desc::Static(self.desc_of(op)?));
        }
        match op {
            Operand::Param(id) => Some(Desc::Param(id.0)),
            Operand::ValueRef(id) => match self.layout.ref_desc_offsets[id.0 as usize] {
                Some(_) => Some(Desc::Ref(id.0)),
                None => Some(Desc::Static(self.desc_of(op)?)),
            },
            _ => None,
        }
    }

    /// An operand's place and descriptor, for an op that hands them to an IR
    /// walker routine.
    fn rt_operand(&self, op: &Operand) -> Option<(Loc, Desc)> {
        Some((self.loc(op)?, self.desc(op)?))
    }

    /// Record operands for an op that hands them to an IR walker routine.
    fn rt_push(&mut self, operands: &[(Loc, Desc)]) -> u32 {
        let mut triple = [(Loc(0), Desc::Static(std::ptr::null())); 3];
        triple[..operands.len()].copy_from_slice(operands);
        self.rt.push(triple);
        (self.rt.len() - 1) as u32
    }

    fn loc(&self, op: &Operand) -> Option<Loc> {
        let layout = self.layout;
        Some(match op {
            Operand::Value(id) => Loc::direct(layout.value_offsets[id.0 as usize]),
            Operand::Slot(id) => Loc::direct(layout.slot_offsets[id.0 as usize]),
            Operand::Param(id) => match layout.param_copies[id.0 as usize] {
                Some((at, _)) => Loc::direct(at),
                None => Loc::indirect(layout.param_offsets[id.0 as usize]),
            },
            Operand::ValueRef(id) => Loc::indirect(layout.value_offsets[id.0 as usize]),
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => return None,
        })
    }

    fn value_loc(&self, id: ValueId) -> Loc {
        Loc::direct(self.layout.value_offsets[id.0 as usize])
    }

    fn value_type(&self, id: ValueId) -> Option<&'a IrType> {
        self.typed(&Operand::Value(id))
    }

    /// An operand an instruction consumes, where the frame need not record
    /// its being consumed: anything without a tracking byte, which the frame
    /// of a body run as bytecode keeps no other record of.
    fn consumed(&self, op: &Operand) -> Option<Loc> {
        if self.tracking(op).is_some() {
            return None;
        }
        self.loc(op)
    }

    /// A slot's or parameter's tracking byte, if it has one.
    fn tracking(&self, op: &Operand) -> Option<u32> {
        match op {
            Operand::Slot(id) => self.layout.slot_tracking[id.0 as usize],
            Operand::Param(id) => self.layout.param_tracking[id.0 as usize],
            _ => None,
        }
    }

    /// Whether a read of an operand leaves it where it was.
    fn is_copy(&self, op: &Operand) -> bool {
        let func = self.func;
        let ty = match op {
            Operand::Value(id) | Operand::ValueRef(id) => func.value_types.get(id.0 as usize),
            Operand::Slot(id) => func.slot_types.get(id.0 as usize),
            Operand::Param(id) => func.function_context().and_then(|c| c.param_types.get(id.0 as usize)),
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => None,
        };
        ty.is_some_and(|t| t.is_copy())
    }

    /// A routine from destination `dest` and source `src`, where the source
    /// is left behind or consumed without a record of it.
    fn lower_rt_unary(&mut self, dest: ValueId, src: &Operand, op: fn(u32) -> Op) -> bool {
        if !self.is_copy(src) && self.consumed(src).is_none() {
            return false;
        }
        let (Some(d), Some(s)) = (self.rt_operand(&Operand::Value(dest)), self.rt_operand(src)) else {
            return false;
        };
        let at = self.rt_push(&[d, s]);
        self.emit(op(at));
        true
    }

    /// The descriptor of what an operand holds, from the layout.
    fn desc_of(&self, op: &Operand) -> Option<*const rtdt::TyDesc> {
        let layout = self.layout;
        Some(match op {
            Operand::Value(id) => layout.value_tydescs[id.0 as usize],
            Operand::Slot(id) => layout.slot_tydescs[id.0 as usize],
            Operand::Param(id) => layout.param_tydescs[id.0 as usize],
            // A reference's descriptor wraps what it points at as a one-field
            // tuple, as `Frame::value_deref` reads it.
            Operand::ValueRef(id) => unsafe {
                let tydesc = layout.value_tydescs[id.0 as usize];
                (*(*tydesc).type_info.tuple.fields).tydesc
            },
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => return None,
        })
    }

    fn desc_index(&mut self, desc: *const rtdt::TyDesc) -> u32 {
        self.descs.push(desc);
        (self.descs.len() - 1) as u32
    }

    fn copy(dst: Loc, src: Loc, len: u32) -> Option<Op> {
        Some(match len {
            0 => return None,
            1 => Op::Copy1 { dst, src },
            4 => Op::Copy4 { dst, src },
            8 => Op::Copy8 { dst, src },
            _ => Op::CopyN { dst, src, len },
        })
    }

    fn emit(&mut self, op: Op) {
        self.blocks[self.cur].push(op);
    }

    fn escape(&mut self, block: usize, index: usize) {
        self.escapes.push(variant(&self.func.blocks[block].instructions[index]));
        self.emit(Op::Ir { block: block as u32, index: index as u32 });
    }

    fn lower(mut self) -> (BcFunction, Vec<String>) {
        let func = self.func;
        self.count_uses();
        let headers = self.loop_headers();
        for (b, block) in func.blocks.iter().enumerate() {
            self.block = b;
            self.cur = b;
            if headers[b] {
                let at = self.loops.len() as u32;
                self.loops.push(LoopCounter { header: block.id, countdown: Countdown::new(1) });
                self.emit(Op::LoopHead { at });
            }
            for (i, instr) in block.instructions.iter().enumerate() {
                self.index = i;
                if !self.lower_instruction(instr) {
                    self.escape(b, i);
                }
            }
            self.lower_terminator(b, &block.terminator);
        }
        let prologue = if self.optimize {
            let facts = self.facts();
            opt::optimize(&mut self.blocks, &facts)
        } else {
            Vec::new()
        };
        self.assemble(prologue)
    }

    /// What the optimization pass may know of the IR, keyed by where things
    /// are in the frame, which is all its ops name.
    fn facts(&self) -> opt::Facts {
        let mut values = rustc_hash::FxHashMap::default();
        for v in 0..self.func.value_types.len() {
            // A value of no size has the next value's offset, so it is left out
            // to keep that one's fact; and an op naming it moves no bytes,
            // which no fact matches.
            let size = layout_of(&self.func.value_types[v]).size;
            if size == 0 {
                continue;
            }
            values.insert(self.layout.value_offsets[v], opt::ValueFact {
                size,
                uses: self.uses[v],
                const_u32: self.const_u32[v],
                pinned: self.pinned[v],
            });
        }
        // The blocks in their final order, each IR block followed by its stubs.
        let order = self.layout_order();
        let mut next = vec![None; self.blocks.len()];
        for pair in order.windows(2) {
            next[pair[0] as usize] = Some(pair[1]);
        }
        let mut in_loop = self.in_loop.clone();
        in_loop.resize(self.blocks.len(), false);
        opt::Facts { values, in_loop, next }
    }

    /// Each IR block, followed by the stubs its edges go through.
    fn layout_order(&self) -> Vec<u32> {
        (0..self.func.blocks.len())
            .flat_map(|b| std::iter::once(b as u32).chain(self.stubs[b].iter().copied()))
            .collect()
    }

    /// Lay the blocks out in one sequence, with the prologue after them, and
    /// point every jump at the op its block starts at.
    fn assemble(mut self, prologue: Vec<Op>) -> (BcFunction, Vec<String>) {
        let order = self.layout_order();
        let mut start = vec![0u32; self.blocks.len()];
        let mut ops = Vec::new();
        for &b in &order {
            start[b as usize] = ops.len() as u32;
            ops.append(&mut self.blocks[b as usize]);
        }
        let mut map = |t: &mut u32| *t = start[*t as usize];
        for op in ops.iter_mut() {
            op.targets_mut(&mut map);
        }
        for table in self.switches.iter_mut() {
            for (_, to) in &mut table.cases {
                map(to);
            }
            map(&mut table.default);
        }
        // The prologue goes last, so that adding it moves nothing.
        let entry = ops.len() as u32;
        ops.extend(prologue);
        ops.push(Op::Jump { to: start[0] });
        let escapes = self.escapes;
        (BcFunction {
            func: self.func, ops, entry, pool: self.pool, switches: self.switches, calls: self.calls,
            descs: self.descs, rt: self.rt, statics: self.statics, loops: self.loops,
        }, escapes)
    }

    /// Lower one instruction to ops, or say it has to run on the IR walker.
    fn lower_instruction(&mut self, instr: &Instruction) -> bool {
        match instr {
            Instruction::Const { dest, value } => self.lower_const(*dest, value),
            Instruction::Copy { dest, src } => {
                let (Some(ty), Some(src)) = (self.typed(src), self.loc(src)) else { return false };
                if let Some(op) = Self::copy(self.value_loc(*dest), src, layout_of(ty).size) {
                    self.emit(op);
                }
                true
            }
            Instruction::Move { dest, src } => {
                let (Some(ty), Some(s)) = (self.typed(src), self.loc(src)) else { return false };
                let (dst, len) = (self.value_loc(*dest), layout_of(ty).size);
                match self.tracking(src) {
                    Some(track) => self.emit(Op::LoadMoveTracked { dst, src: s, track, len }),
                    None => if let Some(op) = Self::copy(dst, s, len) {
                        self.emit(op);
                    },
                }
                true
            }
            Instruction::ParamStore { param, value } => {
                // What the parameter held, a `Drop` in front of the store
                // destroyed, if it owned anything.
                let p = Operand::Param(*param);
                let (Some(ty), Some(dst), Some(src)) =
                    (self.typed(&p), self.loc(&p), self.consumed(value)) else { return false };
                if self.typed(value).is_none() {
                    return false;
                }
                if let Some(op) = Self::copy(dst, src, layout_of(ty).size) {
                    self.emit(op);
                }
                true
            }
            // A store through a reference of a type that owns nothing, which is
            // a copy to where the reference points: the drop in front of a
            // store, which a type that owns nothing never has, did the rest.
            Instruction::RefStore { dest, value } => {
                let (ty, dst) = match dest {
                    Operand::ValueRef(_) => (self.typed(dest), self.loc(dest)),
                    Operand::Value(r) => match self.typed(dest) {
                        Some(IrType::Ref(inner)) if !has_data(inner) => {
                            (Some(&**inner), Some(Loc::indirect(self.layout.value_offsets[r.0 as usize])))
                        }
                        _ => return false,
                    },
                    _ => return false,
                };
                let (Some(ty), Some(dst), Some(src)) = (ty, dst, self.loc(value)) else { return false };
                if !ty.is_copy() {
                    return false;
                }
                if let Some(op) = Self::copy(dst, src, layout_of(ty).size) {
                    self.emit(op);
                }
                true
            }
            Instruction::ListBoundsCheck { is_valid, list, index } => {
                let (Some(l), Some(i)) = (self.loc(list), self.loc(index)) else { return false };
                self.emit(Op::ListBoundsCheck { dst: self.value_loc(*is_valid), list: l, index: i });
                true
            }
            Instruction::ListGet { dest, is_valid, list, index } => {
                let (Some(IrType::List(elem)), Some(l), Some(i)) =
                    (self.typed(list), self.loc(list), self.loc(index)) else { return false };
                if !elem.is_copy() {
                    return false;
                }
                let size = layout_of(elem).size;
                self.emit(Op::ListGetCopy {
                    dst: self.value_loc(*dest), valid: self.value_loc(*is_valid), list: l, index: i, size,
                });
                true
            }
            Instruction::ListSet { is_valid, list, index, value } => {
                let (Some(IrType::List(elem)), Some(l), Some(i), Some(v)) =
                    (self.typed(list), self.loc(list), self.loc(index), self.loc(value)) else { return false };
                if !elem.is_copy() {
                    return false;
                }
                let size = layout_of(elem).size;
                self.emit(Op::ListSetCopy { valid: self.value_loc(*is_valid), list: l, index: i, value: v, size });
                true
            }
            Instruction::ListElementRef { dest, list, index } => {
                let Some(i) = self.loc(index) else { return false };
                // Static only where the reference has no descriptor word to
                // write.
                let static_ref = self.layout.ref_desc_offsets[dest.0 as usize].is_none();
                if let (true, Some(IrType::List(elem)), Some(l)) = (static_ref, self.typed(list), self.loc(list)) {
                    let size = layout_of(elem).size;
                    self.emit(Op::ListElementRef { dst: self.value_loc(*dest), list: l, index: i, size });
                    return true;
                }
                let (Some(d), Some(l)) = (self.rt_operand(&Operand::Value(*dest)), self.rt_operand(list)) else {
                    return false;
                };
                let at = self.rt_push(&[d, l, (i, Desc::Static(std::ptr::null()))]);
                self.emit(Op::ListElementRefRt { dest: dest.0, at });
                true
            }
            Instruction::Erase { dest, src } => self.lower_rt_unary(*dest, src, |at| Op::EraseRt { at }),
            Instruction::Reify { dest, src } => self.lower_rt_unary(*dest, src, |at| Op::ReifyRt { at }),
            // The source is borrowed.
            Instruction::Clone { dest, src } => {
                let (Some(d), Some(s)) = (self.rt_operand(&Operand::Value(*dest)), self.rt_operand(src)) else {
                    return false;
                };
                let at = self.rt_push(&[d, s]);
                self.emit(Op::CloneRt { at });
                true
            }
            Instruction::WidenFixed { dest, src } => {
                let (Some(from), Some(to), Some(s)) = (self.typed(src), self.value_type(*dest), self.loc(src)) else {
                    return false;
                };
                let dst = self.value_loc(*dest);
                match (from, to) {
                    (IrType::U8, IrType::U32) => self.emit(Op::ZextU8U32 { dst, src: s }),
                    (IrType::U8, IrType::U64) => self.emit(Op::ZextU8U64 { dst, src: s }),
                    (IrType::U32, IrType::U64) => self.emit(Op::ZextU32U64 { dst, src: s }),
                    _ => return self.lower_rt_unary(*dest, src, |at| Op::WidenFixedRt { at }),
                }
                true
            }
            Instruction::UnaryOp { dest, op: UnaryOp::Neg, operand } => {
                let (Some(ty), Some(src)) = (self.typed(operand), self.loc(operand)) else { return false };
                let dst = self.value_loc(*dest);
                match ty {
                    IrType::F64 => self.emit(Op::NegF64 { dst, src }),
                    IrType::F32 => self.emit(Op::NegF32 { dst, src }),
                    _ => return false,
                }
                true
            }
            Instruction::UnaryOp { dest, op: UnaryOp::Not | UnaryOp::LogicNot, operand } => {
                let (Some(IrType::Bool), Some(src)) = (self.typed(operand), self.loc(operand)) else {
                    return false;
                };
                self.emit(Op::NotBool { dst: self.value_loc(*dest), src });
                true
            }
            Instruction::SlotLoadCopy { dest, slot } | Instruction::SlotLoadMove { dest, slot } => {
                // Untracked only: a tracked slot's byte is the IR walker's.
                if self.layout.slot_tracking[slot.0 as usize].is_some() {
                    return false;
                }
                let op = Operand::Slot(*slot);
                let (Some(ty), Some(src)) = (self.typed(&op), self.loc(&op)) else { return false };
                if let Some(op) = Self::copy(self.value_loc(*dest), src, layout_of(ty).size) {
                    self.emit(op);
                }
                true
            }
            Instruction::SlotStoreCopy { dest: SlotDest::Local(slot), value }
            | Instruction::SlotStoreMove { dest: SlotDest::Local(slot), value } => {
                if self.layout.slot_tracking[slot.0 as usize].is_some() {
                    return false;
                }
                let src = match instr {
                    Instruction::SlotStoreMove { .. } => self.consumed(value),
                    _ => self.loc(value),
                };
                let (Some(ty), Some(src)) = (self.typed(value), src) else { return false };
                let dst = Loc::direct(self.layout.slot_offsets[slot.0 as usize]);
                if let Some(op) = Self::copy(dst, src, layout_of(ty).size) {
                    self.emit(op);
                }
                true
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                self.lower_binop(*dest, *op, lhs, rhs) || self.lower_binop_rt(*dest, *op, lhs, rhs)
            }
            Instruction::BinOpChecked { dest, overflow, op, lhs, rhs } => {
                self.lower_checked(*dest, *overflow, *op, lhs, rhs)
            }
            Instruction::OpAssign { place, op, rhs } => {
                let (Some(p), Some(r)) = (self.rt_operand(place), self.rt_operand(rhs)) else { return false };
                let at = self.rt_push(&[p, r]);
                self.emit(Op::OpAssignRt { op: *op, at });
                true
            }
            Instruction::OpAssignChecked { overflow, place, op, rhs } => {
                self.lower_checked_assign(*overflow, *op, place, rhs)
            }
            // A reference to a const, where the reference has no descriptor
            // word to write.
            Instruction::StaticRef { dest, value } => {
                if self.layout.ref_desc_offsets[dest.0 as usize].is_some() {
                    return false;
                }
                // A reference's descriptor holds what it points at as its one field.
                let pointee = unsafe { (*(*self.layout.value_tydescs[dest.0 as usize]).type_info.tuple.fields).tydesc };
                let slot = self.statics.len() as u32;
                self.statics.push(StaticSlot {
                    value: std::sync::Arc::clone(value),
                    pointee,
                    resolved: std::cell::Cell::new(std::ptr::null()),
                });
                self.emit(Op::StaticRef { dst: self.value_loc(*dest), slot });
                true
            }
            Instruction::GetFieldRef { dest, src, field_index } => {
                if self.layout.ref_desc_offsets[dest.0 as usize].is_some() {
                    return false;
                }
                let (Some(ty), Some(src)) = (self.typed(src), self.loc(src)) else { return false };
                let Some((offset, _)) = static_field(ty, *field_index) else { return false };
                self.emit(Op::FieldRef { dst: self.value_loc(*dest), src, offset });
                true
            }
            // A shallow copy of the field, as `field_read` makes where no
            // `data` is involved.
            Instruction::GetField { dest, src, field_index } => {
                let (Some(ty), Some(src)) = (self.typed(src), self.loc(src)) else { return false };
                let Some((offset, field_ty)) = static_field(ty, *field_index) else { return false };
                let len = layout_of(field_ty).size;
                let dst = self.value_loc(*dest);
                let op = match src.advanced(offset) {
                    Some(at) => Self::copy(dst, at, len),
                    None if len > 0 => Some(Op::CopyAt { dst, src, offset, len }),
                    None => None,
                };
                if let Some(op) = op {
                    self.emit(op);
                }
                true
            }
            // The discriminant is the first four bytes of every enum.
            Instruction::EnumDiscriminant { dest, src } => {
                let (Some(IrType::Enum(_)), Some(src)) = (self.typed(src), self.loc(src)) else { return false };
                self.emit(Op::Copy4 { dst: self.value_loc(*dest), src });
                true
            }
            // Each field moved into place, as `execute_pack_tuple` does.
            Instruction::Pack { dest, fields, .. } => {
                let Some(ty) = self.value_type(*dest) else { return false };
                let mut copies = Vec::with_capacity(fields.len());
                for (i, field) in fields.iter().enumerate() {
                    let Some((offset, field_ty)) = static_field(ty, i as u32) else { return false };
                    let Some(src) = self.consumed(field) else { return false };
                    let Some(dst) = self.value_loc(*dest).advanced(offset) else { return false };
                    copies.extend(Self::copy(dst, src, layout_of(field_ty).size));
                }
                copies.into_iter().for_each(|op| self.emit(op));
                true
            }
            Instruction::MapGet { dest, is_valid, map, key } => {
                let (Some(d), Some(m), Some(k)) = (
                    self.rt_operand(&Operand::Value(*dest)), self.rt_operand(map), self.rt_operand(key),
                ) else {
                    return false;
                };
                let at = self.rt_push(&[d, m, k]);
                self.emit(Op::MapGetRt { at, valid: self.value_loc(*is_valid) });
                true
            }
            Instruction::Intrinsic { dest, intrinsic, args } => {
                self.lower_intrinsic(*dest, *intrinsic, args)
            }
            Instruction::WrapSome { dest, inner } => {
                let (Some(ty), Some(src)) = (self.typed(inner), self.consumed(inner)) else { return false };
                if self.value_type(*dest).is_none() {
                    return false;
                }
                let at = option_payload_offset(ty);
                let len = layout_of(ty).size;
                self.emit(Op::WrapSome { dst: self.value_loc(*dest), src, at, len });
                true
            }
            Instruction::WrapNone { dest } => {
                if self.value_type(*dest).is_none() {
                    return false;
                }
                self.emit(Op::WrapNone { dst: self.value_loc(*dest) });
                true
            }
            Instruction::WrapOk { dest, inner } => {
                let (Some(ty), Some(src)) = (self.typed(inner), self.consumed(inner)) else { return false };
                if self.value_type(*dest).is_none() {
                    return false;
                }
                let at = result_payload_offset(ty);
                let len = layout_of(ty).size;
                self.emit(Op::WrapOk { dst: self.value_loc(*dest), src, at, len });
                true
            }
            Instruction::UnwrapOption { dest, is_some, src } => {
                let (Some(IrType::Option(inner)), Some(src)) = (self.typed(src), self.consumed(src)) else {
                    return false;
                };
                self.emit(Op::UnwrapOption {
                    dst: self.value_loc(*dest),
                    flag: self.value_loc(*is_some),
                    src,
                    at: option_payload_offset(inner),
                    len: layout_of(inner).size,
                });
                true
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                let (Some(IrType::Result(ok_ty)), Some(src)) = (self.typed(src), self.consumed(src)) else {
                    return false;
                };
                let at = result_payload_offset(ok_ty);
                let ok_len = layout_of(ok_ty).size;
                let err_len = layout_of(&IrType::Error).size;
                let (Ok(at), Ok(ok_len), Ok(err_len)) =
                    (u16::try_from(at), u16::try_from(ok_len), u16::try_from(err_len)) else {
                    return false;
                };
                self.emit(Op::UnwrapResult {
                    ok: self.value_loc(*ok_dest),
                    err: self.value_loc(*err_dest),
                    flag: self.value_loc(*is_ok),
                    src,
                    at,
                    ok_len,
                    err_len,
                });
                true
            }
            Instruction::Call { dest, args, .. } => {
                // Anything but an earlier unit's binding, which a function
                // body never names.
                let fast: Option<Vec<_>> = args.iter().map(|arg| {
                    self.loc(arg)?;
                    Some(self.consumed(arg).is_some())
                }).collect();
                let resolved: Option<Vec<_>> = match instr {
                    Instruction::Call { shape_descriptors, .. } if shape_descriptors.is_empty() => {
                        // A `data` handed to a borrowed parameter is read
                        // through its wrapper, which the general lane does.
                        args.iter().map(|arg| {
                            if has_data(self.typed(arg)?) {
                                return None;
                            }
                            Some((self.loc(arg)?, self.desc_of(arg)?))
                        }).collect()
                    }
                    _ => None,
                };
                match fast {
                    Some(args) => {
                        let site = self.calls.len() as u32;
                        self.calls.push(FastCall {
                            block: self.block as u32,
                            index: self.index as u32,
                            args,
                            resolved,
                            dest: self.value_loc(*dest),
                            dest_tydesc: self.layout.value_tydescs[dest.0 as usize],
                            site: std::cell::RefCell::new(SiteCache::default()),
                        });
                        self.emit(Op::CallFast { site });
                    }
                    None => self.emit(Op::Call { block: self.block as u32, index: self.index as u32 }),
                }
                true
            }
            Instruction::ComptimeCall { .. } => {
                self.emit(Op::Call { block: self.block as u32, index: self.index as u32 });
                true
            }
            Instruction::Drop { operand } => {
                // A copied type owns nothing. What has a tracking byte holds
                // something, as at any `drop`, so checking it costs nothing.
                let (Some(ty), Some(src), Some(desc)) =
                    (self.typed(operand), self.loc(operand), self.desc_of(operand)) else { return false };
                if !ty.is_copy() {
                    let desc = self.desc_index(desc);
                    match self.tracking(operand) {
                        Some(track) => self.emit(Op::DropTracked { src, track, desc }),
                        None => self.emit(Op::Drop { src, desc }),
                    }
                }
                true
            }
            Instruction::DropTracked { operand } => {
                let track = match operand {
                    Operand::Slot(id) => self.layout.slot_tracking[id.0 as usize],
                    Operand::Param(id) => self.layout.param_tracking[id.0 as usize],
                    _ => None,
                };
                let (Some(track), Some(_), Some(src), Some(desc)) =
                    (track, self.typed(operand), self.loc(operand), self.desc_of(operand)) else { return false };
                let desc = self.desc_index(desc);
                self.emit(Op::DropTracked { src, track, desc });
                true
            }
            Instruction::SlotStoreCopyTracked { dest: SlotDest::Local(slot), value }
            | Instruction::SlotStoreMoveTracked { dest: SlotDest::Local(slot), value } => {
                let Some(track) = self.layout.slot_tracking[slot.0 as usize] else { return false };
                let src = match instr {
                    Instruction::SlotStoreMoveTracked { .. } => self.consumed(value),
                    _ => self.loc(value),
                };
                let (Some(ty), Some(src)) = (self.typed(value), src) else { return false };
                if self.typed(&Operand::Slot(*slot)).is_none() {
                    return false;
                }
                let dst = Loc::direct(self.layout.slot_offsets[slot.0 as usize]);
                self.emit(Op::StoreTracked { dst, track, src, len: layout_of(ty).size });
                true
            }
            Instruction::SlotLoadMoveTracked { dest, slot } => {
                let Some(track) = self.layout.slot_tracking[slot.0 as usize] else { return false };
                let op = Operand::Slot(*slot);
                let (Some(ty), Some(src)) = (self.typed(&op), self.loc(&op)) else { return false };
                self.emit(Op::LoadMoveTracked { dst: self.value_loc(*dest), src, track, len: layout_of(ty).size });
                true
            }
            Instruction::Widen { dest, src } => {
                let (Some(_), Some(s), Some(desc)) = (self.typed(src), self.loc(src), self.desc_of(src)) else {
                    return false;
                };
                let src_desc = self.desc_index(desc);
                self.emit(Op::Widen { dst: self.value_loc(*dest), src: s, src_desc });
                true
            }
            Instruction::Nop => true,
            _ => false,
        }
    }

    fn lower_const(&mut self, dest: ValueId, value: &ConstValue) -> bool {
        let dst = self.value_loc(dest);
        let bytes: Vec<u8> = match value {
            ConstValue::Unit => return true,
            ConstValue::Bool(b) => {
                self.emit(Op::Const1 { dst, imm: *b as u8 });
                return true;
            }
            ConstValue::U8(n) => {
                self.emit(Op::Const1 { dst, imm: *n });
                return true;
            }
            ConstValue::I8(n) => {
                self.emit(Op::Const1 { dst, imm: *n as u8 });
                return true;
            }
            ConstValue::U32(n) => {
                self.emit(Op::Const4 { dst, imm: *n });
                return true;
            }
            ConstValue::I32(n) => {
                self.emit(Op::Const4 { dst, imm: *n as u32 });
                return true;
            }
            ConstValue::F32(f) => {
                self.emit(Op::Const4 { dst, imm: f.0.to_bits() });
                return true;
            }
            ConstValue::U16(n) => n.to_ne_bytes().to_vec(),
            ConstValue::I16(n) => n.to_ne_bytes().to_vec(),
            ConstValue::U64(n) => n.to_ne_bytes().to_vec(),
            ConstValue::I64(n) => n.to_ne_bytes().to_vec(),
            ConstValue::F64(f) => f.0.to_bits().to_ne_bytes().to_vec(),
            ConstValue::Index(n) => n.to_ne_bytes().to_vec(),
            ConstValue::Offset(n) => n.to_ne_bytes().to_vec(),
            // Anything that owns memory is built fresh at every execution.
            ConstValue::String(text) => {
                let at = self.pool.len() as u32;
                self.pool.extend_from_slice(text.as_bytes());
                let desc = self.desc_index(self.layout.value_tydescs[dest.0 as usize]);
                self.emit(Op::ConstString { dst, at, len: text.len() as u32, desc });
                return true;
            }
            _ => return false,
        };
        let at = self.pool.len() as u32;
        let len = bytes.len() as u32;
        self.pool.extend_from_slice(&bytes);
        self.emit(Op::ConstPool { dst, at, len });
        true
    }

    fn lower_binop(&mut self, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> bool {
        if let Some(arith) = self.lower_float_arith(dest, op, lhs, rhs) {
            return arith;
        }
        let Some(cmp) = Cmp::of(op) else { return false };
        let (Some(lt), Some(a), Some(b)) = (self.typed(lhs), self.loc(lhs), self.loc(rhs)) else {
            return false;
        };
        if self.typed(rhs) != Some(lt) {
            return false;
        }
        let dst = self.value_loc(dest);
        let op = match lt {
            IrType::U8 | IrType::Bool => Op::CmpU8 { cmp, dst, a, b },
            IrType::U32 => Op::CmpU32 { cmp, dst, a, b },
            IrType::I32 => Op::CmpI32 { cmp, dst, a, b },
            IrType::U64 => Op::CmpU64 { cmp, dst, a, b },
            IrType::I64 => Op::CmpI64 { cmp, dst, a, b },
            IrType::Index if rtdt::INDEX_SIZE == 4 => Op::CmpU32 { cmp, dst, a, b },
            IrType::Index => Op::CmpU64 { cmp, dst, a, b },
            IrType::Offset if rtdt::INDEX_SIZE == 4 => Op::CmpI32 { cmp, dst, a, b },
            IrType::Offset => Op::CmpI64 { cmp, dst, a, b },
            IrType::F32 => Op::CmpF32 { cmp, dst, a, b },
            IrType::F64 => Op::CmpF64 { cmp, dst, a, b },
            _ => return false,
        };
        // A bool compares as its byte only for equality.
        if matches!(lt, IrType::Bool) && !matches!(cmp, Cmp::Eq | Cmp::Ne) {
            return false;
        }
        self.emit(op);
        true
    }

    /// Floating-point `+ - * /`, where both operands are the same float type;
    /// `None` if this is not that.
    fn lower_float_arith(&mut self, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> Option<bool> {
        let ty = self.typed(lhs)?;
        if !matches!(ty, IrType::F32 | IrType::F64) || !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
            return None;
        }
        if self.typed(rhs) != Some(ty) {
            return None;
        }
        let (Some(a), Some(b)) = (self.loc(lhs), self.loc(rhs)) else { return Some(false) };
        let dst = self.value_loc(dest);
        let f64 = matches!(ty, IrType::F64);
        self.emit(match (op, f64) {
            (BinOp::Add, true) => Op::AddF64 { dst, a, b },
            (BinOp::Sub, true) => Op::SubF64 { dst, a, b },
            (BinOp::Mul, true) => Op::MulF64 { dst, a, b },
            (BinOp::Div, true) => Op::DivF64 { dst, a, b },
            (BinOp::Add, false) => Op::AddF32 { dst, a, b },
            (BinOp::Sub, false) => Op::SubF32 { dst, a, b },
            (BinOp::Mul, false) => Op::MulF32 { dst, a, b },
            (BinOp::Div, false) => Op::DivF32 { dst, a, b },
            _ => unreachable!("matched above"),
        });
        Some(true)
    }

    /// A binary operation on the IR walker's routine, with its operands
    /// resolved, for the types no typed op covers.
    fn lower_binop_rt(&mut self, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> bool {
        let d = Operand::Value(dest);
        let (Some(d), Some(a), Some(b)) =
            (self.rt_operand(&d), self.rt_operand(lhs), self.rt_operand(rhs)) else { return false };
        let at = self.rt_push(&[d, a, b]);
        self.emit(Op::BinOpRt { op, at });
        true
    }

    fn lower_checked(
        &mut self,
        dest: ValueId,
        overflow: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> bool {
        let (Some(ty), Some(a), Some(b)) = (self.typed(lhs), self.loc(lhs), self.loc(rhs)) else {
            return false;
        };
        let (dst, ovf) = (self.value_loc(dest), self.value_loc(overflow));
        let index32 = rtdt::INDEX_SIZE == 4;
        let width = match ty {
            IrType::U32 => (false, 32),
            IrType::I32 => (true, 32),
            IrType::U64 => (false, 64),
            IrType::I64 => (true, 64),
            IrType::Index => (false, if index32 { 32 } else { 64 }),
            IrType::Offset => (true, if index32 { 32 } else { 64 }),
            _ => return false,
        };
        let op = match (op, width) {
            (BinOp::Add, (false, 32)) => Op::AddCkU32 { dst, ovf, a, b },
            (BinOp::Sub, (false, 32)) => Op::SubCkU32 { dst, ovf, a, b },
            (BinOp::Mul, (false, 32)) => Op::MulCkU32 { dst, ovf, a, b },
            (BinOp::Add, (true, 32)) => Op::AddCkI32 { dst, ovf, a, b },
            (BinOp::Sub, (true, 32)) => Op::SubCkI32 { dst, ovf, a, b },
            (BinOp::Mul, (true, 32)) => Op::MulCkI32 { dst, ovf, a, b },
            (BinOp::Add, (false, 64)) => Op::AddCkU64 { dst, ovf, a, b },
            (BinOp::Sub, (false, 64)) => Op::SubCkU64 { dst, ovf, a, b },
            (BinOp::Mul, (false, 64)) => Op::MulCkU64 { dst, ovf, a, b },
            (BinOp::Add, (true, 64)) => Op::AddCkI64 { dst, ovf, a, b },
            (BinOp::Sub, (true, 64)) => Op::SubCkI64 { dst, ovf, a, b },
            (BinOp::Mul, (true, 64)) => Op::MulCkI64 { dst, ovf, a, b },
            (BinOp::Div, (false, 32)) => Op::DivCkU32 { dst, ovf, a, b },
            (BinOp::Div, (true, 32)) => Op::DivCkI32 { dst, ovf, a, b },
            (BinOp::Div, (false, 64)) => Op::DivCkU64 { dst, ovf, a, b },
            (BinOp::Div, (true, 64)) => Op::DivCkI64 { dst, ovf, a, b },
            _ => return false,
        };
        self.emit(op);
        true
    }

    /// A checked update of a place, for the widths of 32 and 64 bits. The
    /// others, and division, run on the IR walker.
    fn lower_checked_assign(&mut self, overflow: ValueId, op: BinOp, place: &Operand, rhs: &Operand) -> bool {
        let (Some(ty), Some(place), Some(b)) = (self.typed(place), self.loc(place), self.loc(rhs)) else {
            return false;
        };
        let kind = match op {
            BinOp::Add => Ck::Add,
            BinOp::Sub => Ck::Sub,
            BinOp::Mul => Ck::Mul,
            _ => return false,
        };
        let Some(ty) = CkTy::of(ty) else { return false };
        let ovf = self.value_loc(overflow);
        self.emit(Op::CkAssign { ty, kind, place, b, ovf });
        true
    }

    fn lower_intrinsic(&mut self, dest: ValueId, intrinsic: IntrinsicId, args: &[Operand]) -> bool {
        if let [x] = args {
            let Some(src) = self.loc(x) else { return false };
            let dst = self.value_loc(dest);
            let op = match intrinsic {
                IntrinsicId::U64ToIndex => Op::U64ToIndex { dst, src },
                IntrinsicId::SqrtF64 => Op::SqrtF64 { dst, src },
                IntrinsicId::SqrtF32 => Op::SqrtF32 { dst, src },
                IntrinsicId::U64ToF64 => Op::U64ToF64 { dst, src },
                IntrinsicId::I64ToF64 => Op::I64ToF64 { dst, src },
                IntrinsicId::F32ToF64 => Op::F32ToF64 { dst, src },
                IntrinsicId::F64ToF32 => Op::F64ToF32 { dst, src },
                _ => return false,
            };
            self.emit(op);
            return true;
        }
        let [x, y] = args else { return false };
        let (Some(a), Some(b)) = (self.loc(x), self.loc(y)) else { return false };
        let dst = self.value_loc(dest);
        let op = match intrinsic {
            IntrinsicId::AddWrappingU32 => Op::AddWrapU32 { dst, a, b },
            IntrinsicId::SubWrappingU32 => Op::SubWrapU32 { dst, a, b },
            IntrinsicId::MulWrappingU32 => Op::MulWrapU32 { dst, a, b },
            IntrinsicId::RemU32 => Op::RemU32 { dst, a, b },
            IntrinsicId::ShrU32 => Op::ShrU32 { dst, a, b },
            IntrinsicId::ShlU32 => Op::ShlU32 { dst, a, b },
            IntrinsicId::BitandU32 => Op::AndU32 { dst, a, b },
            IntrinsicId::BitxorU32 => Op::XorU32 { dst, a, b },
            IntrinsicId::BitorU32 => Op::OrU32 { dst, a, b },
            IntrinsicId::AddWrappingIndex => Op::AddWrapIndex { dst, a, b },
            IntrinsicId::SubWrappingIndex => Op::SubWrapIndex { dst, a, b },
            _ => return false,
        };
        self.emit(op);
        true
    }

    /// The ops that pass an edge's arguments, or none if the IR walker has to.
    ///
    /// Copies only where every argument is an SSA value and none of them is a
    /// parameter of the target block; anything else -- a parallel move, a slot
    /// or parameter whose consumption a tracking byte records -- runs on the
    /// IR walker.
    fn edge_copies(&self, target: BlockId, args: &[Operand]) -> Option<Vec<Op>> {
        let params = &self.func.blocks[target.0 as usize].params;
        let mut ops = Vec::new();
        for (param, arg) in params.iter().zip(args) {
            let Operand::Value(id) = arg else { return None };
            if params.contains(id) {
                return None;
            }
            let ty = self.value_type(*id)?;
            if let Some(op) = Self::copy(self.value_loc(*param), self.value_loc(*id), layout_of(ty).size) {
                ops.push(op);
            }
        }
        Some(ops)
    }

    /// A stub block that passes an edge's arguments and jumps on, laid out
    /// after `block`, for a branch to go to.
    fn stub(&mut self, block: usize, edge: u32, target: BlockId, args: &[Operand]) -> u32 {
        let id = self.blocks.len();
        self.blocks.push(Vec::new());
        self.stubs[block].push(id as u32);
        let cur = std::mem::replace(&mut self.cur, id);
        self.jump(block, edge, target, args);
        self.cur = cur;
        id as u32
    }

    /// Emit a jump along an edge to `target`, with its arguments.
    fn jump(&mut self, block: usize, edge: u32, target: BlockId, args: &[Operand]) {
        match self.edge_copies(target, args) {
            Some(copies) => {
                copies.into_iter().for_each(|op| self.emit(op));
                self.emit(Op::Jump { to: target.0 });
            }
            None => {
                self.escapes.push("edge".to_string());
                self.emit(Op::EdgeIr { block: block as u32, edge, to: target.0 });
            }
        }
    }

    fn lower_terminator(&mut self, block: usize, term: &Terminator) {
        match term {
            // Into the next block with nothing to pass is no op at all.
            Terminator::Goto { target, args } if args.is_empty() && target.0 as usize == block + 1 => {}
            Terminator::Goto { target, args } => self.jump(block, 0, *target, args),
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                let cond = self.loc(cond).expect("a branch condition is a local bool");
                // An edge with arguments goes through a stub that passes them
                // and jumps on.
                let then = if then_args.is_empty() { then_block.0 } else { self.stub(block, 0, *then_block, then_args) };
                let els = if else_args.is_empty() { else_block.0 } else { self.stub(block, 1, *else_block, else_args) };
                self.emit(Op::BrIf { cond, then, els });
            }
            Terminator::Switch { discriminant, cases, default } => {
                let disc = self.loc(discriminant).expect("a switch discriminant is local");
                let t = self.switches.len();
                self.switches.push(SwitchTable {
                    cases: cases.iter().map(|(v, b)| (*v, b.0)).collect(),
                    default: default.0,
                });
                self.emit(Op::Switch { disc, table: t as u32 });
            }
            Terminator::Return { value: None } => self.emit(Op::ReturnUnit),
            Terminator::Return { value: Some(op) } => {
                match (self.typed(op), self.consumed(op)) {
                    (Some(ty), Some(src)) => {
                        let len = layout_of(ty).size;
                        self.emit(Op::Return { src, len });
                    }
                    _ => {
                        self.escapes.push("return".to_string());
                        self.emit(Op::ReturnIr { block: block as u32 });
                    }
                }
            }
            Terminator::UnitEnd { .. } | Terminator::UnitEarlyReturn { .. } => {
                unreachable!("a function body has no unit terminators")
            }
        }
    }
}

impl Lowering<'_> {
    fn count_uses(&mut self) {
        fn read(uses: &mut [u32], op: &Operand) {
            if let Operand::Value(id) | Operand::ValueRef(id) = op {
                uses[id.0 as usize] += 1;
            }
        }
        let n = self.func.value_types.len();
        let mut uses = vec![0u32; n];
        let mut pinned = vec![false; n];
        for block in &self.func.blocks {
            for instr in &block.instructions {
                instr.for_each_operand(|op| read(&mut uses, op));
                match instr {
                    Instruction::Call { args, .. } | Instruction::ComptimeCall { args, .. } => {
                        for op in args {
                            if let Operand::Value(id) = op {
                                pinned[id.0 as usize] = true;
                            }
                        }
                    }
                    Instruction::RefStore { dest, .. } | Instruction::RefStoreTracked { dest, .. }
                    | Instruction::RefSetField { dest, .. } | Instruction::RefSetFieldTracked { dest, .. }
                    | Instruction::DropField { base: dest, .. }
                    | Instruction::OpAssign { place: dest, .. }
                    | Instruction::OpAssignChecked { place: dest, .. } => {
                        if let Operand::Value(id) | Operand::ValueRef(id) = dest {
                            pinned[id.0 as usize] = true;
                        }
                    }
                    _ => {}
                }
            }
            block.terminator.for_each_operand(|op| read(&mut uses, op));
        }
        self.const_u32 = self.func.blocks.iter()
            .flat_map(|b| &b.instructions)
            .fold(vec![None; n], |mut c, instr| {
                if let Instruction::Const { dest, value: ConstValue::U32(v) } = instr
                    && !pinned[dest.0 as usize]
                {
                    c[dest.0 as usize] = Some(*v);
                }
                c
            });
        self.uses = uses;
        self.pinned = pinned;
        self.in_loop = self.loop_blocks();
    }

    /// Which blocks head a loop: in one, and entered by an edge from a block
    /// at or after them, which is how a loop's body goes back to its top.
    fn loop_headers(&self) -> Vec<bool> {
        let mut headers = vec![false; self.func.blocks.len()];
        for (b, block) in self.func.blocks.iter().enumerate() {
            for target in block.terminator.successors() {
                let t = target.0 as usize;
                if t <= b && self.in_loop[t] {
                    headers[t] = true;
                }
            }
        }
        headers
    }

    /// Which blocks can reach themselves.
    fn loop_blocks(&self) -> Vec<bool> {
        let blocks = &self.func.blocks;
        let successors = |b: usize| blocks[b].terminator.successors().into_iter().map(|t| t.0 as usize);
        (0..blocks.len())
            .map(|start| {
                let mut seen = vec![false; blocks.len()];
                let mut stack: Vec<usize> = successors(start).collect();
                while let Some(b) = stack.pop() {
                    if b == start {
                        return true;
                    }
                    if !std::mem::replace(&mut seen[b], true) {
                        stack.extend(successors(b));
                    }
                }
                false
            })
            .collect()
    }
}

/// Lower a function body against its layout.
pub(crate) fn lower(func: &IrCodeUnit, layout: &IrLayout, optimize: bool) -> (BcFunction, Vec<String>) {
    let lowered = Lowering {
        func,
        layout,
        optimize,
        blocks: vec![Vec::new(); func.blocks.len()],
        stubs: vec![Vec::new(); func.blocks.len()],
        cur: 0,
        pool: Vec::new(),
        switches: Vec::new(),
        calls: Vec::new(),
        descs: Vec::new(),
        rt: Vec::new(),
        statics: Vec::new(),
        escapes: Vec::new(),
        uses: Vec::new(),
        const_u32: Vec::new(),
        pinned: Vec::new(),
        in_loop: Vec::new(),
        loops: Vec::new(),
        block: 0,
        index: 0,
    }
    .lower();
    if cfg!(debug_assertions) {
        lowered.0.verify(func, layout);
    }
    lowered
}
