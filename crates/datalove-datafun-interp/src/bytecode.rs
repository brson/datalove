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
//! # Calls
//!
//! The loop (`run_body`) makes a call one of these ways, the cheapest first:
//!
//! - **Planned** (`valid_plan`, `enter_planned`, `call_planned_native`,
//!   `call_planned_jit`): an `Op::CallFast` whose site has a `Plan` made in
//!   this code epoch, for any call that hands over no shapes and whose
//!   arguments suit it. To a bytecode body of the same unit or a module, it
//!   pushes the callee's frame, writes the arguments, saves the caller in an
//!   `Activation` and carries on in the callee in the same loop. To a rider
//!   function, directly or through a forwarder, it fills in the C words and
//!   calls it. To a body the dispatcher has compiled, it fills in the words
//!   the code is entered with and has the dispatcher enter it. An argument
//!   the lowering resolved is read from its place; one whose type has a
//!   `data` in it, by an `ArgRecipe`, as `fast_call` reads it.
//!
//!   With a dispatcher installed, a site plans a call to a body as the
//!   dispatcher's `SitePolicy` for it says: interpret it, count calls and
//!   offer every so many as one (`Planned::Offer`), enter compiled code, or
//!   plan nothing and offer every call. Calls to natives are planned as
//!   without one, since a dispatcher is never offered those.
//! - **Fast, unplanned** (`run_fast_call` → `fast_call`): any other
//!   `Op::CallFast`. `fast_call` refreshes the site's `CallCache` (making the
//!   plan, if the call can have one, for next time) and then makes the call:
//!   - to a *forwarder*, a body that only passes its parameters on to a
//!     native, by calling the native from here;
//!   - to a *native*, through `call_native` and its cached `NativeTarget`;
//!   - to a *bytecode body*, by pushing and entering its frame and handing it
//!     back as `Called::Enter`, which `switch_to` turns into an activation the loop
//!     carries on in;
//!   - and anything it does not suit -- an `out` argument, a moved argument
//!     with a tracking byte, a body while a dispatcher is installed -- it
//!     hands back as `Called::General`.
//! - **General** (`run_general_call`): an `Op::Call`, or a fast call handed
//!   back. `execute_call`, as the IR walker makes it, which runs a bytecode
//!   callee in a loop of its own, nested on the Rust stack.
//!
//! A return writes the result where the running activation's `ret_dest`
//! says and `leave`s: pops the frame and goes back to the caller's activation,
//! or out of the loop if it was the frame the loop was entered with. An error
//! pops every frame the loop pushed (`unwind`).
//!
//! What keeps a frame's layout and bytecode alive: a planned frame borrows
//! them from its plan, which a call-site cache of the caller's bytecode holds,
//! which the caller's own activation keeps alive -- by `bc_keep`, by the plan
//! it was entered through, or, at the bottom, by whoever entered the loop. A
//! cache replaced while frames may still borrow from it is retired to the
//! interpreter until no frame runs, and every cache is forgotten when bodies
//! are replaced (`IrInterpreter::forget_compiled_bodies`).
//!
//! See `botdocs/plan-bytecode.md` and `botdocs/plan-frame-stack.md`.

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

#[path = "bytecode_opt.rs"]
mod opt;
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

    /// The place `n` bytes further into a place in the frame itself, which an
    /// indirect one is not: its offset is where the pointer is.
    fn advanced(self, n: u32) -> Option<Self> {
        (self.0 & INDIRECT == 0).then(|| Loc::direct(self.0 + n))
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

/// The type a checked update in place works on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum CkTy {
    U32,
    I32,
    U64,
    I64,
}

impl CkTy {
    /// The type of a place of `ty`, if the checked updates cover it.
    fn of(ty: &IrType) -> Option<Self> {
        let index32 = rtdt::INDEX_SIZE == 4;
        Some(match ty {
            IrType::U32 => CkTy::U32,
            IrType::I32 => CkTy::I32,
            IrType::U64 => CkTy::U64,
            IrType::I64 => CkTy::I64,
            IrType::Index if index32 => CkTy::U32,
            IrType::Index => CkTy::U64,
            IrType::Offset if index32 => CkTy::I32,
            IrType::Offset => CkTy::I64,
            _ => return None,
        })
    }

    fn size(self) -> u32 {
        match self {
            CkTy::U32 | CkTy::I32 => 4,
            CkTy::U64 | CkTy::I64 => 8,
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
    /// Count an iteration of the loop `loops[at]` heads, the first op of its
    /// header: where the frame is as entering the header, which is where
    /// compiled code can take over (`IrInterpreter::loop_hot`).
    LoopHead { at: u32 },
    /// Make the call described by `calls[site]`, whose arguments are all in
    /// this frame, without the general call path. Takes the general one whenever
    /// a dispatcher is installed, so the JIT sees every call.
    CallFast { site: u32 },

    Const1 { dst: Loc, imm: u8 },
    Const4 { dst: Loc, imm: u32 },
    /// Copy `len` bytes of the constant pool from `at`.
    ConstPool { dst: Loc, at: u32, len: u32 },
    /// A new string of `len` bytes of the constant pool from `at`.
    ConstString { dst: Loc, at: u32, len: u32, desc: u32 },

    /// Copy `len` bytes from `offset` past the place `src` names: a field
    /// read through a reference.
    CopyAt { dst: Loc, src: Loc, offset: u32, len: u32 },
    /// Write the address `offset` past the place `src` names: a reference to a
    /// field.
    FieldRef { dst: Loc, src: Loc, offset: u32 },
    /// Write the address of `statics[slot]`'s value in the interpreter's pool.
    StaticRef { dst: Loc, slot: u32 },
    /// Look `key` up in `map` and clone what it finds into `dest`, writing
    /// whether there was one to `valid`; `rt[at]` holds the three.
    MapGetRt { at: u32, valid: Loc },

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
    DivCkU32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    DivCkI32 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    DivCkU64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
    DivCkI64 { dst: Loc, ovf: Loc, a: Loc, b: Loc },
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
    /// Update a place with `op`, on the IR walker's routine: `rt[at]` holds
    /// the place and the operand. An `int` is updated in its own buffer.
    OpAssignRt { op: BinOp, at: u32 },
    /// Update `place` with checked `kind` and `b`, writing it only if the
    /// operation doesn't overflow, which goes to `ovf`.
    CkAssign { ty: CkTy, kind: Ck, place: Loc, b: Loc, ovf: Loc },
    CkAssignU32I { kind: Ck, place: Loc, imm: u32, ovf: Loc },

    Jump { to: u32 },
    BrIf { cond: Loc, then: u32, els: u32 },
    /// A comparison whose only use is the branch on it.
    BrCmpU8 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    BrCmpU32I { cmp: Cmp, a: Loc, imm: u32, then: u32, els: u32 },
    /// Checked `u32` arithmetic whose overflow flag only a branch reads:
    /// on to `ovf` if it overflowed, `ok` if not.
    CkU32Br { kind: Ck, dst: Loc, a: Loc, b: Loc, ovf: u32, ok: u32 },
    CkU32IBr { kind: Ck, dst: Loc, a: Loc, imm: u32, ovf: u32, ok: u32 },
    /// A checked update whose overflow flag only a branch reads: on to `ovf`,
    /// the place as it was, if it overflowed, and to `ok` if not.
    CkAssignBr { ty: CkTy, kind: Ck, place: Loc, b: Loc, ovf: u32, ok: u32 },
    CkAssignU32IBr { kind: Ck, place: Loc, imm: u32, ovf: u32, ok: u32 },
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
    /// What the call has found out about its callee.
    ///
    /// Written only on the slow path -- `resolve_callee`, `call_native` and
    /// `fast_call`'s refresh -- none of which runs bytecode, so none of them
    /// holds it borrowed while another does. Read without borrowing it by
    /// `valid_plan` on the hot path, whose reference ends before any of
    /// those can run again.
    site: std::cell::RefCell<SiteCache>,
}

/// What a call site has found out about its callee; see `FastCall::site`.
#[derive(Default)]
struct SiteCache {
    /// What the call found out about its callee the last time, so that a
    /// call need not find it out again unless the body has changed.
    callee: Option<CallCache>,
    /// A native callee's target, with the body and the native table's
    /// generation it was looked up for, which saves hashing its symbol at
    /// every call.
    native: Option<(usize, u64, NativeTarget)>,
    /// A module callee, found once: the registry it was found in, held so that
    /// it cannot be freed and its address reused, and the body. Valid while
    /// the call's registry is that one -- module bodies do not change within
    /// a registry -- which saves two ordered-map lookups a call.
    module_callee: Option<(
        std::sync::Arc<datalove_datafun_ir::registry::ModuleFunctionRegistry>,
        *const IrCodeUnit,
    )>,
}

/// What a fast call found out about its callee, kept until the callee's body
/// changes.
pub(crate) struct CallCache {
    /// The body's address, and the interpreter's code epoch when it was found:
    /// an address only names a body within an epoch.
    address: usize,
    epoch: u64,
    /// The native table's generation when it was found, which a native's
    /// plan holds the function of.
    generation: u64,
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
    /// The dispatcher epoch the plan was made in, if a dispatcher was
    /// installed, whose `SitePolicy` it follows.
    ///
    /// A plan made with none is good while there is none, and a body plan
    /// stays good with one too until the dispatcher is asked; one made with a
    /// dispatcher is good under that dispatcher, and its body plans when there
    /// is none, which is while compiled code that dispatcher entered calls back
    /// into the interpreter.
    dispatcher: Option<u64>,
}

impl CallCache {
    /// Whether the site has to ask the dispatcher, installed in `epoch`, how to
    /// make its calls before it can plan them.
    fn needs_policy(&self, epoch: u64) -> bool {
        if self.dispatcher != Some(epoch) {
            return true;
        }
        matches!(&self.plan, Some(Plan::Body(BodyPlan { countdown: Some(c), .. })) if c.run_out())
    }
}

/// A call worked out once, made by the loop without asking anything again.
///
/// What is left for each call is to check that the callee is still what this
/// was made for, and to read the arguments as their recipes say.
enum Plan {
    Body(BodyPlan),
    Native(NativePlan),
    Jit(JitPlan),
}

/// A call to a bytecode body in the same unit or a module: push its frame,
/// write the arguments and switch to it.
struct BodyPlan {
    layout: std::rc::Rc<IrLayout>,
    bc: std::rc::Rc<BcFunction>,
    func: *const IrCodeUnit,
    /// How the call names the callee, in the caller's body.
    code_ref: *const CodeRef,
    params: PlannedParams,
    /// The calls left until the dispatcher is to be offered one, under
    /// `SitePolicy::Count`.
    countdown: Option<Countdown>,
}

/// Calls a planned site makes, or iterations a loop header counts, before
/// the dispatcher is asked again; see `SitePolicy::Count`.
struct Countdown {
    /// Left to make. Zero once run out, until set again.
    left: std::cell::Cell<u32>,
    /// How many it was set to, which the asking reports.
    batch: std::cell::Cell<u32>,
}

impl Countdown {
    fn new(n: u32) -> Self {
        let n = n.max(1);
        Countdown { left: std::cell::Cell::new(n), batch: std::cell::Cell::new(n) }
    }

    fn set(&self, n: u32) {
        let n = n.max(1);
        self.left.set(n);
        self.batch.set(n);
    }

    /// Count one, and say whether it was the last, which leaves it run out.
    #[inline(always)]
    fn tick(&self) -> bool {
        let left = self.left.get();
        self.left.set(left.saturating_sub(1));
        left <= 1
    }

    fn run_out(&self) -> bool {
        self.left.get() == 0
    }

    fn batch(&self) -> u32 {
        self.batch.get()
    }
}

/// What a planned call site is to do with this call; see `valid_plan`.
enum Planned<'c> {
    Body(&'c BodyPlan),
    Native(&'c NativePlan),
    Jit(&'c JitPlan),
    /// Offer it to the dispatcher by the general path, as standing for this
    /// many calls.
    Offer(u32),
}

/// A call to a function the dispatcher compiled: the words it is entered
/// with, from the arguments as the recipes read them.
struct JitPlan {
    func: crate::dispatch::FuncIdentity,
    entry: crate::dispatch::CompiledEntry,
    args: Box<[ArgRecipe]>,
    /// The parameters whose descriptors follow the arguments, in order.
    descriptor_params: Box<[u32]>,
}

/// The arguments of a planned call to a body.
enum PlannedParams {
    /// Every one where the lowering resolved it, which is most calls outside
    /// generic code, each a place and a descriptor.
    At(Box<[AtParam]>),
    /// Some read as the IR walker reads them.
    Recipes(Box<[PlannedParam]>),
}

/// One argument of a planned call to a body, where the lowering resolved it.
struct AtParam {
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

/// One argument of a planned call to a body, by its recipe.
struct PlannedParam {
    /// How to read it, and with what descriptor the callee's frame holds it
    /// once entered: the callee's own for an owned parameter, the argument's
    /// for a borrowed one.
    arg: ArgRecipe,
    /// Where its `Value` goes in the callee's frame.
    value: u32,
    /// Where the callee's frame keeps a copy of it, and its size, if it does;
    /// see `IrLayout::param_copies`.
    copy: Option<(u32, u32)>,
}

/// A call to a rider function, directly or through a forwarder: the words its
/// C ABI takes, from the arguments as the recipes read them.
struct NativePlan {
    fn_ptr: *const (),
    /// The native table's generation the function was found in, outside of
    /// which it may be another's.
    generation: u64,
    args: Box<[ArgRecipe]>,
    /// The result's descriptor, as the native's own call would give it.
    dest_tydesc: *const rtdt::TyDesc,
}

/// How a planned call reads one argument, and what it hands over.
#[derive(Clone, Copy)]
struct ArgRecipe {
    src: ArgSource,
    /// Read through a `data` wrapper, as a borrowed argument from a generic
    /// caller is.
    unwrap: bool,
    /// The descriptor to hand over in place of the one read.
    retype: Option<*const rtdt::TyDesc>,
    /// Read through a wrapper again, after retyping: a forwarder's borrowed
    /// parameter passed on to its native.
    unwrap_again: bool,
}

/// Where a planned call's argument is.
#[derive(Clone, Copy)]
enum ArgSource {
    /// At a place in the caller's frame, holding what the descriptor says,
    /// as the lowering resolved it.
    At(Loc, *const rtdt::TyDesc),
    /// The operand, read at the call as the IR walker reads it: one whose
    /// type, a `data` in it, only the running frame says.
    Read(Operand),
}

/// What a fast call did.
enum Called<'r> {
    /// It made the call: a native, or one through a forwarder to one.
    Done,
    /// It did not suit the fast path, and the general one has to make it.
    General,
    /// It pushed and entered a bytecode body's frame, for the loop to run.
    Enter(Callee<'r>),
}

/// A callee, its frame pushed and entered, for the loop to carry on in.
struct Callee<'r> {
    frame: Frame,
    bc: *const BcFunction,
    /// What keeps `bc` alive, unless the plan the call was made by does.
    keep: Option<std::rc::Rc<BcFunction>>,
    func: &'r IrCodeUnit,
    code_ref: &'r CodeRef,
    /// Its context, if it is not its caller's.
    ctx: Option<ExecutionContext<'r>>,
    /// Where the result goes, in the caller's frame.
    dest: Destination,
    /// What borrowed arguments were read into, if any were, held until the
    /// call returns.
    scratch: Option<Box<crate::BorrowScratch>>,
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
    frames: &'r mut FrameStore,
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
    statics: Vec<StaticSlot>,
    /// What each `LoopHead` counts.
    loops: Vec<LoopCounter>,
}

/// A loop header's count of iterations, toward asking the dispatcher whether
/// to go on in compiled code from there.
///
/// One per body rather than per frame, so it counts the loop's iterations in
/// every call running it.
struct LoopCounter {
    header: BlockId,
    countdown: Countdown,
}

/// A const a `StaticRef` op names, and where the pool put it.
///
/// Found in the pool the first time the op runs and kept: the pool belongs to
/// the interpreter, as this body does, and an entry lives as long as the
/// interpreter. The IR walker looks the value up by its `Arc` every time.
struct StaticSlot {
    value: std::sync::Arc<datalove_datafun_ir::ConstValue>,
    pointee: *const rtdt::TyDesc,
    resolved: std::cell::Cell<*const u8>,
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
    escapes: u32,
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
        self.escapes += 1;
        self.emit(Op::Ir { block: block as u32, index: index as u32 });
    }

    fn lower(mut self) -> (BcFunction, u32) {
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
            // A value of no size shares its offset with the next one, and
            // nothing reads or writes it.
            if layout_of(&self.func.value_types[v]).size == 0 {
                continue;
            }
            values.insert(self.layout.value_offsets[v], opt::ValueFact {
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
    fn assemble(mut self, prologue: Vec<Op>) -> (BcFunction, u32) {
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
                self.escapes += 1;
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
                    | Instruction::RefSetField { dest, .. } | Instruction::RefSetFieldTracked { dest, .. }
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

impl Op {
    /// Every place in the frame the op reads or writes, with how many bytes it
    /// reads or writes there.
    ///
    /// Exhaustive, so that a new op cannot be added without saying what it
    /// touches; the verifier checks these against the frame.
    fn places(&self, f: &mut impl FnMut(Loc, u32)) {
        const I: u32 = rtdt::INDEX_SIZE;
        match *self {
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. } | Op::Jump { .. } | Op::LoopHead { .. }
            | Op::EdgeIr { .. } | Op::ReturnUnit | Op::ReturnIr { .. }
            | Op::ListElementRefRt { .. } | Op::EraseRt { .. } | Op::ReifyRt { .. }
            | Op::CloneRt { .. } | Op::WidenFixedRt { .. } | Op::BinOpRt { .. }
            | Op::OpAssignRt { .. } => {}
            Op::CkAssign { ty, place, b, ovf, .. } => { f(place, ty.size()); f(b, ty.size()); f(ovf, 1) }
            Op::CkAssignU32I { place, ovf, .. } => { f(place, 4); f(ovf, 1) }
            Op::CkAssignBr { ty, place, b, .. } => { f(place, ty.size()); f(b, ty.size()) }
            Op::CkAssignU32IBr { place, .. } => f(place, 4),
            Op::CopyAt { dst, src, offset, len } => { f(dst, len); f(src, offset + len) }
            Op::FieldRef { dst, src, offset } => { f(dst, 8); f(src, offset) }
            Op::StaticRef { dst, .. } => f(dst, 8),
            Op::MapGetRt { valid, .. } => f(valid, 1),
            Op::Const1 { dst, .. } => f(dst, 1),
            Op::Const4 { dst, .. } => f(dst, 4),
            Op::ConstPool { dst, len, .. } => f(dst, len),
            Op::ConstString { dst, .. } => f(dst, 1),
            Op::Copy1 { dst, src } => { f(dst, 1); f(src, 1) }
            Op::Copy4 { dst, src } => { f(dst, 4); f(src, 4) }
            Op::Copy8 { dst, src } => { f(dst, 8); f(src, 8) }
            Op::CopyN { dst, src, len } => { f(dst, len); f(src, len) }
            Op::AddCkU32 { dst, ovf, a, b } | Op::SubCkU32 { dst, ovf, a, b } | Op::MulCkU32 { dst, ovf, a, b }
            | Op::AddCkI32 { dst, ovf, a, b } | Op::SubCkI32 { dst, ovf, a, b } | Op::MulCkI32 { dst, ovf, a, b }
            | Op::DivCkU32 { dst, ovf, a, b } | Op::DivCkI32 { dst, ovf, a, b } => {
                f(dst, 4); f(ovf, 1); f(a, 4); f(b, 4)
            }
            Op::AddCkU64 { dst, ovf, a, b } | Op::SubCkU64 { dst, ovf, a, b } | Op::MulCkU64 { dst, ovf, a, b }
            | Op::AddCkI64 { dst, ovf, a, b } | Op::SubCkI64 { dst, ovf, a, b } | Op::MulCkI64 { dst, ovf, a, b }
            | Op::DivCkU64 { dst, ovf, a, b } | Op::DivCkI64 { dst, ovf, a, b } => {
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
            Op::CkU32Br { ovf, ok, .. } | Op::CkU32IBr { ovf, ok, .. }
            | Op::CkAssignBr { ovf, ok, .. } | Op::CkAssignU32IBr { ovf, ok, .. } => { f(ovf); f(ok) }
            Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. }
            | Op::Const1 { .. } | Op::Const4 { .. } | Op::ConstPool { .. } | Op::ConstString { .. }
            | Op::Copy1 { .. } | Op::Copy4 { .. } | Op::Copy8 { .. } | Op::CopyN { .. }
            | Op::CopyAt { .. } | Op::FieldRef { .. } | Op::StaticRef { .. } | Op::MapGetRt { .. }
            | Op::DivCkU32 { .. } | Op::DivCkI32 { .. } | Op::DivCkU64 { .. } | Op::DivCkI64 { .. }
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
            | Op::Widen { .. } | Op::BinOpRt { .. } | Op::OpAssignRt { .. }
            | Op::CkAssign { .. } | Op::CkAssignU32I { .. } | Op::Switch { .. }
            | Op::Return { .. } | Op::ReturnOk { .. } | Op::ReturnUnit | Op::ReturnIr { .. }
            | Op::LoopHead { .. } => {}
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
                Op::LoopHead { at } => in_table(at, self.loops.len(), "loop"),
                Op::StaticRef { slot, .. } => in_table(slot, self.statics.len(), "static"),
                Op::ConstPool { at: pool_at, len, .. } | Op::ConstString { at: pool_at, len, .. } => {
                    assert!(pool_at + len <= self.pool.len() as u32, "{}: op {} reads past the pool", func.name, at);
                }
                Op::ListElementRefRt { at: i, .. } | Op::EraseRt { at: i } | Op::ReifyRt { at: i }
                | Op::CloneRt { at: i } | Op::WidenFixedRt { at: i } | Op::BinOpRt { at: i, .. }
                | Op::OpAssignRt { at: i, .. } | Op::MapGetRt { at: i, .. } => {
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

/// Lower a function body against its layout.
pub(crate) fn lower(func: &IrCodeUnit, layout: &IrLayout, optimize: bool) -> (BcFunction, u32) {
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
        escapes: 0,
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

/// Checked division, as the IR walker's: by zero, or the least signed value
/// by minus one, sets the flag and gives zero.
#[inline(always)]
unsafe fn div_checked<T>(base: *mut u8, dst: Loc, ovf: Loc, a: Loc, b: Loc)
where
    T: Copy + num_checked_div::CheckedDiv,
{
    unsafe {
        let (r, o) = match rd::<T>(base, a).checked_div(rd::<T>(base, b)) {
            Some(r) => (r, false),
            None => (T::ZERO, true),
        };
        wr(base, dst, r);
        wr(base, ovf, o);
    }
}

mod num_checked_div {
    /// The integer types checked division is lowered for.
    pub(super) trait CheckedDiv: Sized {
        const ZERO: Self;
        fn checked_div(self, rhs: Self) -> Option<Self>;
    }
    macro_rules! checked_div {
        ($($t:ty),*) => {$(
            impl CheckedDiv for $t {
                const ZERO: Self = 0;
                fn checked_div(self, rhs: Self) -> Option<Self> { <$t>::checked_div(self, rhs) }
            }
        )*};
    }
    checked_div!(u32, i32, u64, i64);
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

/// A checked update of `place` with `b`, written only if it doesn't
/// overflow. Gives whether it did.
#[inline(always)]
unsafe fn ck_assign(base: *mut u8, ty: CkTy, kind: Ck, place: Loc, b: Loc) -> bool {
    unsafe {
        match ty {
            CkTy::U32 => ck_assign_as::<u32>(base, kind, place, rd(base, b)),
            CkTy::I32 => ck_assign_as::<i32>(base, kind, place, rd(base, b)),
            CkTy::U64 => ck_assign_as::<u64>(base, kind, place, rd(base, b)),
            CkTy::I64 => ck_assign_as::<i64>(base, kind, place, rd(base, b)),
        }
    }
}

#[inline(always)]
unsafe fn ck_assign_as<T: Copy + CheckedIntOps>(base: *mut u8, kind: Ck, place: Loc, b: T) -> bool {
    unsafe {
        let a = rd::<T>(base, place);
        let (r, o) = match kind {
            Ck::Add => a.overflowing_add_impl(b),
            Ck::Sub => a.overflowing_sub_impl(b),
            Ck::Mul => a.overflowing_mul_impl(b),
        };
        if !o {
            wr(base, place, r);
        }
        o
    }
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
                        match self.valid_plan(call) {
                            Some(Planned::Body(plan)) => {
                                pc = tri!(self.enter_planned(regs, call, plan, base, pc.wrapping_add(1)));
                                bc = &*regs.bc;
                                ops = bc.ops.as_ptr();
                                base = regs.frame.base_ptr();
                                continue;
                            }
                            // Outside the generation it was found in, the
                            // function may be another's, which the slow path
                            // finds again.
                            Some(Planned::Native(plan)) if plan.generation == self.native_table.generation() => {
                                self.call_planned_native(regs, call, plan, base);
                                pc = pc.wrapping_add(1);
                                continue;
                            }
                            Some(Planned::Jit(plan)) => {
                                tri!(self.call_planned_jit(regs, call, plan, base));
                                pc = pc.wrapping_add(1);
                                continue;
                            }
                            Some(Planned::Offer(weight)) => {
                                self.call_weight = weight;
                                let result = self.run_general_call(regs, call.block, call.index);
                                self.call_weight = 1;
                                tri!(result);
                                pc = pc.wrapping_add(1);
                                continue;
                            }
                            _ => {}
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
                    Op::CopyAt { dst, src, offset, len } => {
                        copy_bytes(src.at(base).add(offset as usize), dst.at(base), len as usize);
                    }
                    Op::FieldRef { dst, src, offset } => wr(base, dst, src.at(base).add(offset as usize)),
                    Op::StaticRef { dst, slot } => {
                        let slot = &bc.statics[slot as usize];
                        let mut ptr = slot.resolved.get();
                        if ptr.is_null() {
                            ptr = self.static_const_with_tydesc(&slot.value, slot.pointee);
                            slot.resolved.set(ptr);
                        }
                        wr(base, dst, ptr);
                    }
                    Op::DivCkU32 { dst, ovf, a, b } => div_checked::<u32>(base, dst, ovf, a, b),
                    Op::DivCkI32 { dst, ovf, a, b } => div_checked::<i32>(base, dst, ovf, a, b),
                    Op::DivCkU64 { dst, ovf, a, b } => div_checked::<u64>(base, dst, ovf, a, b),
                    Op::DivCkI64 { dst, ovf, a, b } => div_checked::<i64>(base, dst, ovf, a, b),

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
                    | Op::Widen { .. } | Op::BinOpRt { .. } | Op::OpAssignRt { .. } | Op::MapGetRt { .. } => {
                        self.run_routine_op(*ops.add(pc), bc, &mut regs.frame, base)
                    }
                    Op::CkAssign { ty, kind, place, b, ovf } => wr(base, ovf, ck_assign(base, ty, kind, place, b)),
                    Op::CkAssignU32I { kind, place, imm, ovf } => {
                        wr(base, ovf, ck_assign_as::<u32>(base, kind, place, imm))
                    }
                    Op::CkAssignBr { ty, kind, place, b, ovf, ok } => {
                        pc = if ck_assign(base, ty, kind, place, b) { ovf } else { ok } as usize;
                        continue;
                    }
                    Op::LoopHead { at } => {
                        let counter = &bc.loops[at as usize];
                        if counter.countdown.tick() && tri!(self.loop_hot(regs, counter, base)) {
                            // Compiled code ran the rest of the call.
                            ret!();
                        }
                    }
                    Op::CkAssignU32IBr { kind, place, imm, ovf, ok } => {
                        pc = if ck_assign_as::<u32>(base, kind, place, imm) { ovf } else { ok } as usize;
                        continue;
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
    /// is pushed and entered; `pc` is where the caller goes on from. Returns
    /// where the callee starts.
    #[inline(always)]
    fn switch_to<'r>(&mut self, regs: &mut Regs<'r>, callee: Callee<'r>, pc: usize) -> usize {
        push_in_place(&mut regs.stack, Activation {
            bc: regs.bc,
            bc_keep: std::mem::replace(&mut regs.bc_keep, callee.keep),
            code_ref: regs.code_ref,
            ctx: callee.ctx.map(|ctx| std::mem::replace(&mut regs.ctx, ctx)),
            base: regs.frame.base_ptr(),
            layout: regs.frame.layout_ptr(),
            ret_dest: regs.ret_dest,
            pc,
            _scratch: callee.scratch,
        });
        regs.bc = callee.bc;
        regs.func = callee.func;
        regs.code_ref = Some(callee.code_ref);
        regs.frame = callee.frame;
        regs.ret_dest = callee.dest;
        // SAFETY: kept alive by `bc_keep` or by the plan the call was made by.
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
        // SAFETY: the top activation, taken off the vector first and then
        // read out of where it was, its two owned fields moved out, so that
        // nothing in it is dropped twice whatever happens after.
        //
        // Field by field, and volatile so that the compiler does not merge
        // the reads into wider ones: the activation was just written field
        // by field, and a load wider than the stores it reads waits for them
        // to reach memory rather than taking their values as they go. Moving
        // it out of the vector whole did that, at about 4% of a call.
        unsafe {
            use std::ptr::{addr_of, read_volatile};
            regs.stack.set_len(len - 1);
            let top = regs.stack.as_ptr().add(len - 1);
            let scratch = std::ptr::read(addr_of!((*top)._scratch));
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
            drop(scratch);
            Some(pc)
        }
    }

    /// The plan for a call, if it has one and it still holds: no body
    /// replaced since it was made, and if a dispatcher is installed, made by
    /// what it said (`CallCache::dispatcher`).
    ///
    /// Within one code epoch no body changes -- bodies are replaced only
    /// between units, and doing so starts a new epoch -- so a call site that
    /// found its callee in this epoch has the callee it would find again.
    ///
    /// Counts the call down, for a site counting calls for the dispatcher.
    #[inline(always)]
    fn valid_plan<'c>(&self, call: &'c FastCall) -> Option<Planned<'c>> {
        // SAFETY: see `FastCall::site`: only the slow path writes it, and
        // nothing holds the plan across the slow path.
        let cache = unsafe { &*call.site.as_ptr() }.callee.as_ref()?;
        if cache.epoch != self.code_epoch {
            return None;
        }
        let dispatched = self.call_dispatcher.borrow().is_some();
        match cache.plan.as_ref()? {
            Plan::Native(plan) => Some(Planned::Native(plan)),
            // With no dispatcher to count for or enter compiled code through,
            // a body is interpreted, and compiled code is left to the slow
            // path, which interprets it.
            Plan::Body(plan) if !dispatched => Some(Planned::Body(plan)),
            Plan::Jit(_) if !dispatched => None,
            _ if cache.dispatcher != Some(self.dispatcher_epoch.get()) => None,
            Plan::Body(plan) => match &plan.countdown {
                None => Some(Planned::Body(plan)),
                // Offered, and the site has to ask again.
                Some(countdown) if countdown.run_out() => None,
                Some(countdown) if countdown.tick() => Some(Planned::Offer(countdown.batch())),
                Some(_) => Some(Planned::Body(plan)),
            },
            Plan::Jit(plan) => Some(Planned::Jit(plan)),
        }
    }

    /// Make a planned call: push the callee's frame, write the arguments and
    /// switch to it. Returns where the callee starts.
    #[inline(always)]
    fn enter_planned<'r>(
        &mut self,
        regs: &mut Regs<'r>,
        call: &FastCall,
        plan: &BodyPlan,
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
        let mut scratch = None;
        match &plan.params {
            PlannedParams::At(params) => for param in params.iter() {
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
            },
            PlannedParams::Recipes(params) => {
                // Allocated only if an argument is read through a wrapper.
                let mut read: crate::BorrowScratch = Vec::new();
                for param in params.iter() {
                    // SAFETY: the recipe was made against the caller's frame,
                    // which `base` is, and `value` and `copy` are places in
                    // the callee's.
                    unsafe {
                        let mut arg = self.arg_value(&param.arg, base, &regs.frame, regs.frames, &mut read);
                        if let Some((at, size)) = param.copy {
                            let copy = callee_base.add(at as usize);
                            copy_bytes(arg.ptr, copy, size as usize);
                            arg.ptr = copy;
                        }
                        (callee_base.add(param.value as usize) as *mut crate::value::Value).write(arg);
                    }
                }
                if !read.is_empty() {
                    scratch = Some(Box::new(read));
                }
            }
        }
        frame.stop_keeping_liveness();
        frame.clear_tracking();
        let callee = Callee {
            frame,
            bc: std::rc::Rc::as_ptr(&plan.bc),
            keep: None,
            // SAFETY: as the frame's layout, and the caller's body holds
            // `code_ref`.
            func: unsafe { &*plan.func },
            code_ref: unsafe { &*plan.code_ref },
            ctx: None,
            // SAFETY: lowered against the caller's frame.
            dest: Destination { ptr: unsafe { call.dest.at(base) }, tydesc: call.dest_tydesc },
            scratch,
        };
        Ok(self.switch_to(regs, callee, resume))
    }

    /// An argument of a planned call, as its recipe reads it from the caller's
    /// frame, `base`.
    ///
    /// # Safety
    ///
    /// The recipe was made against the frame `base` is.
    #[inline(always)]
    unsafe fn arg_value(
        &self,
        recipe: &ArgRecipe,
        base: *mut u8,
        frame: &Frame,
        frames: &FrameStore,
        scratch: &mut crate::BorrowScratch,
    ) -> crate::value::Value {
        let mut arg = match recipe.src {
            // SAFETY: the caller's.
            ArgSource::At(loc, tydesc) => crate::value::Value { ptr: unsafe { loc.at(base) }, tydesc },
            ArgSource::Read(operand) => self.read_operand(&operand, frame, frames),
        };
        if recipe.unwrap {
            arg = IrInterpreter::borrow_through_wrapper(arg, scratch);
        }
        if let Some(tydesc) = recipe.retype {
            arg.tydesc = tydesc;
        }
        if recipe.unwrap_again {
            arg = IrInterpreter::borrow_through_wrapper(arg, scratch);
        }
        arg
    }

    /// Make a planned call to a rider function: its C words straight from the
    /// arguments, and the call.
    #[inline(never)]
    fn call_planned_native(&mut self, regs: &mut Regs<'_>, call: &FastCall, plan: &NativePlan, base: *mut u8) {
        let n = plan.args.len();
        let len = 1 + 2 * n + 2;
        let mut words = [0usize; MAX_C_WORDS];
        let mut scratch: crate::BorrowScratch = Vec::new();
        words[0] = self.runtime.handle() as usize;
        for (i, recipe) in plan.args.iter().enumerate() {
            // SAFETY: the recipe was made against the caller's frame.
            let arg = unsafe { self.arg_value(recipe, base, &regs.frame, regs.frames, &mut scratch) };
            words[1 + 2 * i] = arg.ptr as usize;
            words[2 + 2 * i] = arg.tydesc as usize;
        }
        // SAFETY: the destination was lowered against the caller's frame.
        words[1 + 2 * n] = unsafe { call.dest.at(base) } as usize;
        words[2 + 2 * n] = plan.dest_tydesc as usize;
        // SAFETY: the table registered this as a rider function in the
        // generation the plan was checked against, and holds its code until
        // that changes.
        unsafe { call_c(plan.fn_ptr, &words[..len]) };
    }

    /// Make a planned call into compiled code: the words it is entered with
    /// straight from the arguments, and the call, through the dispatcher.
    #[inline(never)]
    fn call_planned_jit(&mut self, regs: &mut Regs<'_>, call: &FastCall, plan: &JitPlan, base: *mut u8) -> Result<(), InterpError> {
        let mut words = [0usize; crate::dispatch::MAX_ENTRY_WORDS];
        let mut len = 0;
        let mut push = |word: usize| {
            words[len] = word;
            len += 1;
        };
        let mut scratch: crate::BorrowScratch = Vec::new();
        push(self.runtime.handle() as usize);
        if plan.entry.uses_sret {
            // SAFETY: the destination was lowered against the caller's frame.
            push(unsafe { call.dest.at(base) } as usize);
        }
        let mut args = [crate::value::Value { ptr: std::ptr::null_mut(), tydesc: std::ptr::null() };
            crate::dispatch::MAX_ENTRY_WORDS];
        for (i, recipe) in plan.args.iter().enumerate() {
            // SAFETY: the recipe was made against the caller's frame.
            args[i] = unsafe { self.arg_value(recipe, base, &regs.frame, regs.frames, &mut scratch) };
            push(args[i].ptr as usize);
        }
        for &param in plan.descriptor_params.iter() {
            push(args[param as usize].tydesc as usize);
        }

        self.with_dispatcher(&regs.ctx, regs.registry, regs.frames, &[], 1,
                |dispatcher, call_ctx| dispatcher.call_compiled(plan.func, plan.entry, &words[..len], call_ctx))
            .expect("a planned call into compiled code is made with its dispatcher installed")
    }

    /// Ask the dispatcher about a loop whose counter ran out, and go on in
    /// compiled code if it says to. Returns whether it did, in which case the
    /// call is over, its result written.
    ///
    /// The frame is the running one, at the loop's header.
    #[inline(never)]
    fn loop_hot(&mut self, regs: &mut Regs<'_>, counter: &LoopCounter, base: *mut u8) -> Result<bool, InterpError> {
        use crate::dispatch::SitePolicy;
        // How long to go before asking again when there is nothing to ask:
        // long enough to cost nothing, and a dispatcher may yet be installed.
        const UNASKED: u32 = 1 << 16;
        let countdown = &counter.countdown;
        // A script unit's loops stay in the interpreter: only a function has
        // a frame compiled code can carry on in.
        let (Some(code_ref), Some(_)) = (regs.code_ref, regs.func.function_context()) else {
            countdown.set(u32::MAX);
            return Ok(false);
        };
        let func = crate::dispatch::FuncIdentity::of(code_ref, regs.ctx.unit());
        let body = regs.func;
        let policy = self.with_dispatcher(&regs.ctx, regs.registry, regs.frames, &[], 1,
            |dispatcher, call_ctx| dispatcher.loop_policy(func, body, counter.header, countdown.batch(), call_ctx));
        let entry = match policy.transpose()? {
            None => {
                countdown.set(UNASKED);
                return Ok(false);
            }
            Some(SitePolicy::EveryCall) => {
                countdown.set(1);
                return Ok(false);
            }
            Some(SitePolicy::Count(n)) => {
                countdown.set(n);
                return Ok(false);
            }
            Some(SitePolicy::Interpret) => {
                countdown.set(u32::MAX);
                return Ok(false);
            }
            Some(SitePolicy::Enter(entry)) => entry,
        };
        // The next call to run the loop asks again, and is told the same at
        // once.
        countdown.set(1);
        let words = [self.runtime.handle() as usize, regs.ret_dest.ptr as usize, base as usize];
        let words = if entry.uses_sret { &words[..] } else { &[words[0], words[2]][..] };
        self.with_dispatcher(&regs.ctx, regs.registry, regs.frames, &[], 1,
                |dispatcher, call_ctx| dispatcher.call_compiled(func, entry, words, call_ctx))
            .expect("the dispatcher that gave the entry")
            .map(|()| true)
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
        let frames = &mut *regs.frames;
        if !self.execute_hot(instr, &mut regs.frame, frames)
            && !self.execute_warm(instr, &UnitTypes::of(regs.func), &mut regs.frame, frames)
        {
            self.execute_instruction(instr, &mut regs.frame, &regs.ctx, regs.registry, frames)?;
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
        let (func_ref, args, shapes, dest) =
            match &regs.func.blocks[block as usize].instructions[index as usize] {
                Instruction::Call { dest, func: f, args, shape_descriptors, .. }
                | Instruction::ComptimeCall { dest, func: f, args, shape_descriptors, .. } => {
                    (f, args, shape_descriptors, *dest)
                }
                i => unreachable!("Call op on {:?}", i),
            };
        let frames = &mut *regs.frames;
        self.execute_call(func_ref, args, shapes, dest, &mut regs.frame, &regs.ctx, regs.registry, frames)
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
        let frames = &mut *regs.frames;
        // SAFETY: lowered against the running frame, which `base` is.
        match unsafe { self.fast_call(call, base, &mut regs.frame, &regs.ctx, regs.registry, frames, regs.func) }? {
            Called::Done => Ok(None),
            Called::Enter(callee) => Ok(Some(self.switch_to(regs, callee, pc.wrapping_add(1)))),
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
        let frames = &mut *regs.frames;
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
        let frames = &mut *regs.frames;
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
                Op::MapGetRt { at, valid } => {
                    let [d, m, k] = bc.rt[at as usize];
                    let found = self.map_get(&rt_value(frame, base, m), &rt_value(frame, base, k), rt_dest(frame, base, d));
                    wr(base, valid, found);
                }
                Op::BinOpRt { op, at } => {
                    let [d, a, b] = bc.rt[at as usize];
                    let (lhs, rhs) = (rt_value(frame, base, a), rt_value(frame, base, b));
                    self.execute_binop(op, &lhs, &rhs, rt_dest(frame, base, d));
                }
                Op::OpAssignRt { op, at } => {
                    let [p, r, _] = bc.rt[at as usize];
                    let (place, rhs) = (rt_value(frame, base, p), rt_value(frame, base, r));
                    self.execute_op_assign(op, &place, &rhs);
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
                let mut site = call.site.borrow_mut();
                let found = &mut site.module_callee;
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
            let mut site = call.site.borrow_mut();
            let found = &mut site.native;
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

    /// The plan for a call whose arguments suit it and that hands over no
    /// shapes: a body, a rider function, or a forwarder to one.
    ///
    /// Each argument is read as `fast_call` would read it, so the plan makes
    /// the same call: from where the lowering resolved it, or as the IR walker
    /// reads its operand, through a wrapper if it is borrowed; then, for a
    /// forwarder, as the forwarder's own call reads its parameter.
    #[allow(clippy::too_many_arguments)]
    fn make_plan(
        &mut self,
        call: &FastCall,
        ir_args: &[Operand],
        callee: &IrCodeUnit,
        code_ref: &CodeRef,
        callee_ctx: &ExecutionContext,
        registry: &FunctionRegistry,
        layout: Option<&std::rc::Rc<IrLayout>>,
        forward: Option<&std::rc::Rc<BcFunction>>,
        policy: Option<crate::dispatch::SitePolicy>,
    ) -> Option<Plan> {
        use crate::dispatch::SitePolicy;
        // The argument as the call reads it, `mode` being the parameter's.
        let read = |i: usize, mode: ParamMode| match &call.resolved {
            Some(resolved) => ArgRecipe {
                src: ArgSource::At(resolved[i].0, resolved[i].1),
                unwrap: false, retype: None, unwrap_again: false,
            },
            None => ArgRecipe {
                src: ArgSource::Read(ir_args[i]),
                unwrap: matches!(mode, ParamMode::Ref | ParamMode::Mut),
                retype: None, unwrap_again: false,
            },
        };
        let native_plan = |this: &Self, native: &datalove_datafun_ir::NativeContext, args: Box<[ArgRecipe]>, dest_tydesc| {
            let NativeTarget::C(fn_ptr) = this.native_table.lookup(native.symbol()).ok()? else { return None };
            (1 + 2 * args.len() + 2 <= MAX_C_WORDS).then(|| Plan::Native(NativePlan {
                fn_ptr, generation: this.native_table.generation(), args, dest_tydesc,
            }))
        };
        match (layout, forward) {
            // A rider function.
            (None, _) => {
                let CodeUnitContext::Native(native) = &callee.context else { unreachable!("a body without a layout is a native") };
                let mode = |i: usize| native.param_modes.get(i).copied().unwrap_or(ParamMode::In);
                let args = (0..call.args.len()).map(|i| read(i, mode(i))).collect();
                native_plan(self, native, args, call.dest_tydesc)
            }
            // A forwarder: each argument as its frame would hold it -- an owned
            // one with its own descriptor -- then as its call reads that.
            (Some(layout), Some(wrapper)) => {
                let inner = &wrapper.calls[0];
                let Instruction::Call { func: inner_ref, .. } = &callee.blocks[0].instructions[0] else {
                    unreachable!("a forwarder is a call")
                };
                let native_body = Self::resolve_callee(inner, inner_ref, callee_ctx, registry);
                let CodeUnitContext::Native(native) = &native_body.context else {
                    unreachable!("a forwarder calls a native")
                };
                let args = (0..call.args.len()).map(|i| {
                    let mut recipe = read(i, layout.param_modes[i]);
                    if matches!(layout.param_modes[i], ParamMode::In | ParamMode::Out) {
                        recipe.retype = Some(layout.param_tydescs[i]);
                    }
                    match &inner.resolved {
                        Some(resolved) => recipe.retype = Some(resolved[i].1),
                        None => recipe.unwrap_again = matches!(
                            native.param_modes.get(i).copied().unwrap_or(ParamMode::In),
                            ParamMode::Ref | ParamMode::Mut,
                        ),
                    }
                    recipe
                }).collect();
                native_plan(self, native, args, inner.dest_tydesc)
            }
            // A body the dispatcher wants to see every call to.
            (Some(_), None) if matches!(policy, Some(SitePolicy::EveryCall)) => None,
            // A body compiled, entered as the dispatcher would enter it from
            // the call's arguments: each as the caller has it, descriptor and
            // all, which is what a call offered to it is handed.
            (Some(layout), None) if matches!(policy, Some(SitePolicy::Enter(_))) => {
                let Some(SitePolicy::Enter(entry)) = policy else { unreachable!() };
                let args: Box<[ArgRecipe]> = layout.param_modes.iter().enumerate()
                    .map(|(i, mode)| read(i, *mode))
                    .collect();
                let function = callee.function_context().expect("a body with a layout is a function");
                let descriptor_params: Box<[u32]> = function.descriptor_params.iter().map(|p| p.0).collect();
                let width = 1 + entry.uses_sret as usize + args.len() + descriptor_params.len();
                (function.descriptor_shapes.is_empty() && width <= crate::dispatch::MAX_ENTRY_WORDS)
                    .then(|| Plan::Jit(JitPlan {
                        func: crate::dispatch::FuncIdentity::of(code_ref, callee_ctx.unit()),
                        entry, args, descriptor_params,
                    }))
            }
            // A body, whose frame holds an owned argument with its own
            // descriptor and a borrowed one with the argument's.
            (Some(layout), None) => Some(Plan::Body(BodyPlan {
                countdown: match policy {
                    Some(SitePolicy::Count(n)) => Some(Countdown::new(n)),
                    _ => None,
                },
                layout: std::rc::Rc::clone(layout),
                bc: self.bytecode_for(layout, callee),
                func: callee,
                code_ref,
                params: match &call.resolved {
                    Some(resolved) => PlannedParams::At(layout.param_modes.iter().enumerate().map(|(i, mode)| AtParam {
                        src: resolved[i].0,
                        value: layout.param_offsets[i],
                        tydesc: match mode {
                            ParamMode::In | ParamMode::Out => layout.param_tydescs[i],
                            ParamMode::Ref | ParamMode::Mut => resolved[i].1,
                        },
                        copy: layout.param_copies[i],
                    }).collect()),
                    None => PlannedParams::Recipes(layout.param_modes.iter().enumerate().map(|(i, mode)| {
                        let mut arg = read(i, *mode);
                        if matches!(mode, ParamMode::In | ParamMode::Out) {
                            arg.retype = Some(layout.param_tydescs[i]);
                        }
                        PlannedParam { arg, value: layout.param_offsets[i], copy: layout.param_copies[i] }
                    }).collect()),
                },
            })),
        }
    }

    /// Make a fast call, or say the general path has to.
    ///
    /// Does what `execute_call_site` does for a call whose arguments are in
    /// this frame, with no `out` parameter among them and none moved that the
    /// frame would have to record. A call to a body while a dispatcher is
    /// installed is left to the general path, which offers it; what the
    /// dispatcher says about such calls is what the site's plan follows.
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
        // A dispatcher is asked how this site is to make its calls to a body,
        // which the plan made here then follows; this call it is offered.
        let dispatched = self.call_dispatcher.borrow().is_some();
        let dispatcher_epoch = self.dispatcher_epoch.get();
        let Instruction::Call { func: code_ref, args: ir_args, shape_descriptors, .. } =
            &caller.blocks[call.block as usize].instructions[call.index as usize] else {
            unreachable!("a fast call is a call")
        };
        let callee = Self::resolve_callee(call, code_ref, ctx, registry);
        let callee_ctx = ctx.for_callee(code_ref, registry);
        let address = callee as *const IrCodeUnit as usize;
        let (layout, suits, forward) = {
            let mut site = call.site.borrow_mut();
            let cache = &mut site.callee;
            match &*cache {
                Some(c) if c.address == address && c.epoch == self.code_epoch
                    && c.generation == self.native_table.generation()
                    && !(dispatched && c.needs_policy(dispatcher_epoch)) =>
                {
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
                    let plannable = suits && shape_descriptors.is_empty() && !matches!(code_ref, CodeRef::External { .. });
                    let policy = (plannable && dispatched && layout.is_some() && forward.is_none()).then(|| {
                        let identity = crate::dispatch::FuncIdentity::of(code_ref, ctx.unit());
                        self.call_dispatcher.borrow_mut().as_mut()
                            .expect("installed")
                            .site_policy(identity, callee)
                    });
                    let plan = if plannable {
                        self.make_plan(call, ir_args, callee, code_ref, &callee_ctx, registry, layout.as_ref(), forward.as_ref(), policy)
                    } else {
                        None
                    };
                    let fresh = CallCache {
                        address, epoch: self.code_epoch, generation: self.native_table.generation(),
                        layout: layout.clone(), suits, forward: forward.clone(), plan,
                        dispatcher: dispatched.then_some(dispatcher_epoch),
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
        // A body goes to the dispatcher, which a native, or a forwarder to
        // one, never does.
        if !suits || (dispatched && layout.is_some() && forward.is_none()) {
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
        let bc = self.bytecode_for(&layout, callee);
        Ok(Called::Enter(Callee {
            frame: callee_frame,
            bc: std::rc::Rc::as_ptr(&bc),
            keep: Some(bc),
            func: callee,
            code_ref,
            ctx: Some(callee_ctx),
            dest,
            scratch: (!scratch.is_empty()).then(|| Box::new(scratch)),
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
        if matches!(self.engine, crate::Engine::Bytecode | crate::Engine::PlainBytecode)
            && std::env::var_os("DATALOVE_BC_STATS").is_some()
        {
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
