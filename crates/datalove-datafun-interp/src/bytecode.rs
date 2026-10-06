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
    BinOp, BlockId, UnaryOp, CodeRef, CodeUnitContext, ConstValue, Instruction, IrCodeUnit, IrType, Operand,
    ParamMode, SlotDest, Terminator, ValueId,
};
use datalove_datafun_intrinsics::IntrinsicId;
use datalove_rtdt as rtdt;

use crate::env::{ExecutionContext, FunctionRegistry};
use crate::error::InterpError;
use crate::frame::{Frame, FrameStore};
use crate::layout::IrLayout;
use crate::native::{call_c, NativeTarget, MAX_C_WORDS};
use crate::ops::CheckedIntOps;
use crate::value::Destination;
use datalove_datafun_ir::frame_layout::tracking;
use crate::{copy_bytes, IrInterpreter, UnitTypes};

/// Where an operand is: in the frame at an offset, or, with the top bit set,
/// behind a pointer the frame holds at that offset.
///
/// Values and slots are direct. Parameters and references are indirect: a
/// parameter's pointer is in the frame's parameter region, where the caller
/// sets it, and a reference is a value holding a pointer.
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

/// Which checked operation a fused op does.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Ck {
    Add,
    Sub,
    Mul,
}

impl Ck {
    #[inline(always)]
    fn apply(self, a: u32, b: u32) -> (u32, bool) {
        match self {
            Ck::Add => a.overflowing_add(b),
            Ck::Sub => a.overflowing_sub(b),
            Ck::Mul => a.overflowing_mul(b),
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
    /// Make the call described by `calls[site]`, whose arguments are all in
    /// this frame, without the general call path. Takes the general one whenever
    /// a dispatcher is installed, so the JIT and the inliner see every call.
    CallFast { site: u32 },

    Const1 { dst: Loc, imm: u8 },
    Const4 { dst: Loc, imm: u32 },
    /// Copy `len` bytes of the constant pool from `at`.
    ConstPool { dst: Loc, at: u32, len: u32 },
    /// A new string of `len` bytes of the constant pool from `at`.
    ConstString { dst: Loc, at: u32, len: u32, desc: u32 },

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

    /// Ops with a `u32` constant for an operand, which a value nothing else
    /// writes held: the constant's own op goes, if every read of it went into
    /// one of these.
    CmpU32I { cmp: Cmp, dst: Loc, a: Loc, imm: u32 },
    AddCkU32I { dst: Loc, ovf: Loc, a: Loc, imm: u32 },
    SubCkU32I { dst: Loc, ovf: Loc, a: Loc, imm: u32 },
    MulCkU32I { dst: Loc, ovf: Loc, a: Loc, imm: u32 },

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
    AddWrapIndex { dst: Loc, a: Loc, b: Loc },
    SubWrapIndex { dst: Loc, a: Loc, b: Loc },
    U64ToIndex { dst: Loc, src: Loc },
    ZextU8U32 { dst: Loc, src: Loc },
    ZextU8U64 { dst: Loc, src: Loc },
    ZextU32U64 { dst: Loc, src: Loc },
    NotBool { dst: Loc, src: Loc },

    /// Whether `index` is inside the list at `list`.
    ListBoundsCheck { dst: Loc, list: Loc, index: Loc },
    /// A reference to element `index` of the list at `list`, whose elements
    /// are `size` bytes.
    ListElementRef { dst: Loc, list: Loc, index: Loc, size: u32 },
    /// The same, for a list whose descriptor is known only at run time,
    /// which value `dest` records what it points at by: `rt[at]` holds the
    /// destination, the list and the index.
    ListElementRefRt { dest: u32, at: u32 },
    /// IR walker routines on resolved operands: `rt[at]` holds the
    /// destination and the source.
    EraseRt { at: u32 },
    ReifyRt { at: u32 },
    CloneRt { at: u32 },
    WidenFixedRt { at: u32 },

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
    BrCmpU32I { cmp: Cmp, a: Loc, imm: u32, then: u32, els: u32 },
    /// Checked `u32` arithmetic whose overflow flag only a branch reads:
    /// on to `ovf` if it overflowed, `ok` if not.
    CkU32Br { kind: Ck, dst: Loc, a: Loc, b: Loc, ovf: u32, ok: u32 },
    CkU32IBr { kind: Ck, dst: Loc, a: Loc, imm: u32, ovf: u32, ok: u32 },
    /// Unwrap a result whose flag only a branch reads: on `Ok`, its payload
    /// to `ok` and on to `then`; otherwise its error to `err` and on to the
    /// next op, the start of the block the branch takes then.
    UnwrapOkBr { ok: Loc, err: Loc, src: Loc, at: u16, ok_len: u16, then: u32 },
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
    /// Return `Ok` of the `len` bytes at `src`, the payload at `at`.
    ReturnOk { src: Loc, at: u32, len: u32 },
    ReturnUnit,
    /// Run block `block`'s `Return` on the IR walker.
    ReturnIr { block: u32 },
}

/// Where an operand's descriptor is.
#[derive(Clone, Copy, Debug)]
enum Desc {
    /// In the layout, which the lowering read.
    Static(*const rtdt::TyDesc),
    /// With parameter `n`, which the caller supplied.
    Param(u32),
    /// With what value `n`, a reference, points at.
    Ref(u32),
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
    /// Whether each argument can be moved into the call without the frame
    /// recording it: whether it has no tracking byte. The arguments
    /// themselves are read as the IR walker reads them, which knows where a
    /// parameter's or a reference's descriptor comes from at run time.
    args: Vec<bool>,
    /// Each argument's place and descriptor, where every one is statically
    /// typed and there are no shapes to hand over: then reading them is a
    /// pointer each, which is most calls outside generic code.
    resolved: Option<Vec<(Loc, *const rtdt::TyDesc)>>,
    dest: Loc,
    dest_tydesc: *const rtdt::TyDesc,
    /// What the call found out about its callee the last time, so that a
    /// call need not find it out again unless the body has changed.
    cache: std::cell::RefCell<Option<CallCache>>,
    /// A module callee, found once: the registry it was found in, held so that
    /// it cannot be freed and its address reused, and the body. Valid while
    /// the call's registry is that one -- module bodies do not change within
    /// a registry -- which saves two ordered-map lookups a call.
    /// A native callee's target, with the body and the native table's
    /// generation it was looked up for, which saves hashing its symbol at
    /// every call.
    native: std::cell::RefCell<Option<(usize, u64, NativeTarget)>>,
    module_callee: std::cell::RefCell<Option<(
        std::sync::Arc<datalove_datafun_ir::registry::ModuleFunctionRegistry>,
        *const IrCodeUnit,
    )>>,
}

/// What a fast call found out about its callee, kept until the callee's body
/// changes.
pub(crate) struct CallCache {
    /// The body's address, and the interpreter's code epoch when it was found:
    /// an address only names a body within an epoch.
    address: usize,
    epoch: u64,
    /// Its layout; none for a native.
    layout: Option<std::rc::Rc<IrLayout>>,
    /// Whether the call's arguments suit the fast path for it.
    suits: bool,
    /// The body's bytecode, if it only forwards its parameters to a native;
    /// see `forwarder`.
    forward: Option<std::rc::Rc<BcFunction>>,
    /// The call worked out, if it is one the loop can make without asking
    /// anything again; see `Plan`.
    plan: Option<Plan>,
}

/// A call to a bytecode body in the same unit or a module, with statically
/// typed arguments, worked out once.
///
/// What is left for each call is to check that the callee is still the body
/// this was made for, push its frame, write the arguments and switch to it.
struct Plan {
    layout: std::rc::Rc<IrLayout>,
    bc: std::rc::Rc<BcFunction>,
    func: *const IrCodeUnit,
    /// How the call names the callee, in the caller's body.
    code_ref: *const CodeRef,
    /// Each argument: where it is in the caller's frame, where its `Value`
    /// goes in the callee's, and its descriptor as the callee's frame holds
    /// it once entered -- the callee's own for an owned parameter, the
    /// argument's for a borrowed one.
    params: Box<[PlannedParam]>,
}

/// One argument of a planned call.
struct PlannedParam {
    /// Where the argument is in the caller's frame.
    src: Loc,
    /// Where its `Value` goes in the callee's.
    value: u32,
    /// Its descriptor as the callee's frame holds it once entered.
    tydesc: *const rtdt::TyDesc,
    /// Where the callee's frame keeps a copy of it, and its size, if it does;
    /// see `IrLayout::param_copies`.
    copy: Option<(u32, u32)>,
}

/// What a fast call did.
enum Called<'r> {
    /// It made the call: a native, or one through a forwarder to one.
    Done,
    /// It did not suit the fast path, and the general one has to make it.
    General,
    /// It pushed and entered a bytecode body's frame, for the loop to run.
    Enter(Entered<'r>),
}

/// A bytecode body's frame, entered, and what running it needs.
struct Entered<'r> {
    frame: Frame,
    bc: std::rc::Rc<BcFunction>,
    func: &'r IrCodeUnit,
    code_ref: &'r CodeRef,
    ctx: ExecutionContext<'r>,
    /// Where the result goes, in the caller's frame.
    dest: Destination,
    /// What borrowed arguments were read into, held until the call returns.
    scratch: crate::BorrowScratch,
}

/// What a call made from the loop saves of its caller, to go back to.
///
/// As little as a call changes, since saving it and restoring it is much of
/// what a call costs: the body is read off the bytecode, the frame is rebuilt
/// from its base and layout, and the context is saved only by a call that
/// changes it.
struct Activation<'r> {
    bc: *const BcFunction,
    /// What keeps `bc` alive, if the caller's activation held it.
    bc_keep: Option<std::rc::Rc<BcFunction>>,
    code_ref: Option<&'r CodeRef>,
    /// The caller's context, if the call changed it: only a call into another
    /// unit's function does.
    ctx: Option<ExecutionContext<'r>>,
    base: *mut u8,
    layout: *const IrLayout,
    ret_dest: Destination,
    /// The op after the call.
    pc: usize,
    /// The callee's borrowed arguments' scratch, if it has any, held, not
    /// read, until it returns.
    _scratch: Option<Box<crate::BorrowScratch>>,
}

/// Push onto a vector, writing the value where it goes rather than building it
/// on the stack first and copying it, which a plain `push` of a struct this size
/// did.
#[inline(always)]
fn push_in_place<T>(v: &mut Vec<T>, value: T) {
    if v.len() == v.capacity() {
        v.reserve(1);
    }
    // SAFETY: there is room for one more, just made if there was not.
    unsafe {
        v.as_mut_ptr().add(v.len()).write(value);
        v.set_len(v.len() + 1);
    }
}

/// What the loop knows about the body it is running, and the callers it will
/// go back to.
struct Regs<'r> {
    bc: *const BcFunction,
    /// What keeps `bc` alive, unless the loop was entered with it.
    bc_keep: Option<std::rc::Rc<BcFunction>>,
    func: &'r IrCodeUnit,
    code_ref: Option<&'r CodeRef>,
    ctx: ExecutionContext<'r>,
    frame: Frame,
    ret_dest: Destination,
    stack: Vec<Activation<'r>>,
    registry: &'r FunctionRegistry,
    /// The frame store, which the loop was lent for as long as it runs.
    frames: *mut FrameStore,
}

/// A function body, lowered.
pub(crate) struct BcFunction {
    /// The body this was lowered from.
    func: *const IrCodeUnit,
    calls: Vec<FastCall>,
    /// Descriptors the ops name by index.
    descs: Vec<*const rtdt::TyDesc>,
    /// Operand triples for the ops that hand theirs to an IR walker routine.
    rt: Vec<[(Loc, Desc); 3]>,
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
    rt: Vec<[(Loc, Desc); 3]>,
    /// Op indices still naming a block, to be patched to its first op.
    fixups: Vec<(usize, Fixup)>,
    block_start: Vec<u32>,
    escapes: u32,
    /// How many times each value is read, for fusing an op into its only use.
    uses: Vec<u32>,
    /// The `u32` each value nothing but its constant writes holds, how many of
    /// its reads an op took as an immediate, and where its constant's op is:
    /// in the prologue or not, and at what index.
    const_u32: Vec<Option<u32>>,
    absorbed: Vec<u32>,
    const_op: Vec<Option<(bool, usize)>>,
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
    OkThen,
    EdgeTo,
    SwitchCase(usize, usize),
    SwitchDefault(usize),
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
                Fixup::OkThen => if let Op::UnwrapOkBr { then, .. } = &mut self.ops[at] { *then = start(*then) },
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
        // Constants every read of which an op took as an immediate go.
        let mut removed = vec![false; self.ops.len()];
        for (v, at) in self.const_op.iter().enumerate() {
            if let Some((in_prologue, i)) = *at && self.absorbed[v] == self.uses[v] {
                removed[if in_prologue { entry as usize + i } else { i }] = true;
            }
        }
        let entry = compact(&mut self.ops, &removed, &mut self.switches, entry);
        let escapes = self.escapes;
        (BcFunction {
            func: self.func, ops: self.ops, entry, pool: self.pool, switches: self.switches, calls: self.calls,
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
                // A `mut` parameter always holds something, which goes first.
                let p = Operand::Param(*param);
                let (Some(ty), Some(dst), Some(desc), Some(src)) =
                    (self.typed(&p), self.loc(&p), self.desc_of(&p), self.consumed(value)) else { return false };
                if self.typed(value).is_none() {
                    return false;
                }
                if !ty.is_copy() {
                    let desc = self.desc_index(desc);
                    self.emit(Op::Drop { src: dst, desc });
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
                let src = self.fold_copy(inner, src);
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
                            cache: std::cell::RefCell::new(None),
                            native: std::cell::RefCell::new(None),
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
        let at = if self.pinned[dest.0 as usize] || !self.in_loop[self.block] {
            self.emit(op);
            (false, self.ops.len() - 1)
        } else {
            self.prologue.push(op);
            (true, self.prologue.len() - 1)
        };
        self.const_op[dest.0 as usize] = Some(at);
    }

    /// The constant a `u32` operand always holds, if it does, counted as read
    /// into an immediate: only for an op that is going to take it.
    fn imm_u32(&mut self, op: &Operand) -> Option<u32> {
        let Operand::Value(v) = op else { return None };
        let imm = self.const_u32[v.0 as usize]?;
        self.absorbed[v.0 as usize] += 1;
        Some(imm)
    }

    /// Fold a copy just made into `loc`, the only read of `op`, into the op
    /// about to read it: the copy's source is still what it copied.
    fn fold_copy(&mut self, op: &Operand, loc: Loc) -> Loc {
        let Operand::Value(v) = op else { return loc };
        if self.uses[v.0 as usize] != 1 || self.ops.len() <= self.block_start[self.block] as usize {
            return loc;
        }
        let src = match *self.ops.last().expect("an op in this block") {
            Op::Copy1 { dst, src } | Op::Copy4 { dst, src } | Op::Copy8 { dst, src }
            | Op::CopyN { dst, src, .. } if dst.0 == loc.0 => src,
            _ => return loc,
        };
        self.ops.pop();
        src
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
        if matches!(lt, IrType::U32) && let Some(imm) = self.imm_u32(rhs) {
            self.emit(Op::CmpU32I { cmp, dst, a, imm });
            return true;
        }
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
        if matches!(ty, IrType::U32) && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
            if let Some(imm) = self.imm_u32(rhs) {
                self.emit(match op {
                    BinOp::Add => Op::AddCkU32I { dst, ovf, a, imm },
                    BinOp::Sub => Op::SubCkU32I { dst, ovf, a, imm },
                    _ => Op::MulCkU32I { dst, ovf, a, imm },
                });
                return true;
            }
        }
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
        if let ([x], IntrinsicId::U64ToIndex) = (args, intrinsic) {
            let Some(src) = self.loc(x) else { return false };
            self.emit(Op::U64ToIndex { dst: self.value_loc(dest), src });
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
            Terminator::Branch { cond, then_block, then_args, else_block, else_args }
                if then_args.is_empty() && else_args.is_empty()
                    && else_block.0 as usize == block + 1
                    && let Some(op) = self.fuse_unwrap(cond, then_block.0) =>
            {
                self.fixups.push((self.ops.len(), Fixup::OkThen));
                self.emit(op);
            }
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
                        // A result wrapped only to be returned is written
                        // straight into the caller's slot: copying it there
                        // afterwards read the narrow stores that had just
                        // written its tag and payload back with wide loads,
                        // which the processor cannot forward from.
                        let in_block = self.ops.len() > self.block_start[block] as usize;
                        let wrapped_here = match (op, self.ops.last().filter(|_| in_block)) {
                            (Operand::Value(v), Some(&Op::WrapOk { dst, src, at, len }))
                                if self.uses[v.0 as usize] == 1 && dst.0 == self.value_loc(*v).0 =>
                                Some((src, at, len)),
                            _ => None,
                        };
                        match wrapped_here {
                            Some((src, at, len)) => {
                                self.ops.pop();
                                self.emit(Op::ReturnOk { src, at, len });
                            }
                            None => {
                                let len = layout_of(ty).size;
                                self.emit(Op::Return { src, len });
                            }
                        }
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
        self.absorbed = vec![0; n];
        self.const_op = vec![None; n];
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

    /// A branch on the flag of a result just unwrapped in this block, fused
    /// with the unwrap, if the branch is the flag's only use.
    fn fuse_unwrap(&mut self, cond: &Operand, then: u32) -> Option<Op> {
        let Operand::Value(id) = cond else { return None };
        if self.uses[id.0 as usize] != 1 || self.ops.len() <= self.block_start[self.block] as usize {
            return None;
        }
        let want = self.value_loc(*id).0;
        let fused = match *self.ops.last()? {
            Op::UnwrapResult { ok, err, flag, src, at, ok_len, err_len }
                if flag.0 == want && err_len as u32 == layout_of(&IrType::Error).size =>
                Op::UnwrapOkBr { ok, err, src, at, ok_len, then },
            _ => return None,
        };
        self.ops.pop();
        Some(fused)
    }

    /// A branch on a comparison just emitted, fused with it, if the branch is
    /// the comparison's only use.
    fn fuse_compare(&mut self, cond: &Operand, then: u32, els: u32) -> Option<Op> {
        let Operand::Value(id) = cond else { return None };
        // Only an op of this block: the last op before it may be the end of
        // the previous one, which falls through into this one, and moving it
        // here would leave every other way into this block skipping it.
        if self.uses[id.0 as usize] != 1 || self.ops.len() <= self.block_start[self.block] as usize {
            return None;
        }
        let want = self.value_loc(*id).0;
        let fused = match *self.ops.last()? {
            Op::CmpU8 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU8 { cmp, a, b, then, els },
            Op::CmpU32 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU32 { cmp, a, b, then, els },
            Op::CmpU32I { cmp, dst, a, imm } if dst.0 == want => Op::BrCmpU32I { cmp, a, imm, then, els },
            Op::AddCkU32 { dst, ovf, a, b } if ovf.0 == want => Op::CkU32Br { kind: Ck::Add, dst, a, b, ovf: then, ok: els },
            Op::SubCkU32 { dst, ovf, a, b } if ovf.0 == want => Op::CkU32Br { kind: Ck::Sub, dst, a, b, ovf: then, ok: els },
            Op::MulCkU32 { dst, ovf, a, b } if ovf.0 == want => Op::CkU32Br { kind: Ck::Mul, dst, a, b, ovf: then, ok: els },
            Op::AddCkU32I { dst, ovf, a, imm } if ovf.0 == want => Op::CkU32IBr { kind: Ck::Add, dst, a, imm, ovf: then, ok: els },
            Op::SubCkU32I { dst, ovf, a, imm } if ovf.0 == want => Op::CkU32IBr { kind: Ck::Sub, dst, a, imm, ovf: then, ok: els },
            Op::MulCkU32I { dst, ovf, a, imm } if ovf.0 == want => Op::CkU32IBr { kind: Ck::Mul, dst, a, imm, ovf: then, ok: els },
            Op::CmpI32 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpI32 { cmp, a, b, then, els },
            Op::CmpU64 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU64 { cmp, a, b, then, els },
            Op::CmpI64 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpI64 { cmp, a, b, then, els },
            _ => return None,
        };
        self.ops.pop();
        Some(fused)
    }
}

impl Op {
    /// Every place in the frame the op reads or writes, with how many bytes it
    /// reads or writes there.
    ///
    /// Exhaustive, so that a new op cannot be added without saying what it
    /// touches; the verifier checks these against the frame.
    fn places(&self, f: &mut impl FnMut(Loc, u32)) {
        const I: u32 = rtdt::INDEX_SIZE;
        match *self {
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. } | Op::Jump { .. }
            | Op::EdgeIr { .. } | Op::ReturnUnit | Op::ReturnIr { .. }
            | Op::ListElementRefRt { .. } | Op::EraseRt { .. } | Op::ReifyRt { .. }
            | Op::CloneRt { .. } | Op::WidenFixedRt { .. } | Op::BinOpRt { .. } => {}
            Op::Const1 { dst, .. } => f(dst, 1),
            Op::Const4 { dst, .. } => f(dst, 4),
            Op::ConstPool { dst, len, .. } => f(dst, len),
            Op::ConstString { dst, .. } => f(dst, 1),
            Op::Copy1 { dst, src } => { f(dst, 1); f(src, 1) }
            Op::Copy4 { dst, src } => { f(dst, 4); f(src, 4) }
            Op::Copy8 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::CopyN { dst, src, len } => { f(dst, len); f(src, len) }
            Op::AddCkU32 { dst, ovf, a, b } | Op::SubCkU32 { dst, ovf, a, b } | Op::MulCkU32 { dst, ovf, a, b }
            | Op::AddCkI32 { dst, ovf, a, b } | Op::SubCkI32 { dst, ovf, a, b } | Op::MulCkI32 { dst, ovf, a, b } => {
                f(dst, 4); f(ovf, 1); f(a, 4); f(b, 4)
            }
            Op::AddCkU64 { dst, ovf, a, b } | Op::SubCkU64 { dst, ovf, a, b } | Op::MulCkU64 { dst, ovf, a, b }
            | Op::AddCkI64 { dst, ovf, a, b } | Op::SubCkI64 { dst, ovf, a, b } | Op::MulCkI64 { dst, ovf, a, b } => {
                f(dst, 8); f(ovf, 1); f(a, 8); f(b, 8)
            }
            Op::CmpU32I { dst, a, .. } => { f(dst, 1); f(a, 4) }
            Op::AddCkU32I { dst, ovf, a, .. } | Op::SubCkU32I { dst, ovf, a, .. }
            | Op::MulCkU32I { dst, ovf, a, .. } => { f(dst, 4); f(ovf, 1); f(a, 4) }
            Op::CmpU8 { dst, a, b, .. } => { f(dst, 1); f(a, 1); f(b, 1) }
            Op::CmpU32 { dst, a, b, .. } | Op::CmpI32 { dst, a, b, .. } => { f(dst, 1); f(a, 4); f(b, 4) }
            Op::CmpU64 { dst, a, b, .. } | Op::CmpI64 { dst, a, b, .. } => { f(dst, 1); f(a, 8); f(b, 8) }
            Op::AddWrapU32 { dst, a, b } | Op::SubWrapU32 { dst, a, b } | Op::MulWrapU32 { dst, a, b }
            | Op::RemU32 { dst, a, b } | Op::ShrU32 { dst, a, b } | Op::ShlU32 { dst, a, b }
            | Op::AndU32 { dst, a, b } => { f(dst, 4); f(a, 4); f(b, 4) }
            Op::AddWrapIndex { dst, a, b } | Op::SubWrapIndex { dst, a, b } => { f(dst, I); f(a, I); f(b, I) }
            Op::U64ToIndex { dst, src } => { f(dst, I); f(src, 8) }
            Op::ZextU8U32 { dst, src } => { f(dst, 4); f(src, 1) }
            Op::ZextU8U64 { dst, src } => { f(dst, 8); f(src, 1) }
            Op::ZextU32U64 { dst, src } => { f(dst, 8); f(src, 4) }
            Op::NotBool { dst, src } => { f(dst, 1); f(src, 1) }
            Op::ListBoundsCheck { dst, list, index } => {
                f(dst, 1); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I)
            }
            Op::ListElementRef { dst, list, index, .. } => {
                f(dst, 8); f(list, std::mem::size_of::<rtdt::List>() as u32); f(index, I)
            }
            Op::WrapSome { dst, src, at, len } | Op::WrapOk { dst, src, at, len } => { f(dst, at + len); f(src, len) }
            Op::WrapNone { dst } => f(dst, 1),
            Op::UnwrapOption { dst, flag, src, at, len } => { f(dst, len); f(flag, 1); f(src, at + len) }
            Op::UnwrapResult { ok, err, flag, src, at, ok_len, err_len } => {
                f(ok, ok_len as u32); f(err, err_len as u32); f(flag, 1);
                f(src, at as u32 + (ok_len.max(err_len)) as u32)
            }
            Op::UnwrapOkBr { ok, err, src, at, ok_len, .. } => {
                let err_len = std::mem::size_of::<rtdt::Error>() as u32;
                f(ok, ok_len as u32); f(err, err_len); f(src, at as u32 + (ok_len as u32).max(err_len))
            }
            Op::Drop { src, .. } | Op::DropTracked { src, .. } => f(src, 1),
            Op::StoreTracked { dst, src, len, .. } | Op::LoadMoveTracked { dst, src, len, .. } => {
                f(dst, len); f(src, len)
            }
            Op::Widen { dst, src, .. } => { f(dst, std::mem::size_of::<rtdt::Int>() as u32); f(src, 1) }
            Op::BrIf { cond, .. } => f(cond, 1),
            Op::BrCmpU8 { a, b, .. } => { f(a, 1); f(b, 1) }
            Op::BrCmpU32I { a, .. } => f(a, 4),
            Op::CkU32Br { dst, a, b, .. } => { f(dst, 4); f(a, 4); f(b, 4) }
            Op::CkU32IBr { dst, a, .. } => { f(dst, 4); f(a, 4) }
            Op::BrCmpU32 { a, b, .. } | Op::BrCmpI32 { a, b, .. } => { f(a, 4); f(b, 4) }
            Op::BrCmpU64 { a, b, .. } | Op::BrCmpI64 { a, b, .. } => { f(a, 8); f(b, 8) }
            Op::Switch { disc, .. } => f(disc, 4),
            Op::Return { src, len } => f(src, len),
            Op::ReturnOk { src, len, .. } => f(src, len),
        }
    }

    /// Every op the op may go to next, other than the one after it.
    ///
    /// Exhaustive, so that a new op that jumps cannot be left out of the
    /// fixups, the compaction or the verifier, which all go through this.
    fn targets_mut(&mut self, f: &mut impl FnMut(&mut u32)) {
        match self {
            Op::Jump { to } | Op::EdgeIr { to, .. } => f(to),
            Op::UnwrapOkBr { then, .. } => f(then),
            Op::BrIf { then, els, .. } | Op::BrCmpU8 { then, els, .. } | Op::BrCmpU32I { then, els, .. }
            | Op::BrCmpU32 { then, els, .. } | Op::BrCmpI32 { then, els, .. }
            | Op::BrCmpU64 { then, els, .. } | Op::BrCmpI64 { then, els, .. } => { f(then); f(els) }
            Op::CkU32Br { ovf, ok, .. } | Op::CkU32IBr { ovf, ok, .. } => { f(ovf); f(ok) }
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. }
            | Op::Const1 { .. } | Op::Const4 { .. } | Op::ConstPool { .. } | Op::ConstString { .. }
            | Op::Copy1 { .. } | Op::Copy4 { .. } | Op::Copy8 { .. } | Op::CopyN { .. }
            | Op::AddCkU32 { .. } | Op::SubCkU32 { .. } | Op::MulCkU32 { .. }
            | Op::AddCkI32 { .. } | Op::SubCkI32 { .. } | Op::MulCkI32 { .. }
            | Op::AddCkU64 { .. } | Op::SubCkU64 { .. } | Op::MulCkU64 { .. }
            | Op::AddCkI64 { .. } | Op::SubCkI64 { .. } | Op::MulCkI64 { .. }
            | Op::CmpU32I { .. } | Op::AddCkU32I { .. } | Op::SubCkU32I { .. } | Op::MulCkU32I { .. }
            | Op::CmpU8 { .. } | Op::CmpU32 { .. } | Op::CmpI32 { .. } | Op::CmpU64 { .. } | Op::CmpI64 { .. }
            | Op::AddWrapU32 { .. } | Op::SubWrapU32 { .. } | Op::MulWrapU32 { .. } | Op::RemU32 { .. }
            | Op::ShrU32 { .. } | Op::ShlU32 { .. } | Op::AndU32 { .. }
            | Op::AddWrapIndex { .. } | Op::SubWrapIndex { .. } | Op::U64ToIndex { .. }
            | Op::ZextU8U32 { .. } | Op::ZextU8U64 { .. } | Op::ZextU32U64 { .. } | Op::NotBool { .. }
            | Op::ListBoundsCheck { .. } | Op::ListElementRef { .. } | Op::ListElementRefRt { .. }
            | Op::EraseRt { .. } | Op::ReifyRt { .. } | Op::CloneRt { .. } | Op::WidenFixedRt { .. }
            | Op::WrapSome { .. } | Op::WrapNone { .. } | Op::WrapOk { .. }
            | Op::UnwrapOption { .. } | Op::UnwrapResult { .. }
            | Op::Drop { .. } | Op::DropTracked { .. } | Op::StoreTracked { .. } | Op::LoadMoveTracked { .. }
            | Op::Widen { .. } | Op::BinOpRt { .. } | Op::Switch { .. }
            | Op::Return { .. } | Op::ReturnOk { .. } | Op::ReturnUnit | Op::ReturnIr { .. } => {}
        }
    }
}

impl BcFunction {
    /// Check, in a debug build, everything the loop takes on trust: that every
    /// place an op names is inside the frame, every jump lands on an op, every
    /// index names an entry of its table, and every fused unwrap has a block
    /// after it to fall through into.
    ///
    /// The loop reads and writes the frame through raw offsets, and this is
    /// what those offsets rest on.
    fn verify(&self, func: &IrCodeUnit, layout: &IrLayout) {
        let size = layout.frame_size;
        let n = self.ops.len() as u32;
        let place = |at: usize, loc: Loc, len: u32| {
            let offset = loc.0 & !INDIRECT;
            // An indirect place is read through the pointer the frame holds;
            // what it points at is the caller's or a reference's.
            let len = if loc.0 & INDIRECT != 0 { 8 } else { len };
            assert!(offset + len <= size,
                "{}: op {} ({:?}) names {} bytes at {} in a frame of {}",
                func.name, at, self.ops[at], len, offset, size);
        };
        let target = |at: usize, to: u32| {
            assert!(to < n, "{}: op {} ({:?}) goes to {}, past the last op", func.name, at, self.ops[at], to);
        };
        assert!(self.entry < n, "{}: the entry {} is past the last op", func.name, self.entry);
        for (at, op) in self.ops.iter().enumerate() {
            op.places(&mut |loc, len| place(at, loc, len));
            op.clone().targets_mut(&mut |to| target(at, *to));
            let in_table = |i: u32, len: usize, what: &str| {
                assert!((i as usize) < len, "{}: op {} ({:?}) names {} {} of {}", func.name, at, op, what, i, len);
            };
            match *op {
                Op::Ir { block, index } | Op::Call { block, index } => {
                    in_table(block, func.blocks.len(), "block");
                    in_table(index, func.blocks[block as usize].instructions.len(), "instruction");
                }
                Op::EdgeIr { block, .. } | Op::ReturnIr { block } => in_table(block, func.blocks.len(), "block"),
                Op::CallFast { site } => in_table(site, self.calls.len(), "call site"),
                Op::ConstPool { at: pool_at, len, .. } | Op::ConstString { at: pool_at, len, .. } => {
                    assert!(pool_at + len <= self.pool.len() as u32, "{}: op {} reads past the pool", func.name, at);
                }
                Op::ListElementRefRt { at: i, .. } | Op::EraseRt { at: i } | Op::ReifyRt { at: i }
                | Op::CloneRt { at: i } | Op::WidenFixedRt { at: i } | Op::BinOpRt { at: i, .. } => {
                    in_table(i, self.rt.len(), "routine operands");
                    // Unused entries of a triple are padding, with no descriptor.
                    for (loc, desc) in self.rt[i as usize] {
                        if !matches!(desc, Desc::Static(d) if d.is_null()) {
                            place(at, loc, 1);
                        }
                    }
                }
                Op::Switch { table, .. } => {
                    in_table(table, self.switches.len(), "switch table");
                    let t = &self.switches[table as usize];
                    t.cases.iter().for_each(|(_, to)| target(at, *to));
                    target(at, t.default);
                }
                Op::UnwrapOkBr { .. } => assert!((at as u32) + 1 < n,
                    "{}: op {} unwraps with nothing after it to fall through into", func.name, at),
                _ => {}
            }
            match *op {
                Op::ConstString { desc, .. } | Op::Drop { desc, .. } | Op::DropTracked { desc, .. }
                | Op::StoreTracked { desc, .. } | Op::Widen { src_desc: desc, .. } => {
                    in_table(desc, self.descs.len(), "descriptor");
                }
                _ => {}
            }
            match *op {
                Op::DropTracked { track, .. } | Op::StoreTracked { track, .. } | Op::LoadMoveTracked { track, .. } => {
                    assert!(track < size, "{}: op {} names tracking byte {} in a frame of {}", func.name, at, track, size);
                }
                _ => {}
            }
        }
        for call in &self.calls {
            place(0, call.dest, 1);
            if let Some(resolved) = &call.resolved {
                for &(loc, _) in resolved {
                    place(0, loc, 1);
                }
            }
        }
    }
}

/// Drop the `removed` ops, pointing every jump at where its target op, or the
/// next one kept after it, now is. Returns where `entry` now is.
fn compact(ops: &mut Vec<Op>, removed: &[bool], switches: &mut [SwitchTable], entry: u32) -> u32 {
    if !removed.contains(&true) {
        return entry;
    }
    let mut new_index = Vec::with_capacity(ops.len() + 1);
    let mut kept = 0u32;
    for &r in removed {
        new_index.push(kept);
        if !r {
            kept += 1;
        }
    }
    new_index.push(kept);
    let mut map = |t: &mut u32| *t = new_index[*t as usize];
    for op in ops.iter_mut() {
        op.targets_mut(&mut map);
    }
    for table in switches.iter_mut() {
        for (_, to) in &mut table.cases {
            map(to);
        }
        map(&mut table.default);
    }
    let mut i = 0;
    ops.retain(|_| {
        i += 1;
        !removed[i - 1]
    });
    new_index[entry as usize]
}

/// A conditional branch's then and else targets.
fn branch_targets(op: &mut Op) -> (&mut u32, &mut u32) {
    match op {
        Op::CkU32Br { ovf: then, ok: els, .. } | Op::CkU32IBr { ovf: then, ok: els, .. } => (then, els),
        Op::BrIf { then, els, .. }
        | Op::BrCmpU8 { then, els, .. }
        | Op::BrCmpU32I { then, els, .. }
        | Op::BrCmpU32 { then, els, .. }
        | Op::BrCmpI32 { then, els, .. }
        | Op::BrCmpU64 { then, els, .. }
        | Op::BrCmpI64 { then, els, .. } => (then, els),
        op => unreachable!("{:?} is not a branch", op),
    }
}

/// Lower a function body against its layout.
pub(crate) fn lower(func: &IrCodeUnit, layout: &IrLayout) -> (BcFunction, u32) {
    let lowered = Lowering {
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
        const_u32: Vec::new(),
        absorbed: Vec::new(),
        const_op: Vec::new(),
        pinned: Vec::new(),
        prologue: Vec::new(),
        in_loop: Vec::new(),
        block: 0,
        index: 0,
    }
    .lower();
    if cfg!(debug_assertions) {
        lowered.0.verify(func, layout);
    }
    lowered
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

/// An operand an op hands to an IR walker routine.
#[inline(always)]
unsafe fn rt_value(frame: &Frame, base: *mut u8, (loc, desc): (Loc, Desc)) -> crate::value::Value {
    let tydesc = match desc {
        Desc::Static(tydesc) => tydesc,
        Desc::Param(n) => frame.param(datalove_datafun_ir::ParamId(n)).tydesc,
        Desc::Ref(n) => frame.value_deref(ValueId(n)).tydesc,
    };
    crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc }
}

#[inline(always)]
unsafe fn rt_dest(frame: &Frame, base: *mut u8, operand: (Loc, Desc)) -> Destination {
    let val = unsafe { rt_value(frame, base, operand) };
    Destination { ptr: val.ptr, tydesc: val.tydesc }
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
    ///
    /// Calls the fast path makes to other bytecode bodies run here too, without
    /// recursing: the call pushes the callee's frame, saves the caller's
    /// registers -- the body, `pc`, the frame, where its result goes -- in an
    /// `Activation`, and carries on in the callee; its return pops the frame
    /// and restores them. So a datalove call costs no Rust stack, and
    /// recursion that stays in bytecode is bounded by the frame stack. Calls
    /// the general path makes, and an instruction run on the IR walker that
    /// calls, still nest, starting a loop of their own.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run_bytecode<'r>(
        &mut self,
        bc: &'r BcFunction,
        func: &'r IrCodeUnit,
        frame: &mut Frame,
        ret_dest: Destination,
        ctx: &ExecutionContext<'r>,
        registry: &'r FunctionRegistry,
        frames: &mut FrameStore,
        code_ref: Option<&'r CodeRef>,
    ) -> Result<(), InterpError> {
        let mut regs = Regs {
            bc,
            bc_keep: None,
            func,
            code_ref,
            ctx: *ctx,
            frame: *frame,
            ret_dest,
            stack: Vec::new(),
            registry,
            frames,
        };
        // SAFETY: the entry is an op of the body.
        let result = unsafe { self.run_body(&mut regs, bc.entry as usize) };
        if result.is_err() {
            // Every frame the loop pushed is popped before the error goes.
            self.unwind(&mut regs);
        }
        result
    }

    /// The loop: run from `pc` in the body `regs` says, through every call
    /// it makes to another and back, until the body the loop was entered with
    /// returns or something fails.
    ///
    /// The ops, the frame's base and `pc` are locals, which the compiler keeps
    /// in machine registers, and everything else `regs`, behind a pointer;
    /// a call or return reloads the three from it.
    #[inline(never)]
    unsafe fn run_body<'r>(&mut self, regs: &mut Regs<'r>, mut pc: usize) -> Result<(), InterpError> {
        // SAFETY: kept alive by `regs.bc_keep`, or by whoever entered the loop.
        let mut bc: &BcFunction = unsafe { &*regs.bc };
        let mut ops = bc.ops.as_ptr();
        let mut base = regs.frame.base_ptr();

        macro_rules! tri {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                }
            };
        }
        // A return goes back to the caller this loop saved, or out of the loop
        // if there is none.
        macro_rules! ret {
            () => {
                match self.leave(regs) {
                    Some(resume) => {
                        bc = &*regs.bc;
                        ops = bc.ops.as_ptr();
                        base = regs.frame.base_ptr();
                        pc = resume;
                        continue;
                    }
                    None => return Ok(()),
                }
            };
        }

        // SAFETY, for every frame access below: the ops were lowered against
        // the running frame's layout, so every offset is inside it, and `base`
        // stays the frame's data while it runs -- an instruction run on the IR
        // walker uses the same frame, and frames never move.
        unsafe {
            loop {
                // Every block ends in a jump or a return, and every target is
                // an op of this body, so `pc` never runs off the end.
                debug_assert!(pc < bc.ops.len());
                match *ops.add(pc) {
                    Op::Ir { block, index } => tri!(self.run_ir_op(regs, block, index)),
                    Op::Call { block, index } => tri!(self.run_general_call(regs, block, index)),
                    Op::CallFast { site } => {
                        let call = &bc.calls[site as usize];
                        if let Some(plan) = self.valid_plan(call) {
                            pc = tri!(self.enter_planned(regs, call, plan, base, pc.wrapping_add(1)));
                            bc = &*regs.bc;
                            ops = bc.ops.as_ptr();
                            base = regs.frame.base_ptr();
                            continue;
                        }
                        if let Some(resume) = tri!(self.run_fast_call(regs, call, base, pc)) {
                            pc = resume;
                            bc = &*regs.bc;
                            ops = bc.ops.as_ptr();
                            base = regs.frame.base_ptr();
                            continue;
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
                    Op::CmpU32I { cmp, dst, a, imm } => wr(base, dst, cmp.apply(rd::<u32>(base, a), imm)),
                    Op::AddCkU32I { dst, ovf, a, imm } => {
                        let (r, o) = rd::<u32>(base, a).overflowing_add(imm);
                        wr(base, dst, r);
                        wr(base, ovf, o);
                    }
                    Op::SubCkU32I { dst, ovf, a, imm } => {
                        let (r, o) = rd::<u32>(base, a).overflowing_sub(imm);
                        wr(base, dst, r);
                        wr(base, ovf, o);
                    }
                    Op::MulCkU32I { dst, ovf, a, imm } => {
                        let (r, o) = rd::<u32>(base, a).overflowing_mul(imm);
                        wr(base, dst, r);
                        wr(base, ovf, o);
                    }
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
                    Op::AddWrapIndex { dst, a, b } => {
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, a).wrapping_add(rd(base, b)))
                    }
                    Op::SubWrapIndex { dst, a, b } => {
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, a).wrapping_sub(rd(base, b)))
                    }
                    Op::U64ToIndex { dst, src } => wr(base, dst, rd::<u64>(base, src) as rtdt::IndexRepr),
                    Op::ZextU8U32 { dst, src } => wr(base, dst, rd::<u8>(base, src) as u32),
                    Op::ZextU8U64 { dst, src } => wr(base, dst, rd::<u8>(base, src) as u64),
                    Op::ZextU32U64 { dst, src } => wr(base, dst, rd::<u32>(base, src) as u64),
                    Op::NotBool { dst, src } => wr(base, dst, rd::<u8>(base, src) == 0),

                    Op::ListBoundsCheck { dst, list, index } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        wr(base, dst, rd::<rtdt::IndexRepr>(base, index) < list.size.0);
                    }
                    Op::ListElementRef { dst, list, index, size } => {
                        let list = &*(list.at(base) as *const rtdt::List);
                        let i = rd::<rtdt::IndexRepr>(base, index) as usize;
                        wr(base, dst, (list.data as *mut u8).add(i * size as usize));
                    }

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
                    Op::ConstString { .. } | Op::ListElementRefRt { .. } | Op::EraseRt { .. }
                    | Op::ReifyRt { .. } | Op::CloneRt { .. } | Op::WidenFixedRt { .. }
                    | Op::Widen { .. } | Op::BinOpRt { .. } => {
                        self.run_routine_op(*ops.add(pc), bc, &mut regs.frame, base)
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
                    Op::BrCmpU32I { cmp, a, imm, then, els } => {
                        pc = if cmp.apply(rd::<u32>(base, a), imm) { then } else { els } as usize;
                        continue;
                    }
                    Op::CkU32Br { kind, dst, a, b, ovf, ok } => {
                        let (r, o) = kind.apply(rd::<u32>(base, a), rd::<u32>(base, b));
                        wr(base, dst, r);
                        pc = if o { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::CkU32IBr { kind, dst, a, imm, ovf, ok } => {
                        let (r, o) = kind.apply(rd::<u32>(base, a), imm);
                        wr(base, dst, r);
                        pc = if o { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::UnwrapOkBr { ok, err, src, at, ok_len, then } => {
                        let s = src.at(base).add(at as usize);
                        if *src.at(base) == rtdt::ResultTag::Ok as u8 {
                            copy_bytes(s, ok.at(base), ok_len as usize);
                            pc = then as usize;
                            continue;
                        }
                        copy_bytes(s, err.at(base), std::mem::size_of::<rtdt::Error>());
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
                        tri!(self.run_edge(regs, block, edge));
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
                        copy_bytes(src.at(base), regs.ret_dest.ptr, len as usize);
                        ret!();
                    }
                    Op::ReturnOk { src, at, len } => {
                        let d = regs.ret_dest.ptr;
                        *d = rtdt::ResultTag::Ok as u8;
                        copy_bytes(src.at(base), d.add(at as usize), len as usize);
                        ret!();
                    }
                    Op::ReturnUnit => ret!(),
                    Op::ReturnIr { block } => {
                        self.run_return_ir(regs, block);
                        ret!();
                    }
                }
                // Wrapping, so that it is not checked: release builds check
                // overflow, and `pc` is an op index, which cannot overflow.
                // The check and its panic path cost the loop about an eighth
                // of its instructions.
                pc = pc.wrapping_add(1);
            }
        }
    }
}

impl IrInterpreter {
    /// Save the running body's registers and carry on in `callee`, whose frame
    /// a fast call pushed; `pc` is where the caller goes on from. Returns where
    /// the callee starts.
    #[inline(always)]
    fn enter<'r>(&mut self, regs: &mut Regs<'r>, callee: Entered<'r>, pc: usize) -> usize {
        let callee_bc = std::rc::Rc::as_ptr(&callee.bc);
        regs.stack.push(Activation {
            bc: regs.bc,
            bc_keep: std::mem::replace(&mut regs.bc_keep, Some(callee.bc)),
            code_ref: regs.code_ref,
            ctx: Some(regs.ctx),
            base: regs.frame.base_ptr(),
            layout: regs.frame.layout_ptr(),
            ret_dest: regs.ret_dest,
            pc,
            _scratch: (!callee.scratch.is_empty()).then(|| Box::new(callee.scratch)),
        });
        regs.bc = callee_bc;
        regs.func = callee.func;
        regs.code_ref = Some(callee.code_ref);
        regs.ctx = callee.ctx;
        regs.frame = callee.frame;
        regs.ret_dest = callee.dest;
        // SAFETY: `bc_keep` keeps the body alive.
        unsafe { (*regs.bc).entry as usize }
    }

    /// Pop the running frame and go back to its caller, or say there is none
    /// and the loop is done. Returns where the caller goes on from.
    #[inline(always)]
    fn leave(&mut self, regs: &mut Regs<'_>) -> Option<usize> {
        let len = regs.stack.len();
        if len == 0 {
            return None;
        }
        self.frame_stack.pop(regs.frame);
        // SAFETY: the top activation, read field by field and then forgotten,
        // its owned fields moved out or dropped.
        unsafe {
            use std::ptr::{addr_of, addr_of_mut, read_volatile};
            let top = regs.stack.as_mut_ptr().add(len - 1);
            regs.bc = read_volatile(addr_of!((*top).bc));
            regs.bc_keep = std::ptr::read(addr_of!((*top).bc_keep));
            regs.func = &*(*regs.bc).func;
            regs.code_ref = read_volatile(addr_of!((*top).code_ref));
            if let Some(ctx) = &*addr_of!((*top).ctx) {
                regs.ctx = *ctx;
            }
            let base = read_volatile(addr_of!((*top).base));
            let layout = read_volatile(addr_of!((*top).layout));
            regs.frame = Frame::without_liveness(base, layout);
            regs.ret_dest = Destination {
                ptr: read_volatile(addr_of!((*top).ret_dest.ptr)),
                tydesc: read_volatile(addr_of!((*top).ret_dest.tydesc)),
            };
            let pc = read_volatile(addr_of!((*top).pc));
            std::ptr::drop_in_place(addr_of_mut!((*top)._scratch));
            regs.stack.set_len(len - 1);
            Some(pc)
        }
    }

    /// The plan for a call, if it has one and it still holds: no dispatcher
    /// installed, and no body replaced since it was made.
    ///
    /// Within one code epoch no body changes -- bodies are replaced only
    /// between units, and doing so starts a new epoch -- so a call site that
    /// found its callee in this epoch has the callee it would find again.
    #[inline(always)]
    fn valid_plan<'c>(&self, call: &'c FastCall) -> Option<&'c Plan> {
        // SAFETY: only `fast_call` takes the cache mutably, and nothing holds
        // the plan across one.
        let cache = unsafe { &*call.cache.as_ptr() }.as_ref()?;
        if cache.epoch != self.code_epoch || self.call_dispatcher.borrow().is_some() {
            return None;
        }
        cache.plan.as_ref()
    }

    /// Make a planned call: push the callee's frame, write the arguments and
    /// switch to it. Returns where the callee starts.
    #[inline(always)]
    fn enter_planned<'r>(
        &mut self,
        regs: &mut Regs<'r>,
        call: &FastCall,
        plan: &Plan,
        base: *mut u8,
        resume: usize,
    ) -> Result<usize, InterpError> {
        // SAFETY: the frame borrows the layout, and the activation the
        // bytecode, from the plan, which outlives the frame: the plan is in a
        // call-site cache of the caller's bytecode, which the caller's own
        // activation keeps alive -- by `bc_keep`, by the plan it was entered
        // through, or, at the bottom, by whoever entered the loop -- and a
        // cache that is replaced while frames may still borrow from it is
        // retired to the interpreter rather than dropped, until no frame is
        // left (`release_retired_call_caches`).
        let mut frame = unsafe { self.frame_stack.push_borrowed(&plan.layout) }?;
        let callee_base = frame.base_ptr();
        for param in plan.params.iter() {
            // SAFETY: `src` was lowered against the caller's frame, which
            // `base` is, and `value` and `copy` are places in the callee's.
            unsafe {
                let mut ptr = param.src.at(base);
                if let Some((at, size)) = param.copy {
                    let copy = callee_base.add(at as usize);
                    copy_bytes(ptr, copy, size as usize);
                    ptr = copy;
                }
                (callee_base.add(param.value as usize) as *mut crate::value::Value)
                    .write(crate::value::Value { ptr, tydesc: param.tydesc });
            }
        }
        frame.stop_keeping_liveness();
        frame.clear_tracking();
        push_in_place(&mut regs.stack, Activation {
            bc: regs.bc,
            bc_keep: regs.bc_keep.take(),
            code_ref: regs.code_ref,
            ctx: None,
            base: regs.frame.base_ptr(),
            layout: regs.frame.layout_ptr(),
            ret_dest: regs.ret_dest,
            pc: resume,
            _scratch: None,
        });
        regs.bc = std::rc::Rc::as_ptr(&plan.bc);
        // SAFETY: as the frame's layout, and the caller's body holds
        // `code_ref`.
        regs.func = unsafe { &*plan.func };
        regs.code_ref = Some(unsafe { &*plan.code_ref });
        regs.frame = frame;
        // SAFETY: lowered against the caller's frame.
        regs.ret_dest = Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc };
        Ok(plan.bc.entry as usize)
    }

    /// Pop every frame the loop pushed, on the way out with an error.
    #[cold]
    fn unwind(&mut self, regs: &mut Regs<'_>) {
        while let Some(caller) = regs.stack.pop() {
            self.frame_stack.pop(regs.frame);
            // SAFETY: the caller's frame, still on the stack.
            regs.frame = unsafe { Frame::without_liveness(caller.base, caller.layout) };
        }
    }
}

impl IrInterpreter {
    // The loop's rarer ops, out of it, so that the loop's own code is what
    // the common ops need: every value live across a call in an arm is one
    // the loop has to keep somewhere, and with these inline it kept its
    // registers on the stack.

    /// Run an instruction on the IR walker.
    #[inline(never)]
    fn run_ir_op(&mut self, regs: &mut Regs<'_>, block: u32, index: u32) -> Result<(), InterpError> {
        let instr = &regs.func.blocks[block as usize].instructions[index as usize];
        if self.bc_stats.counting {
            self.bc_stats.count(|| variant(instr));
        }
        // SAFETY: the frame store the loop was lent.
        let frames = unsafe { &mut *regs.frames };
        if !self.execute_hot(instr, &mut regs.frame, frames)
            && !self.execute_warm(instr, &UnitTypes::of(regs.func), &mut regs.frame, frames)
        {
            self.execute_instruction(instr, &mut regs.frame, &regs.ctx, regs.registry, frames, regs.code_ref)?;
        }
        Ok(())
    }

    /// Make the call that is instruction `index` of block `block` by the
    /// general path.
    #[inline(never)]
    fn run_general_call(&mut self, regs: &mut Regs<'_>, block: u32, index: u32) -> Result<(), InterpError> {
        if self.bc_stats.counting {
            self.bc_stats.count(|| "(general call)".into());
        }
        let (call_site_info, func_ref, args, shapes, dest) =
            match &regs.func.blocks[block as usize].instructions[index as usize] {
                Instruction::Call { site_id, dest, func: f, args, shape_descriptors, .. } => (
                    regs.code_ref.map(|caller| crate::dispatch::CallSiteInfo {
                        caller: caller.clone(),
                        caller_unit: regs.ctx.unit(),
                        call_site_id: *site_id,
                    }),
                    f, args, shape_descriptors, *dest,
                ),
                Instruction::ComptimeCall { dest, func: f, args, shape_descriptors, .. } => {
                    (None, f, args, shape_descriptors, *dest)
                }
                i => unreachable!("Call op on {:?}", i),
            };
        // SAFETY: the frame store the loop was lent.
        let frames = unsafe { &mut *regs.frames };
        self.execute_call(func_ref, args, shapes, dest, call_site_info, &mut regs.frame, &regs.ctx, regs.registry, frames)
    }

    /// Make a fast call without a plan: a native, a forwarder, a bytecode
    /// body the loop goes on in -- then where it starts -- or the general
    /// path.
    #[inline(never)]
    unsafe fn run_fast_call<'r>(
        &mut self,
        regs: &mut Regs<'r>,
        call: &'r FastCall,
        base: *mut u8,
        pc: usize,
    ) -> Result<Option<usize>, InterpError> {
        // SAFETY: the frame store the loop was lent.
        let frames = unsafe { &mut *regs.frames };
        // SAFETY: lowered against the running frame, which `base` is.
        match unsafe { self.fast_call(call, base, &mut regs.frame, &regs.ctx, regs.registry, frames, regs.func) }? {
            Called::Done => Ok(None),
            Called::Enter(callee) => Ok(Some(self.enter(regs, callee, pc.wrapping_add(1)))),
            Called::General => {
                if self.bc_stats.counting {
                    self.bc_stats.count(|| "(fast call fell back)".into());
                }
                let Instruction::Call { .. } = &regs.func.blocks[call.block as usize].instructions[call.index as usize] else {
                    unreachable!("a fast call is a call")
                };
                self.run_general_call(regs, call.block, call.index).map(|()| None)
            }
        }
    }

    /// Pass edge `edge` of block `block`'s terminator's arguments on the IR
    /// walker.
    #[inline(never)]
    fn run_edge(&mut self, regs: &mut Regs<'_>, block: u32, edge: u32) -> Result<(), InterpError> {
        if self.bc_stats.counting {
            self.bc_stats.count(|| "(edge)".into());
        }
        let (target, args) = match &regs.func.blocks[block as usize].terminator {
            Terminator::Goto { target, args } => (*target, args),
            Terminator::Branch { then_block, then_args, .. } if edge == 0 => (*then_block, then_args),
            Terminator::Branch { else_block, else_args, .. } => (*else_block, else_args),
            t => unreachable!("an edge from {:?}", t),
        };
        // SAFETY: the frame store the loop was lent.
        let frames = unsafe { &mut *regs.frames };
        self.pass_block_args(&regs.func.blocks, target, args, &mut regs.frame, frames)
    }

    /// Return block `block`'s value on the IR walker.
    #[inline(never)]
    fn run_return_ir(&mut self, regs: &mut Regs<'_>, block: u32) {
        if self.bc_stats.counting {
            self.bc_stats.count(|| "(return)".into());
        }
        let Terminator::Return { value: Some(op) } = &regs.func.blocks[block as usize].terminator else {
            unreachable!("ReturnIr on a block that does not return a value")
        };
        // SAFETY: the frame store the loop was lent.
        let frames = unsafe { &mut *regs.frames };
        let val = self.read_operand(op, &regs.frame, frames);
        // SAFETY: the value read is the operand the IR returns, and the
        // destination the caller gave for it.
        unsafe { self.move_value(&val, regs.ret_dest) };
        Self::mark_source_dropped_all(op, &mut regs.frame, frames);
    }
}

impl IrInterpreter {
    /// Run an op that hands its operands to an IR walker routine.
    ///
    /// Out of the loop, since the routines do the work and the arms for
    /// them made the loop's own code worse: primes, which runs none of
    /// them, was 7% slower with them inline.
    #[inline(never)]
    unsafe fn run_routine_op(&mut self, op: Op, bc: &BcFunction, frame: &mut Frame, base: *mut u8) {
        unsafe {
            match op {
                Op::ConstString { dst, at, len, desc } => {
                    let bytes = if len == 0 { std::ptr::null() } else { bc.pool.as_ptr().add(at as usize) };
                    datalove_rt::c::dtlv_rti_string_from_bytes(
                        self.runtime.handle(), bytes, len as rtdt::IndexRepr, dst.at(base), bc.descs[desc as usize]);
                }
                Op::ListElementRefRt { dest, at } => {
                    let [(dst, _), l, (index, _)] = bc.rt[at as usize];
                    let list_val = rt_value(frame, base, l);
                    let (list, element_tydesc, size) = crate::list_element_info(&list_val);
                    let i = rd::<rtdt::IndexRepr>(base, index) as usize;
                    wr(base, dst, (list.data as *mut u8).add(i * size));
                    frame.set_value_tydesc(ValueId(dest), element_tydesc);
                }
                Op::EraseRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.execute_erase(&rt_value(frame, base, s), rt_dest(frame, base, d));
                }
                Op::ReifyRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.execute_reify(&rt_value(frame, base, s), rt_dest(frame, base, d));
                }
                Op::CloneRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    let (src, dest) = (rt_value(frame, base, s), rt_dest(frame, base, d));
                    datalove_rt::c::dtlv_rti_clone_erased_local(
                        self.runtime.handle(), src.ptr, src.tydesc, dest.ptr, dest.tydesc);
                }
                Op::WidenFixedRt { at } => {
                    let [d, s, _] = bc.rt[at as usize];
                    self.widen_fixed(&rt_value(frame, base, s), &rt_dest(frame, base, d));
                }
                Op::Widen { dst, src, src_desc } => {
                    let val = crate::value::Value { ptr: src.at(base), tydesc: bc.descs[src_desc as usize] };
                    self.widen_to_int(&val, &mut *(dst.at(base) as *mut rtdt::Int));
                }
                Op::BinOpRt { op, at } => {
                    let [d, a, b] = bc.rt[at as usize];
                    let (lhs, rhs) = (rt_value(frame, base, a), rt_value(frame, base, b));
                    self.execute_binop(op, &lhs, &rhs, rt_dest(frame, base, d));
                }
                op => unreachable!("{:?} is not a routine op", op),
            }
        }
    }
}

impl IrInterpreter {
    /// The body a call lands on: a module callee from the call site, while
    /// the registry is the one it was found in.
    fn resolve_callee<'r>(
        call: &FastCall,
        code_ref: &CodeRef,
        ctx: &ExecutionContext<'r>,
        registry: &'r FunctionRegistry,
    ) -> &'r IrCodeUnit {
        match code_ref {
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
        }
    }

    /// Whether a call's arguments suit a fast call into a native.
    fn suits_native(call: &FastCall, native: &datalove_datafun_ir::NativeContext) -> bool {
        call.args.iter().enumerate().all(|(i, &movable)| {
            match native.param_modes.get(i).copied().unwrap_or(ParamMode::In) {
                ParamMode::Out => false,
                ParamMode::In => movable || native.param_types[i].is_copy(),
                ParamMode::Ref | ParamMode::Mut => true,
            }
        })
    }

    /// The bytecode of a body that only forwards its parameters to a native,
    /// if `callee` is one.
    ///
    /// A forwarder is one call that passes the parameters in order and has no
    /// shapes to hand over, then returns what the call returned: most of the
    /// standard library's wrappers around natives. Calling the native from
    /// the forwarder's caller saves a frame and a call.
    fn forwarder(
        &mut self,
        layout: &IrLayout,
        callee: &IrCodeUnit,
        callee_ctx: &ExecutionContext,
        registry: &FunctionRegistry,
    ) -> Option<std::rc::Rc<BcFunction>> {
        let [block] = &callee.blocks[..] else { return None };
        let [Instruction::Call { func, dest, args, shape_descriptors, .. }] = &block.instructions[..] else {
            return None;
        };
        let in_order = args.len() == layout.param_modes.len()
            && args.iter().enumerate().all(|(i, a)| matches!(a, Operand::Param(p) if p.0 as usize == i));
        let returns = match &block.terminator {
            Terminator::Return { value: None } => true,
            Terminator::Return { value: Some(Operand::Value(v)) } => v == dest,
            _ => false,
        };
        if !in_order || !returns || !shape_descriptors.is_empty() {
            return None;
        }
        let bc = self.bytecode_for(layout, callee);
        let [inner] = &bc.calls[..] else { return None };
        if !matches!(bc.ops.first(), Some(Op::CallFast { site: 0 })) {
            return None;
        }
        let CodeUnitContext::Native(native) = &Self::resolve_callee(inner, func, callee_ctx, registry).context else {
            return None;
        };
        Self::suits_native(inner, native).then_some(bc)
    }

    /// Call a native from a fast call, with each argument from `arg`.
    ///
    /// # Safety
    ///
    /// `arg` must give what the native's ABI requires.
    #[allow(clippy::too_many_arguments)]
    unsafe fn call_native(
        &self,
        call: &FastCall,
        address: usize,
        native: &datalove_datafun_ir::NativeContext,
        dest: Destination,
        shapes: &[*const rtdt::TyDesc],
        mut arg: impl FnMut(&Self, usize, ParamMode) -> crate::value::Value,
    ) -> Result<(), InterpError> {
        let mode = |i: usize| native.param_modes.get(i).copied().unwrap_or(ParamMode::In);
        let n = call.args.len();
        let target = {
            let generation = self.native_table.generation();
            let mut found = call.native.borrow_mut();
            match &*found {
                Some((at, seen, target)) if *at == address && *seen == generation => target.clone(),
                _ => {
                    let target = self.native_table.lookup(native.symbol())?;
                    *found = Some((address, generation, target.clone()));
                    target
                }
            }
        };
        if let NativeTarget::C(fn_ptr) = target {
            // The C words straight from the arguments, with no list of them
            // in between; see `native::c_words`.
            let len = 1 + 2 * n + 2 + shapes.len();
            if len > MAX_C_WORDS {
                todo!("native functions with {} C args not yet supported", len);
            }
            let mut words = [0usize; MAX_C_WORDS];
            words[0] = self.runtime.handle() as usize;
            for i in 0..n {
                let a = arg(self, i, mode(i));
                words[1 + 2 * i] = a.ptr as usize;
                words[2 + 2 * i] = a.tydesc as usize;
            }
            let at = 1 + 2 * n;
            words[at] = dest.ptr as usize;
            words[at + 1] = dest.tydesc as usize;
            for (i, tydesc) in shapes.iter().enumerate() {
                words[at + 2 + i] = *tydesc as usize;
            }
            // SAFETY: the table registered this as a rider function and holds
            // its code until its generation changes.
            unsafe { call_c(fn_ptr, &words[..len]) };
            return Ok(());
        }
        // On the stack when there are few, which is nearly always: a list per
        // call was an allocation per call.
        const ON_STACK: usize = 8;
        let mut stack = [crate::value::Value { ptr: std::ptr::null_mut(), tydesc: std::ptr::null() }; ON_STACK];
        let heap: Vec<crate::value::Value>;
        let args: &[crate::value::Value] = if n <= ON_STACK {
            for (i, slot) in stack.iter_mut().enumerate().take(n) {
                *slot = arg(self, i, mode(i));
            }
            &stack[..n]
        } else {
            heap = (0..n).map(|i| arg(self, i, mode(i))).collect();
            &heap
        };
        let NativeTarget::Rust(f) = target else { unreachable!("a C native was called above") };
        f(self.runtime.handle(), args, dest, shapes)
    }

    /// Make a fast call, or say the general path has to.
    ///
    /// Does what `execute_call_site` does for a call whose arguments are in
    /// this frame, with no `out` parameter among them and none moved that the
    /// frame would have to record, and none of what it would skip: no
    /// dispatcher is installed, so there is nothing to offer the call to and
    /// no optimized body to run instead.
    ///
    /// A native is called here. A bytecode body is not run here: its frame is
    /// pushed and entered and handed back, for the loop to run without
    /// recursing.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    unsafe fn fast_call<'r>(
        &mut self,
        call: &FastCall,
        base: *mut u8,
        frame: &mut Frame,
        ctx: &ExecutionContext<'r>,
        registry: &'r FunctionRegistry,
        frames: &mut FrameStore,
        caller: &'r IrCodeUnit,
    ) -> Result<Called<'r>, InterpError> {
        if self.call_dispatcher.borrow().is_some() {
            return Ok(Called::General);
        }
        let Instruction::Call { func: code_ref, args: ir_args, shape_descriptors, .. } =
            &caller.blocks[call.block as usize].instructions[call.index as usize] else {
            unreachable!("a fast call is a call")
        };
        let callee = Self::resolve_callee(call, code_ref, ctx, registry);
        let callee_ctx = ctx.for_callee(code_ref, registry);
        let address = callee as *const IrCodeUnit as usize;
        let (layout, suits, forward) = {
            let mut cache = call.cache.borrow_mut();
            match &*cache {
                Some(c) if c.address == address && c.epoch == self.code_epoch => {
                    (c.layout.clone(), c.suits, c.forward.clone())
                }
                _ => {
                    let (layout, suits, forward) = match &callee.context {
                        CodeUnitContext::Native(native) => (None, Self::suits_native(call, native), None),
                        _ => {
                            let identity = crate::dispatch::FuncIdentity::of(code_ref, ctx.unit());
                            let layout = self.layout_cache.get_or_compute(identity, callee, &mut self.tydesc_table);
                            let suits = call.args.iter().enumerate().all(|(i, &movable)| {
                                match layout.param_modes[i] {
                                    ParamMode::Out => false,
                                    ParamMode::In => movable || !layout.param_moves[i],
                                    ParamMode::Ref | ParamMode::Mut => true,
                                }
                            });
                            let forward = self.forwarder(&layout, callee, &callee_ctx, registry);
                            (Some(layout), suits, forward)
                        }
                    };
                    let plan = match (&layout, &call.resolved, &forward, code_ref) {
                        (_, _, _, CodeRef::External { .. }) | (None, ..) | (_, None, ..) | (_, _, Some(_), _) => None,
                        _ if !suits => None,
                        (Some(layout), Some(resolved), None, _) => Some(Plan {
                            layout: std::rc::Rc::clone(layout),
                            bc: self.bytecode_for(layout, callee),
                            func: callee,
                            code_ref,
                            params: layout.param_modes.iter().enumerate().map(|(i, mode)| PlannedParam {
                                src: resolved[i].0,
                                value: layout.param_offsets[i],
                                tydesc: match mode {
                                    ParamMode::In | ParamMode::Out => layout.param_tydescs[i],
                                    ParamMode::Ref | ParamMode::Mut => resolved[i].1,
                                },
                                copy: layout.param_copies[i],
                            }).collect(),
                        }),
                    };
                    let fresh = CallCache {
                        address, epoch: self.code_epoch, layout: layout.clone(), suits, forward: forward.clone(), plan,
                    };
                    // A planned call's frame borrows its layout and bytecode
                    // from the plan, so a replaced one is kept.
                    if let Some(old) = cache.replace(fresh) {
                        self.retired_call_caches.push(old);
                    }
                    (layout, suits, forward)
                }
            }
        };
        if !suits {
            return Ok(Called::General);
        }
        let dest = Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc };

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

        if let (Some(wrapper), Some(layout)) = (&forward, &layout) {
            // The forwarder's own call, made from here: each argument as the
            // forwarder's frame would hold it -- an owned one with the
            // forwarder's descriptor, a borrowed one with the one it came
            // with -- then as its call would read it, and the result where
            // the forwarder would have copied it, in its shape.
            let inner = &wrapper.calls[0];
            let Instruction::Call { func: inner_ref, .. } = &callee.blocks[0].instructions[0] else {
                unreachable!("a forwarder is a call")
            };
            let native_body = Self::resolve_callee(inner, inner_ref, &callee_ctx, registry);
            let CodeUnitContext::Native(native) = &native_body.context else {
                unreachable!("a forwarder calls a native, and its body is fixed while the registry is")
            };
            let mut inner_scratch: crate::BorrowScratch = Vec::new();
            let inner_dest = Destination { ptr: dest.ptr, tydesc: inner.dest_tydesc };
            // SAFETY: the arguments are read as the forwarder would read them.
            unsafe { self.call_native(
                inner, native_body as *const IrCodeUnit as usize, native, inner_dest, &[],
                |this, i, mode| {
                    let mut val = read(this, i, layout.param_modes[i], frame);
                    if matches!(layout.param_modes[i], ParamMode::In | ParamMode::Out) {
                        val.tydesc = layout.param_tydescs[i];
                    }
                    match &inner.resolved {
                        Some(resolved) => crate::value::Value { ptr: val.ptr, tydesc: resolved[i].1 },
                        None if matches!(mode, ParamMode::Ref | ParamMode::Mut) => {
                            IrInterpreter::borrow_through_wrapper(val, &mut inner_scratch)
                        }
                        None => val,
                    }
                },
            ) }?;
            return Ok(Called::Done);
        }

        // The common case, a function called with statically typed arguments
        // and no shapes, reads nothing but the resolved places.
        if let (Some(resolved), Some(layout)) = (&call.resolved, &layout) {
            let mut callee_frame = self.frame_stack.push(std::rc::Rc::clone(layout))?;
            for (i, &(loc, tydesc)) in resolved.iter().enumerate() {
                // SAFETY: lowered against this frame's layout.
                callee_frame.set_param(i, crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc });
            }
            callee_frame.enter();
            callee_frame.stop_keeping_liveness();
            return Ok(Called::Enter(Entered {
                frame: callee_frame,
                bc: self.bytecode_for(layout, callee),
                func: callee,
                code_ref,
                ctx: callee_ctx,
                dest,
                scratch: Vec::new(),
            }));
        }

        let shapes: Vec<*const rtdt::TyDesc> = if shape_descriptors.is_empty() {
            Vec::new()
        } else {
            shape_descriptors.iter().map(|r| self.resolve_shape_ref(r, frame)).collect()
        };

        let Some(layout) = layout else {
            // A native: no frame, just the arguments.
            let CodeUnitContext::Native(native) = &callee.context else { unreachable!() };
            // SAFETY: the arguments are read from this frame as lowered.
            unsafe { self.call_native(call, address, native, dest, &shapes, |this, i, mode| read(this, i, mode, frame)) }?;
            return Ok(Called::Done);
        };

        let mut callee_frame = self.frame_stack.push(std::rc::Rc::clone(&layout))?;
        for i in 0..call.args.len() {
            callee_frame.set_param(i, read(self, i, layout.param_modes[i], frame));
        }
        for (i, tydesc) in shapes.into_iter().enumerate() {
            callee_frame.set_shape_descriptor(i, tydesc);
        }
        callee_frame.enter();
        callee_frame.stop_keeping_liveness();
        // The caller's frame keeps no record of an argument moved into a fast
        // call: those with tracking bytes do not suit it.
        Ok(Called::Enter(Entered {
            frame: callee_frame,
            bc: self.bytecode_for(&layout, callee),
            func: callee,
            code_ref,
            ctx: callee_ctx,
            dest,
            scratch,
        }))
    }
}

impl BcFunction {
    /// The ops, one per line, for reading.
    pub(crate) fn dump(&self, func: &IrCodeUnit) -> String {
        self.ops.iter().enumerate().map(|(i, op)| match op {
            Op::Ir { block, index } | Op::Call { block, index } => format!(
                "  {i:3}: {op:?} {:?}\n", func.blocks[*block as usize].instructions[*index as usize]),
            Op::CallFast { site } => {
                let call = &self.calls[*site as usize];
                format!("  {i:3}: {op:?} {:?}\n", func.blocks[call.block as usize].instructions[call.index as usize])
            }
            _ => format!("  {i:3}: {op:?}\n"),
        }).collect()
    }
}

impl BcStats {
    pub(crate) fn record(&mut self, bc: &BcFunction, escapes: u32) {
        self.bodies += 1;
        self.ops += bc.ops.len() as u32;
        self.escapes += escapes;
    }
}

impl IrInterpreter {
    /// Print what the bytecode lowering did, for `DATALOVE_BC_STATS`.
    pub(crate) fn report_bc_stats(&self) {
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
