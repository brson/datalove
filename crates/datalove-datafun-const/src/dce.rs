//! Dead Code Elimination (DCE) pass for IR.
//!
//! This module provides functions to eliminate dead code from IR:
//! - Dead instruction elimination: removes instructions producing unused values
//! - Dead block elimination: removes unreachable blocks after branch simplification
//!
//! These passes are used after const inlining to clean up orphaned code.

use std::collections::{HashMap, HashSet};
use datalove_datafun_ir::{
    BlockId, ExportBinding, Instruction, IrBlock, IrCodeUnit,
    Operand, Terminator, ValueId,
};

/// Get the destination ValueId of an instruction, if any.
///
/// This returns the primary dest of an instruction that produces a value.
/// Instructions that don't produce values (like Drop, Store, etc.) return None.
/// Every value an instruction defines.
///
/// [`instruction_dest`] answers with one, which is all its callers want and is
/// not enough to decide whether an instruction is dead. Eight instructions
/// define a second or third value, and each of the extra ones is the flag a
/// branch tests: dropping `UnwrapOption` because nobody reads the payload
/// leaves the branch on `is_some` reading a value nothing defines.
///
/// The three collection gets are the same shape -- `l[i]?` lowers to a get and
/// a branch on whether the index was in bounds -- and were missing from here.
/// A `let x = l[i]?` whose `x` nothing reads had its get dropped and its
/// branch kept, so it took the out-of-bounds arm on whatever the undefined
/// flag happened to be, and the function early-returned `none` from an index
/// that was in bounds.
pub fn instruction_dests(instr: &Instruction) -> Vec<ValueId> {
    match instr {
        Instruction::UnwrapOption { dest, is_some, .. } => vec![*dest, *is_some],
        Instruction::UnwrapResult { ok_dest, err_dest, is_ok, .. } => {
            vec![*ok_dest, *err_dest, *is_ok]
        }
        Instruction::BinOpChecked { dest, overflow, .. } => vec![*dest, *overflow],
        Instruction::UnaryOpChecked { dest, overflow, .. } => vec![*dest, *overflow],
        Instruction::ListGet { dest, is_valid, .. } => vec![*dest, *is_valid],
        Instruction::MapGet { dest, is_valid, .. } => vec![*dest, *is_valid],
        Instruction::TensorGet { dest, is_valid, .. } => vec![*dest, *is_valid],
        Instruction::Unpack { dests, .. } => dests.clone(),
        _ => instruction_dest(instr).into_iter().collect(),
    }
}

pub fn instruction_dest(instr: &Instruction) -> Option<ValueId> {
    match instr {
        // Constants and copies
        Instruction::Const { dest, .. } => Some(*dest),
        Instruction::Copy { dest, .. } => Some(*dest),
        Instruction::Move { dest, .. } => Some(*dest),

        // Arithmetic operations
        Instruction::BinOp { dest, .. } => Some(*dest),
        Instruction::UnaryOp { dest, .. } => Some(*dest),
        Instruction::BinOpChecked { dest, .. } => Some(*dest),
        Instruction::UnaryOpChecked { dest, .. } => Some(*dest),
        Instruction::Widen { dest, .. } => Some(*dest),
        Instruction::WidenFixed { dest, .. } => Some(*dest),
        Instruction::Clone { dest, .. } => Some(*dest),

        // Function calls
        Instruction::Call { dest, .. } => Some(*dest),
        Instruction::ComptimeCall { dest, .. } => Some(*dest),

        // Aggregate construction
        Instruction::Pack { dest, .. } => Some(*dest),
        Instruction::Unpack { .. } => None, // Multiple dests, handled separately if needed
        Instruction::GetField { dest, .. } => Some(*dest),
        Instruction::GetFieldRef { dest, .. } => Some(*dest),

        // Option/Result construction
        Instruction::WrapSome { dest, .. } => Some(*dest),
        Instruction::WrapNone { dest, .. } => Some(*dest),
        Instruction::WrapOk { dest, .. } => Some(*dest),
        Instruction::WrapErr { dest, .. } => Some(*dest),

        // Enum construction
        Instruction::EnumVariant { dest, .. } => Some(*dest),
        Instruction::EnumDiscriminant { dest, .. } => Some(*dest),
        Instruction::EnumPayload { dest, .. } => Some(*dest),

        // Option/Result unwrapping (primary dest)
        Instruction::UnwrapOption { dest, .. } => Some(*dest),
        Instruction::UnwrapResult { ok_dest, .. } => Some(*ok_dest),

        // Boxing operations
        Instruction::ErrorFrom { dest, .. } => Some(*dest),
        Instruction::DataFrom { dest, .. } => Some(*dest),
        Instruction::Erase { dest, .. } => Some(*dest),
        Instruction::EraseTracked { dest, .. } => Some(*dest),
        Instruction::Reify { dest, .. } => Some(*dest),

        // Collection construction
        Instruction::ListNew { dest, .. } => Some(*dest),
        Instruction::SetNew { dest, .. } => Some(*dest),
        Instruction::MapNew { dest, .. } => Some(*dest),
        Instruction::TensorNew { dest, .. } => Some(*dest),
        Instruction::TableNew { dest, .. } => Some(*dest),

        // Slot load operations
        Instruction::SlotLoadCopy { dest, .. } => Some(*dest),
        Instruction::SlotLoadMove { dest, .. } => Some(*dest),
        Instruction::SlotLoadMoveTracked { dest, .. } => Some(*dest),

        // Intrinsics
        Instruction::Intrinsic { dest, .. } => Some(*dest),

        // Slot store operations (no dest)
        Instruction::SlotStoreCopy { .. } => None,
        Instruction::SlotStoreCopyTracked { .. } => None,
        Instruction::SlotStoreMove { .. } => None,
        Instruction::SlotStoreMoveTracked { .. } => None,
        Instruction::SetField { .. } => None,
        Instruction::SetFieldTracked { .. } => None,
        Instruction::ParamStore { .. } => None,
        Instruction::ParamStoreTracked { .. } => None,
        Instruction::ParamSetField { .. } => None,
        Instruction::ParamSetFieldTracked { .. } => None,
        Instruction::RefStore { .. } => None,
        Instruction::RefStoreTracked { .. } => None,
        Instruction::RefSetField { .. } => None,
        Instruction::RefSetFieldTracked { .. } => None,

        // Drop operations (no dest)
        Instruction::Drop { .. } => None,
        Instruction::DropTracked { .. } => None,
        Instruction::DropViaRef { .. } => None,
        Instruction::UnitEndDrop { .. } => None,
        Instruction::UnitEndDropTracked { .. } => None,

        // List indexing
        Instruction::ListGet { dest, .. } => Some(*dest),
        Instruction::ListBoundsCheck { is_valid, .. } => Some(*is_valid),
        Instruction::ListSet { .. } => None,
        Instruction::ListElementRef { dest, .. } => Some(*dest),

        // Map indexing
        Instruction::MapGet { dest, .. } => Some(*dest),
        Instruction::MapContainsKey { is_valid, .. } => Some(*is_valid),
        Instruction::MapSetValue { .. } => None,
        Instruction::MapValueRef { dest, .. } => Some(*dest),
        Instruction::MapUpsert { .. } => None,

        // Tensor indexing
        Instruction::TensorGet { dest, .. } => Some(*dest),
        Instruction::TensorBoundsCheck { is_valid, .. } => Some(*is_valid),
        Instruction::TensorSet { .. } => None,
        Instruction::TensorIndexRef { dest, .. } => Some(*dest),

        // Misc
        Instruction::DebugLog { .. } => None,
        Instruction::Nop => None,
    }
}

/// Eliminate dead code from an IrCodeUnit.
///
/// After const inlining, some instructions may produce values that are no longer
/// used. For example, if a Pack instruction is replaced with a Const containing
/// the struct value, the intermediate instructions that produced the Pack's operands
/// become dead code.
///
/// This function removes instructions that:
/// - Define values that are not used by any other instruction
/// - Have no side effects (Const, Copy, etc.)
pub fn eliminate_dead_code_unit(unit: &mut IrCodeUnit) {
    // Iterate until no changes (removing an instruction may make its operands unused).
    loop {
        let used_values = collect_used_values(unit);
        let changed = remove_dead_instructions(unit, &used_values);
        if !changed {
            break;
        }
    }
}

/// Eliminate dead (unreachable) blocks from an IrCodeUnit.
///
/// After constant branch simplification, some blocks may become unreachable.
/// For example, when `Branch(is_ok, then, else)` is simplified to `Goto(then)`,
/// the `else` block may become unreachable if no other paths lead to it.
///
/// We must remove these blocks because they may reference undefined values
/// (like err_dest when a result unwrap is known to succeed at compile time).
///
/// After removal, blocks are renumbered to maintain the invariant that
/// `blocks[i].id.0 == i` (required by the interpreter).
pub fn eliminate_dead_blocks_func(func: &mut IrCodeUnit) {
    if func.blocks.is_empty() {
        return;
    }

    // Find all reachable blocks starting from block 0 (entry).
    let mut reachable: HashSet<BlockId> = HashSet::new();
    let mut worklist: Vec<BlockId> = vec![BlockId(0)];

    while let Some(block_id) = worklist.pop() {
        if reachable.contains(&block_id) {
            continue;
        }
        reachable.insert(block_id);

        // Find this block and get its successors.
        if let Some(block) = func.blocks.iter().find(|b| b.id == block_id) {
            match &block.terminator {
                Terminator::Goto { target, .. } => {
                    worklist.push(*target);
                }
                Terminator::Branch { then_block, else_block, .. } => {
                    worklist.push(*then_block);
                    worklist.push(*else_block);
                }
                Terminator::Switch { cases, default, .. } => {
                    for (_, target) in cases {
                        worklist.push(*target);
                    }
                    worklist.push(*default);
                }
                Terminator::Return { .. }
                | Terminator::UnitEnd { .. }
                | Terminator::UnitEarlyReturn { .. } => {
                    // No successors.
                }
            }
        }
    }

    // Check if any blocks are actually dead.
    let all_reachable = func.blocks.iter().all(|b| reachable.contains(&b.id));
    if all_reachable {
        return; // No dead blocks, nothing to do.
    }

    // Build mapping from old block IDs to new IDs.
    // New IDs are assigned in order: 0, 1, 2, ...
    let mut old_to_new: HashMap<BlockId, BlockId> = HashMap::new();
    let mut new_id = 0u32;
    for block in &func.blocks {
        if reachable.contains(&block.id) {
            old_to_new.insert(block.id, BlockId(new_id));
            new_id += 1;
        }
    }

    // Remove unreachable blocks and renumber.
    func.blocks.retain(|block| reachable.contains(&block.id));
    for block in &mut func.blocks {
        // Update this block's ID.
        block.id = *old_to_new.get(&block.id).unwrap();

        // Update terminator targets.
        match &mut block.terminator {
            Terminator::Goto { target, .. } => {
                *target = *old_to_new.get(target).unwrap();
            }
            Terminator::Branch { then_block, else_block, .. } => {
                *then_block = *old_to_new.get(then_block).unwrap();
                *else_block = *old_to_new.get(else_block).unwrap();
            }
            Terminator::Switch { cases, default, .. } => {
                for (_, target) in cases.iter_mut() {
                    *target = *old_to_new.get(target).unwrap();
                }
                *default = *old_to_new.get(default).unwrap();
            }
            Terminator::Return { .. }
            | Terminator::UnitEnd { .. }
            | Terminator::UnitEarlyReturn { .. } => {
                // No targets to update.
            }
        }
    }
}

/// Collect all ValueIds that are used in the unit.
fn collect_used_values(unit: &IrCodeUnit) -> HashSet<ValueId> {
    let mut used = HashSet::new();

    // Access script context if this is a script unit.
    if let Some(script_ctx) = unit.script_context() {
        // Values in unit_end_values are used (need to be cleaned up).
        for vid in &script_ctx.unit_end_values {
            used.insert(*vid);
        }

        // Exported values must be kept.
        for (_name, binding) in &script_ctx.exports {
            if let ExportBinding::Value(vid) = binding {
                used.insert(*vid);
            }
        }

        // Result value must be kept.
        if let Some(vid) = &script_ctx.result {
            used.insert(*vid);
        }
    }

    // Collect from blocks.
    for block in &unit.blocks {
        collect_block_used_values(block, &mut used);
    }

    // Collect from nested units.
    for nested in &unit.nested_units {
        for block in &nested.blocks {
            collect_block_used_values(block, &mut used);
        }
    }

    used
}

/// Add a ValueId from an operand if it's a local value.
fn add_operand_value(op: &Operand, used: &mut HashSet<ValueId>) {
    match op {
        Operand::Value(vid) | Operand::ValueRef(vid) => {
            used.insert(*vid);
        }
        // Slots, Params, and External operands are not local values.
        _ => {}
    }
}

/// Collect used ValueIds from a block's instructions and terminator.
fn collect_block_used_values(block: &IrBlock, used: &mut HashSet<ValueId>) {
    // Collect from instructions.
    for instr in &block.instructions {
        collect_instruction_operands(instr, used);
    }

    // Collect from terminator.
    match &block.terminator {
        Terminator::Return { value: Some(op) } => {
            add_operand_value(op, used);
        }
        Terminator::UnitEnd { result: Some(op) } => {
            add_operand_value(op, used);
        }
        Terminator::Branch { cond, then_args, else_args, .. } => {
            add_operand_value(cond, used);
            for arg in then_args {
                add_operand_value(arg, used);
            }
            for arg in else_args {
                add_operand_value(arg, used);
            }
        }
        Terminator::Goto { args, .. } => {
            for arg in args {
                add_operand_value(arg, used);
            }
        }
        Terminator::UnitEarlyReturn { value } => {
            add_operand_value(value, used);
        }
        Terminator::Switch { discriminant, .. } => {
            add_operand_value(discriminant, used);
        }
        _ => {}
    }
}

/// Collect all ValueIds used as operands in an instruction.
fn collect_instruction_operands(instr: &Instruction, used: &mut HashSet<ValueId>) {

    match instr {
        // No operands.
        Instruction::Const { .. }
        | Instruction::WrapNone { .. }
        | Instruction::Nop => {}

        // Single src operand.
        Instruction::Copy { src, .. }
        | Instruction::Move { src, .. }
        | Instruction::Widen { src, .. }
        | Instruction::WidenFixed { src, .. }
        | Instruction::Clone { src, .. }
        | Instruction::Unpack { src, .. }
        | Instruction::GetField { src, .. }
        | Instruction::GetFieldRef { src, .. }
        | Instruction::UnwrapOption { src, .. }
        | Instruction::UnwrapResult { src, .. } => add_operand_value(src, used),

        // Binary operands.
        Instruction::BinOp { lhs, rhs, .. }
        | Instruction::BinOpChecked { lhs, rhs, .. } => {
            add_operand_value(lhs, used);
            add_operand_value(rhs, used);
        }

        // Single operand field.
        Instruction::UnaryOp { operand, .. }
        | Instruction::UnaryOpChecked { operand, .. }
        | Instruction::Drop { operand }
        | Instruction::DropTracked { operand }
        | Instruction::UnitEndDrop { operand }
        | Instruction::UnitEndDropTracked { operand }
        | Instruction::DebugLog { operand } => add_operand_value(operand, used),

        // DropViaRef uses ref_value (ValueId), not operand.
        Instruction::DropViaRef { ref_value } => {
            used.insert(*ref_value);
        }

        // Single inner operand.
        Instruction::WrapSome { inner, .. }
        | Instruction::WrapOk { inner, .. }
        | Instruction::WrapErr { inner, .. } => add_operand_value(inner, used),

        // Enum variant with optional payload.
        Instruction::EnumVariant { payload, .. } => {
            if let Some(p) = payload {
                add_operand_value(p, used);
            }
        }

        // Enum discriminant/payload extraction.
        Instruction::EnumDiscriminant { src, .. }
        | Instruction::EnumPayload { src, .. } => add_operand_value(src, used),

        // Function call with args.
        Instruction::Call { args, .. } => {
            for arg in args {
                add_operand_value(arg, used);
            }
        }
        Instruction::ComptimeCall { args, .. } => {
            for arg in args {
                add_operand_value(arg, used);
            }
        }

        // Pack with fields.
        Instruction::Pack { fields, .. } => {
            for field in fields {
                add_operand_value(field, used);
            }
        }

        // List/Set/Map/Tensor/Table construction.
        Instruction::ListNew { elements, .. }
        | Instruction::SetNew { elements, .. } => {
            for elem in elements {
                add_operand_value(elem, used);
            }
        }
        Instruction::MapNew { entries, .. } => {
            for (k, v) in entries {
                add_operand_value(k, used);
                add_operand_value(v, used);
            }
        }
        Instruction::TensorNew { elements, .. } => {
            for elem in elements {
                add_operand_value(elem, used);
            }
        }
        Instruction::TableNew { rows, .. } => {
            for row in rows {
                add_operand_value(row, used);
            }
        }

        // Boxing operations.
        Instruction::ErrorFrom { inner, .. }
        | Instruction::DataFrom { inner, .. } => add_operand_value(inner, used),
        Instruction::Erase { src, .. }
        | Instruction::EraseTracked { src, .. }
        | Instruction::Reify { src, .. } => add_operand_value(src, used),

        // Slot load operations (no operand, just slot reference).
        Instruction::SlotLoadCopy { .. }
        | Instruction::SlotLoadMove { .. }
        | Instruction::SlotLoadMoveTracked { .. } => {}

        // Slot store operations.
        Instruction::SlotStoreCopy { value, .. }
        | Instruction::SlotStoreCopyTracked { value, .. }
        | Instruction::SlotStoreMove { value, .. }
        | Instruction::SlotStoreMoveTracked { value, .. } => add_operand_value(value, used),

        // Field set operations.
        Instruction::SetField { value, .. }
        | Instruction::SetFieldTracked { value, .. } => add_operand_value(value, used),

        // Param store operations.
        Instruction::ParamStore { value, .. }
        | Instruction::ParamStoreTracked { value, .. } => add_operand_value(value, used),

        // Param field set operations.
        Instruction::ParamSetField { value, .. }
        | Instruction::ParamSetFieldTracked { value, .. } => add_operand_value(value, used),

        // Ref store operations (from inlining mut/out params).
        Instruction::RefStore { dest, value }
        | Instruction::RefStoreTracked { dest, value } => {
            add_operand_value(dest, used);
            add_operand_value(value, used);
        }
        Instruction::RefSetField { dest, value, .. }
        | Instruction::RefSetFieldTracked { dest, value, .. } => {
            add_operand_value(dest, used);
            add_operand_value(value, used);
        }

        // Intrinsics with args.
        Instruction::Intrinsic { args, .. } => {
            for arg in args {
                add_operand_value(arg, used);
            }
        }

        // List indexing.
        Instruction::ListGet { list, index, .. } => {
            add_operand_value(list, used);
            add_operand_value(index, used);
        }
        Instruction::ListBoundsCheck { list, index, .. } => {
            add_operand_value(list, used);
            add_operand_value(index, used);
        }
        Instruction::ListSet { list, index, value } => {
            add_operand_value(list, used);
            add_operand_value(index, used);
            add_operand_value(value, used);
        }
        Instruction::ListElementRef { list, index, .. } => {
            add_operand_value(list, used);
            add_operand_value(index, used);
        }
        Instruction::MapGet { map, key, .. } => {
            add_operand_value(map, used);
            add_operand_value(key, used);
        }
        Instruction::MapContainsKey { map, key, .. } => {
            add_operand_value(map, used);
            add_operand_value(key, used);
        }
        Instruction::MapSetValue { map, key, value } => {
            add_operand_value(map, used);
            add_operand_value(key, used);
            add_operand_value(value, used);
        }
        Instruction::MapValueRef { map, key, .. } => {
            add_operand_value(map, used);
            add_operand_value(key, used);
        }
        Instruction::MapUpsert { map, key, value } => {
            add_operand_value(map, used);
            add_operand_value(key, used);
            add_operand_value(value, used);
        }
        Instruction::TensorGet { tensor, index, .. } => {
            add_operand_value(tensor, used);
            add_operand_value(index, used);
        }
        Instruction::TensorBoundsCheck { tensor, index, .. } => {
            add_operand_value(tensor, used);
            add_operand_value(index, used);
        }
        Instruction::TensorSet { tensor, index, value } => {
            add_operand_value(tensor, used);
            add_operand_value(index, used);
            add_operand_value(value, used);
        }
        Instruction::TensorIndexRef { tensor, index, .. } => {
            add_operand_value(tensor, used);
            add_operand_value(index, used);
        }
    }
}

/// Remove instructions that define unused values.
///
/// Returns true if any instructions were removed.
fn remove_dead_instructions(unit: &mut IrCodeUnit, used: &HashSet<ValueId>) -> bool {
    let mut changed = false;

    for block in &mut unit.blocks {
        let original_len = block.instructions.len();
        block.instructions.retain(|instr| {
            // Keep instructions with side effects.
            if has_side_effects(instr) {
                return true;
            }
            // Keep instructions that define a used value. All of them have to be
            // consulted: an instruction defining one dead value and one live one
            // still has to stay.
            let dests = instruction_dests(instr);
            if dests.is_empty() {
                // No dest means it's a side-effect instruction, already handled above.
                return true;
            }
            dests.iter().any(|d| used.contains(d))
        });
        if block.instructions.len() != original_len {
            changed = true;
        }
    }

    changed
}

/// Check if an instruction has side effects and should not be removed.
fn has_side_effects(instr: &Instruction) -> bool {
    match instr {
        // Drop instructions have side effects (they free memory).
        Instruction::Drop { .. }
        | Instruction::DropTracked { .. }
        | Instruction::DropViaRef { .. }
        | Instruction::UnitEndDrop { .. }
        | Instruction::UnitEndDropTracked { .. } => true,
        // Store instructions have side effects.
        Instruction::SlotStoreCopy { .. }
        | Instruction::SlotStoreCopyTracked { .. }
        | Instruction::SlotStoreMove { .. }
        | Instruction::SlotStoreMoveTracked { .. }
        | Instruction::SetField { .. }
        | Instruction::SetFieldTracked { .. }
        | Instruction::ParamStore { .. }
        | Instruction::ParamStoreTracked { .. }
        | Instruction::ParamSetField { .. }
        | Instruction::ParamSetFieldTracked { .. }
        | Instruction::RefStore { .. }
        | Instruction::RefStoreTracked { .. }
        | Instruction::RefSetField { .. }
        | Instruction::RefSetFieldTracked { .. } => true,
        // Call may have side effects.
        Instruction::Call { .. } => true,
        // DebugLog has side effects (prints).
        Instruction::DebugLog { .. } => true,
        // Intrinsics may have side effects.
        Instruction::Intrinsic { .. } => true,
        // List set has side effects (mutates list).
        Instruction::ListSet { .. } => true,
        // Map set value has side effects (mutates map).
        Instruction::MapSetValue { .. } => true,
        // Map upsert has side effects (mutates map).
        Instruction::MapUpsert { .. } => true,
        // Tensor set has side effects (mutates tensor).
        Instruction::TensorSet { .. } => true,
        // Everything else is pure.
        _ => false,
    }
}
