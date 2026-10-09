//! The bytecode's optimizations, all of them.
//!
//! Lowering turns each IR instruction into plain ops, one at a time, with no
//! fusing, folding or moving. This pass rewrites the ops each block lowered
//! to, before they are laid out, by the rules below, in the order they run.
//! Nothing outside this module changes what lowering emits, so this catalog
//! is the whole list.
//!
//! A rule here is general: it applies wherever its condition holds, whatever
//! the program, and its condition is a fact about values and the frame -- that
//! a value has one reader, that a location holds a constant -- not about which
//! source construct the ops came from. Each says what it matches, what it
//! needs, and what it saves.
//!
//! 1. `absorb_immediates`: an operand that is a `u32` constant becomes an
//!    immediate in the op, and the constant's op goes once no op reads it.
//! 2. `fuse_branches`: an op whose flag only the block's branch reads becomes
//!    one op with the branch.
//! 3. `fold_copies`: a copy only a `WrapOk` reads is read past.
//! 4. `fuse_returns`: a result wrapped only to be returned is written into the
//!    caller's slot.
//! 5. `hoist_constants`: a constant written in a loop is written once on entry.
//! 6. `duplicate_short_branches`: a jump to a short block ending in a branch
//!    becomes a copy of that block.
//!
//! The pass can be left out, for `Engine::PlainBytecode`, which the engine
//! tests run every fixture on, so that each rule is checked against the ops
//! it replaces.

use rustc_hash::FxHashMap;

use super::{Ck, CkTy, Loc, Op};

/// What the pass may know of the IR, keyed by where values are in the frame,
/// which is all an op names.
pub(super) struct Facts {
    /// Each value of nonzero size, by its offset.
    pub values: FxHashMap<u32, ValueFact>,
    /// Whether each block is in a loop.
    pub in_loop: Vec<bool>,
    /// The block laid out after each block, if there is one.
    pub next: Vec<Option<u32>>,
}

pub(super) struct ValueFact {
    /// How many times the value is read.
    pub uses: u32,
    /// The `u32` it holds, if nothing but its constant writes it.
    pub const_u32: Option<u32>,
    /// Whether something other than its definition may write it.
    pub pinned: bool,
}

impl Facts {
    fn value(&self, loc: Loc) -> Option<&ValueFact> {
        if loc.0 & super::INDIRECT != 0 {
            return None;
        }
        self.values.get(&loc.0)
    }

    /// Whether `loc` is a value read exactly once.
    fn read_once(&self, loc: Loc) -> bool {
        self.value(loc).is_some_and(|v| v.uses == 1)
    }

    fn const_u32(&self, loc: Loc) -> Option<u32> {
        self.value(loc)?.const_u32
    }
}

/// Run every rule, in order, over the blocks, giving the prologue: the ops
/// to run once on entry.
pub(super) fn optimize(blocks: &mut [Vec<Op>], facts: &Facts) -> Vec<Op> {
    absorb_immediates(blocks, facts);
    for b in 0..blocks.len() {
        fuse_branches(&mut blocks[b], b, facts);
        fold_copies(&mut blocks[b], facts);
        fuse_returns(&mut blocks[b], facts);
    }
    let prologue = hoist_constants(blocks, facts);
    duplicate_short_branches(blocks);
    prologue
}

/// 1. A `u32` constant read as an op's second operand becomes an immediate.
///
/// Matches a comparison, a checked add, subtract or multiply, or a checked
/// update, of `u32`s, whose second operand is a value nothing but its constant
/// writes. Saves the operand's load; and once every read of the constant has
/// been absorbed, the constant's own op, which is removed, as is that of a
/// constant nothing reads.
fn absorb_immediates(blocks: &mut [Vec<Op>], facts: &Facts) {
    let mut absorbed: FxHashMap<u32, u32> = FxHashMap::default();
    for op in blocks.iter_mut().flatten() {
        let (b, imm_op): (Loc, fn(Op, u32) -> Op) = match *op {
            Op::CmpU32 { b, .. } => (b, |op, imm| match op {
                Op::CmpU32 { cmp, dst, a, .. } => Op::CmpU32I { cmp, dst, a, imm },
                _ => unreachable!(),
            }),
            Op::AddCkU32 { b, .. } | Op::SubCkU32 { b, .. } | Op::MulCkU32 { b, .. } => (b, |op, imm| match op {
                Op::AddCkU32 { dst, ovf, a, .. } => Op::AddCkU32I { dst, ovf, a, imm },
                Op::SubCkU32 { dst, ovf, a, .. } => Op::SubCkU32I { dst, ovf, a, imm },
                Op::MulCkU32 { dst, ovf, a, .. } => Op::MulCkU32I { dst, ovf, a, imm },
                _ => unreachable!(),
            }),
            Op::CkAssign { ty: CkTy::U32, b, .. } => (b, |op, imm| match op {
                Op::CkAssign { kind, place, ovf, .. } => Op::CkAssignU32I { kind, place, imm, ovf },
                _ => unreachable!(),
            }),
            _ => continue,
        };
        let Some(imm) = facts.const_u32(b) else { continue };
        *op = imm_op(*op, imm);
        *absorbed.entry(b.0).or_default() += 1;
    }
    // Constants every read of which was absorbed, or that nothing reads.
    for block in blocks.iter_mut() {
        block.retain(|op| match *op {
            Op::Const1 { dst, .. } | Op::Const4 { dst, .. } | Op::ConstPool { dst, .. } => {
                let read = facts.value(dst).map_or(0, |v| v.uses);
                absorbed.get(&dst.0).copied().unwrap_or(0) != read
            }
            _ => true,
        });
    }
}

/// 2. The op that writes a flag only the block's branch reads, fused with it.
///
/// Matches a comparison, a checked arithmetic op or update, or a result's
/// unwrap, followed by the branch on what it wrote, where the branch is that
/// value's only reader. Saves the flag's store and load and a dispatch. An
/// unwrap fuses only where its error path falls through into the next block.
fn fuse_branches(block: &mut Vec<Op>, b: usize, facts: &Facts) {
    let [.., prev, Op::BrIf { cond, then, els }] = block.as_slice() else { return };
    let (cond, then, els) = (*cond, *then, *els);
    if !facts.read_once(cond) {
        return;
    }
    let want = cond.0;
    let fused = match *prev {
        Op::UnwrapResult { ok, err, flag, src, at, ok_len, err_len }
            if flag.0 == want
                && err_len as usize == std::mem::size_of::<datalove_rtdt::Error>()
                && facts.next[b] == Some(els) =>
            Op::UnwrapOkBr { ok, err, src, at, ok_len, then },
        Op::CmpU8 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU8 { cmp, a, b, then, els },
        Op::CmpU32 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU32 { cmp, a, b, then, els },
        Op::CmpU32I { cmp, dst, a, imm } if dst.0 == want => Op::BrCmpU32I { cmp, a, imm, then, els },
        Op::CmpI32 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpI32 { cmp, a, b, then, els },
        Op::CmpU64 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpU64 { cmp, a, b, then, els },
        Op::CmpI64 { cmp, dst, a, b } if dst.0 == want => Op::BrCmpI64 { cmp, a, b, then, els },
        Op::AddCkU32 { dst, ovf, a, b } if ovf.0 == want => Op::CkU32Br { kind: Ck::Add, dst, a, b, ovf: then, ok: els },
        Op::SubCkU32 { dst, ovf, a, b } if ovf.0 == want => Op::CkU32Br { kind: Ck::Sub, dst, a, b, ovf: then, ok: els },
        Op::MulCkU32 { dst, ovf, a, b } if ovf.0 == want => Op::CkU32Br { kind: Ck::Mul, dst, a, b, ovf: then, ok: els },
        Op::AddCkU32I { dst, ovf, a, imm } if ovf.0 == want => Op::CkU32IBr { kind: Ck::Add, dst, a, imm, ovf: then, ok: els },
        Op::SubCkU32I { dst, ovf, a, imm } if ovf.0 == want => Op::CkU32IBr { kind: Ck::Sub, dst, a, imm, ovf: then, ok: els },
        Op::MulCkU32I { dst, ovf, a, imm } if ovf.0 == want => Op::CkU32IBr { kind: Ck::Mul, dst, a, imm, ovf: then, ok: els },
        Op::CkAssign { ty, kind, place, b, ovf } if ovf.0 == want => Op::CkAssignBr { ty, kind, place, b, ovf: then, ok: els },
        Op::CkAssignU32I { kind, place, imm, ovf } if ovf.0 == want => Op::CkAssignU32IBr { kind, place, imm, ovf: then, ok: els },
        _ => return,
    };
    block.truncate(block.len() - 2);
    block.push(fused);
}

/// 3. A copy that only the `WrapOk` after it reads, read past.
///
/// Matches a copy into a value read once, by the next op, a `WrapOk`. The
/// copy's source still holds what it copied, so the wrap reads that. Saves
/// the copy.
fn fold_copies(block: &mut Vec<Op>, facts: &Facts) {
    let mut i = 1;
    while i < block.len() {
        if let (Some(copied), Op::WrapOk { dst, src, at, len }) = (copy_of(&block[i - 1]), block[i])
            && copied.0.0 == src.0
            && facts.read_once(src)
        {
            block[i] = Op::WrapOk { dst, src: copied.1, at, len };
            block.remove(i - 1);
            continue;
        }
        i += 1;
    }
}

/// A copy's destination and source.
fn copy_of(op: &Op) -> Option<(Loc, Loc)> {
    match *op {
        Op::Copy1 { dst, src } | Op::Copy4 { dst, src } | Op::Copy8 { dst, src } | Op::CopyN { dst, src, .. } => {
            Some((dst, src))
        }
        _ => None,
    }
}

/// 4. A result wrapped only to be returned, written straight into the caller's
/// slot.
///
/// Matches a `WrapOk` whose result only the `Return` after it reads. Saves the
/// wrap's store and the copy out: copying it afterwards read the narrow
/// stores that had just written its tag and payload back with wide loads,
/// which the processor cannot forward from.
fn fuse_returns(block: &mut Vec<Op>, facts: &Facts) {
    let [.., Op::WrapOk { dst, src, at, len }, Op::Return { src: returned, .. }] = *block.as_slice() else {
        return;
    };
    if returned.0 != dst.0 || !facts.read_once(dst) {
        return;
    }
    block.truncate(block.len() - 2);
    block.push(Op::ReturnOk { src, at, len });
}

/// 5. A constant written in a loop, written once on entry instead.
///
/// Matches a constant's op, in a block that is in a loop, writing a value
/// nothing else writes. Saves its execution at every iteration, for one at
/// every call: outside a loop that would only add work.
fn hoist_constants(blocks: &mut [Vec<Op>], facts: &Facts) -> Vec<Op> {
    let mut prologue = Vec::new();
    for (b, block) in blocks.iter_mut().enumerate() {
        if !facts.in_loop[b] {
            continue;
        }
        block.retain(|op| match *op {
            Op::Const1 { dst, .. } | Op::Const4 { dst, .. } | Op::ConstPool { dst, .. }
                if facts.value(dst).is_some_and(|v| !v.pinned) =>
            {
                prologue.push(*op);
                false
            }
            _ => true,
        });
    }
    prologue
}

/// The most ops a block may have for a jump to it to be replaced by a copy.
const SHORT_BRANCH: usize = 4;

/// 6. A jump to a short block that ends in a branch, replaced by a copy of the
/// block.
///
/// Matches a block ending in a jump to another, of at most `SHORT_BRANCH` ops,
/// that ends in a two-way branch and has nothing in it that a second copy
/// would change the meaning of. The copy runs the same ops in the same order
/// the jump would have reached, so saves the jump's dispatch. The back edge of
/// a loop is the common case: its body then ends in the loop's test rather
/// than a jump back to it.
fn duplicate_short_branches(blocks: &mut [Vec<Op>]) {
    for b in 0..blocks.len() {
        let Some(&Op::Jump { to }) = blocks[b].last() else { continue };
        let target = &blocks[to as usize];
        if to as usize == b || target.len() > SHORT_BRANCH || !target.last().is_some_and(two_way) {
            continue;
        }
        if !target.iter().all(duplicable) {
            continue;
        }
        let copy = target.clone();
        blocks[b].pop();
        blocks[b].extend(copy);
    }
}

/// Whether an op is a branch to two blocks, with nothing falling through.
fn two_way(op: &Op) -> bool {
    matches!(op, Op::BrIf { .. } | Op::BrCmpU8 { .. } | Op::BrCmpU32I { .. } | Op::BrCmpU32 { .. }
        | Op::BrCmpI32 { .. } | Op::BrCmpU64 { .. } | Op::BrCmpI64 { .. } | Op::CkU32Br { .. }
        | Op::CkU32IBr { .. } | Op::CkAssignBr { .. } | Op::CkAssignU32IBr { .. })
}

/// Whether an op means the same wherever it is: not a call, which a call site
/// is cached for, nor an escape to the IR walker, which runs an instruction
/// by its place in the IR, nor one that falls through or leaves the body.
fn duplicable(op: &Op) -> bool {
    !matches!(op, Op::Ir { .. } | Op::Call { .. } | Op::CallFast { .. } | Op::EdgeIr { .. }
        | Op::UnwrapOkBr { .. } | Op::Switch { .. } | Op::Jump { .. }
        | Op::Return { .. } | Op::ReturnOk { .. } | Op::ReturnUnit | Op::ReturnIr { .. })
}
