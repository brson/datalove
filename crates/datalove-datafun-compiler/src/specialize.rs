//! Const parameter specialization by monomorphization.
//!
//! A function with `const` parameters gets one copy per distinct combination of
//! const argument values found at its call sites, with those parameters removed
//! and the values substituted into the body. Call sites whose instantiation is
//! known are rewritten to call the copy.
//!
//! # Overview
//!
//! ```text
//! fun repeat(const n: i32, s: string): string ...
//!
//! const N = 3
//! const M = 5
//! let a = repeat(N, "x")
//! let b = repeat(M, "y")
//! ```
//!
//! becomes
//!
//! ```text
//! fun repeat(n, s) ...          // kept, unchanged
//! fun repeat__ct0(s) ...        // n = 3 substituted
//! fun repeat__ct1(s) ...        // n = 5 substituted
//!
//! let a = repeat__ct0("x")
//! let b = repeat__ct1("y")
//! ```
//!
//! # Why the original is kept
//!
//! Specialization is additive. The original function stays, with the signature
//! it was lowered with, so a call site nothing specialized still calls
//! something that exists and still takes the const argument. That matters
//! because the module graph is not the whole program: script units compile
//! afterwards, one at a time, against modules already specialized, and a script
//! line may name an instantiation no module call site asked for. Those calls
//! keep their `ComptimeCall`, which every backend executes as a plain `Call`.
//!
//! # Integration
//!
//! This runs within lowering as phase 5c, after const evaluation and before
//! assembly. Instantiations are read out of the IR rather than out of the
//! typechecker's record of which const binding each call site named: the call
//! site's const argument is already an operand defined by a `Const`
//! instruction, so the values the rewrite will look for are the values the plan
//! is built from, and the two cannot disagree.

use std::collections::{BTreeMap, HashMap};

use datalove_datafun_const::inline_function_consts;
use datalove_datafun_ir::{
    CodeRef, CodeUnitContext, CodeUnitId, ConstValue, FunctionContext, Instruction,
    IrBlock, IrCodeUnit, IrModuleId, IrType, Operand, ParamId, ValueId,
    replace_params_in_instruction, replace_params_in_terminator,
};

/// Most instantiations one function may have before it is an error.
///
/// Each one is a whole copy of the body in the object file.
pub const MAX_INSTANTIATIONS: usize = 64;

/// How many times to look for instantiations the last round's copies revealed.
///
/// A comptime function calling another one only tells you what it passes once
/// its own const parameters have been substituted, so a round of copying can
/// uncover instantiations the round before could not see. This terminates on
/// its own -- the callees are the program's functions and each is capped at
/// [`MAX_INSTANTIATIONS`] -- and the cap is a backstop against a cycle nobody
/// has thought of rather than a limit anything real should reach.
pub const MAX_ROUNDS: usize = 16;

/// Where a comptime call reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CalleeKey {
    /// A unit beside the one making the call, in its `nested_units`.
    Local(CodeUnitId),
    /// A module function.
    Module(IrModuleId, CodeUnitId),
}

/// The instantiations found for one function with const parameters.
#[derive(Clone, Debug)]
pub struct FuncMonomorphization {
    /// Indices of the const parameters, as the call sites report them.
    pub comptime_param_indices: Vec<usize>,
    /// Distinct const argument tuples, in the order they were found.
    pub instantiations: Vec<Vec<ConstValue>>,
    /// Which instantiation a given tuple is.
    value_to_index: HashMap<Vec<ConstValue>, usize>,
    /// How to reach the copy for each instantiation, in the same order.
    ///
    /// Empty until the copies are made, and left empty for a function that
    /// exceeded [`MAX_INSTANTIATIONS`], which is what stops its call sites
    /// being rewritten to copies that were never built.
    pub copies: Vec<CodeRef>,
}

impl FuncMonomorphization {
    /// The copy to call for a given set of const argument values.
    pub fn copy_for(&self, values: &[ConstValue]) -> Option<CodeRef> {
        let index = *self.value_to_index.get(values)?;
        self.copies.get(index).cloned()
    }
}

/// Every function that needs copies, keyed by where it lives.
///
/// A `BTreeMap` because the order decides which unit ids the copies get.
#[derive(Clone, Debug, Default)]
pub struct MonomorphizationPlan {
    pub funcs: BTreeMap<CalleeKey, FuncMonomorphization>,
}

impl MonomorphizationPlan {
    /// True when no call site named a function with const parameters.
    pub fn is_empty(&self) -> bool {
        self.funcs.is_empty()
    }
}

/// Add every instantiation this unit names to the plan.
///
/// `key_of` says where a reference reaches, which differs between a module
/// function and a script unit: the first addresses every call by module, the
/// second can also name a unit beside itself.
///
/// The caller decides the order units are visited in, which decides the order
/// instantiations are numbered in, and so the names and ids the copies get.
pub fn collect_instantiations_into(
    plan: &mut MonomorphizationPlan,
    unit: &IrCodeUnit,
    key_of: &dyn Fn(&CodeRef) -> Option<CalleeKey>,
) {
    let consts = const_value_map(unit);

    for block in &unit.blocks {
        for instr in &block.instructions {
            let Instruction::ComptimeCall { func, args, comptime_param_indices, .. } = instr else {
                continue;
            };
            let Some(callee) = key_of(func) else {
                continue;
            };
            let Some(values) = comptime_values(args, comptime_param_indices, &consts) else {
                continue;
            };

            let entry = plan.funcs.entry(callee).or_insert_with(|| FuncMonomorphization {
                comptime_param_indices: comptime_param_indices.clone(),
                instantiations: Vec::new(),
                value_to_index: HashMap::new(),
                copies: Vec::new(),
            });

            if !entry.value_to_index.contains_key(&values) {
                entry.value_to_index.insert(values.clone(), entry.instantiations.len());
                entry.instantiations.push(values);
            }
        }
    }
}

/// Where a call in a module function reaches.
///
/// Module functions address every call by module, including calls within their
/// own module, so a local reference is not something this can be handed.
pub fn module_callee_key(code_ref: &CodeRef) -> Option<CalleeKey> {
    match code_ref {
        CodeRef::Module { module, id } => Some(CalleeKey::Module(*module, *id)),
        CodeRef::Local(id) => {
            panic!("a module function addresses its calls by module, not local unit {}", id.0)
        }
        // A call into a previous script execution names a unit that was
        // compiled and run before this one existed.
        CodeRef::External { .. } => None,
    }
}

/// Where a call in a script unit reaches.
pub fn script_callee_key(code_ref: &CodeRef) -> Option<CalleeKey> {
    match code_ref {
        CodeRef::Local(id) => Some(CalleeKey::Local(*id)),
        CodeRef::Module { module, id } => Some(CalleeKey::Module(*module, *id)),
        CodeRef::External { .. } => None,
    }
}

/// Build one copy of a function with its const parameters substituted away.
pub fn monomorphize_function(
    original: &IrCodeUnit,
    comptime_param_indices: &[usize],
    values: &[ConstValue],
    new_id: CodeUnitId,
    new_name: String,
) -> IrCodeUnit {
    let CodeUnitContext::Function(func_ctx) = &original.context else {
        panic!("only a function has const parameters, and {} is not one", original.name);
    };

    // What each parameter becomes: a const parameter becomes the value it was
    // called with, and everything else keeps its meaning at a new index.
    let mut substitutions: HashMap<ParamId, Operand> = HashMap::new();

    let mut new_param_types = Vec::new();
    let mut new_param_modes = Vec::new();
    let mut param_remap: HashMap<u32, u32> = HashMap::new();
    for (old_idx, (ty, mode)) in func_ctx.param_types.iter().zip(func_ctx.param_modes.iter()).enumerate() {
        if comptime_param_indices.contains(&old_idx) {
            continue;
        }
        let new_idx = new_param_types.len() as u32;
        param_remap.insert(old_idx as u32, new_idx);
        substitutions.insert(ParamId(old_idx as u32), Operand::Param(ParamId(new_idx)));
        new_param_types.push(ty.clone());
        new_param_modes.push(*mode);
    }

    // One fresh value per const parameter, defined at entry. A const
    // parameter is borrowed, so the body never drops it, and what stands for
    // it is borrowed too: a static of a non-copy type, read through the
    // reference as the parameter was, or a `Const` of a copy type.
    let mut value_types = original.value_types.clone();
    let mut next_value = original.value_count;
    let mut entry_consts = Vec::new();
    for (&param_idx, value) in comptime_param_indices.iter().zip(values.iter()) {
        let dest = ValueId(next_value);
        next_value += 1;
        let ty = func_ctx.param_types[param_idx].clone();
        if ty.is_copy() {
            value_types.push(ty);
            entry_consts.push(Instruction::Const { dest, value: value.clone() });
            substitutions.insert(ParamId(param_idx as u32), Operand::Value(dest));
        } else {
            value_types.push(IrType::Ref(Box::new(ty)));
            entry_consts.push(Instruction::StaticRef { dest, value: std::sync::Arc::new(value.clone()) });
            substitutions.insert(ParamId(param_idx as u32), Operand::ValueRef(dest));
        }
    }

    let blocks: Vec<IrBlock> = original.blocks.iter().enumerate()
        .map(|(block_idx, block)| {
            let mut instructions = if block_idx == 0 { entry_consts.clone() } else { Vec::new() };
            instructions.extend(block.instructions.iter()
                .map(|instr| replace_params_in_instruction(instr, &substitutions)));
            IrBlock {
                id: block.id,
                params: block.params.clone(),
                instructions,
                terminator: replace_params_in_terminator(&block.terminator, &substitutions),
            }
        })
        .collect();

    IrCodeUnit {
        id: new_id,
        name: new_name,
        blocks,
        value_count: next_value,
        slot_count: original.slot_count,
        value_types,
        slot_types: original.slot_types.clone(),
        tracked_slots: original.tracked_slots.clone(),
        const_values: original.const_values.clone(),
        symbols: original.symbols.clone(),
        context: CodeUnitContext::Function(FunctionContext {
            params: (0..new_param_types.len() as u32).map(ParamId).collect(),
            param_modes: new_param_modes,
            param_types: new_param_types,
            return_type: func_ctx.return_type.clone(),
            tracked_params: remap_params(&func_ctx.tracked_params, &param_remap),
            descriptor_shapes: func_ctx.descriptor_shapes.clone(),
            descriptor_params: remap_params(&func_ctx.descriptor_params, &param_remap),
        }),
        nested_units: original.nested_units.clone(),
    }
}

/// Point each comptime call whose instantiation was built at the copy built for it.
///
/// A call the plan has nothing for is left as it is. It names the original
/// function, which is still there and still takes the const argument.
pub fn rewrite_comptime_calls(
    unit: &IrCodeUnit,
    plan: &MonomorphizationPlan,
    key_of: &dyn Fn(&CodeRef) -> Option<CalleeKey>,
) -> IrCodeUnit {
    let consts = const_value_map(unit);
    let mut blocks = Vec::with_capacity(unit.blocks.len());

    for block in &unit.blocks {
        let mut instructions = Vec::with_capacity(block.instructions.len());

        for instr in &block.instructions {
            let Instruction::ComptimeCall {
                dest, func, args, comptime_param_indices, type_args, shape_descriptors, ..
            } = instr
            else {
                instructions.push(instr.clone());
                continue;
            };

            let copy = key_of(func)
                .and_then(|key| plan.funcs.get(&key))
                .and_then(|mono| {
                    let values = comptime_values(args, comptime_param_indices, &consts)?;
                    mono.copy_for(&values)
                });
            let Some(copy_ref) = copy else {
                instructions.push(instr.clone());
                continue;
            };

            // The copy does not take the const arguments. They were borrowed,
            // so there is nothing to give back.
            let kept_args = args.iter().enumerate()
                .filter(|(i, _)| !comptime_param_indices.contains(i))
                .map(|(_, arg)| arg.clone())
                .collect();

            instructions.push(Instruction::Call {
                dest: *dest,
                func: copy_ref,
                args: kept_args,
                // The copy is as generic as the original: `monomorphize_function`
                // clones `descriptor_shapes` unchanged, so what this site was
                // going to hand the original is what the copy wants. Worked out
                // before specialization ran, and carried across rather than
                // recomputed, because the shape sets settled then.
                type_args: type_args.clone(),
                shape_descriptors: shape_descriptors.clone(),
            });
        }

        blocks.push(IrBlock {
            id: block.id,
            params: block.params.clone(),
            instructions,
            terminator: block.terminator.clone(),
        });
    }

    IrCodeUnit {
        blocks,
        ..unit.clone()
    }
}

/// Specialize the comptime calls a script unit makes.
///
/// The copies go in the script unit's own `nested_units`, reached by
/// `CodeRef::Local`, so that nothing in the module graph is disturbed. That is
/// what makes this work one script unit at a time: a REPL line may name an
/// instantiation no module call site asked for, and gets its own copy of the
/// callee without the module it came from having to change.
///
/// `module_unit` reads a module function out of the registry, which is where a
/// script's callee usually lives. Returns the unit and any errors.
pub fn specialize_script_unit(
    unit: &IrCodeUnit,
    module_unit: &dyn Fn(IrModuleId, CodeUnitId) -> Option<IrCodeUnit>,
    instantiation_consts: &dyn Fn(CalleeKey, &[usize], &[ConstValue]) -> (HashMap<String, ConstValue>, Vec<String>),
) -> (IrCodeUnit, Vec<String>) {
    let mut unit = unit.clone();
    let mut plan = MonomorphizationPlan::default();
    let mut errors = Vec::new();

    for _ in 0..MAX_ROUNDS {
        collect_instantiations_into(&mut plan, &unit, &script_callee_key);
        for nested in &unit.nested_units {
            collect_instantiations_into(&mut plan, nested, &script_callee_key);
        }

        let mut next_id = unit.nested_units.iter()
            .map(|f| f.id.0 + 1)
            .max()
            .unwrap_or(0);
        let mut copies = Vec::new();

        for (callee, mono) in plan.funcs.iter_mut() {
            let original = match callee {
                CalleeKey::Local(id) => unit.nested_units.iter().find(|f| f.id == *id).cloned(),
                CalleeKey::Module(module, id) => module_unit(*module, *id),
            };
            let Some(original) = original else {
                continue;
            };

            if let Some(error) = over_limit(&original.name, mono) {
                errors.push(error);
                continue;
            }

            for values in mono.instantiations.iter().skip(mono.copies.len()) {
                let new_id = CodeUnitId(next_id);
                next_id += 1;
                // Numbered by unit id rather than by instantiation, so that two
                // callees that share a name cannot produce two copies that do. A
                // nested unit's name is what the C backend emits as its symbol,
                // and unlike a module's it carries nothing to tell them apart.
                let mut copy = monomorphize_function(
                    &original,
                    &mono.comptime_param_indices,
                    values,
                    new_id,
                    format!("{}__ct{}", original.name, new_id.0),
                );

                // Now that the parameters have values, the body's consts have
                // one each, so they are evaluated and written in.
                let (evaluated, const_errors) =
                    instantiation_consts(*callee, &mono.comptime_param_indices, values);
                errors.extend(const_errors);
                inline_function_consts(&mut copy, &evaluated);

                copies.push(copy);
                mono.copies.push(CodeRef::Local(new_id));
            }
        }

        if copies.is_empty() {
            break;
        }
        unit.nested_units.extend(copies);
    }

    unit.nested_units = unit.nested_units.iter()
        .map(|nested| rewrite_comptime_calls(nested, &plan, &script_callee_key))
        .collect();
    let unit = rewrite_comptime_calls(&unit, &plan, &script_callee_key);

    (unit, errors)
}

/// Report a function that wants more copies than it may have.
///
/// Leaving `copies` short of `instantiations` is what stops the call sites it
/// could not be copied for being pointed at copies that were never built.
pub fn over_limit(name: &str, mono: &FuncMonomorphization) -> Option<String> {
    if mono.instantiations.len() <= MAX_INSTANTIATIONS {
        return None;
    }
    Some(format!(
        "`{}` has {} const parameter instantiations, over the limit of {}; \
         each one is a copy of the function",
        name, mono.instantiations.len(), MAX_INSTANTIATIONS,
    ))
}

/// The const argument values at a call site, or `None` if any is not a constant.
fn comptime_values(
    args: &[Operand],
    comptime_param_indices: &[usize],
    consts: &HashMap<ValueId, ConstValue>,
) -> Option<Vec<ConstValue>> {
    comptime_param_indices.iter()
        .map(|&idx| match args.get(idx)? {
            // A const argument is passed by reference: the binding itself, or
            // a static one.
            Operand::Value(value) | Operand::ValueRef(value) => consts.get(value).cloned(),
            _ => None,
        })
        .collect()
}

/// Values in this unit that a `Const` instruction defines.
fn const_value_map(unit: &IrCodeUnit) -> HashMap<ValueId, ConstValue> {
    let mut map = HashMap::new();

    for block in &unit.blocks {
        for instr in &block.instructions {
            match instr {
                Instruction::Const { dest, value } => {
                    map.insert(*dest, value.clone());
                }
                Instruction::StaticRef { dest, value } => {
                    map.insert(*dest, (**value).clone());
                }
                // A constant carried somewhere is still that constant. Reading
                // a const of a linear type clones it, so that each read has a
                // value of its own; binding one to a name moves or copies it,
                // which is what a const naming a const parameter lowers to,
                // since it has a value per instantiation rather than one and
                // only becomes a constant inside the copy.
                Instruction::Clone { dest, src: Operand::Value(src) }
                | Instruction::Move { dest, src: Operand::Value(src) }
                | Instruction::Copy { dest, src: Operand::Value(src) } => {
                    if let Some(value) = map.get(src).cloned() {
                        map.insert(*dest, value);
                    }
                }
                _ => {}
            }
        }
    }

    map
}

/// Carry parameter ids across the renumbering, dropping the const parameters.
fn remap_params(params: &[ParamId], param_remap: &HashMap<u32, u32>) -> Vec<ParamId> {
    params.iter()
        .filter_map(|p| param_remap.get(&p.0).copied().map(ParamId))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mono(instantiations: Vec<Vec<ConstValue>>, copies: Vec<CodeRef>) -> FuncMonomorphization {
        let value_to_index = instantiations.iter().enumerate()
            .map(|(i, v)| (v.clone(), i))
            .collect();
        FuncMonomorphization {
            comptime_param_indices: vec![0],
            instantiations,
            value_to_index,
            copies,
        }
    }

    #[test]
    fn copy_for_finds_the_copy_built_for_a_tuple() {
        let m = mono(
            vec![vec![ConstValue::I32(3)], vec![ConstValue::I32(5)]],
            vec![CodeRef::Local(CodeUnitId(7)), CodeRef::Local(CodeUnitId(8))],
        );

        assert_eq!(m.copy_for(&[ConstValue::I32(3)]), Some(CodeRef::Local(CodeUnitId(7))));
        assert_eq!(m.copy_for(&[ConstValue::I32(5)]), Some(CodeRef::Local(CodeUnitId(8))));
        assert_eq!(m.copy_for(&[ConstValue::I32(9)]), None);
    }

    #[test]
    fn copy_for_is_none_until_the_copies_are_built() {
        let m = mono(vec![vec![ConstValue::I32(3)]], Vec::new());

        assert_eq!(m.copy_for(&[ConstValue::I32(3)]), None);
    }
}
