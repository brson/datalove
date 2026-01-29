//! Function inlining pass for datafun IR.
//!
//! This crate provides:
//! - Inline directive types for specifying which functions to inline
//! - Function inlining transformation on IR modules

use std::collections::HashMap;

use datalove_datafun_ir::{
    BlockId, FuncId, FuncRef, Instruction, IrBlock, IrFunction, IrModule, IrType,
    Operand, ParamId, SlotDest, SlotId, Terminator, ValueId,
};

/// Directive specifying which function calls to inline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineDirective {
    /// Inline all calls to `callee` within `caller`.
    Inline { caller: String, callee: String },
    /// Inline only the Nth call (0-indexed) to `callee` within `caller`.
    InlineAt {
        caller: String,
        callee: String,
        call_index: usize,
    },
    /// Inline all calls to `callee` in all functions.
    InlineAll { callee: String },
}

/// Parse inline directives from source text.
///
/// Format:
/// ```text
/// inline caller_function callee_function
/// inline caller_function callee_function at N
/// inline-all callee_function
/// ```
pub fn parse_inline_directives(source: &str) -> Result<Vec<InlineDirective>, String> {
    let mut directives = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line = line.trim();

        // Skip empty lines and comments.
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();

        let directive = match parts.as_slice() {
            ["inline", caller, callee] => InlineDirective::Inline {
                caller: (*caller).to_string(),
                callee: (*callee).to_string(),
            },
            ["inline", caller, callee, "at", idx] => {
                let call_index: usize = idx.parse().map_err(|_| {
                    format!(
                        "line {}: invalid call index '{}' (expected integer)",
                        line_num + 1,
                        idx
                    )
                })?;
                InlineDirective::InlineAt {
                    caller: (*caller).to_string(),
                    callee: (*callee).to_string(),
                    call_index,
                }
            }
            ["inline-all", callee] => InlineDirective::InlineAll {
                callee: (*callee).to_string(),
            },
            _ => {
                return Err(format!(
                    "line {}: invalid inline directive '{}' \
                     (expected 'inline caller callee', 'inline caller callee at N', \
                     or 'inline-all callee')",
                    line_num + 1,
                    line
                ))
            }
        };

        directives.push(directive);
    }

    Ok(directives)
}

/// Specifies which call sites to inline.
#[derive(Clone, Debug)]
pub enum CallSiteFilter {
    /// Inline all calls to the callee.
    All,
    /// Inline only the Nth call (0-indexed).
    AtIndex(usize),
}

/// Request to inline specific calls.
#[derive(Clone, Debug)]
pub struct InlineRequest {
    pub caller: FuncId,
    pub callee: FuncId,
    pub filter: CallSiteFilter,
}

/// Reason why an inlining was skipped.
#[derive(Clone, Debug)]
pub enum InlineSkipReason {
    /// The caller function was not found.
    CallerNotFound { name: String },
    /// The callee function was not found.
    CalleeNotFound { name: String },
    /// Recursive call detected.
    RecursiveCall { func: String },
    /// The specified call site was not found.
    CallSiteNotFound {
        caller: String,
        callee: String,
        index: usize,
    },
    /// No calls to callee found in caller.
    NoCallsFound { caller: String, callee: String },
}

/// Result of the inlining pass.
#[derive(Clone, Debug)]
pub struct InlineResult {
    /// The transformed module.
    pub module: IrModule,
    /// Number of call sites inlined.
    pub inlined_count: usize,
    /// Reasons why some inlinings were skipped.
    pub skipped: Vec<InlineSkipReason>,
}

/// Resolve inline directives to concrete requests using the module's symbol table.
pub fn resolve_directives(
    module: &IrModule,
    directives: &[InlineDirective],
) -> (Vec<InlineRequest>, Vec<InlineSkipReason>) {
    let mut requests = Vec::new();
    let mut skipped = Vec::new();

    // Build name to FuncId map.
    let name_to_id: HashMap<&str, FuncId> = module
        .symbols
        .functions
        .iter()
        .map(|def| (def.name.as_str(), def.id))
        .collect();

    for directive in directives {
        match directive {
            InlineDirective::Inline { caller, callee } => {
                let Some(&caller_id) = name_to_id.get(caller.as_str()) else {
                    skipped.push(InlineSkipReason::CallerNotFound {
                        name: caller.clone(),
                    });
                    continue;
                };
                let Some(&callee_id) = name_to_id.get(callee.as_str()) else {
                    skipped.push(InlineSkipReason::CalleeNotFound {
                        name: callee.clone(),
                    });
                    continue;
                };

                // Check for recursion.
                if caller_id == callee_id {
                    skipped.push(InlineSkipReason::RecursiveCall {
                        func: caller.clone(),
                    });
                    continue;
                }

                requests.push(InlineRequest {
                    caller: caller_id,
                    callee: callee_id,
                    filter: CallSiteFilter::All,
                });
            }

            InlineDirective::InlineAt {
                caller,
                callee,
                call_index,
            } => {
                let Some(&caller_id) = name_to_id.get(caller.as_str()) else {
                    skipped.push(InlineSkipReason::CallerNotFound {
                        name: caller.clone(),
                    });
                    continue;
                };
                let Some(&callee_id) = name_to_id.get(callee.as_str()) else {
                    skipped.push(InlineSkipReason::CalleeNotFound {
                        name: callee.clone(),
                    });
                    continue;
                };

                if caller_id == callee_id {
                    skipped.push(InlineSkipReason::RecursiveCall {
                        func: caller.clone(),
                    });
                    continue;
                }

                requests.push(InlineRequest {
                    caller: caller_id,
                    callee: callee_id,
                    filter: CallSiteFilter::AtIndex(*call_index),
                });
            }

            InlineDirective::InlineAll { callee } => {
                let Some(&callee_id) = name_to_id.get(callee.as_str()) else {
                    skipped.push(InlineSkipReason::CalleeNotFound {
                        name: callee.clone(),
                    });
                    continue;
                };

                // Add request for each function that is not the callee.
                for def in &module.symbols.functions {
                    if def.id != callee_id {
                        requests.push(InlineRequest {
                            caller: def.id,
                            callee: callee_id,
                            filter: CallSiteFilter::All,
                        });
                    }
                }
            }
        }
    }

    (requests, skipped)
}

/// Information about a call site to inline.
#[derive(Clone, Debug)]
struct CallSite {
    block_idx: usize,
    instr_idx: usize,
    dest: ValueId,
    args: Vec<Operand>,
}

/// Find all call sites to a specific callee in a function.
fn find_call_sites(func: &IrFunction, callee_id: FuncId) -> Vec<CallSite> {
    let mut sites = Vec::new();

    for (block_idx, block) in func.blocks.iter().enumerate() {
        for (instr_idx, instr) in block.instructions.iter().enumerate() {
            if let Instruction::Call { dest, func: func_ref, args } = instr {
                // Match both Local and Module function references.
                let matches = match func_ref {
                    FuncRef::Local(id) => *id == callee_id,
                    FuncRef::Module { func, .. } => *func == callee_id,
                    FuncRef::External { .. } => false,
                };
                if matches {
                    sites.push(CallSite {
                        block_idx,
                        instr_idx,
                        dest: *dest,
                        args: args.clone(),
                    });
                }
            }
        }
    }

    sites
}

/// ID remapping context for inlining a callee into a caller.
struct RemapContext {
    value_offset: u32,
    slot_offset: u32,
    block_offset: u32,
}

impl RemapContext {
    fn remap_value(&self, v: ValueId) -> ValueId {
        ValueId(v.0 + self.value_offset)
    }

    fn remap_slot(&self, s: SlotId) -> SlotId {
        SlotId(s.0 + self.slot_offset)
    }

    fn remap_block(&self, b: BlockId) -> BlockId {
        BlockId(b.0 + self.block_offset)
    }

    fn remap_operand(&self, op: &Operand) -> Operand {
        match op {
            Operand::Value(v) => Operand::Value(self.remap_value(*v)),
            Operand::ValueRef(v) => Operand::ValueRef(self.remap_value(*v)),
            Operand::Slot(s) => Operand::Slot(self.remap_slot(*s)),
            // Params will be replaced with actual arguments, not remapped.
            Operand::Param(p) => Operand::Param(*p),
            // External operands are not remapped.
            Operand::ExternalValue { unit, value } => Operand::ExternalValue {
                unit: *unit,
                value: *value,
            },
            Operand::ExternalSlot { unit, slot } => Operand::ExternalSlot {
                unit: *unit,
                slot: *slot,
            },
        }
    }

    fn remap_slot_dest(&self, dest: &SlotDest) -> SlotDest {
        match dest {
            SlotDest::Local(s) => SlotDest::Local(self.remap_slot(*s)),
            SlotDest::External { unit, slot } => SlotDest::External {
                unit: *unit,
                slot: *slot,
            },
        }
    }

    fn remap_instruction(&self, instr: &Instruction) -> Instruction {
        match instr {
            Instruction::Const { dest, value } => Instruction::Const {
                dest: self.remap_value(*dest),
                value: value.clone(),
            },
            Instruction::Copy { dest, src } => Instruction::Copy {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::Move { dest, src } => Instruction::Move {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::BinOp { dest, op, lhs, rhs } => Instruction::BinOp {
                dest: self.remap_value(*dest),
                op: *op,
                lhs: self.remap_operand(lhs),
                rhs: self.remap_operand(rhs),
            },
            Instruction::UnaryOp { dest, op, operand } => Instruction::UnaryOp {
                dest: self.remap_value(*dest),
                op: *op,
                operand: self.remap_operand(operand),
            },
            Instruction::BinOpChecked {
                dest,
                overflow,
                op,
                lhs,
                rhs,
            } => Instruction::BinOpChecked {
                dest: self.remap_value(*dest),
                overflow: self.remap_value(*overflow),
                op: *op,
                lhs: self.remap_operand(lhs),
                rhs: self.remap_operand(rhs),
            },
            Instruction::UnaryOpChecked {
                dest,
                overflow,
                op,
                operand,
            } => Instruction::UnaryOpChecked {
                dest: self.remap_value(*dest),
                overflow: self.remap_value(*overflow),
                op: *op,
                operand: self.remap_operand(operand),
            },
            Instruction::Widen { dest, src } => Instruction::Widen {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::WidenFixed { dest, src } => Instruction::WidenFixed {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::Clone { dest, src } => Instruction::Clone {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
            },
            Instruction::Call { dest, func, args } => Instruction::Call {
                dest: self.remap_value(*dest),
                func: func.clone(),
                args: args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Instruction::Pack { dest, ty, fields } => Instruction::Pack {
                dest: self.remap_value(*dest),
                ty: ty.clone(),
                fields: fields.iter().map(|f| self.remap_operand(f)).collect(),
            },
            Instruction::Unpack { dests, src } => Instruction::Unpack {
                dests: dests.iter().map(|d| self.remap_value(*d)).collect(),
                src: self.remap_operand(src),
            },
            Instruction::GetField {
                dest,
                src,
                field_index,
            } => Instruction::GetField {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
                field_index: *field_index,
            },
            Instruction::GetFieldRef {
                dest,
                src,
                field_index,
            } => Instruction::GetFieldRef {
                dest: self.remap_value(*dest),
                src: self.remap_operand(src),
                field_index: *field_index,
            },
            Instruction::WrapSome { dest, inner } => Instruction::WrapSome {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::WrapNone { dest } => Instruction::WrapNone {
                dest: self.remap_value(*dest),
            },
            Instruction::WrapOk { dest, inner } => Instruction::WrapOk {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::WrapErr { dest, inner } => Instruction::WrapErr {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::EnumVariant {
                dest,
                variant_index,
                payload,
            } => Instruction::EnumVariant {
                dest: self.remap_value(*dest),
                variant_index: *variant_index,
                payload: payload.as_ref().map(|p| self.remap_operand(p)),
            },
            Instruction::UnwrapOption { dest, is_some, src } => Instruction::UnwrapOption {
                dest: self.remap_value(*dest),
                is_some: self.remap_value(*is_some),
                src: self.remap_operand(src),
            },
            Instruction::UnwrapResult {
                ok_dest,
                err_dest,
                is_ok,
                src,
            } => Instruction::UnwrapResult {
                ok_dest: self.remap_value(*ok_dest),
                err_dest: self.remap_value(*err_dest),
                is_ok: self.remap_value(*is_ok),
                src: self.remap_operand(src),
            },
            Instruction::ErrorFrom { dest, inner } => Instruction::ErrorFrom {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::DataFrom { dest, inner } => Instruction::DataFrom {
                dest: self.remap_value(*dest),
                inner: self.remap_operand(inner),
            },
            Instruction::ListNew { dest, elements } => Instruction::ListNew {
                dest: self.remap_value(*dest),
                elements: elements.iter().map(|e| self.remap_operand(e)).collect(),
            },
            Instruction::SetNew { dest, elements } => Instruction::SetNew {
                dest: self.remap_value(*dest),
                elements: elements.iter().map(|e| self.remap_operand(e)).collect(),
            },
            Instruction::MapNew { dest, entries } => Instruction::MapNew {
                dest: self.remap_value(*dest),
                entries: entries
                    .iter()
                    .map(|(k, v)| (self.remap_operand(k), self.remap_operand(v)))
                    .collect(),
            },
            Instruction::TensorNew {
                dest,
                shape,
                elements,
            } => Instruction::TensorNew {
                dest: self.remap_value(*dest),
                shape: shape.clone(),
                elements: elements.iter().map(|e| self.remap_operand(e)).collect(),
            },
            Instruction::TableNew { dest, rows } => Instruction::TableNew {
                dest: self.remap_value(*dest),
                rows: rows.iter().map(|r| self.remap_operand(r)).collect(),
            },
            Instruction::SlotStoreCopy { dest, value } => Instruction::SlotStoreCopy {
                dest: self.remap_slot_dest(dest),
                value: self.remap_operand(value),
            },
            Instruction::SlotStoreCopyTracked { dest, value } => {
                Instruction::SlotStoreCopyTracked {
                    dest: self.remap_slot_dest(dest),
                    value: self.remap_operand(value),
                }
            }
            Instruction::SlotStoreMove { dest, value } => Instruction::SlotStoreMove {
                dest: self.remap_slot_dest(dest),
                value: self.remap_operand(value),
            },
            Instruction::SlotStoreMoveTracked { dest, value } => {
                Instruction::SlotStoreMoveTracked {
                    dest: self.remap_slot_dest(dest),
                    value: self.remap_operand(value),
                }
            }
            Instruction::SetField {
                slot,
                field_path,
                value,
            } => Instruction::SetField {
                slot: self.remap_slot_dest(slot),
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::SetFieldTracked {
                slot,
                field_path,
                value,
            } => Instruction::SetFieldTracked {
                slot: self.remap_slot_dest(slot),
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::ParamStore { param, value } => Instruction::ParamStore {
                param: *param,
                value: self.remap_operand(value),
            },
            Instruction::ParamStoreTracked { param, value } => Instruction::ParamStoreTracked {
                param: *param,
                value: self.remap_operand(value),
            },
            Instruction::ParamSetField {
                param,
                field_path,
                value,
            } => Instruction::ParamSetField {
                param: *param,
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::ParamSetFieldTracked {
                param,
                field_path,
                value,
            } => Instruction::ParamSetFieldTracked {
                param: *param,
                field_path: field_path.clone(),
                value: self.remap_operand(value),
            },
            Instruction::SlotLoadCopy { dest, slot } => Instruction::SlotLoadCopy {
                dest: self.remap_value(*dest),
                slot: self.remap_slot(*slot),
            },
            Instruction::SlotLoadMove { dest, slot } => Instruction::SlotLoadMove {
                dest: self.remap_value(*dest),
                slot: self.remap_slot(*slot),
            },
            Instruction::SlotLoadMoveTracked { dest, slot } => Instruction::SlotLoadMoveTracked {
                dest: self.remap_value(*dest),
                slot: self.remap_slot(*slot),
            },
            Instruction::Drop { operand } => Instruction::Drop {
                operand: self.remap_operand(operand),
            },
            Instruction::DropTracked { operand } => Instruction::DropTracked {
                operand: self.remap_operand(operand),
            },
            Instruction::DropViaRef { ref_value } => Instruction::DropViaRef {
                ref_value: self.remap_value(*ref_value),
            },
            Instruction::UnitEndDrop { operand } => Instruction::UnitEndDrop {
                operand: self.remap_operand(operand),
            },
            Instruction::UnitEndDropTracked { operand } => Instruction::UnitEndDropTracked {
                operand: self.remap_operand(operand),
            },
            Instruction::DebugLog { operand } => Instruction::DebugLog {
                operand: self.remap_operand(operand),
            },
            Instruction::Intrinsic {
                dest,
                intrinsic,
                args,
            } => Instruction::Intrinsic {
                dest: self.remap_value(*dest),
                intrinsic: intrinsic.clone(),
                args: args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Instruction::Nop => Instruction::Nop,
        }
    }

    fn remap_terminator(&self, term: &Terminator, continuation_block: BlockId) -> Terminator {
        match term {
            Terminator::Goto { target, args } => Terminator::Goto {
                target: self.remap_block(*target),
                args: args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Terminator::Branch {
                cond,
                then_block,
                then_args,
                else_block,
                else_args,
            } => Terminator::Branch {
                cond: self.remap_operand(cond),
                then_block: self.remap_block(*then_block),
                then_args: then_args.iter().map(|a| self.remap_operand(a)).collect(),
                else_block: self.remap_block(*else_block),
                else_args: else_args.iter().map(|a| self.remap_operand(a)).collect(),
            },
            Terminator::Return { value } => {
                // Return becomes a goto to the continuation block.
                Terminator::Goto {
                    target: continuation_block,
                    args: value.iter().map(|v| self.remap_operand(v)).collect(),
                }
            }
            // These shouldn't appear in function bodies being inlined.
            Terminator::UnitEnd { result } => Terminator::UnitEnd {
                result: result.as_ref().map(|r| self.remap_operand(r)),
            },
            Terminator::UnitEarlyReturn { value } => Terminator::UnitEarlyReturn {
                value: self.remap_operand(value),
            },
        }
    }
}

/// Inline a single call site in a function.
///
/// Returns the new function with the call inlined, or None if inlining failed.
fn inline_call_site(
    caller: &IrFunction,
    callee: &IrFunction,
    site: &CallSite,
) -> Option<IrFunction> {
    let mut new_func = caller.clone();

    // Set up remapping context.
    let remap = RemapContext {
        value_offset: caller.value_count,
        slot_offset: caller.slot_count,
        block_offset: caller.blocks.len() as u32,
    };

    // The continuation block receives the return value.
    // Its block ID is after all the inlined blocks.
    let continuation_block_id = BlockId(remap.block_offset + callee.blocks.len() as u32);

    // Split the original block at the call site.
    let orig_block = &caller.blocks[site.block_idx];

    // Instructions before the call stay in the original block.
    let before_call: Vec<Instruction> = orig_block.instructions[..site.instr_idx].to_vec();

    // Instructions after the call go to the continuation block.
    let after_call: Vec<Instruction> = orig_block.instructions[site.instr_idx + 1..].to_vec();

    // The original terminator goes to the continuation block.
    let orig_terminator = orig_block.terminator.clone();

    // Build parameter binding instructions for the inlined entry block.
    // These copy/move arguments into the positions the callee expects.
    let mut param_bindings: Vec<Instruction> = Vec::new();
    for (i, (param_id, arg)) in callee.params.iter().zip(site.args.iter()).enumerate() {
        // Create a value that holds the argument in the callee's value space.
        let dest_value = remap.remap_value(ValueId(callee.value_count + i as u32));
        let param_type = &callee.param_types[param_id.0 as usize];

        // Use Copy for copy types, Move for non-copy types.
        let instr = if param_type.is_copy() {
            Instruction::Copy {
                dest: dest_value,
                src: arg.clone(),
            }
        } else {
            Instruction::Move {
                dest: dest_value,
                src: arg.clone(),
            }
        };
        param_bindings.push(instr);
    }

    // Update the original block: keep instructions before call, jump to inlined entry.
    let inlined_entry_block = remap.remap_block(BlockId(0));
    new_func.blocks[site.block_idx] = IrBlock {
        id: BlockId(site.block_idx as u32),
        params: orig_block.params.clone(),
        instructions: before_call,
        terminator: Terminator::Goto {
            target: inlined_entry_block,
            args: vec![],
        },
    };

    // Copy callee blocks with remapped IDs.
    for (i, block) in callee.blocks.iter().enumerate() {
        let mut new_instructions: Vec<Instruction> = Vec::new();

        // Add parameter bindings to the entry block.
        if i == 0 {
            new_instructions.extend(param_bindings.clone());
        }

        // Remap and copy instructions, replacing Param operands with the bound values.
        for instr in &block.instructions {
            let remapped = remap.remap_instruction(instr);
            // Replace Param operands with the corresponding bound values.
            let replaced = replace_params_in_instruction(
                &remapped,
                &callee.params,
                callee.value_count,
                &remap,
            );
            new_instructions.push(replaced);
        }

        let new_block = IrBlock {
            id: remap.remap_block(block.id),
            params: block.params.iter().map(|v| remap.remap_value(*v)).collect(),
            instructions: new_instructions,
            terminator: remap.remap_terminator(&block.terminator, continuation_block_id),
        };
        new_func.blocks.push(new_block);
    }

    // Create continuation block.
    // If the callee returns a value, it becomes a block parameter.
    let cont_params = if callee.return_type != IrType::Unit {
        vec![site.dest]
    } else {
        vec![]
    };

    let continuation_block = IrBlock {
        id: continuation_block_id,
        params: cont_params,
        instructions: after_call,
        terminator: orig_terminator,
    };
    new_func.blocks.push(continuation_block);

    // Update function metadata.
    // Add callee's values + param binding values.
    let extra_values = callee.value_count + callee.params.len() as u32;
    new_func.value_count += extra_values;
    new_func.slot_count += callee.slot_count;

    // Extend type arrays.
    new_func.value_types.extend(callee.value_types.iter().cloned());
    // Add types for param binding values.
    for param_id in &callee.params {
        let ty = callee.param_types[param_id.0 as usize].clone();
        new_func.value_types.push(ty);
    }
    new_func.slot_types.extend(callee.slot_types.iter().cloned());

    // Extend tracked slots (with offset).
    for slot in &callee.tracked_slots {
        new_func.tracked_slots.push(remap.remap_slot(*slot));
    }

    Some(new_func)
}

/// Replace Param operands in an instruction with the corresponding bound values.
fn replace_params_in_instruction(
    instr: &Instruction,
    params: &[ParamId],
    callee_value_count: u32,
    remap: &RemapContext,
) -> Instruction {
    let replace_operand = |op: &Operand| -> Operand {
        if let Operand::Param(p) = op {
            // Find the index of this param.
            if let Some(idx) = params.iter().position(|param| param == p) {
                // The bound value is at callee_value_count + idx.
                let bound_value = remap.remap_value(ValueId(callee_value_count + idx as u32));
                return Operand::Value(bound_value);
            }
        }
        op.clone()
    };

    match instr {
        Instruction::Copy { dest, src } => Instruction::Copy {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Move { dest, src } => Instruction::Move {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::BinOp { dest, op, lhs, rhs } => Instruction::BinOp {
            dest: *dest,
            op: *op,
            lhs: replace_operand(lhs),
            rhs: replace_operand(rhs),
        },
        Instruction::UnaryOp { dest, op, operand } => Instruction::UnaryOp {
            dest: *dest,
            op: *op,
            operand: replace_operand(operand),
        },
        Instruction::BinOpChecked {
            dest,
            overflow,
            op,
            lhs,
            rhs,
        } => Instruction::BinOpChecked {
            dest: *dest,
            overflow: *overflow,
            op: *op,
            lhs: replace_operand(lhs),
            rhs: replace_operand(rhs),
        },
        Instruction::UnaryOpChecked {
            dest,
            overflow,
            op,
            operand,
        } => Instruction::UnaryOpChecked {
            dest: *dest,
            overflow: *overflow,
            op: *op,
            operand: replace_operand(operand),
        },
        Instruction::Widen { dest, src } => Instruction::Widen {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::WidenFixed { dest, src } => Instruction::WidenFixed {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Clone { dest, src } => Instruction::Clone {
            dest: *dest,
            src: replace_operand(src),
        },
        Instruction::Call { dest, func, args } => Instruction::Call {
            dest: *dest,
            func: func.clone(),
            args: args.iter().map(replace_operand).collect(),
        },
        Instruction::Pack { dest, ty, fields } => Instruction::Pack {
            dest: *dest,
            ty: ty.clone(),
            fields: fields.iter().map(replace_operand).collect(),
        },
        Instruction::Unpack { dests, src } => Instruction::Unpack {
            dests: dests.clone(),
            src: replace_operand(src),
        },
        Instruction::GetField {
            dest,
            src,
            field_index,
        } => Instruction::GetField {
            dest: *dest,
            src: replace_operand(src),
            field_index: *field_index,
        },
        Instruction::GetFieldRef {
            dest,
            src,
            field_index,
        } => Instruction::GetFieldRef {
            dest: *dest,
            src: replace_operand(src),
            field_index: *field_index,
        },
        Instruction::WrapSome { dest, inner } => Instruction::WrapSome {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::WrapOk { dest, inner } => Instruction::WrapOk {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::WrapErr { dest, inner } => Instruction::WrapErr {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::EnumVariant {
            dest,
            variant_index,
            payload,
        } => Instruction::EnumVariant {
            dest: *dest,
            variant_index: *variant_index,
            payload: payload.as_ref().map(replace_operand),
        },
        Instruction::UnwrapOption { dest, is_some, src } => Instruction::UnwrapOption {
            dest: *dest,
            is_some: *is_some,
            src: replace_operand(src),
        },
        Instruction::UnwrapResult {
            ok_dest,
            err_dest,
            is_ok,
            src,
        } => Instruction::UnwrapResult {
            ok_dest: *ok_dest,
            err_dest: *err_dest,
            is_ok: *is_ok,
            src: replace_operand(src),
        },
        Instruction::ErrorFrom { dest, inner } => Instruction::ErrorFrom {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::DataFrom { dest, inner } => Instruction::DataFrom {
            dest: *dest,
            inner: replace_operand(inner),
        },
        Instruction::ListNew { dest, elements } => Instruction::ListNew {
            dest: *dest,
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::SetNew { dest, elements } => Instruction::SetNew {
            dest: *dest,
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::MapNew { dest, entries } => Instruction::MapNew {
            dest: *dest,
            entries: entries
                .iter()
                .map(|(k, v)| (replace_operand(k), replace_operand(v)))
                .collect(),
        },
        Instruction::TensorNew {
            dest,
            shape,
            elements,
        } => Instruction::TensorNew {
            dest: *dest,
            shape: shape.clone(),
            elements: elements.iter().map(replace_operand).collect(),
        },
        Instruction::TableNew { dest, rows } => Instruction::TableNew {
            dest: *dest,
            rows: rows.iter().map(replace_operand).collect(),
        },
        Instruction::SlotStoreCopy { dest, value } => Instruction::SlotStoreCopy {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreCopyTracked { dest, value } => Instruction::SlotStoreCopyTracked {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreMove { dest, value } => Instruction::SlotStoreMove {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SlotStoreMoveTracked { dest, value } => Instruction::SlotStoreMoveTracked {
            dest: dest.clone(),
            value: replace_operand(value),
        },
        Instruction::SetField {
            slot,
            field_path,
            value,
        } => Instruction::SetField {
            slot: slot.clone(),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::SetFieldTracked {
            slot,
            field_path,
            value,
        } => Instruction::SetFieldTracked {
            slot: slot.clone(),
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::ParamStore { param, value } => Instruction::ParamStore {
            param: *param,
            value: replace_operand(value),
        },
        Instruction::ParamStoreTracked { param, value } => Instruction::ParamStoreTracked {
            param: *param,
            value: replace_operand(value),
        },
        Instruction::ParamSetField {
            param,
            field_path,
            value,
        } => Instruction::ParamSetField {
            param: *param,
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::ParamSetFieldTracked {
            param,
            field_path,
            value,
        } => Instruction::ParamSetFieldTracked {
            param: *param,
            field_path: field_path.clone(),
            value: replace_operand(value),
        },
        Instruction::Drop { operand } => Instruction::Drop {
            operand: replace_operand(operand),
        },
        Instruction::DropTracked { operand } => Instruction::DropTracked {
            operand: replace_operand(operand),
        },
        Instruction::DebugLog { operand } => Instruction::DebugLog {
            operand: replace_operand(operand),
        },
        Instruction::Intrinsic {
            dest,
            intrinsic,
            args,
        } => Instruction::Intrinsic {
            dest: *dest,
            intrinsic: intrinsic.clone(),
            args: args.iter().map(replace_operand).collect(),
        },
        // These don't have replaceable operands.
        Instruction::Const { .. }
        | Instruction::WrapNone { .. }
        | Instruction::SlotLoadCopy { .. }
        | Instruction::SlotLoadMove { .. }
        | Instruction::SlotLoadMoveTracked { .. }
        | Instruction::DropViaRef { .. }
        | Instruction::UnitEndDrop { .. }
        | Instruction::UnitEndDropTracked { .. }
        | Instruction::Nop => instr.clone(),
    }
}

/// Perform function inlining on a module according to the given directives.
pub fn inline_module(module: &IrModule, directives: &[InlineDirective]) -> InlineResult {
    let (requests, mut skipped) = resolve_directives(module, directives);

    let mut current_module = module.clone();
    let mut inlined_count = 0;

    for request in &requests {
        // Find caller and callee in current state of module.
        let caller_idx = current_module
            .functions
            .iter()
            .position(|f| f.id == request.caller);
        let callee_idx = current_module
            .functions
            .iter()
            .position(|f| f.id == request.callee);

        let (Some(caller_idx), Some(callee_idx)) = (caller_idx, callee_idx) else {
            continue;
        };

        let caller = &current_module.functions[caller_idx];
        let callee = &current_module.functions[callee_idx];

        // Find call sites.
        let call_sites = find_call_sites(caller, request.callee);

        if call_sites.is_empty() {
            let caller_name = caller.name.clone();
            let callee_name = callee.name.clone();
            skipped.push(InlineSkipReason::NoCallsFound {
                caller: caller_name,
                callee: callee_name,
            });
            continue;
        }

        // Determine which sites to inline.
        let sites_to_inline: Vec<&CallSite> = match &request.filter {
            CallSiteFilter::All => call_sites.iter().collect(),
            CallSiteFilter::AtIndex(idx) => {
                if *idx < call_sites.len() {
                    vec![&call_sites[*idx]]
                } else {
                    let caller_name = caller.name.clone();
                    let callee_name = callee.name.clone();
                    skipped.push(InlineSkipReason::CallSiteNotFound {
                        caller: caller_name,
                        callee: callee_name,
                        index: *idx,
                    });
                    continue;
                }
            }
        };

        // Inline each site (in reverse order to avoid index invalidation).
        let mut updated_caller = current_module.functions[caller_idx].clone();
        for site in sites_to_inline.into_iter().rev() {
            // Recompute site location in updated caller.
            let new_sites = find_call_sites(&updated_caller, request.callee);
            // Find the matching site by comparing block and instruction indices.
            // For simplicity, just use the site directly if it's still valid.
            if let Some(new_site) = new_sites.iter().find(|s| {
                s.block_idx == site.block_idx && s.instr_idx == site.instr_idx
            }) {
                if let Some(inlined) = inline_call_site(&updated_caller, callee, new_site) {
                    updated_caller = inlined;
                    inlined_count += 1;
                }
            }
        }

        current_module.functions[caller_idx] = updated_caller;
    }

    InlineResult {
        module: current_module,
        inlined_count,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_inline_directives() {
        let source = r#"
            // Comment
            inline foo bar
            inline baz qux at 2
            inline-all helper
        "#;

        let directives = parse_inline_directives(source).unwrap();
        assert_eq!(directives.len(), 3);

        assert_eq!(
            directives[0],
            InlineDirective::Inline {
                caller: "foo".to_string(),
                callee: "bar".to_string(),
            }
        );

        assert_eq!(
            directives[1],
            InlineDirective::InlineAt {
                caller: "baz".to_string(),
                callee: "qux".to_string(),
                call_index: 2,
            }
        );

        assert_eq!(
            directives[2],
            InlineDirective::InlineAll {
                callee: "helper".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_invalid_directive() {
        let source = "invalid directive line";
        let result = parse_inline_directives(source);
        assert!(result.is_err());
    }
}
