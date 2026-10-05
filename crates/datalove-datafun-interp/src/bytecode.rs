//! A register bytecode for function bodies.
//!
//! A function body is lowered once, beside its `IrLayout`, into a flat array of
//! ops whose operands are offsets into the frame. Everything the IR walker
//! works out again at every execution -- where an operand is, what type it
//! has, how big it is, where an option's payload sits, which block a jump
//! lands on -- is worked out here, once.
//!
//! The frame is the IR walker's frame, laid out the same way, so an
//! instruction this lowering has no op for yet is not a reason to give up on
//! the body: it becomes an `Ir` op, which runs that one instruction on the IR
//! walker against the same frame. Coverage grows an op at a time, and every
//! body runs from the first.
//!
//! See `botdocs/plan-bytecode.md`.

use datalove_datafun_ir::layout::{layout_of, option_payload_offset, result_payload_offset};
use datalove_datafun_ir::{
    BinOp, BlockId, CodeRef, CodeUnitContext, ConstValue, Instruction, IrCodeUnit, IrType, Operand,
    ParamMode, SlotDest, Terminator, ValueId,
};
use datalove_datafun_intrinsics::IntrinsicId;
use datalove_rtdt as rtdt;

use crate::env::{ExecutionContext, FunctionRegistry};
use crate::error::InterpError;
use crate::frame::{Frame, FrameStore};
use crate::layout::IrLayout;
use crate::ops::CheckedIntOps;
use crate::value::Destination;
use datalove_datafun_ir::frame_layout::tracking;
use crate::{copy_bytes, IrInterpreter, UnitTypes};

/// Where an operand is: in the frame at an offset, or, with the top bit set,
/// behind a pointer the frame holds at that offset.
///
/// Values and slots are direct. Parameters and references are indirect: a
/// parameter's pointer is in the frame's parameter region, which
/// `Frame::enter` fills, and a reference is a value holding a pointer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Loc(u32);

const INDIRECT: u32 = 1 << 31;

impl Loc {
    fn direct(offset: u32) -> Self {
        assert!(offset & INDIRECT == 0, "frame offset {} is too large to encode", offset);
        Loc(offset)
    }

    fn indirect(offset: u32) -> Self {
        Loc(Self::direct(offset).0 | INDIRECT)
    }

    /// The address the operand names, in the frame whose data starts at `base`.
    ///
    /// # Safety
    ///
    /// `base` is the data of a frame laid out by the layout this was lowered
    /// against, and an indirect operand's pointer has been written.
    #[inline(always)]
    unsafe fn at(self, base: *mut u8) -> *mut u8 {
        unsafe {
            if self.0 & INDIRECT == 0 {
                base.add(self.0 as usize)
            } else {
                *(base.add((self.0 & !INDIRECT) as usize) as *const *mut u8)
            }
        }
    }
}

/// A comparison, for the ops that take one as a field.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cmp {
    fn of(op: BinOp) -> Option<Self> {
        Some(match op {
            BinOp::Eq => Cmp::Eq,
            BinOp::Ne => Cmp::Ne,
            BinOp::Lt => Cmp::Lt,
            BinOp::Le => Cmp::Le,
            BinOp::Gt => Cmp::Gt,
            BinOp::Ge => Cmp::Ge,
            _ => return None,
        })
    }

    #[inline(always)]
    fn apply<T: PartialOrd>(self, a: T, b: T) -> bool {
        match self {
            Cmp::Eq => a == b,
            Cmp::Ne => a != b,
            Cmp::Lt => a < b,
            Cmp::Le => a <= b,
            Cmp::Gt => a > b,
            Cmp::Ge => a >= b,
        }
    }
}

/// One instruction of the bytecode.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    /// Run instruction `index` of block `block` on the IR walker.
    Ir { block: u32, index: u32 },
    /// Make the call that is instruction `index` of block `block`, straight
    /// into the call path rather than through the IR walker's dispatch.
    Call { block: u32, index: u32 },
    /// Make the call described by `calls[site]`, whose arguments are all SSA
    /// values, without the general call path. Takes the general one whenever
    /// a dispatcher is installed, so the JIT and the inliner see every call.
    CallFast { site: u32 },

    Const1 { dst: Loc, imm: u8 },
    Const4 { dst: Loc, imm: u32 },
    /// Copy `len` bytes of the constant pool from `at`.
    ConstPool { dst: Loc, at: u32, len: u32 },

    Copy1 { dst: Loc, src: Loc },
    Copy4 { dst: Loc, src: Loc },
    Copy8 { dst: Loc, src: Loc },
    CopyN { dst: Loc, src: Loc, len: u32 },

    AddCkU32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    SubCkU32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    MulCkU32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    AddCkI32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    SubCkI32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    MulCkI32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    AddCkU64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    SubCkU64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    MulCkU64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    AddCkI64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    SubCkI64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    MulCkI64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },

    CmpU8 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },
    CmpU32 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },
    CmpI32 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },
    CmpU64 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },
    CmpI64 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },

    AddWrapU32 { dst: Loc, a: Loc, b: Loc },
    SubWrapU32 { dst: Loc, a: Loc, b: Loc },
    MulWrapU32 { dst: Loc, a: Loc, b: Loc },
    RemU32 { dst: Loc, a: Loc, b: Loc },
    ShrU32 { dst: Loc, a: Loc, b: Loc },
    ShlU32 { dst: Loc, a: Loc, b: Loc },
    AndU32 { dst: Loc, a: Loc, b: Loc },

    /// Option and result tags, and payloads at a fixed offset.
    WrapSome { dst: Loc, src: Loc, at: u32, len: u32 },
    WrapNone { dst: Loc },
    WrapOk { dst: Loc, src: Loc, at: u32, len: u32 },
    UnwrapOption { dst: Loc, flag: Loc, src: Loc, at: u32, len: u32 },
    UnwrapResult { ok: Loc, err: Loc, flag: Loc, src: Loc, at: u16, ok_len: u16, err_len: u16 },

    /// Destroy what `src` holds; `desc` indexes the body's descriptors.
    Drop { src: Loc, desc: u32 },
    /// Destroy what a tracked slot or `out` parameter holds, if its tracking
    /// byte at frame offset `track` says it holds anything, and mark it moved.
    DropTracked { src: Loc, track: u32, desc: u32 },
    /// Store into a tracked slot: destroy what it holds if anything, copy
    /// `len` bytes from `src`, mark it live.
    StoreTracked { dst: Loc, track: u32, src: Loc, len: u32, desc: u32 },
    /// Move out of a tracked slot and mark it moved.
    LoadMoveTracked { dst: Loc, src: Loc, track: u32, len: u32 },
    /// Widen a fixed-width integer to an `int`.
    Widen { dst: Loc, src: Loc, src_desc: u32 },
    /// A binary operation the typed ops do not cover, on the IR walker's own
    /// routine with its operands resolved: `rt[at]` holds dst, lhs, rhs.
    BinOpRt { op: BinOp, at: u32 },

    Jump { to: u32 },
    BrIf { cond: Loc, then: u32, els: u32 },
    /// A comparison whose only use is the branch on it.
    BrCmpU8 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    BrCmpU32 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    BrCmpI32 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    BrCmpU64 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    BrCmpI64 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    /// Pass block `block`'s terminator's arguments for its edge `edge` (0 for
    /// a `Goto` or the then side of a `Branch`, 1 for the else side) on the IR
    /// walker, then jump to `to`.
    EdgeIr { block: u32, edge: u32, to: u32 },
    /// Jump through the switch table `table`.
    Switch { disc: Loc, table: u32 },
    Return { src: Loc, len: u32 },
    ReturnUnit,
    /// Run block `block`'s `Return` on the IR walker.
    ReturnIr { block: u32 },
}

/// A switch's cases and default, as op indices.
struct SwitchTable {
    cases: Vec<(u32, u32)>,
    default: u32,
}

/// A call whose operands the lowering resolved.
struct FastCall {
    /// Where the instruction is, for the general path.
    block: u32,
    index: u32,
    /// Whether each argument is an SSA value, the only kind whose being
    /// moved into the call the frame need not record. The arguments
    /// themselves are read as the IR walker reads them, which knows where a
    /// parameter's or a reference's descriptor comes from at run time.
    args: Vec<bool>,
    /// Each argument's place and descriptor, where every one is statically
    /// typed and there are no shapes to hand over: then reading them is a
    /// pointer each, which is most calls outside generic code.
    resolved: Option<Vec<(Loc, *const rtdt::TyDesc)>>,
    dest: Loc,
    dest_tydesc: *const rtdt::TyDesc,
    /// The callee's layout the last time, the body it was for, and whether
    /// this call's arguments suit the fast path for it, so that a call need
    /// not look any of it up again unless the body has changed. No layout for
    /// a native callee.
    cache: std::cell::RefCell<Option<(usize, Option<std::rc::Rc<IrLayout>>, bool)>>,
    /// A module callee, found once: the registry it was found in, held so that
    /// it cannot be freed and its address reused, and the body. Valid while
    /// the call's registry is that one -- module bodies do not change within
    /// a registry -- which saves two ordered-map lookups a call.
    module_callee: std::cell::RefCell<Option<(
        std::sync::Arc<datalove_datafun_ir::registry::ModuleFunctionRegistry>,
        *const IrCodeUnit,
    )>>,
}

/// A function body, lowered.
pub(crate) struct BcFunction {
    calls: Vec<FastCall>,
    /// Descriptors the ops name by index.
    descs: Vec<*const rtdt::TyDesc>,
    /// Operand triples for the ops that hand theirs to an IR walker routine.
    rt: Vec<[(Loc, *const rtdt::TyDesc); 3]>,
    ops: Vec<Op>,
    /// Where execution starts: the prologue that writes the hoisted
    /// constants, which ends by jumping to the first block.
    entry: u32,
    pool: Vec<u8>,
    switches: Vec<SwitchTable>,
}

/// Counts of what was lowered, for judging coverage.
#[derive(Default, Debug)]
pub struct BcStats {
    pub bodies: u32,
    pub ops: u32,
    pub escapes: u32,
    /// Whether to count what runs on the IR walker as it runs, which costs a
    /// formatted string per instruction and so only with `DATALOVE_BC_STATS`.
    pub counting: bool,
    /// How many times each kind of instruction ran on the IR walker.
    pub executed: std::collections::HashMap<String, u64>,
}

impl BcStats {
    /// Count one execution of something the bytecode handed to the IR walker.
    #[cold]
    fn count(&mut self, what: impl FnOnce() -> String) {
        *self.executed.entry(what()).or_default() += 1;
    }
}

/// An instruction's variant name.
fn variant(instr: &Instruction) -> String {
    let text = format!("{instr:?}");
    text.split([' ', '{', '(']).next().unwrap_or("").to_string()
}

// =============================================================================
// Lowering
// =============================================================================

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
    ops: Vec<Op>,
    pool: Vec<u8>,
    switches: Vec<SwitchTable>,
    calls: Vec<FastCall>,
    descs: Vec<*const rtdt::TyDesc>,
    rt: Vec<[(Loc, *const rtdt::TyDesc); 3]>,
    /// Op indices still naming a block, to be patched to its first op.
    fixups: Vec<(usize, Fixup)>,
    block_start: Vec<u32>,
    escapes: u32,
    /// How many times each value is read, for fusing an op into its only use.
    uses: Vec<u32>,
    /// Values something other than their defining instruction may write, an
    /// `out` argument or a reference store, which a hoisted constant would not
    /// be rewritten for.
    pinned: Vec<bool>,
    /// Constants written once on entry rather than at every execution.
    prologue: Vec<Op>,
    /// Whether each block is in a loop, where hoisting a constant out of it
    /// saves work rather than adding it to every call.
    in_loop: Vec<bool>,
    /// The block being lowered, and the instruction in it.
    block: usize,
    index: usize,
}

#[derive(Clone, Copy)]
enum Fixup {
    JumpTo,
    BrThen,
    BrElse,
    EdgeTo,
    SwitchCase(usize, usize),
    SwitchDefault(usize),
}

impl<'a> Lowering<'a> {
    /// The static type of an operand, where it is the truth about what is
    /// there: none for anything that reaches another unit, or whose type has a
    /// `data` in it, or a borrowed parameter, whose descriptor is the caller's.
    fn typed(&self, op: &Operand) -> Option<&'a IrType> {
        let func = self.func;
        let ty = match op {
            Operand::Value(id) => &func.value_types[id.0 as usize],
            Operand::Slot(id) => &func.slot_types[id.0 as usize],
            Operand::Param(id) => {
                let ctx = func.function_context()?;
                let i = id.0 as usize;
                if matches!(ctx.param_modes.get(i), Some(ParamMode::Ref | ParamMode::Mut)) {
                    // The caller's descriptor rides with a borrowed parameter;
                    // outside a generic it agrees with this type anyway.
                    if has_data(&ctx.param_types[i]) {
                        return None;
                    }
                }
                &ctx.param_types[i]
            }
            Operand::ValueRef(id) => match &func.value_types[id.0 as usize] {
                IrType::Ref(inner) => inner,
                _ => return None,
            },
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => return None,
        };
        if has_data(ty) { None } else { Some(ty) }
    }

    fn loc(&self, op: &Operand) -> Option<Loc> {
        let layout = self.layout;
        Some(match op {
            Operand::Value(id) => Loc::direct(layout.value_offsets[id.0 as usize]),
            Operand::Slot(id) => Loc::direct(layout.slot_offsets[id.0 as usize]),
            Operand::Param(id) => Loc::indirect(layout.param_offsets[id.0 as usize]),
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

    /// An operand an instruction consumes: only an SSA value, whose being
    /// consumed the frame need not record. A consumed slot or parameter may
    /// have a tracking byte to update, which is the IR walker's to do.
    fn consumed(&self, op: &Operand) -> Option<Loc> {
        match op {
            Operand::Value(_) => self.loc(op),
            _ => None,
        }
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
        self.ops.push(op);
    }

    fn escape(&mut self, block: usize, index: usize) {
        self.escapes += 1;
        self.emit(Op::Ir { block: block as u32, index: index as u32 });
    }

    fn lower(mut self) -> (BcFunction, u32) {
        let func = self.func;
        self.count_uses();
        for (b, block) in func.blocks.iter().enumerate() {
            self.block = b;
            self.block_start[b] = self.ops.len() as u32;
            for (i, instr) in block.instructions.iter().enumerate() {
                self.index = i;
                if !self.lower_instruction(instr) {
                    self.escape(b, i);
                }
            }
            self.lower_terminator(b, &block.terminator);
        }
        for (at, fixup) in std::mem::take(&mut self.fixups) {
            let start = |target: u32| self.block_start[target as usize];
            match fixup {
                Fixup::JumpTo => if let Op::Jump { to } = &mut self.ops[at] { *to = start(*to) },
                Fixup::BrThen => *branch_targets(&mut self.ops[at]).0 = start(*branch_targets(&mut self.ops[at]).0),
                Fixup::BrElse => *branch_targets(&mut self.ops[at]).1 = start(*branch_targets(&mut self.ops[at]).1),
                Fixup::EdgeTo => if let Op::EdgeIr { to, .. } = &mut self.ops[at] { *to = start(*to) },
                Fixup::SwitchCase(t, c) => {
                    let target = self.switches[t].cases[c].1;
                    self.switches[t].cases[c].1 = start(target);
                }
                Fixup::SwitchDefault(t) => {
                    let target = self.switches[t].default;
                    self.switches[t].default = start(target);
                }
            }
        }
        // The prologue goes last, so that adding it moves nothing.
        let entry = self.ops.len() as u32;
        let prologue = std::mem::take(&mut self.prologue);
        self.ops.extend(prologue);
        self.ops.push(Op::Jump { to: 0 });
        let escapes = self.escapes;
        (BcFunction {
            ops: self.ops, entry, pool: self.pool, switches: self.switches, calls: self.calls,
            descs: self.descs, rt: self.rt,
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
                let (Some(ty), Some(src)) = (self.typed(src), self.consumed(src)) else { return false };
                if let Some(op) = Self::copy(self.value_loc(*dest), src, layout_of(ty).size) {
                    self.emit(op);
                }
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
                    Some(matches!(arg, Operand::Value(_)))
                }).collect();
                let resolved: Option<Vec<_>> = match instr {
                    Instruction::Call { shape_descriptors, .. } if shape_descriptors.is_empty() => {
                        args.iter().map(|arg| {
                            self.typed(arg)?;
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
                            cache: std::cell::RefCell::new(None),
                            module_callee: std::cell::RefCell::new(None),
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
                // A copied type owns nothing, and a value whose drop the frame
                // records is the IR walker's.
                let (Some(ty), Some(src), Some(desc)) =
                    (self.typed(operand), self.consumed(operand), self.desc_of(operand)) else { return false };
                if !ty.is_copy() {
                    let desc = self.desc_index(desc);
                    self.emit(Op::Drop { src, desc });
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
                let slot_op = Operand::Slot(*slot);
                let (Some(ty), Some(src), Some(desc)) =
                    (self.typed(value), src, self.desc_of(&slot_op)) else { return false };
                if self.typed(&slot_op).is_none() {
                    return false;
                }
                let dst = Loc::direct(self.layout.slot_offsets[slot.0 as usize]);
                let desc = self.desc_index(desc);
                self.emit(Op::StoreTracked { dst, track, src, len: layout_of(ty).size, desc });
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

    /// Emit a constant's op, on entry if nothing but it writes the value.
    fn emit_const(&mut self, dest: ValueId, op: Op) {
        if self.pinned[dest.0 as usize] || !self.in_loop[self.block] {
            self.emit(op);
        } else {
            self.prologue.push(op);
        }
    }

    fn lower_const(&mut self, dest: ValueId, value: &ConstValue) -> bool {
        let dst = self.value_loc(dest);
        let bytes: Vec<u8> = match value {
            ConstValue::Unit => return true,
            ConstValue::Bool(b) => {
                self.emit_const(dest, Op::Const1 { dst, imm: *b as u8 });
                return true;
            }
            ConstValue::U8(n) => {
                self.emit_const(dest, Op::Const1 { dst, imm: *n });
                return true;
            }
            ConstValue::I8(n) => {
                self.emit_const(dest, Op::Const1 { dst, imm: *n as u8 });
                return true;
            }
            ConstValue::U32(n) => {
                self.emit_const(dest, Op::Const4 { dst, imm: *n });
                return true;
            }
            ConstValue::I32(n) => {
                self.emit_const(dest, Op::Const4 { dst, imm: *n as u32 });
                return true;
            }
            ConstValue::F32(f) => {
                self.emit_const(dest, Op::Const4 { dst, imm: f.0.to_bits() });
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
            _ => return false,
        };
        let at = self.pool.len() as u32;
        let len = bytes.len() as u32;
        self.pool.extend_from_slice(&bytes);
        self.emit_const(dest, Op::ConstPool { dst, at, len });
        true
    }

    fn lower_binop(&mut self, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> bool {
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
            _ => return false,
        };
        // A bool compares as its byte only for equality.
        if matches!(lt, IrType::Bool) && !matches!(cmp, Cmp::Eq | Cmp::Ne) {
            return false;
        }
        self.emit(op);
        true
    }

    /// A binary operation on the IR walker's routine, with its operands
    /// resolved, for the types no typed op covers.
    fn lower_binop_rt(&mut self, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> bool {
        let d = Operand::Value(dest);
        let mut triple = [(Loc(0), std::ptr::null()); 3];
        for (k, operand) in [&d, lhs, rhs].into_iter().enumerate() {
            let (Some(_), Some(loc), Some(desc)) =
                (self.typed(operand), self.loc(operand), self.desc_of(operand)) else { return false };
            triple[k] = (loc, desc);
        }
        self.rt.push(triple);
        self.emit(Op::BinOpRt { op, at: (self.rt.len() - 1) as u32 });
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
            _ => return false,
        };
        self.emit(op);
        true
    }

    fn lower_intrinsic(&mut self, dest: ValueId, intrinsic: IntrinsicId, args: &[Operand]) -> bool {
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

    /// Emit a jump along an edge to `target`, with its arguments.
    fn jump(&mut self, block: usize, edge: u32, target: BlockId, args: &[Operand]) {
        match self.edge_copies(target, args) {
            Some(copies) => {
                self.ops.extend(copies);
                self.fixups.push((self.ops.len(), Fixup::JumpTo));
                self.emit(Op::Jump { to: target.0 });
            }
            None => {
                self.escapes += 1;
                self.fixups.push((self.ops.len(), Fixup::EdgeTo));
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
                let fused = self.fuse_compare(cond, then_block.0, else_block.0);
                let cond = self.loc(cond).expect("a branch condition is a local bool");
                let direct = |args: &[Operand]| args.is_empty();
                // An edge with arguments gets a stub after the branch that
                // passes them and jumps on.
                let at = self.ops.len();
                self.emit(fused.unwrap_or(Op::BrIf { cond, then: then_block.0, els: else_block.0 }));
                if direct(then_args) {
                    self.fixups.push((at, Fixup::BrThen));
                } else {
                    let stub = self.ops.len() as u32;
                    self.jump(block, 0, *then_block, then_args);
                    *branch_targets(&mut self.ops[at]).0 = stub;
                }
                if direct(else_args) {
                    self.fixups.push((at, Fixup::BrElse));
                } else {
                    let stub = self.ops.len() as u32;
                    self.jump(block, 1, *else_block, else_args);
                    *branch_targets(&mut self.ops[at]).1 = stub;
                }
            }
            Terminator::Switch { discriminant, cases, default } => {
                let disc = self.loc(discriminant).expect("a switch discriminant is local");
                let t = self.switches.len();
                self.switches.push(SwitchTable {
                    cases: cases.iter().map(|(v, b)| (*v, b.0)).collect(),
                    default: default.0,
                });
                for c in 0..cases.len() {
                    self.fixups.push((0, Fixup::SwitchCase(t, c)));
                }
                self.fixups.push((0, Fixup::SwitchDefault(t)));
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
                        self.escapes += 1;
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
                    | Instruction::RefSetField { dest, .. } | Instruction::RefSetFieldTracked { dest, .. } => {
                        if let Operand::Value(id) | Operand::ValueRef(id) = dest {
                            pinned[id.0 as usize] = true;
                        }
                    }
                    _ => {}
                }
            }
            block.terminator.for_each_operand(|op| read(&mut uses, op));
        }
        self.uses = uses;
        self.pinned = pinned;
        self.in_loop = self.loop_blocks();
    }

    /// Which blocks can reach themselves.
    fn loop_blocks(&self) -> Vec<bool> {
        let blocks = &self.func.blocks;
        let successors = |b: usize| -> Vec<usize> {
            match &blocks[b].terminator {
                Terminator::Goto { target, .. } => vec![target.0 as usize],
                Terminator::Branch { then_block, else_block, .. } => {
                    vec![then_block.0 as usize, else_block.0 as usize]
                }
                Terminator::Switch { cases, default, .. } => {
                    cases.iter().map(|(_, b)| b.0 as usize).chain([default.0 as usize]).collect()
                }
                _ => vec![],
            }
        };
        (0..blocks.len())
            .map(|start| {
                let mut seen = vec![false; blocks.len()];
                let mut stack = successors(start);
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

    /// A branch on a comparison just emitted, fused with it, if the branch is
    /// the comparison's only use.
    fn fuse_compare(&mut self, cond: &Operand, then: u32, els: u32) -> Option<Op> {
        let Operand::Value(id) = cond else { return None };
        if self.uses[id.0 as usize] != 1 {
            return None;
        }
        let want = self.value_loc(*id).0;
        let fused = match *self.ops.last()? {
            Op::CmpU8 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU8 { cmp, a, b, then, els },
            Op::CmpU32 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU32 { cmp, a, b, then, els },
            Op::CmpI32 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpI32 { cmp, a, b, then, els },
            Op::CmpU64 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU64 { cmp, a, b, then, els },
            Op::CmpI64 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpI64 { cmp, a, b, then, els },
            _ => return None,
        };
        self.ops.pop();
        Some(fused)
    }
}

/// A conditional branch's then and else targets.
fn branch_targets(op: &mut Op) -> (&mut u32, &mut u32) {
    match op {
        Op::BrIf { then, els, .. }
        | Op::BrCmpU8 { then, els, .. }
        | Op::BrCmpU32 { then, els, .. }
        | Op::BrCmpI32 { then, els, .. }
        | Op::BrCmpU64 { then, els, .. }
        | Op::BrCmpI64 { then, els, .. } => (then, els),
        op => unreachable!("{:?} is not a branch", op),
    }
}

/// Lower a function body against its layout.
pub(crate) fn lower(func: &IrCodeUnit, layout: &IrLayout) -> (BcFunction, u32) {
    Lowering {
        func,
        layout,
        ops: Vec::new(),
        pool: Vec::new(),
        switches: Vec::new(),
        calls: Vec::new(),
        descs: Vec::new(),
        rt: Vec::new(),
        fixups: Vec::new(),
        block_start: vec![0; func.blocks.len()],
        escapes: 0,
        uses: Vec::new(),
        pinned: Vec::new(),
        prologue: Vec::new(),
        in_loop: Vec::new(),
        block: 0,
        index: 0,
    }
    .lower()
}

// =============================================================================
// Execution
// =============================================================================

#[inline(always)]
unsafe fn rd<T: Copy>(base: *mut u8, loc: Loc) -> T {
    unsafe { std::ptr::read_unaligned(loc.at(base) as *const T) }
}

#[inline(always)]
unsafe fn wr<T>(base: *mut u8, loc: Loc, v: T) {
    unsafe { std::ptr::write_unaligned(loc.at(base) as *mut T, v) }
}

#[inline(always)]
unsafe fn checked<T: Copy + CheckedIntOps>(
    base: *mut u8,
    dst: Loc,
    ovf: Loc,
    a: Loc,
    b: Loc,
    f: fn(T, T) -> (T, bool),
) {
    unsafe {
        let (r, o) = f(rd::<T>(base, a), rd::<T>(base, b));
        wr(base, dst, r);
        wr(base, ovf, o);
    }
}

impl IrInterpreter {
    /// Run a lowered function body in a frame `Frame::enter` has made ready.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_bytecode(
        &mut self,
        bc: &BcFunction,
        func: &IrCodeUnit,
        frame: &mut Frame,
        ret_dest: Destination,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        code_ref: Option<&CodeRef>,
    ) -> Result<(), InterpError> {
        let unit_types = UnitTypes::of(func);
        let ops = &bc.ops[..];
        let base = frame.base_ptr();
        let mut pc = bc.entry as usize;
        // SAFETY, for every frame access below: the ops were lowered against
        // this frame's layout, so every offset is inside it, and `base` stays
        // the frame's data for the whole call -- an instruction run on the IR
        // walker uses the same frame and never reallocates it.
        unsafe {
            loop {
                // Every block ends in a jump or a return, and every target is
                // an op of this body, so `pc` never runs off the end.
                debug_assert!(pc < ops.len());
                match *ops.get_unchecked(pc) {
                    Op::Ir { block, index } => {
                        let instr = &func.blocks[block as usize].instructions[index as usize];
                        if self.bc_stats.counting {
                            self.bc_stats.count(|| variant(instr));
                        }
                        if !self.execute_hot(instr, frame, frames) {
                            self.execute_instruction(
                                instr, &unit_types, frame, ctx, registry, frames, code_ref)?;
                        }
                    }
                    Op::Call { block, index } => {
                        if self.bc_stats.counting {
                            self.bc_stats.count(|| "(general call)".into());
                        }
                        let (call_site_info, func_ref, args, shapes, dest) =
                            match &func.blocks[block as usize].instructions[index as usize] {
                                Instruction::Call { site_id, dest, func: f, args, shape_descriptors, .. } => (
                                    code_ref.map(|caller| crate::dispatch::CallSiteInfo {
                                        caller: caller.clone(),
                                        caller_unit: ctx.unit(),
                                        call_site_id: *site_id,
                                    }),
                                    f, args, shape_descriptors, *dest,
                                ),
                                Instruction::ComptimeCall { dest, func: f, args, shape_descriptors, .. } => {
                                    (None, f, args, shape_descriptors, *dest)
                                }
                                i => unreachable!("Call op on {:?}", i),
                            };
                        self.execute_call(
                            func_ref, args, shapes, dest, call_site_info, frame, ctx, registry, frames)?;
                    }
                    Op::CallFast { site } => {
                        let call = &bc.calls[site as usize];
                        if !self.fast_call(call, base, frame, ctx, registry, frames, code_ref, func)? {
                            if self.bc_stats.counting {
                                self.bc_stats.count(|| "(fast call fell back)".into());
                            }
                            let Instruction::Call { site_id, dest, func: f, args, shape_descriptors, .. } =
                                &func.blocks[call.block as usize].instructions[call.index as usize] else {
                                unreachable!("a fast call is a call")
                            };
                            let call_site_info = code_ref.map(|caller| crate::dispatch::CallSiteInfo {
                                caller: caller.clone(),
                                caller_unit: ctx.unit(),
                                call_site_id: *site_id,
                            });
                            self.execute_call(
                                f, args, shape_descriptors, *dest, call_site_info, frame, ctx, registry, frames)?;
                        }
                    }
                    Op::Const1 { dst, imm } => wr(base, dst, imm),
                    Op::Const4 { dst, imm } => wr(base, dst, imm),
                    Op::ConstPool { dst, at, len } => {
                        copy_bytes(bc.pool.as_ptr().add(at as usize), dst.at(base), len as usize)
                    }
                    Op::Copy1 { dst, src } => wr(base, dst, rd::<u8>(base, src)),
                    Op::Copy4 { dst, src } => wr(base, dst, rd::<u32>(base, src)),
                    Op::Copy8 { dst, src } => wr(base, dst, rd::<u64>(base, src)),
                    Op::CopyN { dst, src, len } => {
                        std::ptr::copy_nonoverlapping(src.at(base), dst.at(base), len as usize)
                    }

                    Op::AddCkU32 { dst, ovf, a, b } => checked::<u32>(base, dst, ovf, a, b, u32::overflowing_add_impl),
                    Op::SubCkU32 { dst, ovf, a, b } => checked::<u32>(base, dst, ovf, a, b, u32::overflowing_sub_impl),
                    Op::MulCkU32 { dst, ovf, a, b } => checked::<u32>(base, dst, ovf, a, b, u32::overflowing_mul_impl),
                    Op::AddCkI32 { dst, ovf, a, b } => checked::<i32>(base, dst, ovf, a, b, i32::overflowing_add_impl),
                    Op::SubCkI32 { dst, ovf, a, b } => checked::<i32>(base, dst, ovf, a, b, i32::overflowing_sub_impl),
                    Op::MulCkI32 { dst, ovf, a, b } => checked::<i32>(base, dst, ovf, a, b, i32::overflowing_mul_impl),
                    Op::AddCkU64 { dst, ovf, a, b } => checked::<u64>(base, dst, ovf, a, b, u64::overflowing_add_impl),
                    Op::SubCkU64 { dst, ovf, a, b } => checked::<u64>(base, dst, ovf, a, b, u64::overflowing_sub_impl),
                    Op::MulCkU64 { dst, ovf, a, b } => checked::<u64>(base, dst, ovf, a, b, u64::overflowing_mul_impl),
                    Op::AddCkI64 { dst, ovf, a, b } => checked::<i64>(base, dst, ovf, a, b, i64::overflowing_add_impl),
                    Op::SubCkI64 { dst, ovf, a, b } => checked::<i64>(base, dst, ovf, a, b, i64::overflowing_sub_impl),
                    Op::MulCkI64 { dst, ovf, a, b } => checked::<i64>(base, dst, ovf, a, b, i64::overflowing_mul_impl),

                    Op::CmpU8 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<u8>(base, a), rd::<u8>(base, b))),
                    Op::CmpU32 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<u32>(base, a), rd::<u32>(base, b))),
                    Op::CmpI32 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<i32>(base, a), rd::<i32>(base, b))),
                    Op::CmpU64 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<u64>(base, a), rd::<u64>(base, b))),
                    Op::CmpI64 { cmp, dst, a, b } => wr(base, dst, cmp.apply(rd::<i64>(base, a), rd::<i64>(base, b))),

                    Op::AddWrapU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_add(rd(base, b))),
                    Op::SubWrapU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_sub(rd(base, b))),
                    Op::MulWrapU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_mul(rd(base, b))),
                    Op::RemU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a) % rd::<u32>(base, b)),
                    Op::ShrU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_shr(rd(base, b))),
                    Op::ShlU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a).wrapping_shl(rd(base, b))),
                    Op::AndU32 { dst, a, b } => wr(base, dst, rd::<u32>(base, a) & rd::<u32>(base, b)),

                    Op::WrapSome { dst, src, at, len } => {
                        let d = dst.at(base);
                        *d = rtdt::OptionTag::Some as u8;
                        copy_bytes(src.at(base), d.add(at as usize), len as usize);
                    }
                    Op::WrapNone { dst } => *dst.at(base) = rtdt::OptionTag::None as u8,
                    Op::WrapOk { dst, src, at, len } => {
                        let d = dst.at(base);
                        *d = rtdt::ResultTag::Ok as u8;
                        copy_bytes(src.at(base), d.add(at as usize), len as usize);
                    }
                    Op::UnwrapOption { dst, flag, src, at, len } => {
                        let s = src.at(base);
                        let some = *s != rtdt::OptionTag::None as u8;
                        wr(base, flag, some);
                        if some {
                            copy_bytes(s.add(at as usize), dst.at(base), len as usize);
                        }
                    }
                    Op::UnwrapResult { ok, err, flag, src, at, ok_len, err_len } => {
                        let s = src.at(base);
                        let is_ok = *s == rtdt::ResultTag::Ok as u8;
                        wr(base, flag, is_ok);
                        if is_ok {
                            copy_bytes(s.add(at as usize), ok.at(base), ok_len as usize);
                        } else {
                            copy_bytes(s.add(at as usize), err.at(base), err_len as usize);
                        }
                    }

                    Op::Drop { src, desc } => {
                        let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[desc as usize] };
                        self.execute_drop(&val);
                    }
                    Op::DropTracked { src, track, desc } => {
                        let byte = base.add(track as usize);
                        if *byte == tracking::LIVE {
                            let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[desc as usize] };
                            self.execute_drop(&val);
                            *byte = tracking::MOVED;
                        }
                    }
                    Op::StoreTracked { dst, track, src, len, desc } => {
                        let byte = base.add(track as usize);
                        let d = dst.at(base);
                        if *byte == tracking::LIVE {
                            let old = crate::value::Value { ptr: d, tydesc: bc.descs[desc as usize] };
                            self.execute_drop(&old);
                        }
                        copy_bytes(src.at(base), d, len as usize);
                        *byte = tracking::LIVE;
                    }
                    Op::LoadMoveTracked { dst, src, track, len } => {
                        copy_bytes(src.at(base), dst.at(base), len as usize);
                        *base.add(track as usize) = tracking::MOVED;
                    }
                    Op::Widen { dst, src, src_desc } => {
                        let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[src_desc as usize] };
                        self.widen_to_int(&val, &mut *(dst.at(base) as *mut rtdt::Int));
                    }
                    Op::BinOpRt { op, at } => {
                        let [(d, dd), (a, ad), (b, bd)] = bc.rt[at as usize];
                        let lhs = crate::value::Value { ptr: a.at(base), tydesc: ad };
                        let rhs = crate::value::Value { ptr: b.at(base), tydesc: bd };
                        self.execute_binop(op, &lhs, &rhs, Destination { ptr: d.at(base), tydesc: dd });
                    }
                    Op::Jump { to } => {
                        pc = to as usize;
                        continue;
                    }
                    Op::BrIf { cond, then, els } => {
                        pc = if rd::<u8>(base, cond) != 0 { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpU8 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<u8>(base, a), rd::<u8>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpU32 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<u32>(base, a), rd::<u32>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpI32 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<i32>(base, a), rd::<i32>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpU64 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<u64>(base, a), rd::<u64>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::BrCmpI64 { cmp, a, b, then, els } => {
                        pc = if cmp.apply(rd::<i64>(base, a), rd::<i64>(base, b)) { then } else { els } as usize;
                        continue;
                    }
                    Op::EdgeIr { block, edge, to } => {
                        if self.bc_stats.counting {
                            self.bc_stats.count(|| "(edge)".into());
                        }
                        let (target, args) = match &func.blocks[block as usize].terminator {
                            Terminator::Goto { target, args } => (*target, args),
                            Terminator::Branch { then_block, then_args, .. } if edge == 0 => {
                                (*then_block, then_args)
                            }
                            Terminator::Branch { else_block, else_args, .. } => (*else_block, else_args),
                            t => unreachable!("an edge from {:?}", t),
                        };
                        self.pass_block_args(&func.blocks, target, args, frame, frames)?;
                        pc = to as usize;
                        continue;
                    }
                    Op::Switch { disc, table } => {
                        let value = rd::<u32>(base, disc);
                        let table = &bc.switches[table as usize];
                        pc = table.cases.iter()
                            .find(|(v, _)| *v == value)
                            .map_or(table.default, |(_, to)| *to) as usize;
                        continue;
                    }
                    Op::Return { src, len } => {
                        copy_bytes(src.at(base), ret_dest.ptr, len as usize);
                        return Ok(());
                    }
                    Op::ReturnUnit => return Ok(()),
                    Op::ReturnIr { block } => {
                        if self.bc_stats.counting {
                            self.bc_stats.count(|| "(return)".into());
                        }
                        let Terminator::Return { value: Some(op) } =
                            &func.blocks[block as usize].terminator else {
                            unreachable!("ReturnIr on a block that does not return a value")
                        };
                        let val = self.read_operand(op, frame, frames);
                        self.move_value(&val, ret_dest);
                        Self::mark_source_dropped_all(op, frame, frames);
                        return Ok(());
                    }
                }
                pc += 1;
            }
        }
    }
}

impl IrInterpreter {
    /// Make a fast call, or say the general path has to.
    ///
    /// Does what `execute_call_site` does for a call whose arguments are SSA
    /// values with no `out` parameter among them, and none of what it would
    /// skip: no dispatcher is installed, so there is nothing to offer the call
    /// to and no optimized body to run instead.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    unsafe fn fast_call(
        &mut self,
        call: &FastCall,
        base: *mut u8,
        frame: &mut Frame,
        ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        frames: &mut FrameStore,
        _caller_ref: Option<&CodeRef>,
        caller: &IrCodeUnit,
    ) -> Result<bool, InterpError> {
        if self.call_dispatcher.borrow().is_some() {
            return Ok(false);
        }
        let Instruction::Call { func: code_ref, .. } =
            &caller.blocks[call.block as usize].instructions[call.index as usize] else {
            unreachable!("a fast call is a call")
        };
        let callee = match code_ref {
            CodeRef::Module { .. } => {
                let current = registry.module_registry_arc();
                let mut found = call.module_callee.borrow_mut();
                match &*found {
                    // SAFETY: the registry the body is in is held, and is the
                    // one this call is made against.
                    Some((held, body)) if std::sync::Arc::ptr_eq(held, current) => unsafe { &**body },
                    _ => {
                        let body = ctx.get_unit(code_ref, registry);
                        *found = Some((std::sync::Arc::clone(current), body as *const IrCodeUnit));
                        body
                    }
                }
            }
            _ => ctx.get_unit(code_ref, registry),
        };
        let address = callee as *const IrCodeUnit as usize;
        let (layout, suits) = {
            let mut cache = call.cache.borrow_mut();
            match &*cache {
                Some((at, layout, suits)) if *at == address => (layout.clone(), *suits),
                _ => {
                    let (layout, suits) = match &callee.context {
                        CodeUnitContext::Native(native) => {
                            let mode = |i: usize| native.param_modes.get(i).copied().unwrap_or(ParamMode::In);
                            let suits = call.args.iter().enumerate().all(|(i, &is_value)| match mode(i) {
                                ParamMode::Out => false,
                                ParamMode::In => is_value || native.param_types[i].is_copy(),
                                ParamMode::Ref | ParamMode::Mut => true,
                            });
                            (None, suits)
                        }
                        _ => {
                            let identity = crate::dispatch::FuncIdentity::of(code_ref, ctx.unit());
                            let layout = self.layout_cache.get_or_compute(identity, callee, &mut self.tydesc_table);
                            let suits = call.args.iter().enumerate().all(|(i, &is_value)| {
                                match layout.param_modes[i] {
                                    ParamMode::Out => false,
                                    ParamMode::In => is_value || !layout.param_moves[i],
                                    ParamMode::Ref | ParamMode::Mut => true,
                                }
                            });
                            (Some(layout), suits)
                        }
                    };
                    *cache = Some((address, layout.clone(), suits));
                    (layout, suits)
                }
            }
        };
        if !suits {
            return Ok(false);
        }
        let dest = Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc };

        // The common case, a function called with statically typed arguments
        // and no shapes, reads nothing but the resolved places.
        if let (Some(resolved), Some(layout)) = (&call.resolved, &layout) {
            let mut callee_frame = self.frame_pool.take(std::rc::Rc::clone(layout));
            for &(loc, tydesc) in resolved {
                // SAFETY: lowered against this frame's layout.
                callee_frame.push_param(crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc });
            }
            callee_frame.enter();
            let callee_ctx = ctx.for_callee(code_ref, registry);
            let bc = self.bytecode_for(layout, callee);
            callee_frame.stop_keeping_liveness();
            let result = self.run_bytecode(
                &bc, callee, &mut callee_frame, dest, &callee_ctx, registry, frames, Some(code_ref));
            self.frame_pool.give_back(callee_frame);
            return result.map(|()| true);
        }

        let Instruction::Call { args: ir_args, shape_descriptors, .. } =
            &caller.blocks[call.block as usize].instructions[call.index as usize] else { unreachable!() };
        let shapes: Vec<*const rtdt::TyDesc> = if shape_descriptors.is_empty() {
            Vec::new()
        } else {
            shape_descriptors.iter().map(|r| self.resolve_shape_ref(r, frame)).collect()
        };
        // A borrowed argument that arrives wrapped is read through its
        // wrapper, into here if it has no address of its own; held until the
        // call returns.
        let mut scratch: crate::BorrowScratch = Vec::new();
        let mut read = |this: &Self, i: usize, mode: ParamMode, frame: &Frame| {
            if let Some(resolved) = &call.resolved {
                let (loc, tydesc) = resolved[i];
                // SAFETY: lowered against this frame's layout.
                return crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc };
            }
            let val = this.read_operand(&ir_args[i], frame, frames);
            if matches!(mode, ParamMode::Ref | ParamMode::Mut) {
                IrInterpreter::borrow_through_wrapper(val, &mut scratch)
            } else {
                val
            }
        };

        let Some(layout) = layout else {
            // A native: no frame, just the arguments as a list.
            let CodeUnitContext::Native(native) = &callee.context else { unreachable!() };
            let mode = |i: usize| native.param_modes.get(i).copied().unwrap_or(ParamMode::In);
            let args: Vec<crate::value::Value> = (0..call.args.len())
                .map(|i| read(self, i, mode(i), frame))
                .collect();
            self.native_table.call(&native.symbol, self.runtime.handle(), &args, dest, &shapes)?;
            return Ok(true);
        };

        let mut callee_frame = self.frame_pool.take(std::rc::Rc::clone(&layout));
        for i in 0..call.args.len() {
            callee_frame.push_param(read(self, i, layout.param_modes[i], frame));
        }
        for tydesc in shapes {
            callee_frame.push_shape_descriptor(tydesc);
        }
        callee_frame.enter();
        let callee_ctx = ctx.for_callee(code_ref, registry);
        let bc = self.bytecode_for(&layout, callee);
        callee_frame.stop_keeping_liveness();
        let result = self.run_bytecode(
            &bc, callee, &mut callee_frame, dest, &callee_ctx, registry, frames, Some(code_ref));
        self.frame_pool.give_back(callee_frame);
        drop(scratch);
        // The caller's frame keeps no flags for values, the only arguments
        // that can be moved into a fast call.
        result.map(|()| true)
    }
}

impl BcFunction {
    /// The ops, one per line, for reading.
    pub(crate) fn dump(&self) -> String {
        self.ops.iter().enumerate().map(|(i, op)| format!("  {i:3}: {op:?}\n")).collect()
    }
}

impl BcStats {
    pub(crate) fn record(&mut self, bc: &BcFunction, escapes: u32) {
        self.bodies += 1;
        self.ops += bc.ops.len() as u32;
        self.escapes += escapes;
    }
}

impl Drop for IrInterpreter {
    fn drop(&mut self) {
        if self.use_bytecode && std::env::var_os("DATALOVE_BC_STATS").is_some() {
            let s = &self.bc_stats;
            if s.bodies == 0 {
                return;
            }
            eprintln!("bytecode: {} bodies, {} ops, {} on the IR walker", s.bodies, s.ops, s.escapes);
            let mut executed: Vec<_> = s.executed.iter().collect();
            executed.sort_by(|a, b| b.1.cmp(a.1));
            for (what, n) in executed.iter().take(15) {
                eprintln!("  {n:>12}  {what}");
            }
        }
    }
}
