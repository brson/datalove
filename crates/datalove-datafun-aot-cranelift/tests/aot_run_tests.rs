//! Integration tests that compile, link, and run AOT code.

use datalove_datafun_aot_cranelift::AotCompiler;
use datalove_datafun_ir::{
    BlockId, ConstValue, IrBlock, IrScriptUnit, IrType, Instruction, Operand, Terminator, ValueId,
};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

/// Create a simple script unit that logs an i32 constant.
fn create_debuglog_i32_script(value: i32) -> IrScriptUnit {
    IrScriptUnit {
        blocks: vec![IrBlock {
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I32],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![],
    }
}

static RUNTIME_LIB_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Build the runtime library once and return the path to the lib directory.
fn ensure_runtime_lib() -> &'static Path {
    RUNTIME_LIB_DIR.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .unwrap_or_else(|_| ".".to_string());
        let manifest_path = std::path::PathBuf::from(manifest_dir);
        let workspace_root = manifest_path.join("../..").canonicalize()
            .expect("failed to find workspace root");
        let lib_dir = workspace_root.join("target/debug");

        let status = Command::new("cargo")
            .args(["build", "-p", "datalove-rt"])
            .current_dir(&workspace_root)
            .status()
            .expect("failed to run cargo build");
        if !status.success() {
            panic!("Failed to build datalove-rt");
        }

        lib_dir
    })
}

#[test]
fn test_link_and_run_debuglog_i32() {
    let unit = create_debuglog_i32_script(42);

    // Compile to object file.
    let mut compiler = AotCompiler::new_for_host().expect("failed to create compiler");
    let product = compiler
        .compile_script_unit(&unit)
        .expect("failed to compile script unit");
    let obj_bytes = product.emit().expect("failed to emit object");

    // Write object to temp file.
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let obj_path = dir.path().join("test.o");
    std::fs::write(&obj_path, &obj_bytes).expect("failed to write object file");

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    // Link statically with cc.
    let exe_path = dir.path().join("test");
    let link_status = Command::new("cc")
        .args([
            obj_path.to_str().unwrap(),
            lib_path.to_str().unwrap(),
            "-ldl", "-lpthread", "-lm",
            "-o", exe_path.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run linker");

    if !link_status.success() {
        panic!("Linker failed with status: {:?}", link_status);
    }

    // Run the executable.
    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run executable");

    if !output.status.success() {
        eprintln!("stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("Executable failed with status: {:?}", output.status);
    }

    // Check output.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("42"),
        "Expected stderr to contain '42', got: {:?}",
        stderr
    );
}

#[test]
fn test_link_and_run_debuglog_bool_true() {
    let unit = IrScriptUnit {
        blocks: vec![IrBlock {
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::Bool],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![],
    };

    // Compile to object file.
    let mut compiler = AotCompiler::new_for_host().expect("failed to create compiler");
    let product = compiler
        .compile_script_unit(&unit)
        .expect("failed to compile script unit");
    let obj_bytes = product.emit().expect("failed to emit object");

    // Write object to temp file.
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let obj_path = dir.path().join("test.o");
    std::fs::write(&obj_path, &obj_bytes).expect("failed to write object file");

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    // Link statically with cc.
    let exe_path = dir.path().join("test");
    let link_status = Command::new("cc")
        .args([
            obj_path.to_str().unwrap(),
            lib_path.to_str().unwrap(),
            "-ldl", "-lpthread", "-lm",
            "-o", exe_path.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run linker");

    if !link_status.success() {
        panic!("Linker failed");
    }

    // Run the executable.
    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run executable");

    if !output.status.success() {
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("Executable failed");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("true"),
        "Expected stderr to contain 'true', got: {:?}",
        stderr
    );
}

#[test]
fn test_link_and_run_multiple_debuglogs() {
    let unit = IrScriptUnit {
        blocks: vec![IrBlock {
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I32, IrType::I32, IrType::I32],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![],
    };

    // Compile to object file.
    let mut compiler = AotCompiler::new_for_host().expect("failed to create compiler");
    let product = compiler
        .compile_script_unit(&unit)
        .expect("failed to compile script unit");
    let obj_bytes = product.emit().expect("failed to emit object");

    // Write object to temp file.
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let obj_path = dir.path().join("test.o");
    std::fs::write(&obj_path, &obj_bytes).expect("failed to write object file");

    // Find runtime library.
    let lib_dir = ensure_runtime_lib();
    let lib_path = lib_dir.join("libdatalove_rt.a");

    // Link statically with cc.
    let exe_path = dir.path().join("test");
    let link_status = Command::new("cc")
        .args([
            obj_path.to_str().unwrap(),
            lib_path.to_str().unwrap(),
            "-ldl", "-lpthread", "-lm",
            "-o", exe_path.to_str().unwrap(),
        ])
        .status()
        .expect("failed to run linker");

    if !link_status.success() {
        panic!("Linker failed");
    }

    // Run the executable.
    let output = Command::new(&exe_path)
        .output()
        .expect("failed to run executable");

    if !output.status.success() {
        eprintln!("stderr: {}", String::from_utf8_lossy(&output.stderr));
        panic!("Executable failed");
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("1"), "Expected '1' in output: {:?}", stderr);
    assert!(stderr.contains("2"), "Expected '2' in output: {:?}", stderr);
    assert!(stderr.contains("3"), "Expected '3' in output: {:?}", stderr);
}
