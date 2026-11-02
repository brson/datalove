//! Small aggregates held as words: a value of up to four words in registers
//! rather than at its home in the frame.
//!
//! An aggregate value is otherwise the address of its home, and every
//! instruction making one stores it there and every one reading it loads it
//! back. A value held as words is one Cranelift value per piece of its bytes,
//! cut as `bytes::pieces` cuts a copy: eight bytes at a time, then the tail.
//! A field, a payload or a tag is shifted out of the piece holding it, and an
//! aggregate is built by shifting its parts into place, which the alignment of
//! every field allows: a scalar never straddles two pieces.
//!
//! Which values are held so is decided before the body is compiled
//! (`word_values`), from the value's type and its uses as the IR describes
//! them (`Instruction::for_each_place_use`): a value whose address some use
//! keeps, or writes through, stays in memory. Anything that has not been
//! taught words asks for an address as before, and gets the value's home with
//! the words just stored there, which is right for a use that only reads, the
//! only kind a value held as words has. See
//! `botdocs/plan-values-in-registers.md`.

use std::collections::HashSet;

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{Instruction, IrType, Operand, ParamId, ParamMode, PlaceUse, ValueId};

use crate::types::{self, CraneliftRepr};
use crate::CraneliftError;

use super::bytes::pieces;
use super::{FunctionCompiler, RetPass};

/// The most bytes a value held as words has.
const MAX_WORDS_SIZE: u32 = 32;

/// How a value of `ty` is cut into words, as each piece's offset and type, if
/// it can be held so: a static aggregate of at most `MAX_WORDS_SIZE` bytes.
pub fn word_pieces(ty: &IrType) -> Option<Vec<(i32, cl_ir::Type)>> {
    if ty.is_dynamic() || cfg!(target_endian = "big") {
        return None;
    }
    match types::ir_type_to_cranelift(ty) {
        CraneliftRepr::Aggregate(layout) if layout.size > 0 && layout.size <= MAX_WORDS_SIZE => {
            Some(pieces(layout.size as u64).collect())
        }
        _ => None,
    }
}

/// Whether parts at these byte offsets and of these sizes each lie within one
/// of the pieces `ty` is cut into: whether `compose_words` can build it.
pub(super) fn parts_fit(ty: &IrType, parts: &[(u32, u32)]) -> bool {
    let Some(cut) = word_pieces(ty) else { return false };
    parts.iter().all(|&(off, n)| cut.iter().any(|&(at, t)| off >= at as u32 && off + n <= at as u32 + t.bytes()))
}

/// The offsets, from `base`, of the 32-bit counts in a value of `ty`: a
/// list's or a string's size and capacity, wherever one is inside it.
fn counts_at(ty: &IrType, base: u32, out: &mut Vec<u32>) {
    use datalove_datafun_ir::layout as ir_layout;
    match ty {
        IrType::List(_) | IrType::String if crate::index_types::INDEX_TYPE == cl_types::I32 => {
            out.push(base + std::mem::offset_of!(datalove_rtdt::List, size) as u32);
            out.push(base + std::mem::offset_of!(datalove_rtdt::List, capacity) as u32);
        }
        IrType::Tuple(_) | IrType::Struct(_) => {
            let fields: Vec<IrType> = match ty {
                IrType::Tuple(tys) => tys.clone(),
                IrType::Struct(flds) => flds.iter().map(|(_, t)| t.clone()).collect(),
                _ => unreachable!("matched"),
            };
            for (field, at) in fields.iter().zip(types::compute_tuple_field_offsets(&fields)) {
                counts_at(field, base + at, out);
            }
        }
        IrType::Option(inner) => counts_at(inner, base + ir_layout::option_payload_offset(inner) as u32, out),
        IrType::Result(ok) => counts_at(ok, base + ir_layout::result_payload_offset(ok) as u32, out),
        IrType::Term(_, payload) => counts_at(payload, base, out),
        _ => {}
    }
}

/// Whether a call with a callee of parameter modes `modes` only reads its
/// argument `i`: passes it `in` or `ref`. A callee whose modes are unknown is
/// taken to write through every argument.
pub(super) fn reads_argument(modes: &[ParamMode], i: usize) -> bool {
    matches!(modes.get(i), Some(ParamMode::In | ParamMode::Ref))
}

/// How a value of `ty` is held in registers, as each register's offset in it
/// and type: one for a scalar, its words for a small aggregate. None for one
/// that cannot be: of a dynamic type, or a larger aggregate.
///
/// What is passed by value between compiled functions, and what a slot held
/// in registers is held as.
pub fn reg_pieces(ty: &IrType) -> Option<Vec<(i32, cl_ir::Type)>> {
    if ty.is_dynamic() {
        return None;
    }
    match types::ir_type_to_cranelift(ty) {
        CraneliftRepr::Scalar(t) => Some(vec![(0, t)]),
        CraneliftRepr::Aggregate(_) => word_pieces(ty),
    }
}

/// Store `regs`, the registers of a value of `ty`, at `addr`.
pub fn store_regs(builder: &mut FunctionBuilder, addr: cl_ir::Value, ty: &IrType, regs: &[cl_ir::Value]) {
    match types::ir_type_to_cranelift(ty) {
        CraneliftRepr::Scalar(_) => {
            builder.ins().store(MemFlagsData::new(), regs[0], addr, 0);
        }
        CraneliftRepr::Aggregate(_) => store_words_at(builder, addr, ty, regs),
    }
}

/// Load a value of `ty` at `addr` into the registers it is held in.
pub fn load_regs(builder: &mut FunctionBuilder, addr: cl_ir::Value, ty: &IrType) -> Vec<cl_ir::Value> {
    match types::ir_type_to_cranelift(ty) {
        CraneliftRepr::Scalar(t) => vec![builder.ins().load(t, MemFlagsData::new(), addr, 0)],
        CraneliftRepr::Aggregate(_) => load_words(builder, addr, ty),
    }
}

/// Store `words`, of type `ty`, at `addr`.
pub(super) fn store_words_at(builder: &mut FunctionBuilder, addr: cl_ir::Value, ty: &IrType, words: &[cl_ir::Value]) {
    let cut = word_pieces(ty).expect("words are of a type that can be held as words");
    for ((at, _), &w) in cut.iter().zip(words) {
        builder.ins().store(MemFlagsData::new(), w, addr, *at);
    }
}

/// Load the words of a value of `ty` at `addr`.
///
/// A word holding a list's or a string's two counts, its size and its
/// capacity, is loaded as the two and put together. What stored them may
/// have stored only one -- a push the size, the runtime's growing each
/// as a field of its own -- and a load the processor can forward from a
/// store is one the store covers.
pub(super) fn load_words(builder: &mut FunctionBuilder, addr: cl_ir::Value, ty: &IrType) -> Vec<cl_ir::Value> {
    let cut = word_pieces(ty).expect("words are of a type that can be held as words");
    let mut counts = Vec::new();
    counts_at(ty, 0, &mut counts);
    let flags = MemFlagsData::new();
    cut.iter().map(|&(at, t)| {
        if t == cl_types::I64 && counts.contains(&(at as u32)) && counts.contains(&(at as u32 + 4)) {
            let low = builder.ins().load(cl_types::I32, flags, addr, at);
            let high = builder.ins().load(cl_types::I32, flags, addr, at + 4);
            let low = builder.ins().uextend(cl_types::I64, low);
            let high = builder.ins().uextend(cl_types::I64, high);
            let high = builder.ins().ishl_imm_u(high, 32);
            builder.ins().bor(low, high)
        } else {
            builder.ins().load(t, flags, addr, at)
        }
    }).collect()
}

/// One part of an aggregate being built: a scalar at a byte offset.
pub(super) type Part = (u32, cl_ir::Value);

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// The values to hold as words, and the parameters passed by value to keep
    /// in registers: those no use takes an address of to keep or to write
    /// through. A tracked parameter is written to as it is moved, and stays in
    /// memory.
    pub(super) fn find_word_values(&self) -> (HashSet<ValueId>, HashSet<ParamId>) {
        let mut chosen: HashSet<ValueId> = self.func.value_types.iter().enumerate()
            .filter(|(_, ty)| self.osr_spec.is_none() && word_pieces(ty).is_some())
            .map(|(i, _)| ValueId(i as u32))
            .collect();
        let mut params: HashSet<ParamId> = self.func_ctx.param_modes.iter().zip(&self.func_ctx.param_types).enumerate()
            .filter(|(i, (mode, ty))| matches!(super::arg_pass(**mode, ty), super::ArgPass::Value(_))
                && !self.func_ctx.tracked_params.contains(&ParamId(*i as u32)))
            .map(|(i, _)| ParamId(i as u32))
            .collect();
        let mut pin = |op: &Operand| match op {
            Operand::Value(v) => {
                chosen.remove(v);
            }
            Operand::Param(p) => {
                params.remove(p);
            }
            _ => {}
        };
        for inst in self.func.blocks.iter().flat_map(|b| &b.instructions) {
            let modes = inst.call_target().map(|func| self.callee_param_modes(func)).unwrap_or_default();
            inst.for_each_place_use(|op, place_use| {
                let kept_or_written = match place_use {
                    PlaceUse::Read | PlaceUse::Dropped => false,
                    PlaceUse::Replaced | PlaceUse::Changed | PlaceUse::Lent => true,
                    PlaceUse::Passed(i) => !reads_argument(&modes, i),
                };
                if kept_or_written {
                    pin(&op);
                }
            });
            // A collection whose element is reached, or which is walked, is
            // only read, but each reach would store it to its home again.
            match inst {
                Instruction::ListElementRef { list: coll, .. } | Instruction::MapValueRef { map: coll, .. }
                | Instruction::TensorIndexRef { tensor: coll, .. } | Instruction::CursorBegin { coll, .. }
                | Instruction::CursorAdvance { coll, .. } => pin(coll),
                _ => {}
            }
        }
        (chosen, params)
    }

    /// Write parameter `pid`, kept in registers, to its home, and give the
    /// home's address.
    pub(super) fn materialize_param(&self, builder: &mut FunctionBuilder, pid: ParamId) -> Option<cl_ir::Value> {
        let (regs, home) = self.param_regs.get(&pid)?;
        let addr = builder.ins().stack_addr(crate::types::PTR_TYPE, *home, 0);
        store_regs(builder, addr, &self.func_ctx.param_types[pid.0 as usize], regs);
        Some(addr)
    }

    /// Whether `dest` is to be held as words.
    pub(super) fn holds_words(&self, dest: ValueId) -> bool {
        self.word_values.contains(&dest) && !self.return_slot.contains(&dest)
    }

    /// The words of `op`, if it is a value held as words or a parameter of
    /// an aggregate type kept in registers.
    pub(super) fn words_of(&self, op: &Operand) -> Option<&Vec<cl_ir::Value>> {
        match op {
            Operand::Value(v) => self.words.get(v),
            Operand::Param(p) => self.param_regs.get(p)
                .filter(|_| !types::ir_type_to_cranelift(&self.func_ctx.param_types[p.0 as usize]).is_scalar())
                .map(|(regs, _)| regs),
            _ => None,
        }
    }

    /// What a function returns when its call is being abandoned, or its stack
    /// is too deep: nothing the caller will read, of the types it returns.
    pub(super) fn return_abandoned(&self, builder: &mut FunctionBuilder) {
        let RetPass::Value(tys) = &self.ret else {
            builder.ins().return_(&[]);
            return;
        };
        let zeros: Vec<cl_ir::Value> = tys.iter().map(|&t| match t {
            cl_types::F32 => builder.ins().f32const(0.0),
            cl_types::F64 => builder.ins().f64const(0.0),
            t => builder.ins().iconst(t, 0),
        }).collect();
        builder.ins().return_(&zeros);
    }

    /// Write the words of value `vid` to its home, and give the home's address:
    /// what a use that wants an address gets.
    pub(super) fn materialize_words(&self, builder: &mut FunctionBuilder, vid: ValueId) -> Option<cl_ir::Value> {
        let words = self.words.get(&vid)?;
        let frame = self.frame_slot.expect("a value held as words has a home in the frame");
        let home = frame.addr(builder, self.layout.value_offset(vid.0) as i32);
        let ty = &self.func.value_types[vid.0 as usize];
        store_words_at(builder, home, ty, words);
        Some(home)
    }

    /// Define `dest` as `words`: held so if it is to be, and otherwise stored
    /// at its home.
    ///
    /// A value with no bytes has no words, and is the address of its home.
    pub(super) fn define_words(&mut self, builder: &mut FunctionBuilder, dest: ValueId, words: Vec<cl_ir::Value>) -> Result<(), CraneliftError> {
        if words.is_empty() {
            let home = self.dest_addr(builder, dest)?;
            self.values.insert(dest, home);
            return Ok(());
        }
        if self.holds_words(dest) {
            self.words.insert(dest, words);
            return Ok(());
        }
        let ty = self.func.value_types[dest.0 as usize].clone();
        let home = self.dest_addr(builder, dest)?;
        store_words_at(builder, home, &ty, &words);
        self.values.insert(dest, home);
        Ok(())
    }

    /// The words of operand `op`, of type `ty`: its own if it is held so, or
    /// loaded from where it is.
    pub(super) fn operand_words(&mut self, builder: &mut FunctionBuilder, op: &Operand, ty: &IrType) -> Result<Vec<cl_ir::Value>, CraneliftError> {
        if let Some(words) = self.words_of(op) {
            return Ok(words.clone());
        }
        if let Operand::Slot(s) = op
            && let Some(regs) = self.slot_regs(builder, *s)
        {
            return Ok(regs);
        }
        let addr = self.get_operand_ptr(builder, op)?;
        Ok(load_words(builder, addr, ty))
    }

    /// The parts of operand `op`, of type `ty`, placed `at` bytes into what is
    /// being built: a scalar's value, or an aggregate's words. None if `ty` is
    /// neither, a dynamic type or one too large.
    pub(super) fn operand_parts(&mut self, builder: &mut FunctionBuilder, op: &Operand, ty: &IrType, at: u32) -> Result<Option<Vec<Part>>, CraneliftError> {
        match types::ir_type_to_cranelift(ty) {
            CraneliftRepr::Scalar(_) => Ok(Some(vec![(at, self.get_operand_value(builder, op)?)])),
            CraneliftRepr::Aggregate(layout) if layout.size == 0 => Ok(Some(Vec::new())),
            CraneliftRepr::Aggregate(_) => {
                let Some(cut) = word_pieces(ty) else { return Ok(None) };
                let words = self.operand_words(builder, op, ty)?;
                Ok(Some(cut.iter().zip(words).map(|(&(o, _), w)| (at + o as u32, w)).collect()))
            }
        }
    }

    /// Build the words of a value of `ty` from `parts`, each of which lies
    /// within one piece; the bytes no part covers are zero. None if a part
    /// straddles two pieces.
    pub(super) fn compose_words(builder: &mut FunctionBuilder, ty: &IrType, parts: &[Part]) -> Option<Vec<cl_ir::Value>> {
        let cut = word_pieces(ty)?;
        let mut words = Vec::with_capacity(cut.len());
        for &(at, piece_ty) in &cut {
            let (start, end) = (at as u32, at as u32 + piece_ty.bytes());
            let mut acc: Option<cl_ir::Value> = None;
            for &(off, v) in parts {
                let part_ty = builder.func.dfg.value_type(v);
                let n = part_ty.bytes();
                if off >= end || off + n <= start {
                    continue;
                }
                if off < start || off + n > end {
                    return None;
                }
                let int_ty = cl_ir::Type::int(part_ty.bits() as u16).expect("a part is at most a word");
                let mut bits = if part_ty.is_float() {
                    builder.ins().bitcast(int_ty, MemFlagsData::new(), v)
                } else {
                    v
                };
                if int_ty != piece_ty {
                    bits = builder.ins().uextend(piece_ty, bits);
                }
                if off > start {
                    bits = builder.ins().ishl_imm_u(bits, ((off - start) * 8) as i64);
                }
                acc = Some(match acc {
                    Some(a) => builder.ins().bor(a, bits),
                    None => bits,
                });
            }
            words.push(acc.unwrap_or_else(|| builder.ins().iconst(piece_ty, 0)));
        }
        Some(words)
    }

    /// The scalar of Cranelift type `want` at byte `off` of `words`, the words
    /// of a value of `ty`. None if it straddles two pieces.
    pub(super) fn extract_scalar(builder: &mut FunctionBuilder, ty: &IrType, words: &[cl_ir::Value], off: u32, want: cl_ir::Type) -> Option<cl_ir::Value> {
        let cut = word_pieces(ty)?;
        let n = want.bytes();
        let (i, &(at, piece_ty)) = cut.iter().enumerate()
            .find(|(_, (at, t))| off >= *at as u32 && off + n <= *at as u32 + t.bytes())?;
        let mut v = words[i];
        if off > at as u32 {
            v = builder.ins().ushr_imm_u(v, ((off - at as u32) * 8) as i64);
        }
        let int_ty = cl_ir::Type::int(want.bits() as u16).expect("a scalar is at most a word");
        if int_ty != piece_ty {
            v = builder.ins().ireduce(int_ty, v);
        }
        if want.is_float() {
            v = builder.ins().bitcast(want, MemFlagsData::new(), v);
        }
        Some(v)
    }

    /// The part of type `part_ty` at byte `off` of `words`, the words of a
    /// value of `ty`: a scalar as `Ok(scalar)`, an aggregate as `Err(words)`.
    /// None if any piece of it straddles two of `words`.
    pub(super) fn extract_part(builder: &mut FunctionBuilder, ty: &IrType, words: &[cl_ir::Value], off: u32, part_ty: &IrType) -> Option<Result<cl_ir::Value, Vec<cl_ir::Value>>> {
        match types::ir_type_to_cranelift(part_ty) {
            CraneliftRepr::Scalar(t) => Self::extract_scalar(builder, ty, words, off, t).map(Ok),
            CraneliftRepr::Aggregate(layout) if layout.size == 0 => Some(Err(Vec::new())),
            CraneliftRepr::Aggregate(_) => {
                let cut = word_pieces(part_ty)?;
                let mut out = Vec::with_capacity(cut.len());
                for (at, t) in cut {
                    out.push(Self::extract_scalar(builder, ty, words, off + at as u32, t)?);
                }
                Some(Err(out))
            }
        }
    }

    /// Define `dest` as a part `extract_part` gave.
    pub(super) fn define_part(&mut self, builder: &mut FunctionBuilder, dest: ValueId, part: Result<cl_ir::Value, Vec<cl_ir::Value>>) -> Result<(), CraneliftError> {
        match part {
            Ok(v) => {
                self.values.insert(dest, v);
                Ok(())
            }
            Err(ws) => self.define_words(builder, dest, ws),
        }
    }

    /// Hold `dest` as the words `parts` build, where `dest` is to be held so
    /// and they can be built; false, having defined nothing, if not.
    ///
    /// `parts` is what goes before the operand `rest`, which is placed `at`
    /// bytes in, as a payload or a field is.
    pub(super) fn build_words(&mut self, builder: &mut FunctionBuilder, dest: ValueId, mut parts: Vec<Part>, rest: &[(&Operand, IrType, u32)]) -> Result<bool, CraneliftError> {
        if !self.holds_words(dest) {
            return Ok(false);
        }
        for (op, ty, at) in rest {
            match self.operand_parts(builder, op, ty, *at)? {
                Some(more) => parts.extend(more),
                None => return Ok(false),
            }
        }
        let ty = self.func.value_types[dest.0 as usize].clone();
        match Self::compose_words(builder, &ty, &parts) {
            Some(words) => {
                self.words.insert(dest, words);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// The tag byte at the start of an option or result held as `words`.
    pub(super) fn tag_of_words(builder: &mut FunctionBuilder, ty: &IrType, words: &[cl_ir::Value]) -> cl_ir::Value {
        Self::extract_scalar(builder, ty, words, 0, cl_types::I8).expect("a tag is the first byte")
    }

    /// The Cranelift types of a block parameter held as words, or None.
    pub(super) fn block_param_words(&self, vid: ValueId) -> Option<Vec<cl_ir::Type>> {
        if !self.holds_words(vid) {
            return None;
        }
        word_pieces(&self.func.value_types[vid.0 as usize]).map(|cut| cut.into_iter().map(|(_, t)| t).collect())
    }
}
