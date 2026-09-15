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

use datalove_datafun_ir::{
    CallSiteId, CodeRef, CodeUnitContext, CodeUnitId, ConstValue, FunctionContext, Instruction,
    IrBlock, IrCodeUnit, IrModuleId, Operand, ParamId, ValueId,
    replace_params_in_instruction, replace_params_in_terminator,
};

/// Most instantiations one function may have before it is an error.
///
/// Each one is a whole copy of the body in the object file.
pub const MAX_INSTANTIATIONS: usize = 64;

/// The instantiations found for one function with const parameters.
#[derive(Clone, Debug)]
pub struct FuncMonomorphization {
    /// Indices of the const parameters, as the call sites report them.
    pub comptime_param_indices: Vec<usize>,
    /// Distinct const argument tuples, in the order they were found.
    pub instantiations: Vec<Vec<ConstValue>>,
    /// Which instantiation a given tuple is.
    value_to_index: HashMap<Vec<ConstValue>, usize>,
    /// Unit id of the copy for each instantiation, in the same order.
    ///
    /// Empty until the copies are made, and left empty for a function that
    /// exceeded [`MAX_INSTANTIATIONS`], which is what stops its call sites
    /// being rewritten to copies that were never built.
    pub copies: Vec<CodeUnitId>,
}

impl FuncMonomorphization {
    /// The copy to call for a given set of const argument values.
    pub fn copy_for(&self, values: &[ConstValue]) -> Option<CodeUnitId> {
        let index = *self.value_to_index.get(values)?;
        self.copies.get(index).copied()
    }
}

/// Every function that needs copies, keyed by where it lives.
///
/// A `BTreeMap` because the order decides which unit ids the copies get.
#[derive(Clone, Debug, Default)]
pub struct MonomorphizationPlan {
    pub funcs: BTreeMap<(IrModuleId, CodeUnitId), FuncMonomorphization>,
}

impl MonomorphizationPlan {
    /// True when no call site named a function with const parameters.
    pub fn is_empty(&self) -> bool {
        self.funcs.is_empty()
    }
}

/// Find every instantiation reachable from the given units.
///
/// The caller decides the order, which decides the order instantiations are
/// numbered in, and so the names and ids the copies get.
pub fn collect_instantiations<'a>(
    modules: impl IntoIterator<Item = (IrModuleId, &'a [IrCodeUnit])>,
) -> MonomorphizationPlan {
    let mut funcs: BTreeMap<(IrModuleId, CodeUnitId), FuncMonomorphization> = BTreeMap::new();

    for (ir_module_id, units) in modules {
        for unit in units {
            let consts = const_value_map(unit);

            for block in &unit.blocks {
                for instr in &block.instructions {
                    let Instruction::ComptimeCall { func, args, comptime_param_indices, .. } = instr
                    else {
                        continue;
                    };
                    let Some(callee) = callee_key(func, ir_module_id) else {
                        continue;
                    };
                    let Some(values) = comptime_values(args, comptime_param_indices, &consts) else {
                        continue;
                    };

                    let entry = funcs.entry(callee).or_insert_with(|| FuncMonomorphization {
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
    }

    MonomorphizationPlan { funcs }
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

    // One fresh value per const parameter, defined by a `Const` at entry. The
    // body's own drop of what used to be the parameter becomes a drop of this,
    // so the ownership accounting carries over unchanged.
    let mut value_types = original.value_types.clone();
    let mut next_value = original.value_count;
    let mut entry_consts = Vec::new();
    for (&param_idx, value) in comptime_param_indices.iter().zip(values.iter()) {
        let dest = ValueId(next_value);
        next_value += 1;
        value_types.push(func_ctx.param_types[param_idx].clone());
        entry_consts.push(Instruction::Const { dest, value: value.clone() });
        substitutions.insert(ParamId(param_idx as u32), Operand::Value(dest));
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
        call_site_count: original.call_site_count,
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
    current_module: IrModuleId,
) -> IrCodeUnit {
    let consts = const_value_map(unit);
    let mut next_call_site = unit.call_site_count;
    let mut blocks = Vec::with_capacity(unit.blocks.len());

    for block in &unit.blocks {
        let mut instructions = Vec::with_capacity(block.instructions.len());

        for instr in &block.instructions {
            let Instruction::ComptimeCall { dest, func, args, comptime_param_indices, .. } = instr
            else {
                instructions.push(instr.clone());
                continue;
            };

            let copy = callee_key(func, current_module)
                .and_then(|key| plan.funcs.get(&key))
                .and_then(|mono| {
                    let values = comptime_values(args, comptime_param_indices, &consts)?;
                    mono.copy_for(&values)
                });
            let Some(copy_id) = copy else {
                instructions.push(instr.clone());
                continue;
            };

            // The copy does not take the const arguments, so what this call
            // site computed for them is dropped here rather than by the callee.
            for (i, arg) in args.iter().enumerate() {
                if comptime_param_indices.contains(&i) {
                    instructions.push(Instruction::Drop { operand: arg.clone() });
                }
            }

            let kept_args = args.iter().enumerate()
                .filter(|(i, _)| !comptime_param_indices.contains(i))
                .map(|(_, arg)| arg.clone())
                .collect();

            let site_id = CallSiteId(next_call_site);
            next_call_site += 1;
            instructions.push(Instruction::Call {
                site_id,
                dest: *dest,
                func: with_unit_id(func, copy_id),
                args: kept_args,
                // A function with const parameters is never generic: lowering
                // takes the comptime branch before type arguments are computed.
                type_args: Vec::new(),
                shape_descriptors: Vec::new(),
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
        call_site_count: next_call_site,
        ..unit.clone()
    }
}

/// Where a call reaches, as a key into the plan.
///
/// A call into a previous script execution is not specialized: the unit it
/// names was compiled and run before this one existed.
fn callee_key(code_ref: &CodeRef, current_module: IrModuleId) -> Option<(IrModuleId, CodeUnitId)> {
    match code_ref {
        CodeRef::Local(id) => Some((current_module, *id)),
        CodeRef::Module { module, id } => Some((*module, *id)),
        CodeRef::External { .. } => None,
    }
}

/// The same kind of reference, pointing at a different unit in the same place.
fn with_unit_id(code_ref: &CodeRef, id: CodeUnitId) -> CodeRef {
    match code_ref {
        CodeRef::Local(_) => CodeRef::Local(id),
        CodeRef::Module { module, .. } => CodeRef::Module { module: *module, id },
        CodeRef::External { .. } => {
            panic!("an external call is never specialized, so it never reaches here")
        }
    }
}

/// The const argument values at a call site, or `None` if any is not a constant.
fn comptime_values(
    args: &[Operand],
    comptime_param_indices: &[usize],
    consts: &HashMap<ValueId, ConstValue>,
) -> Option<Vec<ConstValue>> {
    comptime_param_indices.iter()
        .map(|&idx| match args.get(idx)? {
            Operand::Value(value) => consts.get(value).cloned(),
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
                // A clone of a constant is that constant. Reading a const of a
                // linear type clones it, so that each read has a value of its
                // own, and a const argument read that way is still the constant
                // the call site wrote.
                Instruction::Clone { dest, src: Operand::Value(src) } => {
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

    fn mono(instantiations: Vec<Vec<ConstValue>>, copies: Vec<CodeUnitId>) -> FuncMonomorphization {
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
            vec![CodeUnitId(7), CodeUnitId(8)],
        );

        assert_eq!(m.copy_for(&[ConstValue::I32(3)]), Some(CodeUnitId(7)));
        assert_eq!(m.copy_for(&[ConstValue::I32(5)]), Some(CodeUnitId(8)));
        assert_eq!(m.copy_for(&[ConstValue::I32(9)]), None);
    }

    #[test]
    fn copy_for_is_none_until_the_copies_are_built() {
        let m = mono(vec![vec![ConstValue::I32(3)]], Vec::new());

        assert_eq!(m.copy_for(&[ConstValue::I32(3)]), None);
    }
}
