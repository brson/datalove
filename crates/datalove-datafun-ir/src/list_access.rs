//! Reaching a list element once: a bounds check, or a reference to an element,
//! that repeats one made on the only way in, with nothing between that could
//! change the list, is replaced by the first.
//!
//! `xs[i]!` is a check, a branch away on failure, and a reference to the
//! element, and a function using `xs[i]` three times does all three three
//! times: each check loads the list's size, each reference its data. Between
//! two of them the element itself may change -- `mem.swap(mut d.next, mut
//! piles[pile]!)` does that -- but not the list: a `mut` borrow of an element
//! reaches the element and nothing else, and no other argument may be rooted
//! at the same binding (D010). So what changes the list is what names it: a
//! store to it, a drop of it, or passing it to a call, as
//! `Instruction::for_each_place_use` says. A move out of it leaves nothing
//! to reach it by until a store fills it again.
//!
//! What is known flows along the edges in reverse postorder, and a block with
//! more than one way in knows what they all agree on -- the same value for
//! the same element, which is then defined on every path to it. A block one of
//! whose ways in has not been seen yet, a loop's header, starts knowing
//! nothing. That covers the chains `!` makes, each check branching to an error
//! exit, and the joins after an `if`. A block
//! reached only when a check passed knows the index is in bounds, and a check
//! repeated there is `true`, its branch a jump. `simplify` then forwards the
//! copies this leaves and drops the error exits nothing reaches.

use std::collections::HashMap;

use crate::*;

/// What is known about one element of one list.
#[derive(Clone, Copy, Default)]
struct Known {
    /// The value a check of the index left.
    check: Option<ValueId>,
    /// Whether the index is known to be in bounds.
    valid: bool,
    /// A reference to the element.
    element: Option<ValueId>,
}

/// An index: an operand, or a constant, which every `xs[0]` writes afresh.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Index {
    Operand(Operand),
    Const(ConstValue),
}

/// An element, by its list and its index.
type Key = (Operand, Index);

/// Replace repeated list checks and element references in `unit` by the first;
/// see the module docs. Whether any were.
pub fn reuse_list_accesses(unit: &mut IrCodeUnit) -> bool {
    if !may_repeat(unit) {
        return false;
    }
    let n = unit.blocks.len();
    debug_assert!(unit.blocks.iter().enumerate().all(|(i, b)| b.id.0 as usize == i), "blocks are numbered in order");
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (b, block) in unit.blocks.iter().enumerate() {
        for s in block.terminator.successors() {
            preds[s.0 as usize].push(b);
        }
    }
    let order = reverse_postorder(unit);
    let consts: HashMap<ValueId, ConstValue> = unit.blocks.iter()
        .flat_map(|b| &b.instructions)
        .filter_map(|i| match i {
            Instruction::Const { dest, value } => Some((*dest, value.clone())),
            _ => None,
        })
        .collect();
    let index_of = |op: &Operand| match op {
        Operand::Value(v) if consts.contains_key(v) => Index::Const(consts[v].clone()),
        other => Index::Operand(*other),
    };
    let mut at_end: Vec<Option<HashMap<Key, Known>>> = vec![None; n];
    let mut changed = false;

    for &b in &order {
        let id = unit.blocks[b].id;
        // What every way in agrees on, where every way in has been seen.
        let incoming: Option<Vec<HashMap<Key, Known>>> = preds[b].iter()
            .map(|&p| at_end[p].as_ref().map(|end| along_edge(end, &unit.blocks[p].terminator, id)))
            .collect();
        let mut known = match incoming {
            Some(states) if !states.is_empty() => agreed(states),
            _ => HashMap::new(),
        };

        let mut trues: Vec<ValueId> = Vec::new();
        let unchanging = |op: &Operand| unchanging_index(unit, op);
        let mut rewritten = Vec::with_capacity(unit.blocks[b].instructions.len());
        for instr in &unit.blocks[b].instructions {
            let instr = match instr {
                Instruction::ListBoundsCheck { is_valid, list, index } if keyable(list) && unchanging(index) => {
                    let k = known.entry((*list, index_of(index))).or_default();
                    if k.valid {
                        trues.push(*is_valid);
                        Instruction::Const { dest: *is_valid, value: ConstValue::Bool(true) }
                    } else if let Some(c) = k.check {
                        Instruction::Copy { dest: *is_valid, src: Operand::Value(c) }
                    } else {
                        k.check = Some(*is_valid);
                        rewritten.push(instr.clone());
                        continue;
                    }
                }
                Instruction::ListElementRef { dest, list, index } if keyable(list) && unchanging(index) => {
                    let k = known.entry((*list, index_of(index))).or_default();
                    match k.element {
                        Some(r) => Instruction::Copy { dest: *dest, src: Operand::Value(r) },
                        None => {
                            k.element = Some(*dest);
                            rewritten.push(instr.clone());
                            continue;
                        }
                    }
                }
                other => {
                    forget_what_it_may_change(&mut known, other);
                    rewritten.push(other.clone());
                    continue;
                }
            };
            changed = true;
            rewritten.push(instr);
        }

        let block = &mut unit.blocks[b];
        block.instructions = rewritten;
        if let Terminator::Branch { cond: Operand::Value(c), then_block, then_args, .. } = &block.terminator
            && trues.contains(c)
        {
            block.terminator = Terminator::Goto { target: *then_block, args: then_args.clone() };
        }
        at_end[b] = Some(known);
    }
    changed
}

/// What is known on the edge from a block ending in `term` to `to`, given
/// what is known at the block's end: an index a check passed is in bounds on
/// the edge taken when it did.
fn along_edge(end: &HashMap<Key, Known>, term: &Terminator, to: BlockId) -> HashMap<Key, Known> {
    let mut known = end.clone();
    if let Terminator::Branch { cond: Operand::Value(c), then_block, else_block, .. } = term
        && *then_block == to && *else_block != to
    {
        for k in known.values_mut().filter(|k| k.check == Some(*c)) {
            k.valid = true;
        }
    }
    known
}

/// What every one of `states` knows alike.
///
/// A value every way into a block knows is defined on every path to it, so
/// its definition dominates the block.
fn agreed(mut states: Vec<HashMap<Key, Known>>) -> HashMap<Key, Known> {
    let mut known = states.pop().expect("at least one way in");
    for other in &states {
        known.retain(|key, k| {
            let Some(o) = other.get(key) else { return false };
            k.check = k.check.filter(|c| o.check == Some(*c));
            k.element = k.element.filter(|e| o.element == Some(*e));
            k.valid &= o.valid;
            k.check.is_some() || k.element.is_some() || k.valid
        });
    }
    known
}

/// Whether `unit` reaches some list element more than once by check or
/// reference, which is the most this pass can find to replace.
pub fn may_repeat(unit: &IrCodeUnit) -> bool {
    let count = unit.blocks.iter()
        .flat_map(|b| &b.instructions)
        .filter(|i| matches!(i, Instruction::ListBoundsCheck { .. } | Instruction::ListElementRef { .. }))
        .count();
    count > 2
}

/// Whether a list operand can name an element across instructions: a place or
/// a value, or a reference, which what writes through references forgets.
fn keyable(list: &Operand) -> bool {
    matches!(list, Operand::Param(_) | Operand::Slot(_) | Operand::Value(_) | Operand::ValueRef(_))
}

/// Whether an index reads the same wherever it is read: a value, or a
/// parameter the function cannot write.
fn unchanging_index(unit: &IrCodeUnit, op: &Operand) -> bool {
    match op {
        Operand::Value(_) => true,
        Operand::Param(p) => unit.function_context()
            .is_some_and(|ctx| matches!(ctx.param_modes[p.0 as usize], ParamMode::In | ParamMode::Ref)),
        _ => false,
    }
}

/// Forget what `instr` may change: every element of a list it writes, drops
/// or passes to a call, and, if it does any of those, every element reached
/// through a reference, which may be the same place.
fn forget_what_it_may_change(known: &mut HashMap<Key, Known>, instr: &Instruction) {
    if known.is_empty() {
        return;
    }
    let mut changed: Vec<Operand> = Vec::new();
    instr.for_each_place_use(|op, place_use| {
        if !matches!(place_use, PlaceUse::Read | PlaceUse::Lent) {
            changed.push(op);
        }
    });
    if !changed.is_empty() {
        known.retain(|(list, _), _| !matches!(list, Operand::ValueRef(_)) && !changed.contains(list));
    }
}

/// The blocks the entry reaches, each after every block that dominates it.
fn reverse_postorder(unit: &IrCodeUnit) -> Vec<usize> {
    let n = unit.blocks.len();
    let mut seen = vec![false; n];
    let mut postorder = Vec::with_capacity(n);
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
    postorder.reverse();
    postorder
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: Operand = Operand::Param(ParamId(0));
    const INDEX: Operand = Operand::Param(ParamId(1));

    /// A function of a `mut` list of `u32` and an `in` index, of these blocks.
    fn function(blocks: Vec<IrBlock>) -> IrCodeUnit {
        let value_count = 32;
        IrCodeUnit {
            id: CodeUnitId(0),
            name: "f".to_string(),
            blocks,
            value_count,
            slot_count: 1,
            value_types: vec![IrType::Bool; value_count as usize],
            slot_types: vec![IrType::Index],
            tracked_slots: Vec::new(),
            const_values: Vec::new(),
            symbols: SymbolTable::new(),
            context: CodeUnitContext::Function(FunctionContext {
                params: vec![ParamId(0), ParamId(1)],
                param_modes: vec![ParamMode::Mut, ParamMode::In],
                param_types: vec![IrType::List(Box::new(IrType::U32)), IrType::Index],
                return_type: IrType::Unit,
                tracked_params: Vec::new(),
                type_param_count: 0,
                descriptor_table: Vec::new(),
                inline: false,
            }),
            nested_units: Vec::new(),
        }
    }

    fn block(id: u32, instructions: Vec<Instruction>, terminator: Terminator) -> IrBlock {
        IrBlock { id: BlockId(id), params: Vec::new(), instructions, terminator }
    }

    fn check(v: u32, index: Operand) -> Instruction {
        Instruction::ListBoundsCheck { is_valid: ValueId(v), list: LIST, index }
    }

    fn element(v: u32, index: Operand) -> Instruction {
        Instruction::ListElementRef { dest: ValueId(v), list: LIST, index }
    }

    fn call(v: u32, args: Vec<Operand>) -> Instruction {
        Instruction::Call { dest: ValueId(v), func: CodeRef::Local(CodeUnitId(1)), args, bindings: Vec::new() }
    }

    fn branch(cond: u32, then_block: u32, else_block: u32) -> Terminator {
        Terminator::Branch {
            cond: Operand::Value(ValueId(cond)),
            then_block: BlockId(then_block),
            then_args: Vec::new(),
            else_block: BlockId(else_block),
            else_args: Vec::new(),
        }
    }

    fn goto(target: u32) -> Terminator {
        Terminator::Goto { target: BlockId(target), args: Vec::new() }
    }

    fn ret() -> Terminator {
        Terminator::Return { value: None }
    }

    /// `xs[i]!`, a call between, and `xs[i]!` again, as `!` lowers them.
    fn twice_around(between: Instruction, index: Operand) -> IrCodeUnit {
        function(vec![
            block(0, vec![check(0, index)], branch(0, 1, 4)),
            block(1, vec![element(1, index), between], goto(2)),
            block(2, vec![check(2, index)], branch(2, 3, 4)),
            block(3, vec![element(3, index)], ret()),
            block(4, Vec::new(), ret()),
        ])
    }

    fn count(unit: &IrCodeUnit, which: fn(&Instruction) -> bool) -> usize {
        unit.blocks.iter().flat_map(|b| &b.instructions).filter(|i| which(i)).count()
    }

    fn checks(unit: &IrCodeUnit) -> usize {
        count(unit, |i| matches!(i, Instruction::ListBoundsCheck { .. }))
    }

    fn elements(unit: &IrCodeUnit) -> usize {
        count(unit, |i| matches!(i, Instruction::ListElementRef { .. }))
    }

    #[test]
    fn an_element_reached_again_is_reached_once() {
        let mut unit = twice_around(Instruction::Nop, INDEX);
        assert!(reuse_list_accesses(&mut unit));
        assert_eq!((checks(&unit), elements(&unit)), (1, 1));
        // The second check passed where the first did, and branches no more.
        assert_eq!(unit.blocks[2].terminator, goto(3));
    }

    #[test]
    fn a_call_handed_the_list_forgets_its_elements() {
        let mut unit = twice_around(call(9, vec![LIST]), INDEX);
        assert!(!reuse_list_accesses(&mut unit));
        assert_eq!((checks(&unit), elements(&unit)), (2, 2));
    }

    #[test]
    fn a_call_handed_an_element_keeps_the_list() {
        let mut unit = twice_around(call(9, vec![Operand::ValueRef(ValueId(1))]), INDEX);
        assert!(reuse_list_accesses(&mut unit));
        assert_eq!((checks(&unit), elements(&unit)), (1, 1));
    }

    #[test]
    fn a_store_to_the_list_forgets_its_elements() {
        let store = Instruction::ParamStore { param: ParamId(0), value: Operand::Value(ValueId(9)) };
        let mut unit = twice_around(store, INDEX);
        assert!(!reuse_list_accesses(&mut unit));
    }

    #[test]
    fn an_index_in_a_slot_is_read_again() {
        let mut unit = twice_around(Instruction::Nop, Operand::Slot(SlotId(0)));
        assert!(!reuse_list_accesses(&mut unit));
    }

    #[test]
    fn a_join_knows_what_its_ways_in_agree_on() {
        let mut unit = function(vec![
            block(0, vec![check(0, INDEX)], branch(0, 1, 4)),
            block(1, vec![element(1, INDEX)], branch(8, 2, 3)),
            block(2, Vec::new(), goto(3)),
            block(3, vec![check(2, INDEX), element(3, INDEX)], ret()),
            block(4, Vec::new(), ret()),
        ]);
        assert!(reuse_list_accesses(&mut unit));
        assert_eq!((checks(&unit), elements(&unit)), (1, 1));
    }

    #[test]
    fn a_join_one_way_in_forgot_at_knows_nothing() {
        let mut unit = function(vec![
            block(0, vec![check(0, INDEX)], branch(0, 1, 4)),
            block(1, vec![element(1, INDEX)], branch(8, 2, 3)),
            block(2, vec![call(9, vec![LIST])], goto(3)),
            block(3, vec![check(2, INDEX), element(3, INDEX)], ret()),
            block(4, Vec::new(), ret()),
        ]);
        assert!(!reuse_list_accesses(&mut unit));
    }

    #[test]
    fn a_loop_header_knows_nothing() {
        let mut unit = function(vec![
            block(0, vec![check(0, INDEX)], branch(0, 1, 3)),
            block(1, vec![check(2, INDEX), element(1, INDEX)], branch(8, 2, 3)),
            block(2, vec![call(9, vec![LIST])], goto(1)),
            block(3, vec![element(3, INDEX)], ret()),
        ]);
        reuse_list_accesses(&mut unit);
        // Block 1 is reached again after the call, and checks again.
        assert!(matches!(unit.blocks[1].instructions[0], Instruction::ListBoundsCheck { .. }));
    }
    #[test]
    fn a_drop_of_the_list_forgets_its_elements() {
        let mut unit = twice_around(Instruction::Drop { operand: LIST }, INDEX);
        assert!(!reuse_list_accesses(&mut unit));
    }

    #[test]
    fn a_walk_over_the_list_keeps_its_elements() {
        let walk = Instruction::CursorBegin { cursor: SlotId(0), coll: LIST };
        let mut unit = twice_around(walk, INDEX);
        assert!(reuse_list_accesses(&mut unit));
    }

    /// `xs[i]!` twice for a list reached through reference `v20`, with
    /// `between` between.
    fn twice_through_a_reference(between: Instruction) -> IrCodeUnit {
        let list = Operand::ValueRef(ValueId(20));
        let check = |v| Instruction::ListBoundsCheck { is_valid: ValueId(v), list, index: INDEX };
        let element = |v| Instruction::ListElementRef { dest: ValueId(v), list, index: INDEX };
        function(vec![
            block(0, vec![check(0)], branch(0, 1, 4)),
            block(1, vec![element(1), between], goto(2)),
            block(2, vec![check(2)], branch(2, 3, 4)),
            block(3, vec![element(3)], ret()),
            block(4, Vec::new(), ret()),
        ])
    }

    #[test]
    fn a_write_through_any_reference_forgets_what_references_reach() {
        let write = Instruction::RefStore { dest: Operand::ValueRef(ValueId(21)), value: Operand::Value(ValueId(9)) };
        let mut unit = twice_through_a_reference(write);
        assert!(!reuse_list_accesses(&mut unit));
    }

    #[test]
    fn a_read_through_a_reference_keeps_what_references_reach() {
        let read = Instruction::Copy { dest: ValueId(9), src: Operand::ValueRef(ValueId(21)) };
        let mut unit = twice_through_a_reference(read);
        assert!(reuse_list_accesses(&mut unit));
    }

    #[test]
    fn equal_constant_indices_are_one_element() {
        let zero = |v| Instruction::Const { dest: ValueId(v), value: ConstValue::Index(0) };
        let at = |v| Operand::Value(ValueId(v));
        let mut unit = function(vec![
            block(0, vec![zero(20), check(0, at(20))], branch(0, 1, 4)),
            block(1, vec![element(1, at(20))], goto(2)),
            block(2, vec![zero(21), check(2, at(21))], branch(2, 3, 4)),
            block(3, vec![element(3, at(21))], ret()),
            block(4, Vec::new(), ret()),
        ]);
        assert!(reuse_list_accesses(&mut unit));
        assert_eq!((checks(&unit), elements(&unit)), (1, 1));
    }
}
