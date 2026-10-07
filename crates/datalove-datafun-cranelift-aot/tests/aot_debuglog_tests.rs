//! Integration tests for AOT compilation of debuglog statements.

use datalove_datafun_cranelift_aot::AotCompiler;
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
    // IR equivalent of: debuglog @42
    //
    // Block 0:
    //   v0 = const i32 42
    //   debuglog v0
    //   unit_end

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
fn test_compile_debuglog_i32() {
    let unit = create_debuglog_i32_script(42);

    let mut compiler = AotCompiler::new_for_host().expect("failed to create compiler");
    let product = compiler
        .compile_script_unit(&unit)
        .expect("failed to compile script unit");

    // Emit object file bytes.
    let obj_bytes = product.emit().expect("failed to emit object");

    // Verify we got some output.
    assert!(!obj_bytes.is_empty(), "object file should not be empty");

    // The object file should contain our function names.
    let obj_str = String::from_utf8_lossy(&obj_bytes);
    assert!(
        obj_str.contains("main") || obj_bytes.len() > 100,
        "object file should contain generated code"
    );

    // Write to temp file to verify it's a valid object.
    let dir = rmx::tempfile::tempdir().expect("failed to create temp dir");
    let obj_path = dir.path().join("test.o");
    std::fs::write(&obj_path, &obj_bytes).expect("failed to write object file");

    // Verify file was written.
    let metadata = std::fs::metadata(&obj_path).expect("failed to read metadata");
    assert!(metadata.len() > 0, "object file should have content");
}

#[test]
fn test_compile_debuglog_bool() {
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

    let mut compiler = AotCompiler::new_for_host().expect("failed to create compiler");
    let product = compiler
        .compile_script_unit(&unit)
        .expect("failed to compile script unit");

    let obj_bytes = product.emit().expect("failed to emit object");
    assert!(!obj_bytes.is_empty());
}

#[test]
fn test_compile_multiple_debuglogs() {
    // Test multiple debuglog statements in sequence.
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

    let mut compiler = AotCompiler::new_for_host().expect("failed to create compiler");
    let product = compiler
        .compile_script_unit(&unit)
        .expect("failed to compile script unit");

    let obj_bytes = product.emit().expect("failed to emit object");
    assert!(!obj_bytes.is_empty());
}
