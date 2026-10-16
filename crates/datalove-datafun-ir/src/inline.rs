//! Always-inline: replacing every call to an `inline` function by its body.
//!
//! A function whose body begins with `inline` is never called. Each call to it
//! is replaced by a copy of its body, renumbered into the caller, with the
//! arguments bound to its parameters and every return a jump to where the call
//! was. A callee's own calls to inline functions are expanded before it is
//! copied, so what is spliced holds none, and a cycle of inline functions,
//! which would expand for ever, is an error.
//!
//! Parameters, by mode:
//!
//! - `in`: a fresh value the argument is moved or copied into, as the call
//!   would have done. A move out of a tracked slot marks it moved.
//! - `ref` and `mut`: the argument itself, which is the caller's place. A
//!   store to the parameter becomes the store the place takes -- to a slot, a
//!   parameter, or through a reference, tracked if the place is -- and the
//!   drop before it a drop of the place.
//! - `out`: a fresh tracked slot, which the body writes as it would the
//!   parameter, and whose value is moved into the argument's place where the
//!   call returns. What the place held the caller has destroyed before the
//!   call, as lowering emits for every `out` argument.
//!
//! A generic callee's types are written over its type parameters; the call's
//! bindings are substituted into them, and the caller's descriptor table is
//! worked out again over what it now holds.
//!
//! What a splice leaves -- copies of arguments, jumps, a continuation's block
//! parameter, an option built to be unwrapped -- is cleaned up after
//! (`simplify`), which also puts the blocks back in an order the compiled
//! backends can translate.

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use crate::params::{map_operands_in_instruction, map_operands_in_terminator};
use crate::*;

/// `callee`, with its calls to its own unit's functions named so that they
/// still reach them from code in a later unit.
///
/// A `CodeRef::Local` means a function of whichever unit's code it is in, so a
/// body from script unit `unit` spliced into a later unit's function would
/// call the later unit's function of the same id. Its local calls become
/// `CodeRef::External` calls into `unit` before it moves.
pub fn with_calls_into_unit(callee: &IrCodeUnit, unit: u32) -> IrCodeUnit {
    let mut callee = callee.clone();
    for block in &mut callee.blocks {
        for instr in &mut block.instructions {
            if let Instruction::Call { func, .. } | Instruction::ComptimeCall { func, .. } = instr
                && let CodeRef::Local(id) = *func
            {
                *func = CodeRef::External { unit, id };
            }
        }
    }
    callee
}

/// Replace every call in function body `unit` to an inline function by the
/// function's body.
///
/// `resolve` finds a call's callee as code in `unit` names it if it is
/// inline, with its own calls named as they are from `unit`
/// (`with_calls_into_unit`), and none otherwise.
///
/// Inline functions calling each other in a cycle are refused by typecheck
/// (F085), so expanding one always finishes; meeting a cycle here panics.
pub fn inline_calls(
    unit: &mut IrCodeUnit,
    resolve: &dyn Fn(&CodeRef) -> Option<Arc<IrCodeUnit>>,
) -> bool {
    Inliner::new(resolve).inline(unit)
}

/// Whether `unit` has a call to an inline function, as `resolve` finds them:
/// whether `inline_calls` has anything to do.
pub fn calls_inline(unit: &IrCodeUnit, resolve: &dyn Fn(&CodeRef) -> Option<Arc<IrCodeUnit>>) -> bool {
    unit.blocks.iter().flat_map(|b| &b.instructions).any(|instr| matches!(instr,
        Instruction::Call { func, .. } if resolve(func).is_some()))
}

/// Whether a function is to be inlined wherever it is called.
pub fn is_inline(unit: &IrCodeUnit) -> bool {
    unit.function_context().is_some_and(|ctx| ctx.inline)
}

/// Inlining for any number of functions that see their callees the same way,
/// each callee expanded once for all of them.
pub struct Inliner<'r> {
    resolve: &'r dyn Fn(&CodeRef) -> Option<Arc<IrCodeUnit>>,
    /// Each callee met, expanded if it is inline, none if it is not.
    callees: HashMap<CodeRef, Option<Arc<IrCodeUnit>>>,
    /// The inline functions being expanded, outermost first.
    stack: Vec<CodeRef>,
}

impl<'r> Inliner<'r> {
    /// An inliner finding callees with `resolve`; see `inline_calls`.
    pub fn new(resolve: &'r dyn Fn(&CodeRef) -> Option<Arc<IrCodeUnit>>) -> Self {
        Inliner { resolve, callees: HashMap::default(), stack: Vec::new() }
    }

    /// How this inliner finds callees.
    pub fn resolver(&self) -> &'r dyn Fn(&CodeRef) -> Option<Arc<IrCodeUnit>> {
        self.resolve
    }

    /// Replace every call in function body `unit` to an inline function by
    /// the function's body, and simplify what that leaves. Whether there was
    /// one.
    pub fn inline(&mut self, unit: &mut IrCodeUnit) -> bool {
        if !self.expand(unit) {
            return false;
        }
        crate::simplify::simplify(unit);
        let table = descriptor_table(unit);
        if let CodeUnitContext::Function(ctx) = &mut unit.context {
            ctx.descriptor_table = table;
        }
        true
    }

    /// Splice every call to an inline function in `unit`. Whether there was one.
    fn expand(&mut self, unit: &mut IrCodeUnit) -> bool {
        let mut spliced = false;
        // A splice ends the block at the call and puts what followed it in a
        // new block at the end, which this comes to in turn.
        let mut b = 0;
        while b < unit.blocks.len() {
            if let Some((i, callee)) = self.first_inline_call(unit, b) {
                splice(unit, b, i, &callee);
                spliced = true;
            }
            b += 1;
        }
        spliced
    }

    /// The first call in block `b` to an inline function, and the function
    /// expanded.
    fn first_inline_call(&mut self, unit: &IrCodeUnit, b: usize) -> Option<(usize, Arc<IrCodeUnit>)> {
        unit.blocks[b].instructions.iter().enumerate().find_map(|(i, instr)| match instr {
            Instruction::Call { func, .. } => Some((i, self.inline_callee(func)?)),
            _ => None,
        })
    }

    /// The callee of a call if it is inline, with its own inline calls
    /// expanded.
    fn inline_callee(&mut self, func: &CodeRef) -> Option<Arc<IrCodeUnit>> {
        if let Some(callee) = self.callees.get(func) {
            return callee.clone();
        }
        let Some(callee) = (self.resolve)(func) else {
            self.callees.insert(func.clone(), None);
            return None;
        };
        debug_assert!(is_inline(&callee), "`resolve` found {}, which is not inline", callee.name);
        assert!(!self.stack.contains(func),
            "inline function {} is recursive, which typecheck refuses (F085)", callee.name);
        self.stack.push(func.clone());
        let mut body = (*callee).clone();
        self.expand(&mut body);
        self.stack.pop();
        let body = Arc::new(body);
        self.callees.insert(func.clone(), Some(Arc::clone(&body)));
        Some(body)
    }
}

/// Replace the call at instruction `i` of block `b` of `caller` by `callee`'s
/// body.
fn splice(caller: &mut IrCodeUnit, b: usize, i: usize, callee: &IrCodeUnit) {
    let Instruction::Call { dest, args, bindings, .. } = caller.blocks[b].instructions[i].clone() else {
        unreachable!("a splice is at a call");
    };
    // A generic callee is bound first, and what its body does for a `T` that
    // is not a copy type -- dropping one, cloning one -- is taken out where
    // the binding is one. Left in, a drop of a copy value is harmless to the
    // backends, but once `simplify` has replaced a copy of the caller's value
    // by the value itself, it is a drop of the caller's own, which the
    // interpreter then takes to be gone.
    let bound;
    let (callee, bindings) = if bindings.is_empty() {
        (callee, bindings)
    } else {
        let mut unit = crate::instantiate::substitute_types(callee, &bindings);
        crate::instantiate::clean_copy_types(&mut unit);
        bound = unit;
        (&bound, Vec::new())
    };
    let ctx = callee.function_context().expect("an inline callee is a function");
    let subst = |ty: &IrType| if bindings.is_empty() { ty.clone() } else { ty.substitute(&bindings) };
    let caller_modes: Vec<ParamMode> = caller.function_context()
        .map(|c| c.param_modes.clone())
        .unwrap_or_default();
    let caller_tracked: HashSet<SlotId> = caller.tracked_slots.iter().copied().collect();

    // The callee's ids go after the caller's, and what the splice adds after
    // the callee's: a value per `in` parameter and per `out` write-back, a
    // slot per `out` parameter.
    let value_base = caller.value_count;
    let slot_base = caller.slot_count;
    let block_base = caller.blocks.len() as u32;
    let continuation = BlockId(block_base + callee.blocks.len() as u32);
    // Each parameter adds at most a value and a slot.
    let params = ctx.params.len();
    let mut values: Vec<IrType> = Vec::with_capacity(callee.value_types.len() + params);
    values.extend(callee.value_types.iter().map(subst));
    let mut slots: Vec<IrType> = Vec::with_capacity(callee.slot_types.len() + params);
    slots.extend(callee.slot_types.iter().map(subst));
    let mut tracked: Vec<SlotId> = Vec::with_capacity(callee.tracked_slots.len() + params);
    tracked.extend_from_slice(&callee.tracked_slots);

    // What each parameter becomes: in the callee's numbering for one bound to
    // something the splice adds, in the caller's for one that is the argument.
    let mut in_callee: HashMap<ParamId, Operand> = HashMap::default();
    let mut in_caller: HashMap<ParamId, Operand> = HashMap::default();
    let mut out_slots: HashMap<ParamId, SlotId> = HashMap::default();
    let mut bind: Vec<Instruction> = Vec::with_capacity(params);
    let mut write_back: Vec<Instruction> = Vec::new();
    // The parameters of a copy type, whose stores copy.
    let copy_params: HashSet<ParamId> = ctx.params.iter().copied()
        .filter(|p| subst(&ctx.param_types[p.0 as usize]).is_copy())
        .collect();
    for (k, &param) in ctx.params.iter().enumerate() {
        let arg = args[k].clone();
        let ty = subst(&ctx.param_types[param.0 as usize]);
        match ctx.param_modes[param.0 as usize] {
            ParamMode::In => {
                let v = ValueId(values.len() as u32);
                values.push(ty.clone());
                let dest = ValueId(value_base + v.0);
                bind.push(match &arg {
                    _ if ty.is_copy() => Instruction::Copy { dest, src: arg.clone() },
                    Operand::Slot(s) if caller_tracked.contains(s) => Instruction::SlotLoadMoveTracked { dest, slot: *s },
                    Operand::Slot(s) => Instruction::SlotLoadMove { dest, slot: *s },
                    _ => Instruction::Move { dest, src: arg.clone() },
                });
                in_callee.insert(param, Operand::Value(v));
            }
            ParamMode::Ref | ParamMode::Mut => {
                in_caller.insert(param, arg);
            }
            ParamMode::Out => {
                let copy = ty.is_copy();
                let s = SlotId(slots.len() as u32);
                slots.push(ty.clone());
                tracked.push(s);
                out_slots.insert(param, s);
                in_callee.insert(param, Operand::Slot(s));
                let w = ValueId(values.len() as u32);
                values.push(ty);
                let w = ValueId(value_base + w.0);
                write_back.push(Instruction::SlotLoadMoveTracked { dest: w, slot: SlotId(slot_base + s.0) });
                write_back.push(match &arg {
                    Operand::Slot(t) => slot_store(*t, Operand::Value(w), caller_tracked.contains(t), copy),
                    Operand::Param(q) if caller_modes[q.0 as usize] == ParamMode::Out => {
                        Instruction::ParamStoreTracked { param: *q, value: Operand::Value(w) }
                    }
                    Operand::Param(q) => Instruction::ParamStore { param: *q, value: Operand::Value(w) },
                    Operand::Value(_) | Operand::ValueRef(_) => Instruction::RefStore { dest: arg.clone(), value: Operand::Value(w) },
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                        unreachable!("a function body names no earlier unit's binding")
                    }
                });
            }
        }
    }

    let remap_value = |v: ValueId| ValueId(value_base + v.0);
    let remap_slot = |s: SlotId| SlotId(slot_base + s.0);
    let remap_block = |b: BlockId| BlockId(block_base + b.0);
    // Into the caller's numbering, the `in` and `out` parameters bound to
    // what the splice adds; the `ref` and `mut` ones are left for the
    // arguments.
    let renumbered = |op: &Operand| match op {
        Operand::Param(p) => match in_callee.get(p) {
            Some(Operand::Value(v)) => Operand::Value(remap_value(*v)),
            Some(Operand::Slot(s)) => Operand::Slot(remap_slot(*s)),
            Some(other) => unreachable!("a parameter bound to {other:?}"),
            None => op.clone(),
        },
        Operand::Value(v) => Operand::Value(remap_value(*v)),
        Operand::ValueRef(v) => Operand::ValueRef(remap_value(*v)),
        Operand::Slot(s) => Operand::Slot(remap_slot(*s)),
        Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => op.clone(),
    };
    let argument = |op: &Operand| match op {
        Operand::Param(p) => in_caller.get(p).cloned().unwrap_or_else(|| op.clone()),
        _ => op.clone(),
    };

    let mut blocks: Vec<IrBlock> = Vec::with_capacity(callee.blocks.len() + 1);
    for block in &callee.blocks {
        let instructions = block.instructions.iter().map(|instr| {
            let instr = out_stores(instr, &out_slots, &copy_params);
            let mut instr = map_operands_in_instruction(&instr, renumbered);
            instr.for_each_id_mut(&mut |v| *v = remap_value(*v), &mut |s| *s = remap_slot(*s));
            if let Instruction::Call { bindings: inner, .. } | Instruction::ComptimeCall { bindings: inner, .. } = &mut instr {
                *inner = inner.iter().map(subst).collect();
            }
            through_arguments(&instr, &in_caller, &caller_modes, &caller_tracked, &copy_params, &argument)
                .unwrap_or_else(|| map_operands_in_instruction(&instr, argument))
        }).collect();
        let terminator = match &block.terminator {
            Terminator::Return { value } => Terminator::Goto {
                target: continuation,
                args: match value {
                    Some(op) if subst(&ctx.return_type) != IrType::Unit => vec![argument(&renumbered(op))],
                    _ => vec![],
                },
            },
            Terminator::UnitEnd { .. } | Terminator::UnitEarlyReturn { .. } => {
                unreachable!("a function body ends in returns")
            }
            other => {
                let other = map_operands_in_terminator(other, |op| argument(&renumbered(op)));
                retarget(&other, remap_block)
            }
        };
        blocks.push(IrBlock {
            id: remap_block(block.id),
            params: block.params.iter().map(|v| remap_value(*v)).collect(),
            instructions,
            terminator,
        });
    }

    // The continuation: what followed the call, after the `out` parameters'
    // values are where the caller keeps them.
    let call_block = &mut caller.blocks[b];
    let after: Vec<Instruction> = call_block.instructions.split_off(i + 1);
    call_block.instructions.pop();
    call_block.instructions.extend(bind);
    let after_terminator = std::mem::replace(&mut call_block.terminator, Terminator::Goto {
        target: remap_block(BlockId(0)),
        args: vec![],
    });
    let unit_result = subst(&ctx.return_type) == IrType::Unit;
    let mut cont_instructions = write_back;
    cont_instructions.reserve(after.len() + 1);
    if unit_result {
        cont_instructions.push(Instruction::Const { dest, value: ConstValue::Unit });
    }
    cont_instructions.extend(after);
    blocks.push(IrBlock {
        id: continuation,
        params: if unit_result { vec![] } else { vec![dest] },
        instructions: cont_instructions,
        terminator: after_terminator,
    });

    caller.blocks.extend(blocks);
    caller.value_count += values.len() as u32;
    caller.value_types.extend(values);
    caller.slot_count += slots.len() as u32;
    caller.slot_types.extend(slots);
    caller.tracked_slots.extend(tracked.into_iter().map(remap_slot));
}

/// A store of `value` to slot `slot`: a copy for a copy type, which has
/// nothing dropped before it, and tracked where the slot is.
fn slot_store(slot: SlotId, value: Operand, tracked: bool, copy: bool) -> Instruction {
    let dest = SlotDest::Local(slot);
    match (tracked, copy) {
        (true, true) => Instruction::SlotStoreCopyTracked { dest, value },
        (true, false) => Instruction::SlotStoreMoveTracked { dest, value },
        (false, true) => Instruction::SlotStoreCopy { dest, value },
        (false, false) => Instruction::SlotStoreMove { dest, value },
    }
}

/// A store to an `out` parameter, as a store to the slot standing for it.
fn out_stores(instr: &Instruction, out_slots: &HashMap<ParamId, SlotId>, copy: &HashSet<ParamId>) -> Instruction {
    match instr {
        Instruction::ParamStoreTracked { param, value } | Instruction::ParamStore { param, value }
            if out_slots.contains_key(param) =>
        {
            slot_store(out_slots[param], value.clone(), true, copy.contains(param))
        }
        Instruction::ParamSetFieldTracked { param, field_path, value } | Instruction::ParamSetField { param, field_path, value }
            if out_slots.contains_key(param) =>
        {
            Instruction::SetFieldTracked {
                slot: SlotDest::Local(out_slots[param]),
                field_path: field_path.clone(),
                value: value.clone(),
            }
        }
        _ => instr.clone(),
    }
}

/// A drop of or a store to a `ref` or `mut` parameter, as the instruction
/// that drops or stores to the argument's place: through the reference where
/// the argument is one, and tracked where the place is a tracked slot or the
/// caller's own `out` parameter, so that what the place holds stays recorded.
fn through_arguments(
    instr: &Instruction,
    in_caller: &HashMap<ParamId, Operand>,
    caller_modes: &[ParamMode],
    caller_tracked: &HashSet<SlotId>,
    copy: &HashSet<ParamId>,
    argument: &impl Fn(&Operand) -> Operand,
) -> Option<Instruction> {
    let tracked_param = |q: &ParamId| caller_modes[q.0 as usize] == ParamMode::Out;
    Some(match instr {
        Instruction::Drop { operand: Operand::Param(p) } => match in_caller.get(p)? {
            Operand::Value(r) | Operand::ValueRef(r) => Instruction::DropViaRef { ref_value: *r },
            Operand::Param(q) if tracked_param(q) => Instruction::DropTracked { operand: Operand::Param(*q) },
            other => Instruction::Drop { operand: other.clone() },
        },
        Instruction::ParamStore { param, value } => {
            let value = argument(value);
            match in_caller.get(param)? {
                Operand::Slot(t) => slot_store(*t, value, caller_tracked.contains(t), copy.contains(param)),
                Operand::Param(q) if tracked_param(q) => Instruction::ParamStoreTracked { param: *q, value },
                Operand::Param(q) => Instruction::ParamStore { param: *q, value },
                dest => Instruction::RefStore { dest: dest.clone(), value },
            }
        }
        Instruction::ParamSetField { param, field_path, value } => {
            let (field_path, value) = (field_path.clone(), argument(value));
            match in_caller.get(param)? {
                Operand::Slot(t) if caller_tracked.contains(t) => {
                    Instruction::SetFieldTracked { slot: SlotDest::Local(*t), field_path, value }
                }
                Operand::Slot(t) => Instruction::SetField { slot: SlotDest::Local(*t), field_path, value },
                Operand::Param(q) if tracked_param(q) => {
                    Instruction::ParamSetFieldTracked { param: *q, field_path, value }
                }
                Operand::Param(q) => Instruction::ParamSetField { param: *q, field_path, value },
                dest => Instruction::RefSetField { dest: dest.clone(), field_path, value },
            }
        }
        _ => return None,
    })
}

/// A terminator with its targets renumbered.
pub(crate) fn retarget(term: &Terminator, block: impl Fn(BlockId) -> BlockId) -> Terminator {
    match term {
        Terminator::Goto { target, args } => Terminator::Goto { target: block(*target), args: args.clone() },
        Terminator::Branch { cond, then_block, then_args, else_block, else_args } => Terminator::Branch {
            cond: cond.clone(),
            then_block: block(*then_block),
            then_args: then_args.clone(),
            else_block: block(*else_block),
            else_args: else_args.clone(),
        },
        Terminator::Switch { discriminant, cases, default } => Terminator::Switch {
            discriminant: discriminant.clone(),
            cases: cases.iter().map(|(v, b)| (*v, block(*b))).collect(),
            default: block(*default),
        },
        Terminator::Return { .. } | Terminator::UnitEnd { .. } | Terminator::UnitEarlyReturn { .. } => term.clone(),
    }
}
