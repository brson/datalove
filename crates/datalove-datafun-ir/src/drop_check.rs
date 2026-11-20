//! A static check that a code unit destroys what it owns exactly once.
//!
//! Invariant 6 of `botdocs/ir.md`: every value of a type that owns something
//! is consumed on every path, once, by an instruction that consumes it or by
//! a drop. The check follows each path through a unit's blocks and keeps what
//! holds a value at each point: the values defined and not yet consumed, the
//! slots and parameters, and what a reference refers to where a drop through
//! it has emptied it. It refuses an instruction that reads or consumes what
//! holds nothing, a store over what still holds a value, two ways into a
//! block that disagree about a place whose state is not tracked at run time,
//! and a way out that leaves anything holding a value it should not.
//!
//! A slot or parameter with a tracking byte may hold a value on one way into
//! a block and not on another; such a place may be dropped through its byte
//! (`drop.tracked`), and nothing else is done to it until it is stored to
//! again.
//!
//! What a reference refers to is followed per reference value, not per place:
//! two references to one element are two places to the check. The fields of a
//! place are followed only as far as a drop of one and the store that fills
//! it again.

use rmx::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::dense::ValueSet;
use crate::simplify::holding_nothing;
use crate::{
    BlockId, CodeRef, CodeUnitContext, Instruction, IrCodeUnit, IrType, Operand, ParamId, ParamMode,
    SlotDest, SlotId, Terminator, ValueId,
};

/// What a slot, a parameter or the referent of a reference holds.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Held {
    /// Nothing.
    Empty,
    /// A value, less the fields at these paths, which a drop emptied.
    Full(BTreeSet<Vec<u32>>),
    /// A value on some ways here and not others: a place with a tracking
    /// byte only.
    Maybe,
}

/// Where a value or a place lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Slot(SlotId),
    Param(ParamId),
    /// What a reference value refers to. Absent from the state where it is
    /// whole, which is wherever the reference was not dropped through.
    Referent(ValueId),
}

/// Whether a value that owns something holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Live {
    Yes,
    /// Defined only where `flag` is `when`: an unwrap's payload or a get's
    /// element. A branch on the flag settles it.
    If { flag: ValueId, when: bool },
    /// Holds a variant that owns nothing, as a switch on its tag found: it
    /// may be dropped or not.
    Nothing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    /// The values that own something and hold it.
    values: BTreeMap<ValueId, Live>,
    places: BTreeMap<Place, Held>,
}

/// Check a code unit, resolving the callees of its calls with `callee`.
///
/// A native has nothing to check. An error says where, and what.
pub fn check_unit<'a>(
    unit: &IrCodeUnit,
    callee: &dyn Fn(&CodeRef) -> Option<&'a IrCodeUnit>,
) -> Result<(), String> {
    if matches!(unit.context, CodeUnitContext::Native(_)) {
        return Ok(());
    }
    Checker { unit, callee, tracked: tracked_places(unit), empty: holding_nothing(unit) }.run()
}

/// The modes of the parameters of a function or native.
pub fn param_modes(unit: &IrCodeUnit) -> &[ParamMode] {
    match &unit.context {
        CodeUnitContext::Function(ctx) => &ctx.param_modes,
        CodeUnitContext::Native(ctx) => &ctx.param_modes,
        CodeUnitContext::Script(_) => panic!("{} is a script unit, which nothing calls", unit.name),
    }
}

fn tracked_places(unit: &IrCodeUnit) -> BTreeSet<Place> {
    let mut tracked: BTreeSet<Place> = unit.tracked_slots.iter().map(|&s| Place::Slot(s)).collect();
    if let Some(ctx) = unit.function_context() {
        tracked.extend(ctx.tracked_params.iter().map(|&p| Place::Param(p)));
    }
    tracked
}

struct Checker<'u, 'a> {
    unit: &'u IrCodeUnit,
    callee: &'u dyn Fn(&CodeRef) -> Option<&'a IrCodeUnit>,
    tracked: BTreeSet<Place>,
    /// The values that hold nothing, a `none` or an empty list, whose drops
    /// `remove_empty_drops` takes away: they may be left holding it.
    empty: ValueSet,
}

type Step = Result<(), String>;

impl Checker<'_, '_> {
    fn run(&self) -> Step {
        let index: BTreeMap<BlockId, usize> =
            self.unit.blocks.iter().enumerate().map(|(i, b)| (b.id, i)).collect();
        let mut entry: BTreeMap<BlockId, State> = BTreeMap::new();
        // Earliest first, which is reverse postorder where the blocks are in
        // it, so that two ways into a block meet before it is followed on.
        let mut work = BTreeSet::from([0]);
        entry.insert(self.unit.blocks[0].id, self.initial());
        while let Some(at) = work.pop_first() {
            let block = &self.unit.blocks[at];
            let id = block.id;
            let mut state = entry[&id].C();
            for (i, instr) in block.instructions.iter().enumerate() {
                self.instruction(&mut state, instr)
                    .map_err(|e| self.error(&format!("block{} instruction {i} `{instr}`", id.0), &e))?;
            }
            let at = format!("block{} `{}`", id.0, block.terminator);
            let edges = self.terminator(state, &block.terminator).map_err(|e| self.error(&at, &e))?;
            for (target, state) in edges {
                let target_index = *index.get(&target)
                    .ok_or_else(|| self.error(&at, &format!("jumps to block{}, which is not there", target.0)))?;
                let params = &self.unit.blocks[target_index].params;
                let mut state = state;
                for &p in params {
                    self.define(&mut state, p).map_err(|e| self.error(&at, &e))?;
                }
                match entry.get(&target) {
                    None => {
                        entry.insert(target, state);
                        work.insert(target_index);
                    }
                    Some(old) => {
                        let joined = self.join(old, &state).map_err(|e| {
                            self.error(&at, &format!("on its way into block{}, {e}", target.0))
                        })?;
                        if &joined != old {
                            entry.insert(target, joined);
                            work.insert(target_index);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn error(&self, at: &str, what: &str) -> String {
        let mut s = format!("{}: at {at}: {what}\n", self.unit.name);
        write!(s, "{}", self.unit).X();
        s
    }

    fn initial(&self) -> State {
        let mut state = State::default();
        if let Some(ctx) = self.unit.function_context() {
            for ((&p, mode), ty) in ctx.params.iter().zip(&ctx.param_modes).zip(&ctx.param_types) {
                if ty.is_copy() {
                    continue;
                }
                match mode {
                    ParamMode::In | ParamMode::Mut => {
                        state.places.insert(Place::Param(p), Held::Full(BTreeSet::new()));
                    }
                    ParamMode::Out => {
                        state.places.insert(Place::Param(p), Held::Empty);
                    }
                    ParamMode::Ref => {}
                }
            }
        }
        for s in 0..self.unit.slot_count {
            if !self.slot_type(SlotId(s)).is_copy() {
                state.places.insert(Place::Slot(SlotId(s)), Held::Empty);
            }
        }
        state
    }

    fn value_type(&self, v: ValueId) -> &IrType {
        &self.unit.value_types[v.0 as usize]
    }

    fn slot_type(&self, s: SlotId) -> &IrType {
        &self.unit.slot_types[s.0 as usize]
    }

    fn param_mode(&self, p: ParamId) -> ParamMode {
        let ctx = self.unit.function_context().expect("a parameter outside a function");
        ctx.param_modes[p.0 as usize]
    }

    fn param_type(&self, p: ParamId) -> &IrType {
        let ctx = self.unit.function_context().expect("a parameter outside a function");
        &ctx.param_types[p.0 as usize]
    }

    /// The type of what a reference value refers to.
    fn referent_type(&self, v: ValueId) -> Result<IrType, String> {
        match self.value_type(v) {
            IrType::Ref(inner) => Ok(inner.held()),
            other => Err(format!("v{} is used as a reference, and is a {other}", v.0)),
        }
    }

    /// The place an operand names, with the type it holds, or `None` for
    /// what this unit does not own: a copy, an earlier unit's, or a `ref`
    /// parameter.
    fn place(&self, op: &Operand) -> Result<Option<(Place, IrType)>, String> {
        let (place, ty) = match op {
            Operand::Slot(s) => (Place::Slot(*s), self.slot_type(*s).C()),
            Operand::Param(p) => {
                if self.param_mode(*p) == ParamMode::Ref {
                    return Ok(None);
                }
                (Place::Param(*p), self.param_type(*p).C())
            }
            Operand::ValueRef(v) => (Place::Referent(*v), self.referent_type(*v)?),
            Operand::Value(v) => match self.value_type(*v) {
                IrType::Ref(_) => (Place::Referent(*v), self.referent_type(*v)?),
                other => return Err(format!("v{} is used as a place, and is a {other}", v.0)),
            },
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => return Ok(None),
        };
        Ok((!ty.is_copy()).then_some((place, ty)))
    }

    fn held<'s>(&self, state: &'s State, place: Place) -> std::borrow::Cow<'s, Held> {
        match (state.places.get(&place), place) {
            (Some(held), _) => std::borrow::Cow::Borrowed(held),
            (None, Place::Referent(_)) => std::borrow::Cow::Owned(Held::Full(BTreeSet::new())),
            (None, _) => panic!("{place:?} is not followed"),
        }
    }

    fn set(&self, state: &mut State, place: Place, held: Held) {
        if matches!(place, Place::Referent(_)) && held == Held::Full(BTreeSet::new()) {
            state.places.remove(&place);
        } else {
            state.places.insert(place, held);
        }
    }

    fn define(&self, state: &mut State, v: ValueId) -> Step {
        if self.value_type(v).is_copy() {
            return Ok(());
        }
        if state.values.contains_key(&v) {
            return Err(format!("v{} is defined again while it still holds a value", v.0));
        }
        state.values.insert(v, Live::Yes);
        Ok(())
    }

    fn define_if(&self, state: &mut State, v: ValueId, flag: ValueId, when: bool) -> Step {
        self.define(state, v)?;
        if let Some(live) = state.values.get_mut(&v) {
            *live = Live::If { flag, when };
        }
        Ok(())
    }

    fn whole(&self, state: &State, place: Place, what: &str) -> Step {
        match &*self.held(state, place) {
            Held::Full(holes) if holes.is_empty() => Ok(()),
            Held::Full(holes) => Err(format!("{what} {} with its fields at {holes:?} emptied", show(place))),
            Held::Empty => Err(format!("{what} {}, which holds nothing", show(place))),
            Held::Maybe => Err(format!("{what} {}, which may hold nothing", show(place))),
        }
    }

    fn read(&self, state: &State, op: &Operand) -> Step {
        if let Operand::Value(v) = op {
            return match (self.value_type(*v).is_copy(), state.values.get(v)) {
                (true, _) | (false, Some(Live::Yes | Live::Nothing)) => Ok(()),
                (false, Some(Live::If { flag, .. })) => {
                    Err(format!("reads v{} before a branch on v{} says it holds anything", v.0, flag.0))
                }
                (false, None) => Err(format!("reads v{}, which holds nothing", v.0)),
            };
        }
        match self.place(op)? {
            Some((place, _)) => self.whole(state, place, "reads"),
            None => Ok(()),
        }
    }

    fn consume(&self, state: &mut State, op: &Operand) -> Step {
        if let Operand::Value(v) = op {
            if !self.value_type(*v).is_copy() {
                self.read(state, op)?;
                state.values.remove(v);
            }
            return Ok(());
        }
        if let Some((place, _)) = self.place(op)? {
            self.whole(state, place, "consumes")?;
            self.set(state, place, Held::Empty);
        }
        Ok(())
    }

    /// Fill an empty place: a store, or an `out` argument.
    fn fill(&self, state: &mut State, op: &Operand) -> Step {
        if let Some((place, _)) = self.place(op)? {
            match &*self.held(state, place) {
                Held::Empty => {}
                Held::Full(_) => return Err(format!("stores to {}, which still holds a value", show(place))),
                Held::Maybe => return Err(format!("stores to {}, which may still hold a value", show(place))),
            }
            self.set(state, place, Held::Full(BTreeSet::new()));
        }
        Ok(())
    }

    /// Fill the field at `path` of a place, emptied by a drop if its type
    /// owns anything.
    fn fill_field(&self, state: &mut State, op: &Operand, path: &[u32]) -> Step {
        let Some((place, ty)) = self.place(op)? else { return Ok(()) };
        let field = field_type(&ty, path)?;
        let Held::Full(mut holes) = self.held(state, place).into_owned() else {
            return Err(format!("stores to a field of {}, which does not hold a value", show(place)));
        };
        if !holes.remove(path) {
            if field.is_copy() {
                return Ok(());
            }
            return Err(format!("stores to the field {path:?} of {}, which still holds a value", show(place)));
        }
        self.set(state, place, Held::Full(holes));
        Ok(())
    }

    fn drop_field(&self, state: &mut State, op: &Operand, path: &[u32]) -> Step {
        let Some((place, ty)) = self.place(op)? else { return Ok(()) };
        if field_type(&ty, path)?.is_copy() {
            return Err(format!("drops the field {path:?} of {}, which owns nothing", show(place)));
        }
        let Held::Full(mut holes) = self.held(state, place).into_owned() else {
            return Err(format!("drops a field of {}, which does not hold a value", show(place)));
        };
        if let Some(hole) = holes.iter().find(|h| h.starts_with(path) || path.starts_with(h)) {
            return Err(format!("drops the field {path:?} of {}, whose field {hole:?} is empty", show(place)));
        }
        holes.insert(path.to_vec());
        self.set(state, place, Held::Full(holes));
        Ok(())
    }

    fn drop_tracked(&self, state: &mut State, op: &Operand) -> Step {
        let Some((place, _)) = self.place(op)? else { return Ok(()) };
        if !self.tracked.contains(&place) {
            return Err(format!("drops {} through a tracking byte it does not have", show(place)));
        }
        if let Held::Full(holes) = &*self.held(state, place) {
            if !holes.is_empty() {
                return Err(format!("drops {}, with its fields at {holes:?} emptied", show(place)));
            }
        }
        self.set(state, place, Held::Empty);
        Ok(())
    }

    fn call(&self, state: &mut State, func: &CodeRef, args: &[Operand], dest: ValueId) -> Step {
        let callee = (self.callee)(func).ok_or_else(|| format!("calls {func:?}, which is not there"))?;
        let modes = param_modes(callee);
        if modes.len() != args.len() {
            return Err(format!("passes {} arguments to {}, which takes {}", args.len(), callee.name, modes.len()));
        }
        for (arg, mode) in args.iter().zip(modes) {
            match mode {
                ParamMode::In => self.consume(state, arg)?,
                ParamMode::Ref | ParamMode::Mut => self.read(state, arg)?,
                ParamMode::Out => self.fill(state, arg)?,
            }
        }
        self.define(state, dest)
    }

    fn instruction(&self, state: &mut State, instr: &Instruction) -> Step {
        use Instruction as I;
        match instr {
            I::Const { dest, .. } | I::StaticRef { dest, .. } | I::WrapNone { dest } => self.define(state, *dest),
            I::Copy { dest, src } | I::Widen { dest, src } | I::WidenFixed { dest, src }
            | I::Clone { dest, src } | I::GetField { dest, src, .. } | I::GetFieldRef { dest, src, .. }
            | I::EnumDiscriminant { dest, src } | I::UnaryOp { dest, operand: src, .. } => {
                self.read(state, src)?;
                self.define(state, *dest)
            }
            I::Move { dest, src } | I::WrapSome { dest, inner: src } | I::WrapOk { dest, inner: src }
            | I::WrapErr { dest, inner: src } | I::EnumPayload { dest, src, .. }
            | I::ErrorFrom { dest, inner: src } | I::DataFrom { dest, inner: src } => {
                self.consume(state, src)?;
                self.define(state, *dest)
            }
            I::EnumVariant { dest, payload, .. } => {
                if let Some(payload) = payload {
                    self.consume(state, payload)?;
                }
                self.define(state, *dest)
            }
            I::BinOp { dest, lhs, rhs, .. } => {
                self.read(state, lhs)?;
                self.read(state, rhs)?;
                self.define(state, *dest)
            }
            I::BinOpChecked { dest, overflow, lhs, rhs, .. } => {
                self.read(state, lhs)?;
                self.read(state, rhs)?;
                self.define(state, *overflow)?;
                self.define_if(state, *dest, *overflow, false)
            }
            I::UnaryOpChecked { dest, overflow, operand, .. } => {
                self.read(state, operand)?;
                self.define(state, *overflow)?;
                self.define_if(state, *dest, *overflow, false)
            }
            I::OpAssign { place, rhs, .. } => {
                self.read(state, place)?;
                self.read(state, rhs)
            }
            I::OpAssignChecked { overflow, place, rhs, .. } => {
                self.read(state, place)?;
                self.read(state, rhs)?;
                self.define(state, *overflow)
            }
            I::Call { dest, func, args, .. } | I::ComptimeCall { dest, func, args, .. } => {
                self.call(state, func, args, *dest)
            }
            I::Pack { dest, fields: ops, .. } | I::ListNew { dest, elements: ops }
            | I::SetNew { dest, elements: ops } | I::TensorNew { dest, elements: ops, .. }
            | I::TableNew { dest, rows: ops } => {
                for op in ops {
                    self.consume(state, op)?;
                }
                self.define(state, *dest)
            }
            I::MapNew { dest, entries } => {
                for (k, v) in entries {
                    self.consume(state, k)?;
                    self.consume(state, v)?;
                }
                self.define(state, *dest)
            }
            I::Unpack { dests, src } => {
                self.consume(state, src)?;
                dests.iter().try_for_each(|d| self.define(state, *d))
            }
            I::UnwrapOption { dest, is_some, src } => {
                self.consume(state, src)?;
                self.define(state, *is_some)?;
                self.define_if(state, *dest, *is_some, true)
            }
            I::UnwrapResult { ok_dest, err_dest, is_ok, src } => {
                self.consume(state, src)?;
                self.define(state, *is_ok)?;
                self.define_if(state, *ok_dest, *is_ok, true)?;
                self.define_if(state, *err_dest, *is_ok, false)
            }
            I::ListGet { dest, is_valid, list: coll, index }
            | I::MapGet { dest, is_valid, map: coll, key: index }
            | I::TensorGet { dest, is_valid, tensor: coll, index } => {
                self.read(state, coll)?;
                self.read(state, index)?;
                self.define(state, *is_valid)?;
                self.define_if(state, *dest, *is_valid, true)
            }
            I::ListBoundsCheck { is_valid, list: coll, index }
            | I::MapContainsKey { is_valid, map: coll, key: index }
            | I::TensorBoundsCheck { is_valid, tensor: coll, index } => {
                self.read(state, coll)?;
                self.read(state, index)?;
                self.define(state, *is_valid)
            }
            I::ListElementRef { dest, list: coll, index }
            | I::MapValueRef { dest, map: coll, key: index }
            | I::TensorIndexRef { dest, tensor: coll, index } => {
                self.read(state, coll)?;
                self.read(state, index)?;
                self.define(state, *dest)
            }
            I::ListSet { is_valid, list: coll, index, value }
            | I::MapSetValue { is_valid, map: coll, key: index, value }
            | I::TensorSet { is_valid, tensor: coll, index, value } => {
                self.read(state, coll)?;
                self.read(state, index)?;
                self.consume(state, value)?;
                self.define(state, *is_valid)
            }
            I::MapUpsert { map, key, value } => {
                self.read(state, map)?;
                self.consume(state, key)?;
                self.consume(state, value)
            }
            I::SlotStoreCopy { dest, value } | I::SlotStoreCopyTracked { dest, value } => {
                self.read(state, value)?;
                self.fill(state, &slot_operand(dest))
            }
            I::SlotStoreMove { dest, value } | I::SlotStoreMoveTracked { dest, value } => {
                self.consume(state, value)?;
                self.fill(state, &slot_operand(dest))
            }
            I::SetField { slot, field_path, value } | I::SetFieldTracked { slot, field_path, value } => {
                self.consume(state, value)?;
                self.fill_field(state, &slot_operand(slot), field_path)
            }
            I::ParamStore { param, value } | I::ParamStoreTracked { param, value } => {
                self.consume(state, value)?;
                self.fill(state, &Operand::Param(*param))
            }
            I::ParamSetField { param, field_path, value } | I::ParamSetFieldTracked { param, field_path, value } => {
                self.consume(state, value)?;
                self.fill_field(state, &Operand::Param(*param), field_path)
            }
            I::RefStore { dest, value } | I::RefStoreTracked { dest, value } => {
                self.consume(state, value)?;
                self.fill(state, dest)
            }
            I::RefSetField { dest, field_path, value } | I::RefSetFieldTracked { dest, field_path, value } => {
                self.consume(state, value)?;
                self.fill_field(state, dest, field_path)
            }
            I::SlotLoadCopy { dest, slot } => {
                self.read(state, &Operand::Slot(*slot))?;
                self.define(state, *dest)
            }
            I::SlotLoadMove { dest, slot } | I::SlotLoadMoveTracked { dest, slot } => {
                self.consume(state, &Operand::Slot(*slot))?;
                self.define(state, *dest)
            }
            I::Drop { operand } | I::UnitEndDrop { operand } => self.consume(state, operand),
            I::DropTracked { operand } | I::UnitEndDropTracked { operand } => self.drop_tracked(state, operand),
            I::DropViaRef { ref_value } => self.consume(state, &Operand::ValueRef(*ref_value)),
            I::DropField { base, field_path } => self.drop_field(state, base, field_path),
            I::CursorBegin { coll, .. } | I::CursorAdvance { coll, .. } => self.read(state, coll),
            I::CursorDone { dest, .. } | I::CursorElem { dest, .. } | I::CursorIndex { dest, .. }
            | I::CursorValue { dest, .. } => self.define(state, *dest),
            I::DebugLog { operand } => self.read(state, operand),
            I::Intrinsic { dest, args, .. } => {
                for arg in args {
                    self.read(state, arg)?;
                }
                self.define(state, *dest)
            }
            I::Nop => Ok(()),
        }
    }

    /// The state on each way out of a block, or the check of a way out of
    /// the unit.
    fn terminator(&self, mut state: State, term: &Terminator) -> Result<Vec<(BlockId, State)>, String> {
        match term {
            Terminator::Goto { target, args } => {
                for arg in args {
                    self.consume(&mut state, arg)?;
                }
                Ok(vec![(*target, state)])
            }
            Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
                self.read(&state, cond)?;
                let mut edges = Vec::new();
                for (target, args, taken) in [(then_block, then_args, true), (else_block, else_args, false)] {
                    let mut state = state.C();
                    if let Operand::Value(flag) = cond {
                        settle(&mut state, *flag, taken);
                    }
                    for arg in args {
                        self.consume(&mut state, arg)?;
                    }
                    edges.push((*target, state));
                }
                Ok(edges)
            }
            Terminator::Switch { discriminant, cases, default } => {
                self.read(&state, discriminant)?;
                let scrutinee = self.scrutinee(discriminant);
                let mut edges: Vec<_> = cases.iter().map(|(case, b)| {
                    let mut state = state.C();
                    if let Some((v, IrType::Enum(variants))) = &scrutinee
                        && variants.get(*case as usize).is_some_and(|(_, payload)| payload.as_ref().is_none_or(IrType::is_copy))
                        && let Some(live) = state.values.get_mut(v)
                    {
                        *live = Live::Nothing;
                    }
                    (*b, state)
                }).collect();
                if !self.covers(discriminant, cases.iter().map(|(case, _)| *case)) {
                    edges.push((*default, state));
                }
                Ok(edges)
            }
            Terminator::Return { value } => {
                if let Some(value) = value {
                    self.consume(&mut state, value)?;
                }
                self.exit(&state)?;
                Ok(Vec::new())
            }
            Terminator::UnitEnd { result } => {
                if let Some(result) = result {
                    self.consume(&mut state, result)?;
                }
                self.exit(&state)?;
                Ok(Vec::new())
            }
            Terminator::UnitEarlyReturn { value } => {
                self.consume(&mut state, value)?;
                self.exit(&state)?;
                Ok(Vec::new())
            }
        }
    }

    /// The value a switch's discriminant was read from, where it was read
    /// from a value, with that value's type.
    fn scrutinee(&self, discriminant: &Operand) -> Option<(ValueId, IrType)> {
        let src = self.discriminated(discriminant)?;
        let Operand::Value(v) = src else { return None };
        Some((*v, self.value_type(*v).held()))
    }

    fn discriminated(&self, discriminant: &Operand) -> Option<&Operand> {
        let Operand::Value(d) = discriminant else { return None };
        self.unit.blocks.iter().flat_map(|b| &b.instructions).find_map(|instr| match instr {
            Instruction::EnumDiscriminant { dest, src } if dest == d => Some(src),
            _ => None,
        })
    }

    /// Whether `cases` are every variant of the enum `discriminant` was read
    /// from, which leaves a switch's default nothing to take.
    fn covers(&self, discriminant: &Operand, cases: impl Iterator<Item = u32>) -> bool {
        let Some(src) = self.discriminated(discriminant) else { return false };
        let ty = match src {
            Operand::Value(v) => self.value_type(*v).held(),
            Operand::ValueRef(v) => match self.referent_type(*v) {
                Ok(ty) => ty,
                Err(_) => return false,
            },
            Operand::Slot(s) => self.slot_type(*s).held(),
            Operand::Param(p) => self.param_type(*p).held(),
            Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => return false,
        };
        let IrType::Enum(variants) = ty else { return false };
        let cases: BTreeSet<u32> = cases.collect();
        cases.len() == variants.len() && cases.iter().all(|&c| (c as usize) < variants.len())
    }

    /// Check a way out of the unit: nothing it owns may still hold a value,
    /// and every parameter its caller gets back must.
    fn exit(&self, state: &State) -> Step {
        let owed = |(v, live): &(&ValueId, &Live)| !self.empty.contains(v) && **live != Live::Nothing;
        if let Some((v, live)) = state.values.iter().find(owed) {
            return Err(match live {
                Live::Yes | Live::Nothing => format!("v{} still holds a value", v.0),
                Live::If { flag, .. } => format!("v{} may still hold a value, by v{}", v.0, flag.0),
            });
        }
        for (&place, held) in &state.places {
            let owes = match place {
                Place::Param(p) => matches!(self.param_mode(p), ParamMode::Mut | ParamMode::Out),
                Place::Slot(_) | Place::Referent(_) => matches!(place, Place::Referent(_)),
            };
            let ok = match held {
                Held::Full(holes) => owes && holes.is_empty(),
                Held::Empty => !owes,
                Held::Maybe => false,
            };
            if !ok {
                return Err(format!("{} is left {held:?}", show(place)));
            }
        }
        Ok(())
    }

    /// The state on a way into a block that two ways reach.
    fn join(&self, a: &State, b: &State) -> Result<State, String> {
        let differ: BTreeSet<ValueId> = a.values.keys().chain(b.values.keys())
            .filter(|v| a.values.get(v) != b.values.get(v))
            .copied()
            .collect();
        let nothing = |s: &State, v: &ValueId| matches!(s.values.get(v), None | Some(Live::Nothing));
        let owed: Vec<_> = differ.iter()
            .filter(|v| !self.empty.contains(v) && !(nothing(a, v) && nothing(b, v)))
            .map(|v| format!("v{}", v.0))
            .collect();
        if !owed.is_empty() {
            return Err(format!("{} hold a value one way and not the other", owed.join(", ")));
        }
        let mut values = a.values.C();
        values.retain(|v, _| !differ.contains(v));
        let mut joined = State { values, places: BTreeMap::new() };
        let places: BTreeSet<Place> = a.places.keys().chain(b.places.keys()).copied().collect();
        for place in places {
            let (x, y) = (self.held(a, place), self.held(b, place));
            let held = if x == y {
                x.into_owned()
            } else if self.tracked.contains(&place) {
                Held::Maybe
            } else {
                return Err(format!("{} is {:?} one way and {:?} the other", show(place), x, y));
            };
            self.set(&mut joined, place, held);
        }
        Ok(joined)
    }
}

/// A branch on `flag` says what holds a value where it was `taken`.
fn settle(state: &mut State, flag: ValueId, taken: bool) {
    state.values.retain(|_, live| match *live {
        Live::If { flag: f, when } if f == flag => when == taken,
        _ => true,
    });
    for live in state.values.values_mut() {
        if matches!(*live, Live::If { flag: f, .. } if f == flag) {
            *live = Live::Yes;
        }
    }
}

fn slot_operand(dest: &SlotDest) -> Operand {
    match dest {
        SlotDest::Local(s) => Operand::Slot(*s),
        SlotDest::External { unit, slot } => Operand::ExternalSlot { unit: *unit, slot: *slot },
    }
}

/// The type of the field at `path` in `ty`.
fn field_type(ty: &IrType, path: &[u32]) -> Result<IrType, String> {
    let mut ty = ty.held();
    for &i in path {
        let field = match &ty {
            IrType::Tuple(fields) => fields.get(i as usize).cloned(),
            IrType::Struct(fields) => fields.get(i as usize).map(|(_, f)| f.C()),
            _ => None,
        };
        ty = field.ok_or_else(|| format!("{ty} has no field {i}"))?.held();
    }
    Ok(ty)
}

fn show(place: Place) -> String {
    match place {
        Place::Slot(s) => format!("s{}", s.0),
        Place::Param(p) => format!("p{}", p.0),
        Place::Referent(v) => format!("*v{}", v.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FunctionContext, IrBlock, SymbolTable};

    const S: IrType = IrType::String;

    fn v(n: u32) -> Operand {
        Operand::Value(ValueId(n))
    }

    fn string(n: u32) -> Instruction {
        Instruction::Const { dest: ValueId(n), value: crate::ConstValue::String("x".S()) }
    }

    fn drop(op: Operand) -> Instruction {
        Instruction::Drop { operand: op }
    }

    fn block(id: u32, instructions: Vec<Instruction>, terminator: Terminator) -> IrBlock {
        IrBlock { id: BlockId(id), params: Vec::new(), instructions, terminator }
    }

    fn ret() -> Terminator {
        Terminator::Return { value: None }
    }

    fn goto(target: u32) -> Terminator {
        Terminator::Goto { target: BlockId(target), args: Vec::new() }
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

    /// A function of these parameters, whose values are `values` and then
    /// strings, with one string slot, tracked or not.
    fn function(params: &[ParamMode], values: &[IrType], tracked: bool, blocks: Vec<IrBlock>) -> IrCodeUnit {
        let mut value_types = values.to_vec();
        value_types.resize(16, S);
        IrCodeUnit {
            id: crate::CodeUnitId(0),
            name: "f".S(),
            blocks,
            value_count: 16,
            slot_count: 1,
            value_types,
            slot_types: vec![S],
            tracked_slots: if tracked { vec![SlotId(0)] } else { Vec::new() },
            const_values: Vec::new(),
            symbols: SymbolTable::new(),
            context: CodeUnitContext::Function(FunctionContext {
                params: (0..params.len() as u32).map(ParamId).collect(),
                param_modes: params.to_vec(),
                param_types: params.iter().map(|_| S).collect(),
                return_type: IrType::Unit,
                tracked_params: params.iter().enumerate()
                    .filter(|(_, m)| **m == ParamMode::Out)
                    .map(|(i, _)| ParamId(i as u32))
                    .collect(),
                type_param_count: 0,
                descriptor_table: Vec::new(),
                inline: false,
            }),
            nested_units: Vec::new(),
        }
    }

    fn check(unit: &IrCodeUnit) -> Result<(), String> {
        check_unit(unit, &|_| None)
    }

    fn refused(unit: &IrCodeUnit, why: &str) {
        let e = check(unit).expect_err("the check let a fault through");
        assert!(e.lines().next().X().contains(why), "refused for something else: {e}");
    }

    #[test]
    fn a_value_dropped_once() {
        check(&function(&[], &[], false, vec![block(0, vec![string(0), drop(v(0))], ret())])).X();
    }

    #[test]
    fn a_value_never_dropped() {
        refused(&function(&[], &[], false, vec![block(0, vec![string(0)], ret())]), "v0 still holds a value");
    }

    #[test]
    fn a_value_dropped_twice() {
        let unit = function(&[], &[], false, vec![block(0, vec![string(0), drop(v(0)), drop(v(0))], ret())]);
        refused(&unit, "reads v0, which holds nothing");
    }

    #[test]
    fn a_store_over_a_full_slot() {
        let store = |n| Instruction::SlotStoreMove { dest: SlotDest::Local(SlotId(0)), value: v(n) };
        let unit = function(&[], &[], false, vec![
            block(0, vec![string(0), store(0), string(1), store(1), drop(Operand::Slot(SlotId(0)))], ret()),
        ]);
        refused(&unit, "stores to s0, which still holds a value");
    }

    /// A slot filled, and emptied on one way to block 3 only.
    fn emptied_one_way(tracked: bool, end: Instruction) -> IrCodeUnit {
        function(&[], &[IrType::Bool], tracked, vec![
            block(0, vec![
                Instruction::Const { dest: ValueId(0), value: crate::ConstValue::Bool(true) },
                string(1),
                Instruction::SlotStoreMove { dest: SlotDest::Local(SlotId(0)), value: v(1) },
            ], branch(v(0), 1, 2)),
            block(1, vec![drop(Operand::Slot(SlotId(0)))], goto(3)),
            block(2, vec![], goto(3)),
            block(3, vec![end], ret()),
        ])
    }

    #[test]
    fn a_precise_slot_emptied_one_way() {
        refused(&emptied_one_way(false, Instruction::Nop), "on its way into block3, s0 is");
    }

    #[test]
    fn a_tracked_slot_emptied_one_way() {
        check(&emptied_one_way(true, Instruction::DropTracked { operand: Operand::Slot(SlotId(0)) })).X();
        refused(&emptied_one_way(true, Instruction::Nop), "s0 is left Maybe");
    }

    /// An option of a string unwrapped, and the payload dropped where it is
    /// there if `drops`.
    fn unwrapped(drops: bool) -> IrCodeUnit {
        function(&[], &[IrType::Option(Box::new(S)), S, IrType::Bool], false, vec![
            block(0, vec![
                string(3),
                Instruction::WrapSome { dest: ValueId(0), inner: v(3) },
                Instruction::UnwrapOption { dest: ValueId(1), is_some: ValueId(2), src: v(0) },
            ], branch(v(2), 1, 2)),
            block(1, if drops { vec![drop(v(1))] } else { Vec::new() }, ret()),
            block(2, vec![], ret()),
        ])
    }

    #[test]
    fn an_unwrapped_payload_is_there_where_its_flag_says() {
        check(&unwrapped(true)).X();
        refused(&unwrapped(false), "v1 still holds a value");
    }

    #[test]
    fn parameters_owed_back() {
        check(&function(&[ParamMode::In], &[], false, vec![block(0, vec![drop(Operand::Param(ParamId(0)))], ret())])).X();
        refused(&function(&[ParamMode::In], &[], false, vec![block(0, vec![], ret())]), "p0 is left");
        refused(&function(&[ParamMode::Out], &[], false, vec![block(0, vec![], ret())]), "p0 is left Empty");
        let dropped = function(&[ParamMode::Mut], &[], false, vec![block(0, vec![drop(Operand::Param(ParamId(0)))], ret())]);
        refused(&dropped, "p0 is left Empty");
    }

    #[test]
    fn a_call_takes_what_its_modes_say() {
        let callee = function(&[ParamMode::In, ParamMode::Ref], &[], false, vec![]);
        let call = |dropped: bool| {
            let mut instructions = vec![string(0), string(1), Instruction::Call {
                dest: ValueId(2),
                func: CodeRef::Local(crate::CodeUnitId(0)),
                args: vec![v(0), v(1)],
                bindings: Vec::new(),
            }];
            if dropped {
                instructions.push(drop(v(0)));
            }
            instructions.push(drop(v(1)));
            instructions.push(drop(v(2)));
            function(&[], &[], false, vec![block(0, instructions, ret())])
        };
        check_unit(&call(false), &|_| Some(&callee)).X();
        let e = check_unit(&call(true), &|_| Some(&callee)).expect_err("a value passed `in` was dropped again");
        assert!(e.contains("reads v0, which holds nothing"), "{e}");
    }
}
