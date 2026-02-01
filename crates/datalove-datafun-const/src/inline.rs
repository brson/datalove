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

use std::collections::HashMap;
use datalove_datafun_ir::{
    ConstValue, IrCodeUnit, IrBlock, Instruction, ValueId,
    Operand, Terminator,
};

use crate::dce::{eliminate_dead_blocks_func, eliminate_dead_code_unit, instruction_dest};

/// Inline const values into an IrCodeUnit (script unit).
///
/// Takes the lowered IR and a map of const names to their evaluated values.
/// For each const binding tracked in the IR's const_values field, replaces
/// the defining instruction with a Const literal.
///
/// Returns the transformed IR.
pub fn inline_script_consts(
    mut unit: IrCodeUnit,
    const_values: &HashMap<String, ConstValue>,
) -> IrCodeUnit {
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

    // Inline consts in nested units.
    for nested in &mut unit.nested_units {
        inline_function_consts(nested, const_values);
    }

    unit
}

/// Inline const values into an IrCodeUnit (function).
///
/// Takes the lowered IR and a map of const names to their evaluated values.
/// For functions, the const_values field tracks function-local consts.
///
/// After inlining, eliminates dead (unreachable) blocks that may result from
/// constant branch simplification.
pub fn inline_function_consts(
    func: &mut IrCodeUnit,
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

    // Eliminate dead blocks that are no longer reachable after branch simplification.
    // This is critical because simplified branches may leave unreachable blocks that
    // reference undefined values (like err_dest when is_ok is constant true).
    eliminate_dead_blocks_func(func);
}

/// Inline const values in a single block.
///
/// For each instruction that defines a const value, replace it with a Const literal.
/// For instructions that produce multiple values (like UnwrapOption), we may need to
/// insert additional Const instructions to define the other outputs.
///
/// Also simplifies constant branches: if a Branch terminator has a condition that
/// is defined as a constant bool, replaces it with an unconditional Goto.
fn inline_block_consts(block: &mut IrBlock, value_to_const: &HashMap<ValueId, ConstValue>) {
    // Track which ValueIds are known to be constant bools (from both the passed-in
    // const values and from newly inserted is_ok/is_some/overflow values).
    let mut const_bools: HashMap<ValueId, bool> = HashMap::new();

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
                    const_bools.insert(*is_some, true);
                }

                // Special handling for UnwrapResult: also define is_ok = true.
                // If we're inlining the ok_dest, the unwrap succeeded, so is_ok must be true.
                // Note: err_dest is NOT defined here - it becomes dead code when is_ok = true.
                // The branch on is_ok will take the success path, so err_dest is never used.
                if let Instruction::UnwrapResult { is_ok, .. } = instr {
                    new_instrs.push(Instruction::Const {
                        dest: *is_ok,
                        value: ConstValue::Bool(true),
                    });
                    const_bools.insert(*is_ok, true);
                }

                // Special handling for BinOpChecked: also define overflow = false.
                // If we're inlining the dest, the operation succeeded without overflow.
                if let Instruction::BinOpChecked { overflow, .. } = instr {
                    new_instrs.push(Instruction::Const {
                        dest: *overflow,
                        value: ConstValue::Bool(false),
                    });
                    const_bools.insert(*overflow, false);
                }

                // Special handling for UnaryOpChecked: also define overflow = false.
                // If we're inlining the dest, the operation succeeded without overflow.
                if let Instruction::UnaryOpChecked { overflow, .. } = instr {
                    new_instrs.push(Instruction::Const {
                        dest: *overflow,
                        value: ConstValue::Bool(false),
                    });
                    const_bools.insert(*overflow, false);
                }

                replacements.push((idx, new_instrs));
            }
        }
    }

    // Apply replacements in reverse order to maintain correct indices.
    for (idx, new_instrs) in replacements.into_iter().rev() {
        block.instructions.splice(idx..=idx, new_instrs);
    }

    // Simplify constant branches: if the terminator is a Branch with a condition
    // that is a known constant bool, replace with an unconditional Goto.
    // This is critical for UnwrapResult where is_ok = true makes the else branch
    // (which references the undefined err_dest) into dead code.
    if let Terminator::Branch { cond, then_block, then_args, else_block, else_args } = &block.terminator {
        if let Operand::Value(cond_vid) = cond {
            if let Some(&cond_value) = const_bools.get(cond_vid) {
                block.terminator = if cond_value {
                    // Condition is true, go to then_block.
                    Terminator::Goto {
                        target: *then_block,
                        args: then_args.clone(),
                    }
                } else {
                    // Condition is false, go to else_block.
                    Terminator::Goto {
                        target: *else_block,
                        args: else_args.clone(),
                    }
                };
            }
        }
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
    functions: &mut [IrCodeUnit],
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

/// Inline const values into a module IrCodeUnit.
///
/// Like inline_function_consts but qualifies const names with the function name
/// when looking them up in the const_values map.
fn inline_module_function_consts(
    func: &mut IrCodeUnit,
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

    // Eliminate dead blocks that are no longer reachable after branch simplification.
    eliminate_dead_blocks_func(func);
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datafun_ir::{BlockId, Terminator, Operand, IrType, BinOp, CodeUnitId, CodeUnitContext, ScriptContext};

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
            terminator: Terminator::Exit { value: None },
        }];

        let unit = IrCodeUnit {
            id: CodeUnitId(0),
            name: String::new(),
            blocks,
            value_count: 3,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![IrType::I32, IrType::I32, IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![("X".to_string(), ValueId(2))],
            symbols: datalove_datafun_ir::SymbolTable::new(),
            context: CodeUnitContext::Script(ScriptContext {
                unit_end_values: vec![],
                unit_end_slots: vec![],
                result: None,
                exports: vec![("X".to_string(), ExportBinding::Value(ValueId(2)))],
            }),
            nested_units: vec![],
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
                    site_id: datalove_datafun_ir::CallSiteId(0),
                    dest: ValueId(0),
                    func: datalove_datafun_ir::CodeRef::Local(datalove_datafun_ir::CodeUnitId(0)),
                    args: vec![],
                },
            ],
            terminator: Terminator::Exit { value: None },
        }];

        let unit = IrCodeUnit {
            id: CodeUnitId(0),
            name: String::new(),
            blocks,
            value_count: 1,
            slot_count: 0,
            call_site_count: 1,
            value_types: vec![IrType::I32],
            slot_types: vec![],
            tracked_slots: vec![],
            const_values: vec![("X".to_string(), ValueId(0))],
            symbols: datalove_datafun_ir::SymbolTable::new(),
            context: CodeUnitContext::Script(ScriptContext {
                unit_end_values: vec![],
                unit_end_slots: vec![],
                result: None,
                exports: vec![("X".to_string(), ExportBinding::Value(ValueId(0)))],
            }),
            nested_units: vec![],
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
        //   branch v2, block1, block2
        // We'll inline v1 with 42. This should also set v2 = true, simplify the branch
        // to goto block1, and DCE removes v2 since it's no longer used.
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

        let unit = IrCodeUnit {
            id: CodeUnitId(0),
            name: String::new(),
            blocks,
            value_count: 3,
            slot_count: 0,
            call_site_count: 0,
            value_types: vec![
                IrType::Option(Box::new(IrType::I32)),
                IrType::I32,
                IrType::Bool,
            ],
            slot_types: vec![],
            tracked_slots: vec![],
            // v1 (the unwrapped value) is tracked as a const.
            const_values: vec![("X".to_string(), ValueId(1))],
            symbols: datalove_datafun_ir::SymbolTable::new(),
            context: CodeUnitContext::Script(ScriptContext {
                unit_end_values: vec![],
                unit_end_slots: vec![],
                result: None,
                exports: vec![("X".to_string(), ExportBinding::Value(ValueId(1)))],
            }),
            nested_units: vec![],
        };

        let mut const_values = HashMap::new();
        const_values.insert("X".to_string(), ConstValue::I32(42));

        let result = inline_script_consts(unit, &const_values);

        // After inlining:
        // - v0 becomes dead (not used by anything) and is removed by DCE
        // - The UnwrapOption is replaced with Const(v1, 42) and Const(v2, true)
        // - The Branch(v2, ...) is simplified to Goto(block1) since v2 is constant true
        // - v2 is no longer used (the branch is now a goto), so DCE removes Const(v2)
        // - Only Const(v1) remains because v1 is exported
        assert_eq!(result.blocks[0].instructions.len(), 1);

        // Only instruction: Const for the unwrapped dest (v1).
        match &result.blocks[0].instructions[0] {
            Instruction::Const { dest, value } => {
                assert_eq!(*dest, ValueId(1));
                assert_eq!(*value, ConstValue::I32(42));
            }
            _ => panic!("expected Const instruction for unwrapped value"),
        }

        // Terminator should now be Goto(block1) instead of Branch.
        match &result.blocks[0].terminator {
            Terminator::Goto { target, args } => {
                assert_eq!(*target, BlockId(1));
                assert!(args.is_empty());
            }
            _ => panic!("expected Goto terminator after branch simplification"),
        }
    }
}
