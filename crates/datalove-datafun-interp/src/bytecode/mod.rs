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

mod call;
mod lower;
mod op;
mod opt;
mod run;
mod stats;
mod verify;

pub(crate) use lower::lower;
use crate::value::Destination;
use datalove_datafun_ir::frame_layout::tracking;
use crate::{copy_bytes, IrInterpreter};

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
    XorU32 { dst: Loc, a: Loc, b: Loc },
    OrU32 { dst: Loc, a: Loc, b: Loc },

    /// Floating point: IEEE arithmetic, as the IR walker does it.
    CmpF32 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },
    CmpF64 { cmp: Cmp, dst: Loc, a: Loc, b: Loc },
    AddF64 { dst: Loc, a: Loc, b: Loc },
    SubF64 { dst: Loc, a: Loc, b: Loc },
    MulF64 { dst: Loc, a: Loc, b: Loc },
    DivF64 { dst: Loc, a: Loc, b: Loc },
    AddF32 { dst: Loc, a: Loc, b: Loc },
    SubF32 { dst: Loc, a: Loc, b: Loc },
    MulF32 { dst: Loc, a: Loc, b: Loc },
    DivF32 { dst: Loc, a: Loc, b: Loc },
    NegF64 { dst: Loc, src: Loc },
    NegF32 { dst: Loc, src: Loc },
    SqrtF64 { dst: Loc, src: Loc },
    SqrtF32 { dst: Loc, src: Loc },
    U64ToF64 { dst: Loc, src: Loc },
    I64ToF64 { dst: Loc, src: Loc },
    F32ToF64 { dst: Loc, src: Loc },
    F64ToF32 { dst: Loc, src: Loc },

    /// Whether `index` is inside the list at `list`.
    ListBoundsCheck { dst: Loc, list: Loc, index: Loc },
    /// Copy element `index` of the list at `list`, `size` bytes, to `dst`, if
    /// the index is in bounds, and say whether it was: a ListGet of an
    /// element type that owns nothing, so a copy is the clone.
    ListGetCopy { dst: Loc, valid: Loc, list: Loc, index: Loc, size: u32 },
    /// Replace element `index` of the list at `list` with the `size` bytes at
    /// `value`, if the index is in bounds, and say whether it was: a ListSet
    /// of an element type that owns nothing, so nothing to destroy either way.
    ListSetCopy { valid: Loc, list: Loc, index: Loc, value: Loc, size: u32 },
    /// Copy `len` bytes, `offset` into element `index` of the list at `list`,
    /// to `dst`: a reference to the element and a read through it.
    ListCopyAt { dst: Loc, list: Loc, index: Loc, size: u32, offset: u16, len: u16 },
    /// Copy `len` bytes from `src` to `offset` into element `index`: a
    /// reference to the element, one to its field, and a store through it.
    ListStoreAt { list: Loc, index: Loc, src: Loc, size: u32, offset: u16, len: u16 },
    /// A reference to element `index` of the list at `list`, whose elements
    /// are `size` bytes.
    ListElementRef { dst: Loc, list: Loc, index: Loc, size: u32 },
    /// The same, for a list whose descriptor is known only at run time,
    /// which value `dest` records what it points at by: `rt[at]` holds the
    /// destination, the list and the index.
    ListElementRefRt { dest: u32, at: u32 },
    /// IR walker routines on resolved operands: `rt[at]` holds the
    /// destination and the source.
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
    /// Store into a tracked slot: copy `len` bytes from `src`, mark it live.
    /// What it held, the `drop.tracked` in front destroyed: a store is pure.
    StoreTracked { dst: Loc, track: u32, src: Loc, len: u32 },
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
    /// `ListBoundsCheck` and the branch that is all that reads it.
    BrListBounds { list: Loc, index: Loc, then: u32, els: u32 },
    BrCmpF32 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    BrCmpF64 { cmp: Cmp, a: Loc, b: Loc, then: u32, els: u32 },
    /// Pass block `block`'s terminator's arguments for its edge `edge` (0 for
    /// a `Goto` or the then side of a `Branch`, 1 for the else side) on the IR
    /// walker, then jump to `to`.
    EdgeIr { block: u32, edge: u32, to: u32 },
    /// Jump through the switch table `table`.
    Switch { disc: Loc, table: u32 },
    Return { src: Loc, len: u32 },
    /// In a generic: copy the value the pointer and
    /// descriptor at `src` name to where the pointer at `dst` says, as many
    /// bytes as the descriptor says. A dynamic value's fat pointer and a
    /// parameter's `Value` are both such a pair.
    CopyFat { dst: Loc, src: Loc },
    /// Return the value the pointer and descriptor at `src` name.
    ReturnFat { src: Loc },
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
    /// Null for a destination of a dynamic type, which is where its fat
    /// pointer says and is read through the frame.
    dest_tydesc: *const rtdt::TyDesc,
    dest_id: datalove_datafun_ir::ValueId,
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
    /// For a generic callee, the descriptors the call hands over:
    /// the callee's type parameters', or for a native, or a forwarder to one,
    /// those the native's signature asks for. Each worked out once.
    descriptors: std::rc::Rc<[crate::BindingRef]>,
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
    /// The callee's type parameters' descriptors.
    descriptors: std::rc::Rc<[crate::BindingRef]>,
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
    /// The descriptors of what the call bound the callee's type parameters
    /// to, entered with after the arguments.
    descriptors: std::rc::Rc<[crate::BindingRef]>,
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
    /// The result's descriptor, as the native's own call would give it; null
    /// where that is dynamic, and the destination's says.
    dest_tydesc: *const rtdt::TyDesc,
    /// What follows the result, for a generic native: the
    /// descriptors the native's signature asks for.
    descriptors: std::rc::Rc<[crate::BindingRef]>,
}

/// How a planned call reads one argument, and what it hands over.
#[derive(Clone, Copy)]
struct ArgRecipe {
    src: ArgSource,
    /// The descriptor to hand over in place of the one read.
    retype: Option<*const rtdt::TyDesc>,
}

/// Where a planned call's argument is.
#[derive(Clone, Copy)]
enum ArgSource {
    /// At a place in the caller's frame, holding what the descriptor says,
    /// as the lowering resolved it.
    At(Loc, *const rtdt::TyDesc),
    /// The operand, read at the call as the IR walker reads it: one whose
    /// type only the running frame says.
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
    /// Each kind of op the bodies lowered to, by name.
    pub op_kinds: std::collections::BTreeSet<String>,
    /// Each kind of IR instruction a body left to the IR walker, by name.
    pub escaped_kinds: std::collections::BTreeSet<String>,
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

/// What the optimization pass may do with an op; see `Op::shape`.
#[derive(Clone, Copy)]
pub(super) struct Shape {
    /// It writes one place, its `dst`, after it has read every operand: what
    /// `coalesce_copies` can point at the place its result is copied to.
    pub one_result: bool,
    /// It ends a block with a branch to one of two others.
    pub two_way: bool,
    /// A second copy of it in another block means the same: not a call, which
    /// a call site is cached for; not an escape, which runs an instruction by
    /// its place in the IR; not one that falls through or leaves the body.
    pub duplicable: bool,
}

impl Shape {
    const ONE_RESULT: Shape = Shape { one_result: true, two_way: false, duplicable: true };
    const TWO_WAY: Shape = Shape { one_result: false, two_way: true, duplicable: true };
    const FIXED: Shape = Shape { one_result: false, two_way: false, duplicable: false };
    const OTHER: Shape = Shape { one_result: false, two_way: false, duplicable: true };
}
