//! Integration tests that compile, link, and run AOT code.

use datalove_datafun::pipeline::aot as pipeline_aot;
use datalove_datafun_ir::{
    BlockId, ConstValue, IrBlock, IrCodeUnit, CodeUnitId, CodeUnitContext, ScriptContext,
    IrType, Instruction, Operand, Terminator, ValueId, SymbolTable,
};

/// Helper to create a script code unit for tests.
fn make_script_unit(
    blocks: Vec<IrBlock>,
    value_types: Vec<IrType>,
    result: Option<ValueId>,
) -> IrCodeUnit {
    IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks,
        value_count: value_types.len() as u32,
        slot_count: 0,
        value_types,
        slot_types: vec![],
        tracked_slots: vec![],
        const_values: vec![],
        symbols: SymbolTable::default(),
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values: vec![],
            unit_end_slots: vec![],
            result,
            result_name: None,
            exports: vec![],
        }),
        nested_units: vec![],
    }
}

/// Create a simple script unit that logs an i32 constant.
fn create_debuglog_i32_script(value: i32) -> IrCodeUnit {
    make_script_unit(
        vec![IrBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::Const {
                    dest: ValueId(0),
                    value: ConstValue::I32(value),
                },
                Instruction::DebugLog {
                    operand: Operand::Value(ValueId(0)),
                },
            ],
            terminator: Terminator::UnitEnd { result: None },
        }],
        vec![IrType::I32],
        None,
    )
}

#[test]
fn test_link_and_run_debuglog_i32() {
    let unit = create_debuglog_i32_script(42);

    // Use pipeline::aot to compile, link, and run.
    let output = pipeline_aot::compile_link_run(&unit)
        .expect("compile_link_run failed");

    // Check output.
    assert!(
        output.stderr.contains("42"),
        "Expected stderr to contain '42', got: {:?}",
        output.stderr
    );
}

#[test]
fn test_link_and_run_debuglog_bool_true() {
    let unit = make_script_unit(
        vec![IrBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::Const {
                    dest: ValueId(0),
                    value: ConstValue::Bool(true),
                },
                Instruction::DebugLog {
                    operand: Operand::Value(ValueId(0)),
                },
            ],
            terminator: Terminator::UnitEnd { result: None },
        }],
        vec![IrType::Bool],
        None,
    );

    // Use pipeline::aot to compile, link, and run.
    let output = pipeline_aot::compile_link_run(&unit)
        .expect("compile_link_run failed");

    assert!(
        output.stderr.contains("true"),
        "Expected stderr to contain 'true', got: {:?}",
        output.stderr
    );
}

#[test]
fn test_link_and_run_multiple_debuglogs() {
    let unit = make_script_unit(
        vec![IrBlock {
            id: BlockId(0),
            params: vec![],
            instructions: vec![
                Instruction::Const {
                    dest: ValueId(0),
                    value: ConstValue::I32(1),
                },
                Instruction::DebugLog {
                    operand: Operand::Value(ValueId(0)),
                },
                Instruction::Const {
                    dest: ValueId(1),
                    value: ConstValue::I32(2),
                },
                Instruction::DebugLog {
                    operand: Operand::Value(ValueId(1)),
                },
                Instruction::Const {
                    dest: ValueId(2),
                    value: ConstValue::I32(3),
                },
                Instruction::DebugLog {
                    operand: Operand::Value(ValueId(2)),
                },
            ],
            terminator: Terminator::UnitEnd { result: None },
        }],
        vec![IrType::I32, IrType::I32, IrType::I32],
        None,
    );

    // Use pipeline::aot to compile, link, and run.
    let output = pipeline_aot::compile_link_run(&unit)
        .expect("compile_link_run failed");

    assert!(output.stderr.contains("1"), "Expected '1' in output: {:?}", output.stderr);
    assert!(output.stderr.contains("2"), "Expected '2' in output: {:?}", output.stderr);
    assert!(output.stderr.contains("3"), "Expected '3' in output: {:?}", output.stderr);
}
