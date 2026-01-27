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
//!
//! This separation allows lowering to be independent of const evaluation.

use std::collections::HashMap;
use datalove_datafun_ir::{
    ConstValue, IrScriptUnit, IrFunction, IrBlock, Instruction, ValueId,
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
            exports: vec![],
            const_values: vec![("X".to_string(), ValueId(2))],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(3));

        let result = inline_script_consts(unit, &const_values);

        // Check that the BinOp instruction was replaced with Const.
        assert_eq!(result.blocks[0].instructions.len(), 3);
        match &result.blocks[0].instructions[2] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(2));
                assert_eq!(*value, ConstValue::I32(3));
            }
            _ => panic!("expected Const instruction, got {:?}", result.blocks[0].instructions[2]),
        }
    }

    #[test]
    fn test_inline_with_function_call() {
        // Simulate: const X = compute(), which lowered to:
        //   v0 = call compute()
        // We'll inline v0 with the pre-computed value 100.
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
            exports: vec![],
            const_values: vec![("X".to_string(), ValueId(0))],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(100));

        let result = inline_script_consts(unit, &const_values);

        // Check that the Call instruction was replaced with Const.
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
            exports: vec![],
            // v1 (the unwrapped value) is tracked as a const.
            const_values: vec![("X".to_string(), ValueId(1))],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(42));

        let result = inline_script_consts(unit, &const_values);

        // Check that we now have 3 instructions (original 2 -> 3 after replacement).
        // The UnwrapOption is replaced with Const for dest AND Const for is_some.
        assert_eq!(result.blocks[0].instructions.len(), 3);

        // First instruction: Const for the option value (unchanged).
        match &result.blocks[0].instructions[0] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(0));
                assert!(matches!(value, ConstValue::OptionSome(_)));
            }
            _ => panic!("expected Const instruction for option"),
        }

        // Second instruction: Const for the unwrapped dest.
        match &result.blocks[0].instructions[1] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(1));
                assert_eq!(*value, ConstValue::I32(42));
            }
            _ => panic!("expected Const instruction for unwrapped value"),
        }

        // Third instruction: Const for is_some = true.
        match &result.blocks[0].instructions[2] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(2));
                assert_eq!(*value, ConstValue::Bool(true));
            }
            _ => panic!("expected Const instruction for is_some"),
        }
    }
}
