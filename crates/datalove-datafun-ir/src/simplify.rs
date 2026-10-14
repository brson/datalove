//! Cleaning up what inlining leaves: copies, jumps and options that only
//! carried a value from a call's arguments to its body and back.
//!
//! A splice binds each `in` argument by a copy, jumps into the callee's
//! blocks, and passes the result to a continuation as a block argument; a
//! callee returning an option builds one only for the caller to unwrap it
//! again. None of it is wrong, and every engine pays for it. This removes:
//!
//! - **Copies of values that cannot change.** A `Copy` of an SSA value, or of
//!   an `in` or `ref` parameter, is replaced by its source wherever it is used.
//! - **Block parameters with one way in.** A block reached by one edge takes
//!   that edge's arguments in place of its parameters, where they are values
//!   that cannot change.
//! - **Options built to be unwrapped.** A block that only unwraps its one
//!   parameter and branches on whether it held something is bypassed: an edge
//!   passing a `some` goes straight to the branch for something, the payload
//!   moved where the unwrap's would have been, and one passing a `none` to the
//!   other branch.
//! - **Jumps to a block with one way in**, which is merged into the block that
//!   jumps, and **jumps to a block that only jumps on**, which go straight on.
//!
//! Then the blocks are put in reverse postorder and those nothing reaches are
//! dropped. Dead instructions are dead code elimination's.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::params::{map_operands_in_instruction, map_operands_in_terminator};
use crate::*;

/// Clean up `unit`; see the module docs.
pub fn simplify(unit: &mut IrCodeUnit) {
    loop {
        let forwarded = forward_copies(unit);
        let threaded = thread_known_options(unit);
        let substituted = substitute_block_params(unit);
        let merged = merge_blocks(unit);
        let skipped = skip_empty_blocks(unit);
        if !(forwarded || threaded || substituted || merged || skipped) {
            break;
        }
    }
    in_dominance_order(unit);
}

/// Whether reading `op` gives the same answer wherever it is read: an SSA
/// value, or a parameter the function cannot write.
fn unchanging(unit: &IrCodeUnit, op: &Operand) -> bool {
    match op {
        Operand::Value(_) => true,
        Operand::Param(p) => unit.function_context()
            .is_some_and(|ctx| matches!(ctx.param_modes[p.0 as usize], ParamMode::In | ParamMode::Ref)),
        _ => false,
    }
}

/// Values named through a reference somewhere -- as `ValueRef`, or as what a
/// `DropViaRef` drops through -- which only a value can stand in for.
fn named_by_reference(unit: &IrCodeUnit) -> FxHashSet<ValueId> {
    let mut named = FxHashSet::default();
    let mut note = |op: &Operand| {
        if let Operand::ValueRef(v) = op {
            named.insert(*v);
        }
    };
    for block in &unit.blocks {
        for instr in &block.instructions {
            instr.for_each_operand(&mut note);
            if let Instruction::DropViaRef { ref_value } = instr {
                note(&Operand::ValueRef(*ref_value));
            }
        }
        block.terminator.for_each_operand(&mut note);
    }
    named
}

/// Replace each value in `subst` by what it maps to, through chains.
fn substitute(unit: &mut IrCodeUnit, subst: &FxHashMap<ValueId, Operand>) {
    if subst.is_empty() {
        return;
    }
    let resolve = |op: &Operand| -> Operand {
        let mut op = op.clone();
        loop {
            match &op {
                Operand::Value(v) | Operand::ValueRef(v) if subst.contains_key(v) => {
                    let to = subst[v].clone();
                    op = match (&op, to) {
                        (Operand::ValueRef(_), Operand::Value(x)) => Operand::ValueRef(x),
                        (Operand::ValueRef(_), other) => unreachable!("a reference through {other:?}"),
                        (_, to) => to,
                    };
                }
                _ => return op,
            }
        }
    };
    for block in &mut unit.blocks {
        for instr in &mut block.instructions {
            *instr = map_operands_in_instruction(instr, resolve);
            if let Instruction::DropViaRef { ref_value } = instr {
                match resolve(&Operand::Value(*ref_value)) {
                    Operand::Value(x) => *ref_value = x,
                    other => unreachable!("a reference through {other:?}"),
                }
            }
        }
        block.terminator = map_operands_in_terminator(&block.terminator, resolve);
    }
}

/// Copies of values that cannot change, replaced by their sources. Whether
/// any were.
fn forward_copies(unit: &mut IrCodeUnit) -> bool {
    let named = named_by_reference(unit);
    let mut subst = FxHashMap::default();
    for block in &unit.blocks {
        for instr in &block.instructions {
            if let Instruction::Copy { dest, src } = instr
                && unchanging(unit, src)
                && (matches!(src, Operand::Value(_)) || !named.contains(dest))
            {
                subst.insert(*dest, src.clone());
            }
        }
    }
    for block in &mut unit.blocks {
        block.instructions.retain(|instr| !matches!(instr, Instruction::Copy { dest, .. } if subst.contains_key(dest)));
    }
    substitute(unit, &subst);
    !subst.is_empty()
}

/// How many edges come into each block from blocks the entry reaches. A
/// block bypassed or merged away is still in the vector until the end, and
/// its edges no longer count.
fn predecessors(unit: &IrCodeUnit) -> Vec<usize> {
    let mut reached = vec![false; unit.blocks.len()];
    let mut work = vec![0];
    reached[0] = true;
    while let Some(b) = work.pop() {
        for s in unit.blocks[b].terminator.successors() {
            if !reached[s.0 as usize] {
                reached[s.0 as usize] = true;
                work.push(s.0 as usize);
            }
        }
    }
    let mut count = vec![0; unit.blocks.len()];
    for (b, block) in unit.blocks.iter().enumerate() {
        if reached[b] {
            for s in block.terminator.successors() {
                count[s.0 as usize] += 1;
            }
        }
    }
    count
}

/// The arguments an edge from `term` to `target` passes, when it is the only
/// edge between them.
fn edge_args_mut(term: &mut Terminator, target: BlockId) -> Option<&mut Vec<Operand>> {
    match term {
        Terminator::Goto { target: t, args } if *t == target => Some(args),
        Terminator::Branch { then_block, then_args, else_block, else_args, .. } => {
            match (*then_block == target, *else_block == target) {
                (true, false) => Some(then_args),
                (false, true) => Some(else_args),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Block parameters of a block with one way in, replaced by that edge's
/// arguments. Whether any were.
fn substitute_block_params(unit: &mut IrCodeUnit) -> bool {
    let preds = predecessors(unit);
    let named = named_by_reference(unit);
    let mut subst = FxHashMap::default();
    for b in 1..unit.blocks.len() {
        if preds[b] != 1 || unit.blocks[b].params.is_empty() {
            continue;
        }
        let target = unit.blocks[b].id;
        let Some(from) = (0..unit.blocks.len())
            .find(|&p| (p == 0 || preds[p] > 0) && unit.blocks[p].terminator.successors().contains(&target))
        else {
            continue;
        };
        let params = unit.blocks[b].params.clone();
        let mut terminator = unit.blocks[from].terminator.clone();
        let Some(args) = edge_args_mut(&mut terminator, target) else { continue };
        let ok = args.iter().zip(&params).all(|(arg, param)| {
            unchanging(unit, arg) && (matches!(arg, Operand::Value(_)) || !named.contains(param))
        });
        if !ok {
            continue;
        }
        for (param, arg) in params.iter().zip(args.drain(..)) {
            subst.insert(*param, arg);
        }
        unit.blocks[from].terminator = terminator;
        unit.blocks[b].params.clear();
    }
    let any = !subst.is_empty();
    substitute(unit, &subst);
    any
}

/// The instruction defining each value, by block and position.
fn definitions(unit: &IrCodeUnit) -> FxHashMap<ValueId, (usize, usize)> {
    let mut defs = FxHashMap::default();
    for (b, block) in unit.blocks.iter().enumerate() {
        for (i, instr) in block.instructions.iter().enumerate() {
            for v in instr.dests() {
                defs.insert(v, (b, i));
            }
        }
    }
    defs
}

/// How many times each value is read.
fn uses(unit: &IrCodeUnit) -> FxHashMap<ValueId, usize> {
    let mut uses = FxHashMap::default();
    let mut note = |op: &Operand| {
        if let Operand::Value(v) | Operand::ValueRef(v) = op {
            *uses.entry(*v).or_insert(0) += 1;
        }
    };
    for block in &unit.blocks {
        for instr in &block.instructions {
            instr.for_each_operand(&mut note);
            if let Instruction::DropViaRef { ref_value } = instr {
                note(&Operand::Value(*ref_value));
            }
        }
        block.terminator.for_each_operand(&mut note);
    }
    uses
}

/// Blocks that only unwrap the option their one parameter holds and branch
/// on it, bypassed by every edge into them that passes an option built for
/// the occasion. Whether any were.
fn thread_known_options(unit: &mut IrCodeUnit) -> bool {
    let mut any = false;
    for b in 1..unit.blocks.len() {
        let block = &unit.blocks[b];
        let ([param], [Instruction::UnwrapOption { dest, is_some, src: Operand::Value(src) }]) =
            (block.params.as_slice(), block.instructions.as_slice()) else { continue };
        let Terminator::Branch { cond: Operand::Value(cond), then_block, then_args, else_block, else_args } = &block.terminator
            else { continue };
        if src != param || cond != is_some || !then_args.is_empty() || !else_args.is_empty() {
            continue;
        }
        let (target, dest, flag) = (block.id, *dest, *is_some);
        let (then_block, else_block) = (*then_block, *else_block);
        let preds = predecessors(unit);
        let uses = uses(unit);
        let defs = definitions(unit);
        // The payload is defined where the unwrap was, which reached the
        // branch for something by one edge: the one edge passing a `some`
        // can define it instead.
        if preds[then_block.0 as usize] != 1 || uses.get(&flag).copied().unwrap_or(0) != 1 {
            continue;
        }
        // Every edge in, with what it passes, has to be one this can bypass.
        let mut edges = Vec::new();
        for (p, pred) in unit.blocks.iter().enumerate() {
            if (p != 0 && preds[p] == 0) || !pred.terminator.successors().contains(&target) {
                continue;
            }
            let mut terminator = pred.terminator.clone();
            let Some(args) = edge_args_mut(&mut terminator, target) else { edges.clear(); break };
            let [Operand::Value(w)] = args.as_slice() else { edges.clear(); break };
            let Some(&(db, di)) = defs.get(w) else { edges.clear(); break };
            if uses.get(w).copied().unwrap_or(0) != 1 {
                edges.clear();
                break;
            }
            match &unit.blocks[db].instructions[di] {
                Instruction::WrapSome { inner, .. } => edges.push((p, db, di, Some(inner.clone()))),
                Instruction::WrapNone { .. } => edges.push((p, db, di, None)),
                _ => {
                    edges.clear();
                    break;
                }
            }
        }
        if edges.is_empty() || edges.iter().filter(|e| e.3.is_some()).count() > 1 {
            continue;
        }
        let copy = unit.value_types[dest.0 as usize].is_copy();
        for (p, db, di, inner) in edges {
            unit.blocks[db].instructions[di] = match &inner {
                Some(x) if copy => Instruction::Copy { dest, src: x.clone() },
                Some(x) => Instruction::Move { dest, src: x.clone() },
                None => Instruction::Nop,
            };
            let to = if inner.is_some() { then_block } else { else_block };
            let terminator = &mut unit.blocks[p].terminator;
            *terminator = match terminator.clone() {
                Terminator::Goto { .. } => Terminator::Goto { target: to, args: vec![] },
                Terminator::Branch { cond, then_block: t, then_args, else_block: e, else_args } => {
                    let (t, then_args) = if t == target { (to, vec![]) } else { (t, then_args) };
                    let (e, else_args) = if e == target { (to, vec![]) } else { (e, else_args) };
                    Terminator::Branch { cond, then_block: t, then_args, else_block: e, else_args }
                }
                other => unreachable!("an edge with arguments from {other:?}"),
            };
        }
        any = true;
    }
    if any {
        for block in &mut unit.blocks {
            block.instructions.retain(|instr| !matches!(instr, Instruction::Nop));
        }
    }
    any
}

/// Jumps to a block with one way in and no parameters, the block merged into
/// the one jumping. Whether any were.
fn merge_blocks(unit: &mut IrCodeUnit) -> bool {
    let mut any = false;
    let mut preds = predecessors(unit);
    for b in 0..unit.blocks.len() {
        loop {
            let Terminator::Goto { target, args } = &unit.blocks[b].terminator else { break };
            let t = target.0 as usize;
            if t == 0 || t == b || preds[t] != 1 || !args.is_empty() || !unit.blocks[t].params.is_empty() {
                break;
            }
            let instructions = std::mem::take(&mut unit.blocks[t].instructions);
            let terminator = std::mem::replace(&mut unit.blocks[t].terminator, Terminator::Return { value: None });
            // What the merged block went on to, now reached from here once
            // each; what it was is reached from nowhere.
            preds[t] = 0;
            unit.blocks[b].instructions.extend(instructions);
            unit.blocks[b].terminator = terminator;
            any = true;
        }
    }
    any
}

/// Jumps to a block that does nothing but jump on, without arguments, sent
/// straight on. Whether any were.
fn skip_empty_blocks(unit: &mut IrCodeUnit) -> bool {
    // Where each empty block goes on to, through chains of them.
    let mut on: Vec<Option<BlockId>> = unit.blocks.iter().enumerate().map(|(b, block)| match &block.terminator {
        Terminator::Goto { target, args } if b != 0 && block.instructions.is_empty()
            && block.params.is_empty() && args.is_empty() && target.0 as usize != b => Some(*target),
        _ => None,
    }).collect();
    let mut any = false;
    for b in 0..on.len() {
        // Follow the chain, cutting a cycle of empty blocks where it closes.
        let mut seen = vec![b];
        while let Some(next) = on[b] {
            match on[next.0 as usize] {
                Some(further) if !seen.contains(&(next.0 as usize)) => {
                    seen.push(next.0 as usize);
                    on[b] = Some(further);
                }
                _ => break,
            }
        }
    }
    for block in &mut unit.blocks {
        let retargeted = crate::inline::retarget(&block.terminator, |t| match on[t.0 as usize] {
            Some(to) if block_args_empty(&block.terminator, t) => to,
            _ => t,
        });
        if retargeted != block.terminator {
            block.terminator = retargeted;
            any = true;
        }
    }
    any
}

/// Whether every edge from `term` to `target` passes no arguments.
fn block_args_empty(term: &Terminator, target: BlockId) -> bool {
    match term {
        Terminator::Goto { target: t, args } => *t != target || args.is_empty(),
        Terminator::Branch { then_block, then_args, else_block, else_args, .. } => {
            (*then_block != target || then_args.is_empty()) && (*else_block != target || else_args.is_empty())
        }
        _ => true,
    }
}

/// Put `unit`'s blocks in reverse postorder from the entry, renumbered, and
/// drop those nothing reaches.
///
/// In reverse postorder every block comes after the blocks that dominate it,
/// and so after every definition it can see, which is the order lowering
/// produces and the compiled backends translate in.
pub fn in_dominance_order(unit: &mut IrCodeUnit) {
    let n = unit.blocks.len();
    let mut seen = vec![false; n];
    let mut postorder = Vec::with_capacity(n);
    // Each block with how many of its successors have been gone into.
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    seen[0] = true;
    while let Some(&mut (b, ref mut next)) = stack.last_mut() {
        let successors = unit.blocks[b].terminator.successors();
        if let Some(s) = successors.get(*next) {
            *next += 1;
            let s = s.0 as usize;
            if !seen[s] {
                seen[s] = true;
                stack.push((s, 0));
            }
        } else {
            postorder.push(b);
            stack.pop();
        }
    }
    let order: Vec<usize> = postorder.into_iter().rev().collect();
    let mut new_id = vec![BlockId(u32::MAX); n];
    for (at, &b) in order.iter().enumerate() {
        new_id[b] = BlockId(at as u32);
    }
    let mut old: Vec<Option<IrBlock>> = std::mem::take(&mut unit.blocks).into_iter().map(Some).collect();
    unit.blocks = order.iter().map(|&b| {
        let mut block = old[b].take().expect("each block once");
        block.id = new_id[b];
        block.terminator = crate::inline::retarget(&block.terminator, |t| new_id[t.0 as usize]);
        block
    }).collect();
}
