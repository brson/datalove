//! Const inlining pass for IR.
//!
//! This pass transforms IR by replacing const binding initializer expressions
//! with their evaluated constant values. This is used after lowering to
//! implement compile-time const evaluation.
//!
//! The pass:
//! 1. Takes IR where consts are lowered as let bindings
//! 2. Uses the const_values metadata to identify which values are consts
//! 3. Replaces the defining instruction with a Const literal
//! 4. Removes dead code (instructions producing unused values)
//!
//! This separation allows lowering to be independent of const evaluation.

use std::collections::{HashMap, HashSet};
use datalove_datafun_ir::{
    ConstValue, IrScriptUnit, IrFunction, IrBlock, Instruction, ValueId,
    Operand, Terminator, ExportBinding,
};

/// Inline const values into an IrScriptUnit.
///
/// Takes the lowered IR and a map of const names to their evaluated values.
/// For each const binding tracked in the IR's const_values field, replaces
/// the defining instruction with a Const literal.
///
/// Returns the transformed IR.
pub fn inline_script_consts(
    mut unit: IrScriptUnit,
    const_values: &HashMap<String, ConstValue>,
) -> IrScriptUnit {
    // Build map of ValueId -> ConstValue for consts that should be inlined.
    let mut value_to_const: HashMap<ValueId, ConstValue> = HashMap::new();
    for (name, value_id) in &unit.const_values {
        if let Some(const_value) = const_values.get(name) {
            value_to_const.insert(*value_id, const_value.clone());
        }
    }

    // Inline consts in the main unit blocks.
    for block in &mut unit.blocks {
        inline_block_consts(block, &value_to_const);
    }

    // Remove dead code (instructions producing unused values).
    // This is needed because CTFE inlining may replace a Pack/struct-building
    // instruction with a Const, leaving the intermediate values orphaned.
    eliminate_dead_code_unit(&mut unit);

    // Inline consts in nested functions.
    for func in &mut unit.functions {
        inline_function_consts(func, const_values);
    }

    unit
}

/// Inline const values into an IrFunction.
///
/// Takes the lowered IR and a map of const names to their evaluated values.
/// For functions, the const_values field tracks function-local consts.
pub fn inline_function_consts(
    func: &mut IrFunction,
    const_values: &HashMap<String, ConstValue>,
) {
    // Build map of ValueId -> ConstValue for consts that should be inlined.
    let mut value_to_const: HashMap<ValueId, ConstValue> = HashMap::new();
    for (name, value_id) in &func.const_values {
        if let Some(const_value) = const_values.get(name) {
            value_to_const.insert(*value_id, const_value.clone());
        }
    }

    // Inline consts in all blocks.
    for block in &mut func.blocks {
        inline_block_consts(block, &value_to_const);
    }
}

/// Inline const values in a single block.
///
/// For each instruction that defines a const value, replace it with a Const literal.
/// For instructions that produce multiple values (like UnwrapOption), we may need to
/// insert additional Const instructions to define the other outputs.
fn inline_block_consts(block: &mut IrBlock, value_to_const: &HashMap<ValueId, ConstValue>) {
    // We may need to insert instructions, so collect replacements first.
    let mut replacements: Vec<(usize, Vec<Instruction>)> = Vec::new();

    for (idx, instr) in block.instructions.iter().enumerate() {
        // Check if this instruction defines a const value.
        if let Some(dest) = instruction_dest(instr) {
            if let Some(const_value) = value_to_const.get(&dest) {
                // Build replacement instructions.
                let mut new_instrs = vec![Instruction::Const {
                    dest,
                    value: const_value.clone(),
                }];

                // Special handling for UnwrapOption: also define is_some = true.
                // If we're inlining the dest, the unwrap succeeded, so is_some must be true.
                if let Instruction::UnwrapOption { is_some, .. } = instr {
                    new_instrs.push(Instruction::Const {
                        dest: *is_some,
                        value: ConstValue::Bool(true),
                    });
                }

                // Special handling for UnwrapResult: also define is_ok = true.
                // If we're inlining the ok_dest, the unwrap succeeded, so is_ok must be true.
                if let Instruction::UnwrapResult { is_ok, .. } = instr {
                    new_instrs.push(Instruction::Const {
                        dest: *is_ok,
                        value: ConstValue::Bool(true),
                    });
                }

                replacements.push((idx, new_instrs));
            }
        }
    }

    // Apply replacements in reverse order to maintain correct indices.
    for (idx, new_instrs) in replacements.into_iter().rev() {
        block.instructions.splice(idx..=idx, new_instrs);
    }
}

/// Inline const values into a collection of module functions.
///
/// Takes the lowered functions and a map of const names to their evaluated values.
/// For each function, consts tracked in the function's const_values field are inlined.
///
/// The const_values map uses qualified names like "func_name::const_name", matching
/// how module consts are stored during evaluation.
///
/// This function modifies the functions in place.
pub fn inline_module_functions(
    functions: &mut [IrFunction],
    const_values: &HashMap<String, ConstValue>,
) {
    for func in functions {
        // For module functions, const_values has qualified names "func_name::const_name"
        // but func.const_values has just the local name "const_name".
        // We need to qualify the names when looking up.
        let func_name = func.name.clone();
        inline_module_function_consts(func, const_values, &func_name);
    }
}

/// Inline const values into a module IrFunction.
///
/// Like inline_function_consts but qualifies const names with the function name
/// when looking them up in the const_values map.
fn inline_module_function_consts(
    func: &mut IrFunction,
    const_values: &HashMap<String, ConstValue>,
    func_name: &str,
) {
    // Build map of ValueId -> ConstValue for consts that should be inlined.
    let mut value_to_const: HashMap<ValueId, ConstValue> = HashMap::new();
    for (name, value_id) in &func.const_values {
        // Look up with qualified name: "func_name::const_name"
        let qualified_name = format!("{}::{}", func_name, name);
        if let Some(const_value) = const_values.get(&qualified_name) {
            value_to_const.insert(*value_id, const_value.clone());
        }
    }

    // Inline consts in all blocks.
    for block in &mut func.blocks {
        inline_block_consts(block, &value_to_const);
    }
}

/// Get the destination ValueId of an instruction, if any.
///
/// This returns the primary dest of an instruction that produces a value.
/// Instructions that don't produce values (like Drop, Store, etc.) return None.
fn instruction_dest(instr: &Instruction) -> Option<ValueId> {
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

        // Function calls
        Instruction::Call { dest, .. } => Some(*dest),

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

        // Option/Result unwrapping (primary dest)
        Instruction::UnwrapOption { dest, .. } => Some(*dest),
        Instruction::UnwrapResult { ok_dest, .. } => Some(*ok_dest),

        // Boxing operations
        Instruction::ErrorFrom { dest, .. } => Some(*dest),
        Instruction::DataFrom { dest, .. } => Some(*dest),

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

        // Drop operations (no dest)
        Instruction::Drop { .. } => None,
        Instruction::DropTracked { .. } => None,
        Instruction::DropViaRef { .. } => None,
        Instruction::UnitEndDrop { .. } => None,
        Instruction::UnitEndDropTracked { .. } => None,

        // Misc
        Instruction::DebugLog { .. } => None,
        Instruction::Nop => None,
    }
}

/// Eliminate dead code from an IrScriptUnit.
///
/// After const inlining, some instructions may produce values that are no longer
/// used. For example, if a Pack instruction is replaced with a Const containing
/// the struct value, the intermediate instructions that produced the Pack's operands
/// become dead code.
///
/// This function removes instructions that:
/// - Define values that are not used by any other instruction
/// - Have no side effects (Const, Copy, etc.)
fn eliminate_dead_code_unit(unit: &mut IrScriptUnit) {
    // Iterate until no changes (removing an instruction may make its operands unused).
    loop {
        let used_values = collect_used_values(unit);
        let changed = remove_dead_instructions(unit, &used_values);
        if !changed {
            break;
        }
    }
}

/// Collect all ValueIds that are used in the unit.
fn collect_used_values(unit: &IrScriptUnit) -> HashSet<ValueId> {
    let mut used = HashSet::new();

    // Values in unit_end_values are used (need to be cleaned up).
    for vid in &unit.unit_end_values {
        used.insert(*vid);
    }

    // Exported values must be kept.
    for (_name, binding) in &unit.exports {
        if let ExportBinding::Value(vid) = binding {
            used.insert(*vid);
        }
    }

    // Result value must be kept.
    if let Some(vid) = &unit.result {
        used.insert(*vid);
    }

    // Collect from blocks.
    for block in &unit.blocks {
        collect_block_used_values(block, &mut used);
    }

    // Collect from functions.
    for func in &unit.functions {
        for block in &func.blocks {
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
        Terminator::Return { value: Some(op) } => {
            add_operand_value(op, used);
        }
        Terminator::Goto { args, .. } => {
            for arg in args {
                add_operand_value(arg, used);
            }
        }
        Terminator::UnitEarlyReturn { value } => {
            add_operand_value(value, used);
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

        // Function call with args.
        Instruction::Call { args, .. } => {
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

        // Intrinsics with args.
        Instruction::Intrinsic { args, .. } => {
            for arg in args {
                add_operand_value(arg, used);
            }
        }
    }
}

/// Remove instructions that define unused values.
///
/// Returns true if any instructions were removed.
fn remove_dead_instructions(unit: &mut IrScriptUnit, used: &HashSet<ValueId>) -> bool {
    let mut changed = false;

    for block in &mut unit.blocks {
        let original_len = block.instructions.len();
        block.instructions.retain(|instr| {
            // Keep instructions with side effects.
            if has_side_effects(instr) {
                return true;
            }
            // Keep instructions that define used values.
            if let Some(dest) = instruction_dest(instr) {
                if used.contains(&dest) {
                    return true;
                }
                // This instruction defines an unused value - remove it.
                false
            } else {
                // No dest means it's a side-effect instruction, already handled above.
                true
            }
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
        | Instruction::ParamSetFieldTracked { .. } => true,
        // Call may have side effects.
        Instruction::Call { .. } => true,
        // DebugLog has side effects (prints).
        Instruction::DebugLog { .. } => true,
        // Intrinsics may have side effects.
        Instruction::Intrinsic { .. } => true,
        // Everything else is pure.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datafun_ir::{BlockId, Terminator, Operand, IrType, BinOp};

    #[test]
    fn test_inline_simple_const() {
        // Create a simple unit with one const binding.
        // Simulate: const X = 1 + 2, which lowered to:
        //   v0 = const 1
        //   v1 = const 2
        //   v2 = binop add v0, v1
        // We'll inline v2 with the pre-computed value 3.
        // Export the const so DCE doesn't remove it.
        use datalove_datafun_ir::ExportBinding;
        let blocks = vec![IrBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::Const { dest: ValueId(0), value: ConstValue::I32(1) },
                Instruction::Const { dest: ValueId(1), value: ConstValue::I32(2) },
                Instruction::BinOp {
                    dest: ValueId(2),
                    op: BinOp::Add,
                    lhs: Operand::Value(ValueId(0)),
                    rhs: Operand::Value(ValueId(1)),
                },
            ],
            terminator: Terminator::UnitEnd { result: None },
        }];

        let unit = IrScriptUnit {
            blocks,
            value_count: 3,
            slot_count: 0,
            value_types: vec![IrType::I32, IrType::I32, IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            unit_end_values: vec![],
            unit_end_slots: vec![],
            functions: vec![],
            symbols: datalove_datafun_ir::SymbolTable::new(),
            result: None,
            exports: vec![("X".to_string(), ExportBinding::Value(ValueId(2)))],
            const_values: vec![("X".to_string(), ValueId(2))],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(3));

        let result = inline_script_consts(unit, &const_values);

        // After inlining, v0 and v1 become dead code and are removed by DCE.
        // Only v2 (the exported const) remains.
        assert_eq!(result.blocks[0].instructions.len(), 1);
        match &result.blocks[0].instructions[0] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(2));
                assert_eq!(*value, ConstValue::I32(3));
            }
            _ => panic!("expected Const instruction, got {:?}", result.blocks[0].instructions[0]),
        }
    }

    #[test]
    fn test_inline_with_function_call() {
        // Simulate: const X = compute(), which lowered to:
        //   v0 = call compute()
        // We'll inline v0 with the pre-computed value 100.
        // Export the const so DCE doesn't remove it.
        use datalove_datafun_ir::ExportBinding;
        let blocks = vec![IrBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::Call {
                    dest: ValueId(0),
                    func: datalove_datafun_ir::FuncRef::Local(datalove_datafun_ir::FuncId(0)),
                    args: vec![],
                },
            ],
            terminator: Terminator::UnitEnd { result: None },
        }];

        let unit = IrScriptUnit {
            blocks,
            value_count: 1,
            slot_count: 0,
            value_types: vec![IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            unit_end_values: vec![],
            unit_end_slots: vec![],
            functions: vec![],
            symbols: datalove_datafun_ir::SymbolTable::new(),
            result: None,
            exports: vec![("X".to_string(), ExportBinding::Value(ValueId(0)))],
            const_values: vec![("X".to_string(), ValueId(0))],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(100));

        let result = inline_script_consts(unit, &const_values);

        // Check that the Call instruction was replaced with Const.
        assert_eq!(result.blocks[0].instructions.len(), 1);
        match &result.blocks[0].instructions[0] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(0));
                assert_eq!(*value, ConstValue::I32(100));
            }
            _ => panic!("expected Const instruction"),
        }
    }

    #[test]
    fn test_inline_unwrap_option() {
        // Simulate: const X = opt_val? where opt_val is Some(42), which lowered to:
        //   v0 = const some 42   (the option value)
        //   v1, v2 = unwrap_option v0  (v1 = inner value, v2 = is_some)
        //   branch v2, ...
        // We'll inline v1 with 42. This should also set v2 = true.
        // Export v1 so DCE doesn't remove it.
        use datalove_datafun_ir::ExportBinding;
        let blocks = vec![IrBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::Const {
                    dest: ValueId(0),
                    value: ConstValue::OptionSome(Box::new(ConstValue::I32(42))),
                },
                Instruction::UnwrapOption {
                    dest: ValueId(1),
                    is_some: ValueId(2),
                    src: Operand::Value(ValueId(0)),
                },
            ],
            terminator: Terminator::Branch {
                cond: Operand::Value(ValueId(2)),
                then_block: BlockId(1),
                then_args: vec![],
                else_block: BlockId(2),
                else_args: vec![],
            },
        }];

        let unit = IrScriptUnit {
            blocks,
            value_count: 3,
            slot_count: 0,
            value_types: vec![
                IrType::Option(Box::new(IrType::I32)),
                IrType::I32,
                IrType::Bool,
            ],
            slot_types: vec![],
            tracked_slots: vec![],
            unit_end_values: vec![],
            unit_end_slots: vec![],
            functions: vec![],
            symbols: datalove_datafun_ir::SymbolTable::new(),
            result: None,
            exports: vec![("X".to_string(), ExportBinding::Value(ValueId(1)))],
            // v1 (the unwrapped value) is tracked as a const.
            const_values: vec![("X".to_string(), ValueId(1))],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(42));

        let result = inline_script_consts(unit, &const_values);

        // After inlining, v0 becomes dead (not used by anything) and is removed by DCE.
        // v1 remains because it's exported, v2 remains because it's used by the branch.
        assert_eq!(result.blocks[0].instructions.len(), 2);

        // First instruction: Const for the unwrapped dest (v1).
        match &result.blocks[0].instructions[0] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(1));
                assert_eq!(*value, ConstValue::I32(42));
            }
            _ => panic!("expected Const instruction for unwrapped value"),
        }

        // Second instruction: Const for is_some = true (v2).
        match &result.blocks[0].instructions[1] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(2));
                assert_eq!(*value, ConstValue::Bool(true));
            }
            _ => panic!("expected Const instruction for is_some"),
        }
    }
}
