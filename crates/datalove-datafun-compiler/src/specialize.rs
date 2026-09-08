//! Const parameter specialization via union-branch transformation.
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
//! // Transformed function: discriminant replaces const param
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
//! 1. Lowering (5a) produces IR functions including const-param functions
//! 2. Const eval (5b) evaluates all const bindings -> ResolvedConsts
//! 3. Specialization (5c) transforms IR functions and rewrites calls
//! 4. Assembly (5d) inlines constants within each branch

use std::collections::HashMap;
use datalove_datafun_ir::{
    ConstValue, IrCodeUnit, IrBlock, Instruction, Terminator, Operand,
    ValueId, BlockId, FuncId, CodeRef, IrType, ParamMode, ParamId,
    IrModuleId, CallSiteId, CodeUnitContext, FunctionContext, SymbolTable,
};
use datalove_datafun_common::ComptimeCallSiteRegistry;

/// Information about a function's const parameter specialization.
#[derive(Clone, Debug)]
pub struct FuncSpecialization {
    /// Original function name.
    pub func_name: String,
    /// Indices of const parameters.
    pub comptime_param_indices: Vec<usize>,
    /// Map from const values tuple to discriminant (0, 1, 2, ...).
    pub value_to_discriminant: HashMap<Vec<ConstValue>, u32>,
    /// All unique instantiations (const value tuples).
    pub instantiations: Vec<Vec<ConstValue>>,
}

impl FuncSpecialization {
    /// Get the discriminant for a given set of const values.
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
            let comptime_indices = registry.get_comptime_indices(call_site.func_name)
                .cloned()
                .unwrap_or_default();

            FuncSpecialization {
                func_name: func_name.clone(),
                comptime_param_indices: comptime_indices,
                value_to_discriminant: HashMap::new(),
                instantiations: Vec::new(),
            }
        });

        // Record the instantiation, giving it a discriminant if it is new.
        // `rewrite_comptime_calls` reads these back by matching the comptime
        // argument values it finds in the IR.
        if !spec.value_to_discriminant.contains_key(&values) {
            let disc = spec.instantiations.len() as u32;
            spec.value_to_discriminant.insert(values.clone(), disc);
            spec.instantiations.push(values);
        }
    }

    SpecializationResult { specialized_funcs }
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
    original: &IrCodeUnit,
    spec: &FuncSpecialization,
) -> IrCodeUnit {
    // For functions with no instantiations, return unchanged.
    if spec.instantiations.is_empty() {
        return original.clone();
    }

    // Extract function context - this function only works on function code units.
    let func_ctx = match &original.context {
        CodeUnitContext::Function(ctx) => ctx,
        _ => return original.clone(),
    };

    // Build new parameter list: replace comptime params with discriminant.
    let mut new_param_types: Vec<IrType> = Vec::new();
    let mut new_param_modes = Vec::new();

    // Add discriminant parameter as first param.
    new_param_types.push(IrType::I32);
    new_param_modes.push(ParamMode::In);

    // Add non-comptime params.
    for (i, (ty, mode)) in func_ctx.param_types.iter().zip(func_ctx.param_modes.iter()).enumerate() {
        if !spec.comptime_param_indices.contains(&i) {
            new_param_types.push(ty.clone());
            new_param_modes.push(*mode);
        }
    }

    // Build parameter ID mapping for the new function.
    // Old param indices → new param indices (accounting for removed comptime params).
    let mut param_remap: HashMap<u32, u32> = HashMap::new();
    let mut new_param_idx = 1u32; // Start after discriminant
    for (old_idx, _) in func_ctx.params.iter().enumerate() {
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
        build_dispatch_blocks(original, func_ctx, spec, &param_remap);

    IrCodeUnit {
        id: original.id,
        name: original.name.clone(),
        blocks: new_blocks,
        value_count: new_value_count,
        slot_count: new_slot_count,
        call_site_count: original.call_site_count,
        value_types: new_value_types,
        slot_types: new_slot_types,
        tracked_slots: original.tracked_slots.clone(),
        const_values: Vec::new(),
        symbols: SymbolTable::default(),
        context: CodeUnitContext::Function(FunctionContext {
            params: new_params,
            param_modes: new_param_modes,
            param_types: new_param_types,
            return_type: func_ctx.return_type.clone(),
            tracked_params: Vec::new(),
            // Specialization drops the comptime parameters and renumbers what
            // is left, so the descriptor-carrying ones move with them.
            descriptor_params: func_ctx.descriptor_params.iter()
                .filter_map(|p| param_remap.get(&p.0).copied().map(ParamId))
                .collect(),
            descriptor_shapes: Vec::new(),
        }),
        nested_units: vec![],
    }
}

/// Build the dispatch blocks for a specialized function.
///
/// Creates:
/// 1. Entry block that loads discriminant and starts dispatch chain
/// 2. Dispatch chain blocks (one per variant except last)
/// 3. Body blocks (cloned original body for each instantiation)
fn build_dispatch_blocks(
    original: &IrCodeUnit,
    func_ctx: &FunctionContext,
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
    let mut value_types = original.value_types.clone();

    // Helper to allocate a fresh value.
    let mut fresh_value = |ty: IrType| -> ValueId {
        let v = ValueId(next_value);
        next_value += 1;
        value_types.push(ty);
        v
    };

    // Block IDs:
    // 0: dispatch block (switch)
    // 1..: body blocks for each variant

    let num_original_blocks = original.blocks.len();
    let body_base_offset = 1u32;

    // Create single dispatch block with Switch terminator.
    let dispatch_block_id = BlockId(0);

    let cases: Vec<(u32, BlockId)> = (0..num_variants)
        .map(|vi| (vi as u32, BlockId(body_base_offset + (vi as u32) * (num_original_blocks as u32))))
        .collect();
    // Default goes to last variant (should be unreachable for well-typed code).
    let default_target = BlockId(body_base_offset + ((num_variants - 1) as u32) * (num_original_blocks as u32));

    blocks.push(IrBlock {
        id: dispatch_block_id,
        params: original.blocks[0].params.clone(),
        instructions: Vec::new(),
        terminator: Terminator::Switch {
            discriminant: Operand::Param(ParamId(0)),
            cases,
            default: default_target,
        },
    });

    // Clone body blocks for each variant.
    for (variant_idx, values) in spec.instantiations.iter().enumerate() {
        let block_offset = body_base_offset + (variant_idx as u32) * (num_original_blocks as u32);

        // Build mapping from comptime param index to the new const value ID for this variant.
        // This mapping is used to rewrite operands in cloned instructions.
        let mut param_to_const: HashMap<u32, ValueId> = HashMap::new();

        for (orig_block_idx, orig_block) in original.blocks.iter().enumerate() {
            let new_block_id = BlockId(block_offset + orig_block_idx as u32);

            // Clone instructions, prepending const instructions for comptime params in first block.
            let mut instructions = Vec::new();
            if orig_block_idx == 0 {
                // Drop the discriminant parameter (p0) - it's only used for dispatch.
                // This must happen in each variant's first block since only one variant executes.
                instructions.push(Instruction::Drop {
                    operand: Operand::Param(ParamId(0)),
                });

                // Add const instructions for comptime param values.
                // Use the original function's param_types to get the correct type for each comptime param.
                for (&param_idx, value) in spec.comptime_param_indices.iter().zip(values.iter()) {
                    let param_type = func_ctx.param_types[param_idx].clone();
                    let dest = fresh_value(param_type);
                    instructions.push(Instruction::Const {
                        dest,
                        value: value.clone(),
                    });
                    // Map the original comptime param to this new const value.
                    param_to_const.insert(param_idx as u32, dest);
                }
            }

            // Clone original instructions with params rewritten.
            // Note: IR instructions don't contain block references, so no block remapping needed.
            for instr in &orig_block.instructions {
                let rewritten = rewrite_comptime_params_in_instruction(instr, &param_to_const, param_remap);
                instructions.push(rewritten);
            }

            // Clone terminator with remapped block references and params.
            let terminator = remap_terminator_blocks(&orig_block.terminator, block_offset, num_original_blocks as u32);
            let terminator = rewrite_comptime_params_in_terminator(&terminator, &param_to_const, param_remap);

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
        Terminator::Switch { discriminant, cases, default } => Terminator::Switch {
            discriminant: discriminant.clone(),
            cases: cases.iter().map(|(v, b)| (*v, BlockId(block_offset + b.0))).collect(),
            default: BlockId(block_offset + default.0),
        },
        Terminator::Return { value } => Terminator::Return { value: value.clone() },
        Terminator::UnitEnd { result } => Terminator::UnitEnd { result: result.clone() },
        Terminator::UnitEarlyReturn { value } => Terminator::UnitEarlyReturn { value: value.clone() },
    }
}

/// Rewrite param references in an instruction.
///
/// - Comptime params: replaced with `Operand::Value(value_id)` pointing to inlined const
/// - Non-comptime params: remapped to new indices (accounting for discriminant + removed params)
fn rewrite_comptime_params_in_instruction(
    instr: &Instruction,
    param_to_const: &HashMap<u32, ValueId>,
    param_remap: &HashMap<u32, u32>,
) -> Instruction {
    let rewrite_operand = |op: &Operand| -> Operand {
        match op {
            Operand::Param(ParamId(idx)) => {
                let idx_u32 = *idx as u32;
                if let Some(&value_id) = param_to_const.get(&idx_u32) {
                    // Comptime param: replace with inlined const value
                    Operand::Value(value_id)
                } else if let Some(&new_idx) = param_remap.get(&idx_u32) {
                    // Non-comptime param: remap to new index
                    Operand::Param(ParamId(new_idx))
                } else {
                    // Should not happen if param_remap is built correctly
                    op.clone()
                }
            }
            _ => op.clone(),
        }
    };

    match instr {
        Instruction::BinOp { dest, op, lhs, rhs } => Instruction::BinOp {
            dest: *dest,
            op: *op,
            lhs: rewrite_operand(lhs),
            rhs: rewrite_operand(rhs),
        },
        Instruction::UnaryOp { dest, op, operand } => Instruction::UnaryOp {
            dest: *dest,
            op: *op,
            operand: rewrite_operand(operand),
        },
        // Rewriting operands leaves the descriptors alone: they say what the
        // callee's type parameters were bound to, which no operand rewrite
        // changes. Blanking them would drop the descriptors a generic callee
        // needs.
        Instruction::Call { site_id, dest, func, args, type_args, shape_descriptors } =>
            Instruction::Call {
                site_id: *site_id,
                dest: *dest,
                func: func.clone(),
                args: args.iter().map(rewrite_operand).collect(),
                type_args: type_args.clone(),
                shape_descriptors: shape_descriptors.clone(),
            },
        Instruction::Copy { dest, src } => Instruction::Copy {
            dest: *dest,
            src: rewrite_operand(src),
        },
        Instruction::Move { dest, src } => Instruction::Move {
            dest: *dest,
            src: rewrite_operand(src),
        },
        Instruction::Drop { operand } => Instruction::Drop {
            operand: rewrite_operand(operand),
        },
        Instruction::DebugLog { operand } => Instruction::DebugLog {
            operand: rewrite_operand(operand),
        },
        Instruction::Widen { dest, src } => Instruction::Widen {
            dest: *dest,
            src: rewrite_operand(src),
        },
        Instruction::WidenFixed { dest, src } => Instruction::WidenFixed {
            dest: *dest,
            src: rewrite_operand(src),
        },
        Instruction::Clone { dest, src } => Instruction::Clone {
            dest: *dest,
            src: rewrite_operand(src),
        },
        // Instructions with aggregate operands
        Instruction::Pack { dest, ty, fields } => Instruction::Pack {
            dest: *dest,
            ty: ty.clone(),
            fields: fields.iter().map(rewrite_operand).collect(),
        },
        Instruction::Unpack { dests, src } => Instruction::Unpack {
            dests: dests.clone(),
            src: rewrite_operand(src),
        },
        Instruction::GetField { dest, src, field_index } => Instruction::GetField {
            dest: *dest,
            src: rewrite_operand(src),
            field_index: *field_index,
        },
        // Pass through instructions without param operands
        _ => instr.clone(),
    }
}

/// Rewrite param references in a terminator.
fn rewrite_comptime_params_in_terminator(
    term: &Terminator,
    param_to_const: &HashMap<u32, ValueId>,
    param_remap: &HashMap<u32, u32>,
) -> Terminator {
    let rewrite_operand = |op: &Operand| -> Operand {
        match op {
            Operand::Param(ParamId(idx)) => {
                let idx_u32 = *idx as u32;
                if let Some(&value_id) = param_to_const.get(&idx_u32) {
                    Operand::Value(value_id)
                } else if let Some(&new_idx) = param_remap.get(&idx_u32) {
                    Operand::Param(ParamId(new_idx))
                } else {
                    op.clone()
                }
            }
            _ => op.clone(),
        }
    };

    match term {
        Terminator::Return { value } => Terminator::Return {
            value: value.as_ref().map(|v| rewrite_operand(v)),
        },
        Terminator::UnitEnd { result } => Terminator::UnitEnd {
            result: result.as_ref().map(|v| rewrite_operand(v)),
        },
        Terminator::UnitEarlyReturn { value } => Terminator::UnitEarlyReturn {
            value: rewrite_operand(value),
        },
        Terminator::Goto { target, args } => Terminator::Goto {
            target: *target,
            args: args.iter().map(rewrite_operand).collect(),
        },
        Terminator::Branch { cond, then_block, then_args, else_block, else_args } => Terminator::Branch {
            cond: rewrite_operand(cond),
            then_block: *then_block,
            then_args: then_args.iter().map(rewrite_operand).collect(),
            else_block: *else_block,
            else_args: else_args.iter().map(rewrite_operand).collect(),
        },
        Terminator::Switch { discriminant, cases, default } => Terminator::Switch {
            discriminant: rewrite_operand(discriminant),
            cases: cases.clone(),
            default: *default,
        },
    }
}

/// Rewrite ComptimeCall instructions in a code unit.
///
/// For each ComptimeCall:
/// 1. Resolve comptime arg values from the IR (find Const instructions that define them)
/// 2. Look up discriminant from the specialization plan
/// 3. Replace with: Const(discriminant) + Call with non-comptime args
///
/// The `func_id_to_name` map is used to resolve CodeRef to function names.
/// It's keyed by (IrModuleId, FuncId) because FuncId is only unique within a module.
///
/// `current_module` is the IrModuleId of the module containing this function,
/// used for resolving local function references.
///
/// Returns the transformed code unit.
pub fn rewrite_comptime_calls(
    func: &IrCodeUnit,
    spec_result: &SpecializationResult,
    func_id_to_name: &HashMap<(IrModuleId, FuncId), String>,
    current_module: IrModuleId,
    value_types: &mut Vec<IrType>,
    next_value: &mut u32,
) -> IrCodeUnit {
    // Build a map from ValueId to ConstValue for values defined by Const instructions.
    let const_values = build_const_value_map(func);

    let mut new_blocks = Vec::new();
    let mut next_call_site = func.call_site_count;

    for block in &func.blocks {
        let mut new_instructions = Vec::new();

        for instr in &block.instructions {
            match instr {
                Instruction::ComptimeCall { dest, func: func_ref, args, discriminant: _, comptime_param_indices } => {
                    // Get the function name from CodeRef.
                    let func_name = get_func_name_from_ref(&func_ref, func_id_to_name, current_module);

                    if let Some(ref name) = func_name {
                        if let Some(spec) = spec_result.specialized_funcs.get(name) {
                            // Resolve comptime arg values.
                            let comptime_values: Vec<ConstValue> = comptime_param_indices.iter()
                                .filter_map(|&idx| {
                                    if idx < args.len() {
                                        resolve_operand_value(&args[idx], &const_values)
                                    } else {
                                        None
                                    }
                                })
                                .collect();

                            // Look up discriminant.
                            if let Some(disc) = spec.get_discriminant(&comptime_values) {
                                // Allocate fresh value for discriminant constant.
                                let disc_val = ValueId(*next_value);
                                *next_value += 1;
                                value_types.push(IrType::I32);

                                // Emit Const instruction for discriminant.
                                new_instructions.push(Instruction::Const {
                                    dest: disc_val,
                                    value: ConstValue::I32(disc as i32),
                                });

                                // Build new args: discriminant + non-comptime args.
                                // Also collect comptime args that need to be dropped.
                                let mut new_args = vec![Operand::Value(disc_val)];
                                let mut comptime_args_to_drop = Vec::new();
                                for (i, arg) in args.iter().enumerate() {
                                    if comptime_param_indices.contains(&i) {
                                        // Comptime arg is not passed - needs to be dropped.
                                        comptime_args_to_drop.push(arg.clone());
                                    } else {
                                        new_args.push(arg.clone());
                                    }
                                }

                                // Drop comptime args that are no longer being passed.
                                // These were originally passed to the function but are now
                                // replaced by the discriminant + inlined const values.
                                for arg in comptime_args_to_drop {
                                    new_instructions.push(Instruction::Drop { operand: arg });
                                }

                                // Emit Call instruction with new site_id.
                                let site_id = CallSiteId(next_call_site);
                                next_call_site += 1;
                                new_instructions.push(Instruction::Call {
                                    site_id,
                                    dest: *dest,
                                    func: func_ref.clone(),
                                    args: new_args,
                                    // A comptime call is not generic.
                                    type_args: Vec::new(),
                                    shape_descriptors: Vec::new(),
                                });
                                continue;
                            }
                        }
                    }

                    // ComptimeCall should always be rewritable if specialization is enabled.
                    // If we reach here, it indicates a bug in the specialization pipeline.
                    panic!(
                        "ComptimeCall not found in specialization plan: func={:?}, \
                         func_name={:?}, comptime_indices={:?}. \
                         This indicates a mismatch between typecheck registry and IR lowering.",
                        func_ref, func_name, comptime_param_indices
                    );
                }
                _ => {
                    new_instructions.push(instr.clone());
                }
            }
        }

        new_blocks.push(IrBlock {
            id: block.id,
            params: block.params.clone(),
            instructions: new_instructions,
            terminator: block.terminator.clone(),
        });
    }

    IrCodeUnit {
        id: func.id,
        name: func.name.clone(),
        blocks: new_blocks,
        value_count: *next_value,
        slot_count: func.slot_count,
        call_site_count: next_call_site,
        value_types: value_types.clone(),
        slot_types: func.slot_types.clone(),
        tracked_slots: func.tracked_slots.clone(),
        const_values: func.const_values.clone(),
        symbols: func.symbols.clone(),
        context: func.context.clone(),
        nested_units: func.nested_units.clone(),
    }
}

/// Get the function name from a CodeRef using the provided mapping.
///
/// `current_module` is the IrModuleId of the function containing this call,
/// used for resolving CodeRef::Local.
fn get_func_name_from_ref(
    code_ref: &CodeRef,
    func_id_to_name: &HashMap<(IrModuleId, FuncId), String>,
    current_module: IrModuleId,
) -> Option<String> {
    match code_ref {
        CodeRef::Local(id) => func_id_to_name.get(&(current_module, FuncId(id.0))).cloned(),
        CodeRef::Module { module, id } => func_id_to_name.get(&(*module, FuncId(id.0))).cloned(),
        // External calls (from previous script units) are not supported for comptime specialization.
        CodeRef::External { .. } => None,
    }
}

/// Build a map from ValueId to ConstValue for values defined by Const instructions.
fn build_const_value_map(func: &IrCodeUnit) -> HashMap<ValueId, ConstValue> {
    let mut map = HashMap::new();

    for block in &func.blocks {
        for instr in &block.instructions {
            match instr {
                Instruction::Const { dest, value } => {
                    map.insert(*dest, value.clone());
                }
                // A clone of a constant is that constant. Reading a const of a
                // linear type clones it, so that each read has a value of its
                // own, and a comptime argument read that way is still the
                // constant the call site wrote.
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

/// Resolve an operand to a ConstValue if possible.
///
/// Looks up the value from the const_values map if the operand is a Value.
fn resolve_operand_value(operand: &Operand, const_values: &HashMap<ValueId, ConstValue>) -> Option<ConstValue> {
    match operand {
        Operand::Value(vid) => const_values.get(vid).cloned(),
        // Other operand types (Slot, Param, etc.) cannot be resolved to const values directly.
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
