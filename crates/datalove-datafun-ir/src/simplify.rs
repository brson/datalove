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
//! - **Results built to be unwrapped**, the same way: an edge passing an `ok`
//!   goes to the branch for `ok`, one passing an `err` to the other. Where
//!   more than one passes the same, they meet first in a block of their own
//!   that defines what the unwrap did, once.
//! - **Jumps to a block with one way in**, which is merged into the block that
//!   jumps, and **jumps to a block that only jumps on**, which go straight on.
//! - **Drops of values that hold nothing**: a `none` or an empty list, which
//!   the drop would hand to the runtime to destroy nothing.
//!
//! Then the blocks are put in reverse postorder and those nothing reaches are
//! dropped. Dead instructions are dead code elimination's.

use rmx::prelude::*;
use crate::dense::{ValueMap, ValueSet};

use crate::params::{map_operands_in_instruction, map_operands_in_terminator};
use crate::*;

/// Clean up `unit`; see the module docs.
pub fn simplify(unit: &mut IrCodeUnit) {
    loop {
        let forwarded = forward_copies(unit);
        let threaded = thread_known_unwraps(unit);
        let substituted = substitute_block_params(unit);
        let merged = merge_blocks(unit);
        let skipped = skip_empty_blocks(unit);
        let undropped = remove_empty_drops(unit);
        if !(forwarded || threaded || substituted || merged || skipped || undropped) {
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
fn named_by_reference(unit: &IrCodeUnit) -> ValueSet {
    let mut named = ValueSet::with_capacity(unit.value_count);
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
fn substitute(unit: &mut IrCodeUnit, subst: &ValueMap<Operand>) {
    if subst.is_empty() {
        return;
    }
    let resolve = |op: &Operand| -> Operand {
        let mut op = op.C();
        loop {
            match &op {
                Operand::Value(v) | Operand::ValueRef(v) if subst.contains_key(v) => {
                    let to = subst[v].C();
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
    let mut subst = ValueMap::with_capacity(unit.value_count);
    for block in &unit.blocks {
        for instr in &block.instructions {
            if let Instruction::Copy { dest, src } = instr
                && unchanging(unit, src)
                && (matches!(src, Operand::Value(_)) || !named.contains(dest))
            {
                subst.insert(*dest, src.C());
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
    let mut subst = ValueMap::with_capacity(unit.value_count);
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
        let params = unit.blocks[b].params.C();
        let mut terminator = unit.blocks[from].terminator.C();
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
fn definitions(unit: &IrCodeUnit) -> ValueMap<(usize, usize)> {
    let mut defs = ValueMap::with_capacity(unit.value_count);
    for (b, block) in unit.blocks.iter().enumerate() {
        for (i, instr) in block.instructions.iter().enumerate() {
            for v in instr.dests() {
                defs.insert(v, (b, i));
            }
        }
    }
    defs
}

/// Drops of values that hold nothing removed. Whether any were.
pub fn remove_empty_drops(unit: &mut IrCodeUnit) -> bool {
    let empty = empty_drops(unit);
    if empty.is_empty() {
        return false;
    }
    for block in &mut unit.blocks {
        block.instructions.retain(|instr| !matches!(instr, Instruction::Drop { operand: Operand::Value(v) } if empty.contains(v)));
    }
    true
}

/// The values `unit` drops that hold nothing.
///
/// A value holds nothing if it was made as a `none` or an empty list, or moved
/// or copied from one that was, and nothing has written it since. A value is
/// written only where an instruction replaces, changes or lends it: a call
/// never writes one, since only a place can be passed `mut` or `out` (O011,
/// O012), and inlining puts the caller's place where the callee's parameter
/// was.
pub fn empty_drops(unit: &IrCodeUnit) -> ValueSet {
    let mut written = ValueSet::with_capacity(unit.value_count);
    let mut drops = false;
    for instr in unit.blocks.iter().flat_map(|b| &b.instructions) {
        drops |= matches!(instr, Instruction::Drop { operand: Operand::Value(_) });
        instr.for_each_place_use(|op, place_use| {
            if let Operand::Value(v) = op
                && matches!(place_use, PlaceUse::Replaced | PlaceUse::Changed | PlaceUse::Lent)
            {
                written.insert(v);
            }
        });
    }
    if !drops {
        return ValueSet::with_capacity(0);
    }
    let defs = definitions(unit);
    let holds_nothing = |mut v: ValueId| loop {
        if written.contains(&v) {
            return false;
        }
        let Some(&(b, i)) = defs.get(&v) else { return false };
        match &unit.blocks[b].instructions[i] {
            Instruction::WrapNone { .. } => return true,
            Instruction::ListNew { elements, .. } => return elements.is_empty(),
            Instruction::Move { src: Operand::Value(w), .. } | Instruction::Copy { src: Operand::Value(w), .. } => v = *w,
            _ => return false,
        }
    };
    unit.blocks.iter()
        .flat_map(|b| &b.instructions)
        .filter_map(|instr| match instr {
            Instruction::Drop { operand: Operand::Value(v) } if holds_nothing(*v) => Some(*v),
            _ => None,
        })
        .collect()
}

/// How many times each value is read.
fn uses(unit: &IrCodeUnit) -> ValueMap<usize> {
    let mut uses = ValueMap::with_capacity(unit.value_count);
    let mut note = |op: &Operand| {
        if let Operand::Value(v) | Operand::ValueRef(v) = op {
            *uses.get_or_insert_with(*v, || 0) += 1;
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

/// A block that only unwraps the option or result its one parameter holds and
/// branches on whether it holds a value or an `ok`.
struct Unwrap {
    target: BlockId,
    /// Whether it unwraps a result.
    result: bool,
    /// The flag the branch tests.
    flag: ValueId,
    /// Each side: what the unwrap defines for it, and where it goes. An
    /// option's `none` side defines nothing.
    sides: [(Option<ValueId>, BlockId); 2],
}

impl Unwrap {
    fn of(block: &IrBlock) -> Option<Self> {
        let Terminator::Branch { cond: Operand::Value(cond), then_block, then_args, else_block, else_args } = &block.terminator
            else { return None };
        let [param] = block.params.as_slice() else { return None };
        let (src, flag, result, sides) = match block.instructions.as_slice() {
            [Instruction::UnwrapOption { dest, is_some, src }] => {
                (src, *is_some, false, [(Some(*dest), *then_block), (None, *else_block)])
            }
            [Instruction::UnwrapResult { ok_dest, err_dest, is_ok, src }] => {
                (src, *is_ok, true, [(Some(*ok_dest), *then_block), (Some(*err_dest), *else_block)])
            }
            _ => return None,
        };
        let plain = *src == Operand::Value(*param) && *cond == flag && then_args.is_empty() && else_args.is_empty()
            && then_block != else_block;
        plain.then_some(Unwrap { target: block.id, result, flag, sides })
    }

    /// Which side a value `instr` builds lands on, and what it carries there.
    fn side_of(&self, instr: &Instruction) -> Option<(usize, Option<Operand>)> {
        match (self.result, instr) {
            (false, Instruction::WrapSome { inner, .. }) | (true, Instruction::WrapOk { inner, .. }) => Some((0, Some(*inner))),
            (false, Instruction::WrapNone { .. }) => Some((1, None)),
            (true, Instruction::WrapErr { inner, .. }) => Some((1, Some(*inner))),
            _ => None,
        }
    }
}

/// Blocks that only unwrap the option or result their one parameter holds and
/// branch on it, bypassed by every edge into them that passes one built for
/// the occasion. Whether any were.
///
/// Each edge goes to the side what it passes is on. A side's value -- an
/// option's or an `ok`'s payload, an `err` -- is defined where it was built
/// if only one edge passes that side, and otherwise in a block of the side's
/// own where its edges meet, which goes on to the side: an inlined function
/// returning `!T` from more than one place leaves several `err`s and an `ok`
/// meeting at the unwrap.
fn thread_known_unwraps(unit: &mut IrCodeUnit) -> bool {
    let mut any = false;
    for b in 1..unit.blocks.len() {
        let Some(unwrap) = Unwrap::of(&unit.blocks[b]) else { continue };
        let target = unwrap.target;
        let preds = predecessors(unit);
        let uses = uses(unit);
        let defs = definitions(unit);
        // A side's value is defined on the way to its branch, which the
        // unwrap reached by one edge: what reaches it now has to be all that
        // does.
        let one_way_in = |&(dest, to): &(Option<ValueId>, BlockId)| dest.is_none() || preds[to.0 as usize] == 1;
        if !unwrap.sides.iter().all(one_way_in) || uses.get(&unwrap.flag).copied().unwrap_or(0) != 1 {
            continue;
        }
        // Every edge in, with the side it passes and what, has to be one this
        // can bypass.
        let mut edges: Vec<(usize, (usize, usize), usize, Option<Operand>)> = Vec::new();
        for (p, pred) in unit.blocks.iter().enumerate() {
            if (p != 0 && preds[p] == 0) || !pred.terminator.successors().contains(&target) {
                continue;
            }
            let mut terminator = pred.terminator.C();
            let built = edge_args_mut(&mut terminator, target)
                .and_then(|args| match args.as_slice() {
                    [Operand::Value(w)] if uses.get(w).copied().unwrap_or(0) == 1 => defs.get(w).copied(),
                    _ => None,
                })
                .and_then(|(db, di)| Some(((db, di), unwrap.side_of(&unit.blocks[db].instructions[di])?)));
            match built {
                Some((def, (side, inner))) => edges.push((p, def, side, inner)),
                None => {
                    edges.clear();
                    break;
                }
            }
        }
        if edges.is_empty() {
            continue;
        }

        let define = |dest: ValueId, src: Operand, unit: &IrCodeUnit| match unit.value_types[dest.0 as usize].is_copy() {
            true => Instruction::Copy { dest, src },
            false => Instruction::Move { dest, src },
        };
        for (side, &(dest, to)) in unwrap.sides.iter().enumerate() {
            let on_side: Vec<_> = edges.iter().filter(|e| e.2 == side).cloned().collect();
            match (dest, on_side.as_slice()) {
                (_, []) => {}
                (None, edges) => {
                    for (p, (db, di), _, _) in edges {
                        unit.blocks[*db].instructions[*di] = Instruction::Nop;
                        retarget_edge(&mut unit.blocks[*p].terminator, target, to, Vec::new());
                    }
                }
                (Some(dest), [(p, (db, di), _, inner)]) => {
                    let inner = inner.expect("a side with a value is passed one");
                    unit.blocks[*db].instructions[*di] = define(dest, inner, unit);
                    retarget_edge(&mut unit.blocks[*p].terminator, target, to, Vec::new());
                }
                (Some(dest), many) => {
                    let ty = unit.value_types[dest.0 as usize].C();
                    let param = ValueId(unit.value_count);
                    unit.value_count += 1;
                    unit.value_types.push(ty);
                    let landing = BlockId(unit.blocks.len() as u32);
                    let instructions = vec![define(dest, Operand::Value(param), unit)];
                    unit.blocks.push(IrBlock {
                        id: landing,
                        params: vec![param],
                        instructions,
                        terminator: Terminator::Goto { target: to, args: Vec::new() },
                    });
                    for (p, (db, di), _, inner) in many {
                        let inner = inner.expect("a side with a value is passed one");
                        unit.blocks[*db].instructions[*di] = Instruction::Nop;
                        retarget_edge(&mut unit.blocks[*p].terminator, target, landing, vec![inner]);
                    }
                }
            }
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

/// Point the edge of `term` to `from` at `to`, passing `args`.
fn retarget_edge(term: &mut Terminator, from: BlockId, to: BlockId, args: Vec<Operand>) {
    *term = match term.C() {
        Terminator::Goto { .. } => Terminator::Goto { target: to, args },
        Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
            let (t, then_args) = if then_block == from { (to, args.C()) } else { (then_block, then_args) };
            let (e, else_args) = if else_block == from { (to, args) } else { (else_block, else_args) };
            Terminator::Branch { cond, then_block: t, then_args, else_block: e, else_args }
        }
        other => unreachable!("an edge with arguments from {other:?}"),
    };
}

/// Jumps to a block with one way in and no parameters, the block merged into
/// the one jumping. Whether any were.
fn merge_blocks(unit: &mut IrCodeUnit) -> bool {
    let mut any = false;
    let mut preds = predecessors(unit);
    for b in 0..unit.blocks.len() {
        // Only into a block the entry reaches. The edges from one it does not
        // are not counted, so its jump to a block with one way in from where
        // the entry reaches is not that one way, and taking the block into it
        // left the way that is to an empty `return`: a function returning a
        // value returned none.
        if b != 0 && preds[b] == 0 {
            continue;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const A: Operand = Operand::Param(ParamId(0));
    const B: Operand = Operand::Param(ParamId(1));
    const C: Operand = Operand::Param(ParamId(2));
    const D: Operand = Operand::Param(ParamId(3));

    /// A function of four `u32`s and two `bool`s, of these blocks, every value
    /// a `u32`.
    fn function(blocks: Vec<IrBlock>) -> IrCodeUnit {
        let value_count = 32;
        let mut param_types = vec![IrType::U32; 4];
        param_types.extend([IrType::Bool, IrType::Bool]);
        IrCodeUnit {
            id: CodeUnitId(0),
            name: "f".S(),
            blocks,
            value_count,
            slot_count: 0,
            value_types: vec![IrType::U32; value_count as usize],
            slot_types: Vec::new(),
            tracked_slots: Vec::new(),
            const_values: Vec::new(),
            symbols: SymbolTable::new(),
            context: CodeUnitContext::Function(FunctionContext {
                params: (0..6).map(ParamId).collect(),
                param_modes: vec![ParamMode::In; 6],
                param_types,
                return_type: IrType::U32,
                tracked_params: Vec::new(),
                type_param_count: 0,
                descriptor_table: Vec::new(),
                inline: false,
            }),
            nested_units: Vec::new(),
        }
    }

    fn v(n: u32) -> ValueId {
        ValueId(n)
    }

    fn block(id: u32, params: Vec<ValueId>, instructions: Vec<Instruction>, terminator: Terminator) -> IrBlock {
        IrBlock { id: BlockId(id), params, instructions, terminator }
    }

    fn branch(cond: Operand, then_block: u32, else_block: u32) -> Terminator {
        Terminator::Branch {
            cond,
            then_block: BlockId(then_block),
            then_args: Vec::new(),
            else_block: BlockId(else_block),
            else_args: Vec::new(),
        }
    }

    fn goto(target: u32, args: Vec<Operand>) -> Terminator {
        Terminator::Goto { target: BlockId(target), args }
    }

    fn ret(value: Operand) -> Terminator {
        Terminator::Return { value: Some(value) }
    }

    const FLAG_A: Operand = Operand::Param(ParamId(4));
    const FLAG_B: Operand = Operand::Param(ParamId(5));

    /// `if flag_a then some(a) else none`, unwrapped: the payload returned, or
    /// `d`. `uses_flag` reads the unwrap's flag once more.
    fn option_unwrapped(uses_flag: bool) -> IrCodeUnit {
        let then_body = match uses_flag {
            true => vec![Instruction::DebugLog { operand: Operand::Value(v(4)) }],
            false => Vec::new(),
        };
        function(vec![
            block(0, vec![], vec![], branch(FLAG_A, 1, 2)),
            block(1, vec![], vec![Instruction::WrapSome { dest: v(0), inner: A }], goto(3, vec![Operand::Value(v(0))])),
            block(2, vec![], vec![Instruction::WrapNone { dest: v(1) }], goto(3, vec![Operand::Value(v(1))])),
            block(3, vec![v(2)], vec![Instruction::UnwrapOption { dest: v(3), is_some: v(4), src: Operand::Value(v(2)) }],
                branch(Operand::Value(v(4)), 4, 5)),
            block(4, vec![], then_body, ret(Operand::Value(v(3)))),
            block(5, vec![], vec![], ret(D)),
        ])
    }

    #[test]
    fn an_option_built_for_the_unwrap_is_bypassed() {
        let mut unit = option_unwrapped(false);
        assert!(thread_known_unwraps(&mut unit));
        assert_eq!(unit.blocks[1].instructions, vec![Instruction::Copy { dest: v(3), src: A }]);
        assert_eq!(unit.blocks[1].terminator, goto(4, vec![]));
        assert!(unit.blocks[2].instructions.is_empty());
        assert_eq!(unit.blocks[2].terminator, goto(5, vec![]));
    }

    #[test]
    fn a_flag_read_elsewhere_keeps_the_unwrap() {
        let mut unit = option_unwrapped(true);
        assert!(!thread_known_unwraps(&mut unit));
    }

    #[test]
    fn a_side_with_another_way_in_keeps_the_unwrap() {
        let mut unit = option_unwrapped(false);
        unit.blocks[0].terminator = branch(FLAG_A, 1, 6);
        unit.blocks.push(block(6, vec![], vec![], branch(FLAG_B, 2, 4)));
        assert!(!thread_known_unwraps(&mut unit));
    }

    /// `ok(a)`, `err(b)` or `err(c)`, unwrapped: the `ok` returned, or the
    /// `err`.
    fn result_unwrapped() -> IrCodeUnit {
        function(vec![
            block(0, vec![], vec![], branch(FLAG_A, 1, 2)),
            block(1, vec![], vec![Instruction::WrapOk { dest: v(0), inner: A }], goto(5, vec![Operand::Value(v(0))])),
            block(2, vec![], vec![], branch(FLAG_B, 3, 4)),
            block(3, vec![], vec![Instruction::WrapErr { dest: v(1), inner: B }], goto(5, vec![Operand::Value(v(1))])),
            block(4, vec![], vec![Instruction::WrapErr { dest: v(2), inner: C }], goto(5, vec![Operand::Value(v(2))])),
            block(5, vec![v(3)], vec![Instruction::UnwrapResult {
                ok_dest: v(4), err_dest: v(5), is_ok: v(6), src: Operand::Value(v(3)),
            }], branch(Operand::Value(v(6)), 6, 7)),
            block(6, vec![], vec![], ret(Operand::Value(v(4)))),
            block(7, vec![], vec![], ret(Operand::Value(v(5)))),
        ])
    }

    #[test]
    fn the_errs_of_a_result_meet_in_a_block_of_their_own() {
        let mut unit = result_unwrapped();
        assert!(thread_known_unwraps(&mut unit));
        assert_eq!(unit.blocks[1].instructions, vec![Instruction::Copy { dest: v(4), src: A }]);
        assert_eq!(unit.blocks[1].terminator, goto(6, vec![]));
        let landing = unit.blocks.len() as u32 - 1;
        assert_eq!(unit.blocks[3].terminator, goto(landing, vec![B]));
        assert_eq!(unit.blocks[4].terminator, goto(landing, vec![C]));
        let landing = &unit.blocks[landing as usize];
        let [param] = landing.params[..] else { panic!("a landing has one parameter") };
        assert_eq!(landing.instructions, vec![Instruction::Copy { dest: v(5), src: Operand::Value(param) }]);
        assert_eq!(landing.terminator, goto(7, vec![]));
    }

    #[test]
    fn options_with_several_payloads_meet_in_a_block_of_their_own() {
        let mut unit = option_unwrapped(false);
        unit.blocks[2].instructions = vec![Instruction::WrapSome { dest: v(1), inner: B }];
        assert!(thread_known_unwraps(&mut unit));
        let landing = unit.blocks.len() as u32 - 1;
        assert_eq!(unit.blocks[1].terminator, goto(landing, vec![A]));
        assert_eq!(unit.blocks[2].terminator, goto(landing, vec![B]));
    }

    /// Drop `v(1)`, made by `make`, after `between`.
    fn made_then_dropped(make: Instruction, between: Vec<Instruction>) -> IrCodeUnit {
        let mut instructions = vec![make];
        instructions.extend(between);
        instructions.push(Instruction::Drop { operand: Operand::Value(v(1)) });
        function(vec![block(0, vec![], instructions, ret(A))])
    }

    fn drops(unit: &IrCodeUnit) -> usize {
        unit.blocks.iter().flat_map(|b| &b.instructions).filter(|i| matches!(i, Instruction::Drop { .. })).count()
    }

    #[test]
    fn a_dropped_none_is_not_dropped() {
        let read = Instruction::DebugLog { operand: Operand::Value(v(1)) };
        let mut unit = made_then_dropped(Instruction::WrapNone { dest: v(1) }, vec![read]);
        assert!(remove_empty_drops(&mut unit));
        assert_eq!(drops(&unit), 0);
    }

    #[test]
    fn an_empty_list_moved_on_is_not_dropped() {
        let made = Instruction::ListNew { dest: v(0), elements: vec![] };
        let moved = Instruction::Move { dest: v(1), src: Operand::Value(v(0)) };
        let mut unit = made_then_dropped(made, vec![moved]);
        assert!(remove_empty_drops(&mut unit));
        assert_eq!(drops(&unit), 0);
    }

    #[test]
    fn a_list_with_elements_is_dropped() {
        let mut unit = made_then_dropped(Instruction::ListNew { dest: v(1), elements: vec![A] }, vec![]);
        assert!(!remove_empty_drops(&mut unit));
        assert_eq!(drops(&unit), 1);
    }

    #[test]
    fn an_empty_list_written_since_is_dropped() {
        let set = Instruction::ListSet { is_valid: v(2), list: Operand::Value(v(1)), index: B, value: C };
        let mut unit = made_then_dropped(Instruction::ListNew { dest: v(1), elements: vec![] }, vec![set]);
        assert!(!remove_empty_drops(&mut unit));
        assert_eq!(drops(&unit), 1);
    }
}
