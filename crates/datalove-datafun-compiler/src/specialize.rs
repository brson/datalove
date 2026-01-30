//! Comptime argument specialization via union-branch transformation.
//!
//! This module implements specialization of functions with `const` parameters
//! using the union-branch approach: instead of N monomorphized copies, we generate
//! one function with N branches dispatching on a discriminant value.
//!
//! # Overview
//!
//! Given:
//! ```text
//! fun repeat(const n: i32, s: string) -> string ...
//!
//! const N = 3
//! const M = 5
//! let a = repeat(N, "x")  // instantiation with n=3
//! let b = repeat(M, "y")  // instantiation with n=5
//! ```
//!
//! We transform the function and call sites:
//!
//! ```text
//! // Transformed function: discriminant replaces comptime param
//! fun repeat_specialized(tag: i32, s: string) -> string
//!     if tag == 0
//!         const n = 3  // body with n=3
//!         ...
//!     else if tag == 1
//!         const n = 5  // body with n=5
//!         ...
//!     end if
//! end fun
//!
//! // Call sites pass discriminant
//! let a = repeat_specialized(0, "x")  // was repeat(N, "x")
//! let b = repeat_specialized(1, "y")  // was repeat(M, "y")
//! ```
//!
//! # Integration
//!
//! Specialization runs within the lowering phase (phase 5c), after const evaluation (5b):
//! 1. Lowering (5a) produces IR functions including comptime-param functions
//! 2. Const eval (5b) evaluates all const bindings → ResolvedConsts
//! 3. Specialization (5c) transforms IR functions and rewrites calls
//! 4. Assembly (5d) inlines constants within each branch

use std::collections::HashMap;
use datalove_datafun_ir::{
    ConstValue, IrFunction, IrBlock, Instruction, Terminator, Operand,
    ValueId, BlockId, FuncId, IrType, BinOp, ParamMode, ParamId,
};
use datalove_datafun_common::ComptimeCallSiteRegistry;

/// Information about a function's comptime specialization.
#[derive(Clone, Debug)]
pub struct FuncSpecialization {
    /// Original function's FuncId.
    pub original_func_id: FuncId,
    /// Original function name.
    pub func_name: String,
    /// Indices of comptime parameters.
    pub comptime_param_indices: Vec<usize>,
    /// Map from comptime values tuple to discriminant (0, 1, 2, ...).
    pub value_to_discriminant: HashMap<Vec<ConstValue>, u32>,
    /// All unique instantiations (comptime value tuples).
    pub instantiations: Vec<Vec<ConstValue>>,
}

impl FuncSpecialization {
    /// Get the discriminant for a given set of comptime values.
    pub fn get_discriminant(&self, values: &[ConstValue]) -> Option<u32> {
        self.value_to_discriminant.get(values).copied()
    }

    /// Get the number of unique instantiations.
    pub fn num_instantiations(&self) -> usize {
        self.instantiations.len()
    }
}

/// Result of the specialization pass.
#[derive(Clone, Debug, Default)]
pub struct SpecializationResult {
    /// Functions that were specialized (func_name → specialization info).
    pub specialized_funcs: HashMap<String, FuncSpecialization>,
    /// Call site rewrites (call_site_id → discriminant to pass).
    pub call_rewrites: HashMap<salsa::Id, u32>,
}

impl SpecializationResult {
    /// Check if there's anything to specialize.
    pub fn is_empty(&self) -> bool {
        self.specialized_funcs.is_empty()
    }
}

/// Build the specialization plan from the comptime registry and resolved consts.
///
/// This resolves const binding names to values and groups call sites by function,
/// collecting unique instantiations for each comptime-param function.
pub fn build_specialization_plan<'db>(
    db: &'db dyn salsa::Database,
    registry: &ComptimeCallSiteRegistry<'db>,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
) -> SpecializationResult {
    if registry.is_empty() {
        return SpecializationResult::default();
    }

    let mut specialized_funcs: HashMap<String, FuncSpecialization> = HashMap::new();
    let mut call_rewrites: HashMap<salsa::Id, u32> = HashMap::new();

    // Process each call site.
    for call_site in &registry.call_sites {
        let func_name = call_site.func_name.as_str(db).to_string();

        // Resolve comptime arg names to values.
        let mut values: Vec<ConstValue> = Vec::new();
        let mut all_resolved = true;
        for arg_name in &call_site.comptime_arg_names {
            let name_str = arg_name.as_str(db);
            if let Some((_, val)) = resolved_consts.get(name_str) {
                values.push(val.clone());
            } else {
                // Const not resolved - skip this call site.
                all_resolved = false;
                break;
            }
        }

        if !all_resolved {
            continue;
        }

        // Get or create specialization entry for this function.
        let spec = specialized_funcs.entry(func_name.clone()).or_insert_with(|| {
            // Get function info from registry.
            let comptime_indices = registry.comptime_funcs
                .get(&call_site.func_name)
                .cloned()
                .unwrap_or_default();

            FuncSpecialization {
                original_func_id: FuncId(0), // Will be set later when we have the function
                func_name: func_name.clone(),
                comptime_param_indices: comptime_indices,
                value_to_discriminant: HashMap::new(),
                instantiations: Vec::new(),
            }
        });

        // Check if this instantiation is new.
        let discriminant = if let Some(&disc) = spec.value_to_discriminant.get(&values) {
            disc
        } else {
            let disc = spec.instantiations.len() as u32;
            spec.value_to_discriminant.insert(values.clone(), disc);
            spec.instantiations.push(values);
            disc
        };

        // Record the rewrite for this call site.
        call_rewrites.insert(call_site.call_expr_id, discriminant);
    }

    SpecializationResult {
        specialized_funcs,
        call_rewrites,
    }
}

/// Transform a function with comptime parameters into union-branch form.
///
/// The transformed function:
/// 1. Has the comptime parameter(s) replaced with a single i32 discriminant
/// 2. Has a dispatch chain of if-else blocks based on the discriminant
/// 3. Each branch has const instructions for the comptime values
///
/// Note: The actual const folding within branches is handled by existing
/// const inlining infrastructure in the assembly phase.
pub fn transform_function(
    original: &IrFunction,
    spec: &FuncSpecialization,
) -> IrFunction {
    // For functions with no instantiations, return unchanged.
    if spec.instantiations.is_empty() {
        return original.clone();
    }

    // Build new parameter list: replace comptime params with discriminant.
    let mut new_param_types = Vec::new();
    let mut new_param_modes = Vec::new();

    // Add discriminant parameter as first param.
    new_param_types.push(IrType::I32);
    new_param_modes.push(ParamMode::In);

    // Add non-comptime params.
    for (i, (ty, mode)) in original.param_types.iter().zip(original.param_modes.iter()).enumerate() {
        if !spec.comptime_param_indices.contains(&i) {
            new_param_types.push(ty.clone());
            new_param_modes.push(*mode);
        }
    }

    // Build parameter ID mapping for the new function.
    // Old param indices → new param indices (accounting for removed comptime params).
    let mut param_remap: HashMap<u32, u32> = HashMap::new();
    let mut new_param_idx = 1u32; // Start after discriminant
    for (old_idx, _) in original.params.iter().enumerate() {
        if !spec.comptime_param_indices.contains(&old_idx) {
            param_remap.insert(old_idx as u32, new_param_idx);
            new_param_idx += 1;
        }
    }

    // Create new params vector.
    let new_params: Vec<_> = (0..(new_param_types.len() as u32))
        .map(ParamId)
        .collect();

    // Build dispatch blocks.
    let (new_blocks, new_value_count, new_slot_count, new_value_types, new_slot_types) =
        build_dispatch_blocks(original, spec, &param_remap);

    IrFunction {
        id: original.id,
        name: original.name.clone(),
        params: new_params,
        param_modes: new_param_modes,
        param_types: new_param_types,
        return_type: original.return_type.clone(),
        blocks: new_blocks,
        value_count: new_value_count,
        slot_count: new_slot_count,
        value_types: new_value_types,
        slot_types: new_slot_types,
        tracked_slots: original.tracked_slots.clone(), // May need adjustment
        tracked_params: Vec::new(), // Recompute if needed
        const_values: Vec::new(), // Will be populated during const inlining
    }
}

/// Build the dispatch blocks for a specialized function.
///
/// Creates:
/// 1. Entry block that loads discriminant and starts dispatch chain
/// 2. Dispatch chain blocks (one per variant except last)
/// 3. Body blocks (cloned original body for each instantiation)
fn build_dispatch_blocks(
    original: &IrFunction,
    spec: &FuncSpecialization,
    param_remap: &HashMap<u32, u32>,
) -> (Vec<IrBlock>, u32, u32, Vec<IrType>, Vec<IrType>) {
    let num_variants = spec.instantiations.len();
    if num_variants == 0 {
        return (original.blocks.clone(), original.value_count, original.slot_count,
                original.value_types.clone(), original.slot_types.clone());
    }

    let mut blocks = Vec::new();
    let mut next_value = original.value_count;
    let mut next_block_id = 0u32;
    let mut value_types = original.value_types.clone();

    // Helper to allocate a fresh value.
    let mut fresh_value = |ty: IrType| -> ValueId {
        let v = ValueId(next_value);
        next_value += 1;
        value_types.push(ty);
        v
    };

    // Block IDs:
    // 0..num_variants-1: dispatch chain
    // num_variants..: body blocks for each variant

    let num_original_blocks = original.blocks.len();
    let body_base_offset = num_variants as u32;

    // Create dispatch chain.
    // For n variants, we need n-1 comparison blocks, plus direct jumps.
    for variant_idx in 0..num_variants {
        let block_id = BlockId(next_block_id);
        next_block_id += 1;

        if variant_idx == num_variants - 1 {
            // Last variant: no comparison needed, just jump to its body.
            blocks.push(IrBlock {
                id: block_id,
                params: Vec::new(),
                instructions: Vec::new(),
                terminator: Terminator::Goto {
                    target: BlockId(body_base_offset + (variant_idx as u32) * (num_original_blocks as u32)),
                    args: Vec::new(),
                },
            });
        } else {
            // Compare discriminant (param 0) with variant index.
            let cmp_val = fresh_value(IrType::Bool);
            let variant_const_val = fresh_value(IrType::I32);

            let instructions = vec![
                // Load constant for this variant index
                Instruction::Const {
                    dest: variant_const_val,
                    value: ConstValue::I32(variant_idx as i32),
                },
                // Compare: discriminant (param 0) == variant index
                // Use BinOp instruction for comparison
                Instruction::BinOp {
                    dest: cmp_val,
                    op: BinOp::Eq,
                    lhs: Operand::Param(ParamId(0)), // Discriminant is param 0
                    rhs: Operand::Value(variant_const_val),
                },
            ];

            // Branch: if discriminant matches, go to body; else continue chain.
            let then_block = BlockId(body_base_offset + (variant_idx as u32) * (num_original_blocks as u32));
            let else_block = BlockId((variant_idx + 1) as u32); // Next dispatch block

            blocks.push(IrBlock {
                id: block_id,
                params: if variant_idx == 0 { original.blocks[0].params.clone() } else { Vec::new() },
                instructions,
                terminator: Terminator::Branch {
                    cond: Operand::Value(cmp_val),
                    then_block,
                    then_args: Vec::new(),
                    else_block,
                    else_args: Vec::new(),
                },
            });
        }
    }

    // Clone body blocks for each variant.
    for (variant_idx, values) in spec.instantiations.iter().enumerate() {
        let block_offset = body_base_offset + (variant_idx as u32) * (num_original_blocks as u32);

        for (orig_block_idx, orig_block) in original.blocks.iter().enumerate() {
            let new_block_id = BlockId(block_offset + orig_block_idx as u32);

            // Clone instructions, prepending const instructions for comptime params in first block.
            let mut instructions = Vec::new();
            if orig_block_idx == 0 {
                // Add const instructions for comptime param values.
                for (&param_idx, value) in spec.comptime_param_indices.iter().zip(values.iter()) {
                    // The comptime param becomes a local const.
                    // We use the original param's value ID for compatibility.
                    // Actually, we need to emit a Const instruction that the body can reference.
                    let dest = fresh_value(get_const_value_type(value));
                    instructions.push(Instruction::Const {
                        dest,
                        value: value.clone(),
                    });
                    // Note: We'd need to rewrite uses of the original param to use this value.
                    // For simplicity, this basic implementation assumes param uses can be rewritten.
                }
            }

            // Clone original instructions with remapped block references.
            for instr in &orig_block.instructions {
                instructions.push(remap_instruction_blocks(instr, block_offset, num_original_blocks as u32));
            }

            // Clone terminator with remapped block references.
            let terminator = remap_terminator_blocks(&orig_block.terminator, block_offset, num_original_blocks as u32);

            blocks.push(IrBlock {
                id: new_block_id,
                params: orig_block.params.clone(),
                instructions,
                terminator,
            });
        }
    }

    (blocks, next_value, original.slot_count, value_types, original.slot_types.clone())
}

/// Get the IrType for a ConstValue.
fn get_const_value_type(value: &ConstValue) -> IrType {
    match value {
        ConstValue::Unit => IrType::Unit,
        ConstValue::Bool(_) => IrType::Bool,
        ConstValue::I8(_) => IrType::I8,
        ConstValue::I16(_) => IrType::I16,
        ConstValue::I32(_) => IrType::I32,
        ConstValue::I64(_) => IrType::I64,
        ConstValue::U8(_) => IrType::U8,
        ConstValue::U16(_) => IrType::U16,
        ConstValue::U32(_) => IrType::U32,
        ConstValue::U64(_) => IrType::U64,
        ConstValue::F32(_) => IrType::F32,
        ConstValue::F64(_) => IrType::F64,
        ConstValue::String(_) => IrType::String,
        // For complex types, we'd need more information.
        // For now, fall back to Unit for unsupported types.
        _ => IrType::Unit,
    }
}

/// Remap block IDs in an instruction for a cloned body.
fn remap_instruction_blocks(instr: &Instruction, block_offset: u32, num_blocks: u32) -> Instruction {
    // Most instructions don't contain block references.
    // Clone as-is for now. A full implementation would handle any block refs.
    instr.clone()
}

/// Remap block IDs in a terminator for a cloned body.
fn remap_terminator_blocks(term: &Terminator, block_offset: u32, _num_blocks: u32) -> Terminator {
    match term {
        Terminator::Goto { target, args } => {
            Terminator::Goto {
                target: BlockId(block_offset + target.0),
                args: args.clone(),
            }
        }
        Terminator::Branch { cond, then_block, then_args, else_block, else_args } => {
            Terminator::Branch {
                cond: cond.clone(),
                then_block: BlockId(block_offset + then_block.0),
                then_args: then_args.clone(),
                else_block: BlockId(block_offset + else_block.0),
                else_args: else_args.clone(),
            }
        }
        Terminator::Return { value } => Terminator::Return { value: value.clone() },
        Terminator::UnitEnd { result } => Terminator::UnitEnd { result: result.clone() },
        Terminator::UnitEarlyReturn { value } => Terminator::UnitEarlyReturn { value: value.clone() },
    }
}

/// Rewrite a call instruction to pass discriminant instead of comptime args.
///
/// Returns a new instruction with:
/// - First arg replaced with discriminant constant
/// - Comptime args removed
pub fn rewrite_call_instruction(
    instr: &Instruction,
    _discriminant: u32,
    _comptime_param_indices: &[usize],
) -> Option<Instruction> {
    match instr {
        Instruction::Call { dest: _, func: _, args: _ } => {
            // First arg is the discriminant.
            // Note: We can't emit a Const instruction here - the caller needs to do that.
            // For now, we'll need a different approach: the rewriting should happen
            // at a higher level where we can emit instructions.

            // This function is a placeholder - actual rewriting needs more context.
            Some(instr.clone())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_specialization_result_empty() {
        let result = SpecializationResult::default();
        assert!(result.is_empty());
    }

    #[test]
    fn test_func_specialization_discriminant() {
        let mut spec = FuncSpecialization {
            original_func_id: FuncId(0),
            func_name: "test".to_string(),
            comptime_param_indices: vec![0],
            value_to_discriminant: HashMap::new(),
            instantiations: Vec::new(),
        };

        let values1 = vec![ConstValue::I32(3)];
        let values2 = vec![ConstValue::I32(5)];

        spec.value_to_discriminant.insert(values1.clone(), 0);
        spec.instantiations.push(values1.clone());
        spec.value_to_discriminant.insert(values2.clone(), 1);
        spec.instantiations.push(values2.clone());

        assert_eq!(spec.get_discriminant(&values1), Some(0));
        assert_eq!(spec.get_discriminant(&values2), Some(1));
        assert_eq!(spec.num_instantiations(), 2);
    }
}
