use crate::*;
use datalove_rtdt as rtdt;
use datalove_datafun_ir::{
    CallSiteId, IrType, IrBlock, Terminator, CodeRef, ParamId, TypeRef, SlotId, SlotDest,
    BinOp, UnaryOp, ValueId, IrCodeUnit, CodeUnitId, CodeUnitContext, FunctionContext, ScriptContext,
};

/// Helper to create a function code unit for tests.
fn make_func_unit(
    id: u32,
    name: &str,
    params: Vec<ParamId>,
    param_types: Vec<IrType>,
    return_type: IrType,
    blocks: Vec<IrBlock>,
    value_count: u32,
    value_types: Vec<IrType>,
    slot_count: u32,
    slot_types: Vec<IrType>,
    call_site_count: u32,
) -> IrCodeUnit {
    IrCodeUnit {
        id: CodeUnitId(id),
        name: name.to_string(),
        blocks,
        value_count,
        slot_count,
        call_site_count,
        value_types,
        slot_types,
        tracked_slots: vec![],
        const_values: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        context: CodeUnitContext::Function(FunctionContext {
            params,
            param_modes: vec![],
            param_types,
            return_type,
            tracked_params: vec![],
        }),
        nested_units: vec![],
    }
}

/// Helper to create a script code unit for tests.
fn make_script_unit(
    blocks: Vec<IrBlock>,
    value_count: u32,
    value_types: Vec<IrType>,
    slot_count: u32,
    slot_types: Vec<IrType>,
    call_site_count: u32,
    unit_end_values: Vec<ValueId>,
    unit_end_slots: Vec<SlotId>,
    result: Option<ValueId>,
    exports: Vec<(String, datalove_datafun_ir::ExportBinding)>,
    nested_units: Vec<IrCodeUnit>,
) -> IrCodeUnit {
    IrCodeUnit {
        id: CodeUnitId(0),
        name: String::new(),
        blocks,
        value_count,
        slot_count,
        call_site_count,
        value_types,
        slot_types,
        tracked_slots: vec![],
        const_values: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values,
            unit_end_slots,
            result,
            result_name: None,
            exports,
        }),
        nested_units,
    }
}

/// Simple helper for parameterless test functions.
fn make_test_func(
    return_type: IrType,
    blocks: Vec<IrBlock>,
    value_types: Vec<IrType>,
) -> IrCodeUnit {
    make_func_unit(
        0,
        "test",
        vec![],
        vec![],
        return_type,
        blocks,
        value_types.len() as u32,
        value_types,
        0,
        vec![],
        0,
    )
}

/// Simple helper for script units without nested functions.
fn make_simple_script(
    blocks: Vec<IrBlock>,
    value_types: Vec<IrType>,
    slot_types: Vec<IrType>,
    unit_end_values: Vec<ValueId>,
    unit_end_slots: Vec<SlotId>,
    result: Option<ValueId>,
) -> IrCodeUnit {
    make_script_unit(
        blocks,
        value_types.len() as u32,
        value_types,
        slot_types.len() as u32,
        slot_types,
        0,
        unit_end_values,
        unit_end_slots,
        result,
        vec![],
        vec![],
    )
}

#[test]
fn test_tydesc_table_primitives() {
    let mut table = IrTyDescTable::new();

    let bool_td = table.get_or_create(&IrType::Bool);
    unsafe {
        assert_eq!((*bool_td).type_tag, rtdt::TyTag::Bool);
        assert_eq!((*bool_td).size, 1);
    }

    let i64_td = table.get_or_create(&IrType::I64);
    unsafe {
        assert_eq!((*i64_td).type_tag, rtdt::TyTag::I64);
        assert_eq!((*i64_td).size, 8);
    }
}

#[test]
fn test_layout_computation() {
    let mut table = IrTyDescTable::new();

    let value_types = vec![IrType::I64, IrType::Bool, IrType::I64];
    let slot_types = vec![IrType::I64];

    let layout = IrLayout::compute(&value_types, &slot_types, &mut table);

    // i64 at 0, bool at 8, i64 at 16, slot i64 at 24.
    assert_eq!(layout.value_offsets[0], 0);
    assert_eq!(layout.value_offsets[1], 8);
    assert_eq!(layout.value_offsets[2], 16);
    assert_eq!(layout.slot_offsets[0], 24);
    assert_eq!(layout.frame_size, 32);
}

/// Helper to create a simple two-parameter function for testing.
///
/// Returns the first parameter (identity). We avoid arithmetic here because
/// fixed-width integer arithmetic requires widening to Int or checked ops.
fn make_identity_function() -> IrCodeUnit {
    // fn identity(a: i64, b: i64) -> i64 { a }
    make_func_unit(
        0,
        "identity",
        vec![ParamId(0), ParamId(1)],  // a, b
        vec![IrType::I64, IrType::I64],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            },
        ],
        0, // value_count
        vec![], // value_types
        0, // slot_count
        vec![], // slot_types
        0, // call_site_count
    )
}

#[test]
fn test_simple_function_call() {
    // Create identity function.
    let identity_fn = make_identity_function();

    // Create main function that calls identity(10, 20).
    // fn main() -> i64 { identity(10, 20) }
    let main_code_unit = make_func_unit(
        1,
        "main",
        vec![],
        vec![],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(10),
                    },
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::I64(20),
                    },
                    Instruction::Call {
                        site_id: CallSiteId(0),
                        dest: ValueId(2),
                        func: CodeRef::Local(CodeUnitId(0)),  // identity function
                        args: vec![
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(2))),
                },
            },
        ],
        3,
        vec![IrType::I64, IrType::I64, IrType::I64],
        0,
        vec![],
        0,
    );

    // Create context with both functions.
    let functions: Vec<IrCodeUnit> = vec![identity_fn, main_code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);

    // Execute main, writing result to our storage.
    let mut interp = IrInterpreter::new();
    let mut result_storage: i64 = 0;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let ret_dest = Destination {
        ptr: &mut result_storage as *mut i64 as *mut u8,
        tydesc: ret_tydesc,
    };
    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(&main_code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();

    // Verify result is 10 (first parameter returned by identity).
    assert_eq!(result_storage, 10);
}

#[test]
fn test_nested_function_calls() {
    // fn passthrough(x: i64) -> i64 { x }
    let passthrough_fn = make_func_unit(
        0,
        "passthrough",
        vec![ParamId(0)],
        vec![IrType::I64],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            },
        ],
        0,
        vec![],
        0,
        vec![],
        0,
    );

    // fn nested(x: i64) -> i64 { passthrough(passthrough(x)) }
    let nested_code_unit = make_func_unit(
        1,
        "nested",
        vec![ParamId(0)],
        vec![IrType::I64],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // First call: passthrough(x)
                    Instruction::Call {
                        site_id: CallSiteId(0),
                        dest: ValueId(0),
                        func: CodeRef::Local(CodeUnitId(0)),
                        args: vec![Operand::Param(ParamId(0))],
                    },
                    // Second call: passthrough(result)
                    Instruction::Call {
                        site_id: CallSiteId(1),
                        dest: ValueId(1),
                        func: CodeRef::Local(CodeUnitId(0)),
                        args: vec![Operand::Value(ValueId(0))],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            },
        ],
        2,
        vec![IrType::I64, IrType::I64],
        0,
        vec![],
        0,
    );

    // Create context with both functions.
    let functions: Vec<IrCodeUnit> = vec![passthrough_fn, nested_code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);

    // Create argument value: 42.
    let mut interp = IrInterpreter::new();
    let mut arg_storage = 42i64;
    let arg_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let arg = Value {
        ptr: &mut arg_storage as *mut i64 as *mut u8,
        tydesc: arg_tydesc,
    };

    // Create destination for result.
    let mut result_storage: i64 = 0;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let ret_dest = Destination {
        ptr: &mut result_storage as *mut i64 as *mut u8,
        tydesc: ret_tydesc,
    };

    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(&nested_code_unit, None, vec![arg], ret_dest, &ctx, &registry, &mut frames).unwrap();

    // Verify result is 42 (passthrough returns input unchanged).
    assert_eq!(result_storage, 42);
}

/// Helper to run a function and get an i64 result.
fn run_i64_function(code_unit: &IrCodeUnit) -> i64 {
    let functions = [code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);
    let mut interp = IrInterpreter::new();
    let mut result: i64 = 0;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let ret_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: ret_tydesc,
    };
    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    result
}

/// Helper to run a function and get a u32 result.
fn run_u32_function(code_unit: &IrCodeUnit) -> u32 {
    let functions = [code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);
    let mut interp = IrInterpreter::new();
    let mut result: u32 = 0;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::U32);
    let ret_dest = Destination {
        ptr: &mut result as *mut u32 as *mut u8,
        tydesc: ret_tydesc,
    };
    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    result
}

/// Helper to run a function and get a bool result.
fn run_bool_function(code_unit: &IrCodeUnit) -> bool {
    let functions = [code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);
    let mut interp = IrInterpreter::new();
    let mut result: bool = false;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::Bool);
    let ret_dest = Destination {
        ptr: &mut result as *mut bool as *mut u8,
        tydesc: ret_tydesc,
    };
    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    result
}

// =========================================================================
// Const tests for all types
// =========================================================================

#[test]
fn test_const_u8() {
    let func = make_test_func(
        IrType::U8,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::U8(42),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(0))),
                },
            },
        ],
        vec![IrType::U8],
    );

    let code_unit = IrCodeUnit::from(func);
    let functions = [code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);
    let mut interp = IrInterpreter::new();
    let mut result: u8 = 0;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::U8);
    let ret_dest = Destination {
        ptr: &mut result as *mut u8,
        tydesc: ret_tydesc,
    };
    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(&code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    assert_eq!(result, 42);
}

#[test]
fn test_const_i32() {
    let func = make_test_func(
        IrType::I32,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I32(-12345),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(0))),
                },
            },
        ],
        vec![IrType::I32],
    );

    let code_unit = IrCodeUnit::from(func);
    let functions = [code_unit.clone()];
    let ctx = ExecutionContext::new(&functions);
    let mut interp = IrInterpreter::new();
    let mut result: i32 = 0;
    let ret_tydesc = interp.tydesc_table.get_or_create(&IrType::I32);
    let ret_dest = Destination {
        ptr: &mut result as *mut i32 as *mut u8,
        tydesc: ret_tydesc,
    };
    let registry = FunctionRegistry::new();
    let mut frames = FrameStore::new();
    interp.call_in_context(&code_unit, None, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    assert_eq!(result, -12345);
}

#[test]
fn test_const_bool() {
    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::Bool(true),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(0))),
                },
            },
        ],
        vec![IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

// =========================================================================
// BinOp tests
// =========================================================================
//
// NOTE: Fixed-width integer arithmetic (Add, Sub, Mul, Div, Mod) is not tested here
// because the language requires widening to Int or using checked operators (+!, etc.).
// Those paths are tested via integration tests in the dual/aot fixtures.

#[test]
fn test_binop_eq() {
    // fn test() -> bool { 42 == 42 }
    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(42) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Eq,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_binop_ne() {
    // fn test() -> bool { 1 != 2 }
    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Ne,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_binop_lt() {
    // fn test() -> bool { 1 < 2 }
    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Lt,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_binop_bitand() {
    // fn test() -> i64 { 0b1100 & 0b1010 } = 0b1000 = 8
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(0b1100) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(0b1010) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::BitAnd,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 8);
}

#[test]
fn test_binop_bitor() {
    // fn test() -> i64 { 0b1100 | 0b1010 } = 0b1110 = 14
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(0b1100) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(0b1010) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::BitOr,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 14);
}

#[test]
fn test_binop_shl() {
    // fn test() -> i64 { 1 << 4 } = 16
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(4) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Shl,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 16);
}

// =========================================================================
// BinOpChecked tests
// =========================================================================

#[test]
fn test_binop_checked_no_overflow() {
    // fn test() -> (i64, bool) { checked_add(10, 20) }
    // Returns (30, false) - no overflow.
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::BinOpChecked {
                        dest: ValueId(2),
                        overflow: ValueId(3),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::I64, IrType::Bool],
    );

    assert_eq!(run_i64_function(&func), 30);
}

#[test]
fn test_binop_checked_overflow() {
    // fn test() -> bool { let (_, overflow) = checked_add(i64::MAX, 1); overflow }
    // Returns true - overflow occurred.
    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(i64::MAX) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                    Instruction::BinOpChecked {
                        dest: ValueId(2),
                        overflow: ValueId(3),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::I64, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

// =========================================================================
// UnaryOp tests
// =========================================================================

#[test]
fn test_unaryop_neg() {
    // fn test() -> i64 { -42 }
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::UnaryOp {
                        dest: ValueId(1),
                        op: UnaryOp::Neg,
                        operand: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        vec![IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), -42);
}

#[test]
fn test_unaryop_not() {
    // fn test() -> bool { !true }
    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(true) },
                    Instruction::UnaryOp {
                        dest: ValueId(1),
                        op: UnaryOp::Not,
                        operand: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        vec![IrType::Bool, IrType::Bool],
    );

    assert!(!run_bool_function(&func));
}

#[test]
fn test_unaryop_bitnot() {
    // fn test() -> u32 { ~0u32 } = 0xFFFFFFFF
    let func = make_test_func(
        IrType::U32,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::U32(0) },
                    Instruction::UnaryOp {
                        dest: ValueId(1),
                        op: UnaryOp::BitNot,
                        operand: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        vec![IrType::U32, IrType::U32],
    );

    assert_eq!(run_u32_function(&func), 0xFFFFFFFF);
}

// =========================================================================
// SlotStore/SlotLoad tests
// =========================================================================

#[test]
fn test_slot_store_load() {
    // var x = 10; x = 42; ret x
    let func = make_func_unit(
        0,
        "test",
        vec![],
        vec![],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // var x = 10
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(0)) },
                    // x = 42
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(42) },
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(1)) },
                    // ret x
                    Instruction::SlotLoadCopy { dest: ValueId(2), slot: SlotId(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        3,
        vec![IrType::I64, IrType::I64, IrType::I64],
        1,
        vec![IrType::I64],
        0,
    );

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_slot_multiple_updates() {
    // var x = 1; x = 2; x = 4; x = 8; ret x
    let func = make_func_unit(
        0,
        "test",
        vec![],
        vec![],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(0)) },
                    // x = 2
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(1)) },
                    // x = 4
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(4) },
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(2)) },
                    // x = 8
                    Instruction::Const { dest: ValueId(3), value: ConstValue::I64(8) },
                    Instruction::SlotStoreCopy { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(3)) },
                    // ret x
                    Instruction::SlotLoadCopy { dest: ValueId(4), slot: SlotId(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        5,
        vec![IrType::I64; 5],
        1,
        vec![IrType::I64],
        0,
    );

    assert_eq!(run_i64_function(&func), 8);
}

// =========================================================================
// Control flow tests
// =========================================================================

#[test]
fn test_branch_true() {
    // fn test() -> i64 { if true { 1 } else { 2 } }
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(true) },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1), then_args: vec![], else_block: BlockId(2), else_args: vec![],
                },
            },
            IrBlock { id: BlockId(1), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
            IrBlock { id: BlockId(2), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::Bool, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 1);
}

#[test]
fn test_branch_false() {
    // fn test() -> i64 { if false { 1 } else { 2 } }
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(false) },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1), then_args: vec![], else_block: BlockId(2), else_args: vec![],
                },
            },
            IrBlock { id: BlockId(1), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
            IrBlock { id: BlockId(2), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::Bool, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 2);
}

#[test]
fn test_goto_chain() {
    // block0 -> block1 -> block2 (return)
    // Tests control flow through multiple blocks.
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Goto { target: BlockId(1), args: vec![] },
            },
            IrBlock { id: BlockId(1), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                ],
                terminator: Terminator::Goto { target: BlockId(2), args: vec![] },
            },
            IrBlock { id: BlockId(2), params: vec![],
                instructions: vec![
                    // Return a value that proves we reached block2 via block1.
                    // Use comparison to verify values from earlier blocks are accessible.
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Eq,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(0)),
                    },
                ],
                // Return the constant from block1 to prove we passed through.
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        vec![IrType::I64, IrType::I64, IrType::Bool],
    );

    // Returns 2 from block1, proving control flow worked.
    assert_eq!(run_i64_function(&func), 2);
}

// =========================================================================
// Copy/Move tests
// =========================================================================

#[test]
fn test_copy() {
    // fn test() -> i64 { let a = 42; let b = a; b }
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::Copy { dest: ValueId(1), src: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        vec![IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_move() {
    // fn test() -> i64 { let a = 42; let b = move a; b }
    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::Move { dest: ValueId(1), src: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        vec![IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 42);
}

// =========================================================================
// Pack/Unpack tests
// =========================================================================

#[test]
fn test_pack_tuple() {
    // fn test() -> i64 { let t = (10, 20); let (a, _) = t; a }
    // Pack tuple, then unpack and return first element.
    let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64]);

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Pack {
                        dest: ValueId(2),
                        ty: TypeRef::Tuple(0),
                        fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                    },
                    Instruction::Unpack {
                        dests: vec![ValueId(3), ValueId(4)],
                        src: Operand::Value(ValueId(2)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        vec![IrType::I64, IrType::I64, tuple_ty, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 10);
}

#[test]
fn test_unpack_tuple() {
    // fn test() -> i64 { let (a, b) = (10, 20); b }
    let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64]);

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create tuple (10, 20).
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Pack {
                        dest: ValueId(2),
                        ty: TypeRef::Tuple(0),
                        fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                    },
                    // Unpack to (a, b).
                    Instruction::Unpack {
                        dests: vec![ValueId(3), ValueId(4)],
                        src: Operand::Value(ValueId(2)),
                    },
                ],
                // Return b to verify unpack worked.
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        vec![
            IrType::I64, IrType::I64, tuple_ty,
            IrType::I64, IrType::I64
        ],
    );

    assert_eq!(run_i64_function(&func), 20);
}

// =========================================================================
// Option tests
// =========================================================================

#[test]
fn test_wrap_some_unwrap() {
    // fn test() -> i64 { let opt = Some(42); opt.unwrap() }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::WrapSome {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapOption {
                        dest: ValueId(2),
                        is_some: ValueId(3),
                        src: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool],
    );

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_is_some() {
    // fn test() -> bool { let opt = Some(42); opt.is_some() }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::WrapSome {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapOption {
                        dest: ValueId(2),
                        is_some: ValueId(3),
                        src: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_is_none() {
    // fn test() -> bool { let opt: ?i64 = none; !opt.is_some() }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::WrapNone { dest: ValueId(0) },
                    Instruction::UnwrapOption {
                        dest: ValueId(1),
                        is_some: ValueId(2),
                        src: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnaryOp {
                        dest: ValueId(3),
                        op: UnaryOp::Not,
                        operand: Operand::Value(ValueId(2)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        vec![opt_ty, IrType::I64, IrType::Bool, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_option_branch() {
    // fn test() -> i64 { if Some(42).is_some() { 1 } else { 0 } }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::WrapSome {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapOption {
                        dest: ValueId(2),
                        is_some: ValueId(3),
                        src: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(3)),
                    then_block: BlockId(1), then_args: vec![], else_block: BlockId(2), else_args: vec![],
                },
            },
            IrBlock { id: BlockId(1), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(4), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
            IrBlock { id: BlockId(2), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(5), value: ConstValue::I64(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
        ],
        vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 1);
}

// =========================================================================
// Result tests
// =========================================================================

#[test]
fn test_wrap_ok_unwrap() {
    // fn test() -> i64 { let res = Ok(42); res.unwrap() }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::WrapOk {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapResult {
                        ok_dest: ValueId(2),
                        err_dest: ValueId(3),
                        is_ok: ValueId(4),
                        src: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        vec![IrType::I64, res_ty, IrType::I64, IrType::Error, IrType::Bool],
    );

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_is_ok() {
    // fn test() -> bool { let res = Ok(42); res.is_ok() }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::WrapOk {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapResult {
                        ok_dest: ValueId(2),
                        err_dest: ValueId(3),
                        is_ok: ValueId(4),
                        src: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        vec![IrType::I64, res_ty, IrType::I64, IrType::Error, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_is_err() {
    // fn test() -> bool { let res: !i64 = err; !res.is_ok() }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create an error value for WrapErr.
                    // Error is represented as IrType::Error.
                    Instruction::Const { dest: ValueId(0), value: ConstValue::Unit },
                    Instruction::WrapErr {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapResult {
                        ok_dest: ValueId(2),
                        err_dest: ValueId(3),
                        is_ok: ValueId(4),
                        src: Operand::Value(ValueId(1)),
                    },
                    Instruction::UnaryOp {
                        dest: ValueId(5),
                        op: UnaryOp::Not,
                        operand: Operand::Value(ValueId(4)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
        ],
        vec![IrType::Unit, res_ty, IrType::I64, IrType::Error, IrType::Bool, IrType::Bool],
    );

    assert!(run_bool_function(&func));
}

#[test]
fn test_result_branch() {
    // fn test() -> i64 { if Ok(42).is_ok() { 1 } else { 0 } }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::WrapOk {
                        dest: ValueId(1),
                        inner: Operand::Value(ValueId(0)),
                    },
                    Instruction::UnwrapResult {
                        ok_dest: ValueId(2),
                        err_dest: ValueId(3),
                        is_ok: ValueId(4),
                        src: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(4)),
                    then_block: BlockId(1), then_args: vec![], else_block: BlockId(2), else_args: vec![],
                },
            },
            IrBlock { id: BlockId(1), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(5), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
            IrBlock { id: BlockId(2), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(6), value: ConstValue::I64(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(6))) },
            },
        ],
        vec![IrType::I64, res_ty, IrType::I64, IrType::Error, IrType::Bool, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 1);
}

// =========================================================================
// Struct tests
// =========================================================================

#[test]
fn test_pack_struct() {
    // fn test() -> i64 { let s = { x: 10, y: 20 }; let { x, _ } = s; x }
    // Pack struct, then unpack and return first field.
    let struct_ty = IrType::Struct(vec![
        ("x".to_string(), IrType::I64),
        ("y".to_string(), IrType::I64),
    ]);

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Pack {
                        dest: ValueId(2),
                        ty: TypeRef::AnonStruct(0),
                        fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                    },
                    Instruction::Unpack {
                        dests: vec![ValueId(3), ValueId(4)],
                        src: Operand::Value(ValueId(2)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        vec![IrType::I64, IrType::I64, struct_ty, IrType::I64, IrType::I64],
    );

    assert_eq!(run_i64_function(&func), 10);
}

#[test]
fn test_unpack_struct() {
    // fn test() -> i64 { let { x, y } = { x: 10, y: 20 }; y }
    let struct_ty = IrType::Struct(vec![
        ("x".to_string(), IrType::I64),
        ("y".to_string(), IrType::I64),
    ]);

    let func = make_test_func(
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create struct { x: 10, y: 20 }.
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Pack {
                        dest: ValueId(2),
                        ty: TypeRef::AnonStruct(0),
                        fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                    },
                    // Unpack to (x, y).
                    Instruction::Unpack {
                        dests: vec![ValueId(3), ValueId(4)],
                        src: Operand::Value(ValueId(2)),
                    },
                ],
                // Return y to verify unpack worked.
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        vec![
            IrType::I64, IrType::I64, struct_ty,
            IrType::I64, IrType::I64
        ],
    );

    assert_eq!(run_i64_function(&func), 20);
}

// ==========================================================================
// Cross-unit reference tests
// ==========================================================================

#[test]
fn test_crossunit_external_value() {
    // Unit 0: let x = 42
    let unit0 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(42),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        1,
        vec![IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![("x".to_string(), datalove_datafun_ir::ExportBinding::Value(ValueId(0)))],
        vec![],
    );

    // Unit 1: return x (from unit 0)
    let unit1 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Copy external value to local for return.
                    Instruction::Copy {
                        dest: ValueId(0),
                        src: Operand::ExternalValue { unit: 0, value: ValueId(0) },
                    },
                ],
                terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
            },
        ],
        1,
        vec![IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        Some(ValueId(0)),
        vec![],
        vec![],
    );

    // Execute both units.
    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    // Create ret_dest for early returns (Result<(), Error>).
    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    // Execute unit 0 (fragment, no result).
    let completion = interp.execute_script_unit_in_env(&unit0, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    // Execute unit 1 (returns x).
    let mut result: i64 = 0;
    let expr_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let expr_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: expr_tydesc,
    };
    let completion = interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest, Some(expr_dest)).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    assert_eq!(result, 42);
}

#[test]
fn test_crossunit_external_slot() {
    // Unit 0: var y = 10
    let unit0 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(10),
                    },
                    Instruction::SlotStoreCopy {
                        dest: SlotDest::Local(SlotId(0)),
                        value: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        1,
        vec![IrType::I64],
        1,
        vec![IrType::I64],
        0,
        vec![],
        vec![],
        None,
        vec![("y".to_string(), datalove_datafun_ir::ExportBinding::Slot(SlotId(0)))],
        vec![],
    );

    // Unit 1: return y (from unit 0's slot)
    let unit1 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Copy external slot to local value for return.
                    Instruction::Copy {
                        dest: ValueId(0),
                        src: Operand::ExternalSlot { unit: 0, slot: SlotId(0) },
                    },
                ],
                terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
            },
        ],
        1,
        vec![IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        Some(ValueId(0)),
        vec![],
        vec![],
    );

    // Execute both units.
    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    // Create ret_dest for early returns (Result<(), Error>).
    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    // Execute unit 0 (fragment).
    let completion = interp.execute_script_unit_in_env(&unit0, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    // Execute unit 1 (returns y).
    let mut result: i64 = 0;
    let expr_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let expr_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: expr_tydesc,
    };
    let completion = interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest, Some(expr_dest)).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    assert_eq!(result, 10);
}

#[test]
fn test_crossunit_external_function() {
    // Unit 0: fn identity(x: i64) -> i64 { x }
    let identity_fn = make_func_unit(
        0,
        "identity",
        vec![ParamId(0)],
        vec![IrType::I64],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            },
        ],
        0,
        vec![],
        0,
        vec![],
        0,
    );

    let unit0 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        0,
        vec![],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![("identity".to_string(), datalove_datafun_ir::ExportBinding::Function(CodeUnitId(0)))],
        vec![identity_fn],
    );

    // Unit 1: return identity(7)
    let unit1 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(7),
                    },
                    Instruction::Call {
                        site_id: CallSiteId(0),
                        dest: ValueId(1),
                        func: CodeRef::External { unit: 0, id: CodeUnitId(0) },
                        args: vec![Operand::Value(ValueId(0))],
                    },
                ],
                terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(1))) },
            },
        ],
        2,
        vec![IrType::I64, IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        Some(ValueId(1)),
        vec![],
        vec![],
    );

    // Execute both units.
    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    // Create ret_dest for early returns (Result<(), Error>).
    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    // Execute unit 0 (fragment with function).
    let completion = interp.execute_script_unit_in_env(&unit0, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    // Execute unit 1 (returns identity(7)).
    let mut result: i64 = 0;
    let expr_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let expr_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: expr_tydesc,
    };
    let completion = interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest, Some(expr_dest)).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    assert_eq!(result, 7);  // identity(7) = 7
}

#[test]
fn test_crossunit_chain() {
    // Unit 0: let a = 5
    let unit0 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(5),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        1,
        vec![IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![("a".to_string(), datalove_datafun_ir::ExportBinding::Value(ValueId(0)))],
        vec![],
    );

    // Unit 1: let b = a (just pass through)
    let unit1 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Load a from unit 0.
                    Instruction::Copy {
                        dest: ValueId(0),
                        src: Operand::ExternalValue { unit: 0, value: ValueId(0) },
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        1,
        vec![IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![("b".to_string(), datalove_datafun_ir::ExportBinding::Value(ValueId(0)))],
        vec![],
    );

    // Unit 2: return b
    let unit2 = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Load b from unit 1.
                    Instruction::Copy {
                        dest: ValueId(0),
                        src: Operand::ExternalValue { unit: 1, value: ValueId(0) },
                    },
                ],
                terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
            },
        ],
        1,
        vec![IrType::I64],
        0,
        vec![],
        0,
        vec![],
        vec![],
        Some(ValueId(0)),
        vec![],
        vec![],
    );

    // Execute all units.
    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    // Create ret_dest for early returns (Result<(), Error>).
    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    // Execute unit 0 and unit 1 (fragments).
    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit0.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);
    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit1.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    // Execute unit 2 (returns b which is a copy of a).
    let mut result: i64 = 0;
    let expr_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let expr_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: expr_tydesc,
    };
    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit2.clone()), &mut env, ret_dest, Some(expr_dest)).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    assert_eq!(result, 5);  // b = a = 5
}

// NOTE: Phi tests removed - Phi instruction has been replaced by block parameters.

// =============================================================================
// Precise Drop Tests
//
// These tests verify that values with explicit Drop instructions are properly destroyed.
// All values are now precise - they require explicit Drop instructions at known points.
// Only slots (var bindings) use runtime tracking.
// =============================================================================

/// Test that a list with explicit Drop is properly destroyed.
#[test]
fn test_list_with_explicit_drop() {
    // Script unit that creates a list and explicitly drops it.
    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create element values.
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    // Create list from elements.
                    Instruction::ListNew {
                        dest: ValueId(2),
                        elements: vec![
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ],
                    },
                    // Explicit drop - this is required since list is not tracked.
                    Instruction::Drop { operand: Operand::Value(ValueId(2)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        3,
        vec![IrType::I64, IrType::I64, IrType::List(Box::new(IrType::I64))],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&unit, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    // destroy_live_values won't touch the list (not tracked), but explicit Drop already handled it.
    env.destroy_live_values(interp.runtime_handle());
}

/// Test script with a function and explicit list drop.
#[test]
fn test_script_with_function_and_list_drop() {
    // A function with two i64 parameters that just returns the first one.
    let identity_fn = make_func_unit(
        0,
        "identity",
        vec![ParamId(0), ParamId(1)],
        vec![IrType::I64, IrType::I64],
        IrType::I64,
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![],
                terminator: Terminator::Return {
                    value: Some(Operand::Param(ParamId(0))),
                },
            },
        ],
        0,
        vec![],
        0,
        vec![],
        0,
    );

    // Script unit that defines the function and creates a list binding with explicit drop.
    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create list [100, 200, 300, 400].
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(100) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(200) },
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(300) },
                    Instruction::Const { dest: ValueId(3), value: ConstValue::I64(400) },
                    Instruction::ListNew {
                        dest: ValueId(4),
                        elements: vec![
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                            Operand::Value(ValueId(2)),
                            Operand::Value(ValueId(3)),
                        ],
                    },
                    // Call the function (just to exercise it).
                    Instruction::Const { dest: ValueId(5), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(6), value: ConstValue::I64(20) },
                    Instruction::Call {
                        site_id: CallSiteId(0),
                        dest: ValueId(7),
                        func: CodeRef::Local(CodeUnitId(0)),
                        args: vec![Operand::Value(ValueId(5)), Operand::Value(ValueId(6))],
                    },
                    // Explicit drop of list (values are precise).
                    Instruction::Drop { operand: Operand::Value(ValueId(4)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        8,
        vec![
            IrType::I64, IrType::I64, IrType::I64, IrType::I64,  // list elements
            IrType::List(Box::new(IrType::I64)),                  // the list
            IrType::I64, IrType::I64, IrType::I64,                // call args and result
        ],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![],
        vec![identity_fn],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&unit, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

/// Test that multiple values with explicit Drops are properly destroyed.
#[test]
fn test_multiple_explicit_drops() {
    // Create two separate lists with explicit drops.
    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // First list [1, 2].
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                    Instruction::ListNew {
                        dest: ValueId(2),
                        elements: vec![
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                        ],
                    },
                    // Second list [3, 4].
                    Instruction::Const { dest: ValueId(3), value: ConstValue::I64(3) },
                    Instruction::Const { dest: ValueId(4), value: ConstValue::I64(4) },
                    Instruction::ListNew {
                        dest: ValueId(5),
                        elements: vec![
                            Operand::Value(ValueId(3)),
                            Operand::Value(ValueId(4)),
                        ],
                    },
                    // Explicit drops for both lists.
                    Instruction::Drop { operand: Operand::Value(ValueId(2)) },
                    Instruction::Drop { operand: Operand::Value(ValueId(5)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        6,
        vec![
            IrType::I64, IrType::I64, IrType::List(Box::new(IrType::I64)),
            IrType::I64, IrType::I64, IrType::List(Box::new(IrType::I64)),
        ],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&unit, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_ctfe_struct_with_string_destruction() {
    // Test that CTFE struct with string field is properly destroyed.
    // const P: {name: string, age: u32} = {name = "Alice", age = 30}
    let struct_type = IrType::Struct(vec![
        ("name".to_string(), IrType::String),
        ("age".to_string(), IrType::U32),
    ]);

    let struct_const = ConstValue::Struct(vec![
        ("name".to_string(), ConstValue::String("Alice".to_string())),
        ("age".to_string(), ConstValue::U32(30)),
    ]);

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create struct constant.
                    Instruction::Const { dest: ValueId(0), value: struct_const },
                    // Explicit drop.
                    Instruction::Drop { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![struct_type],
        0,
        vec![],
        0,
        vec![],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_ctfe_struct_with_string_unit_end_destruction() {
    // Test that CTFE struct with string field is properly destroyed via unit_end_values.
    // This mimics how the test fixture works: const P binding that stays alive until env cleanup.
    let struct_type = IrType::Struct(vec![
        ("name".to_string(), IrType::String),
        ("age".to_string(), IrType::U32),
    ]);

    let struct_const = ConstValue::Struct(vec![
        ("name".to_string(), ConstValue::String("Alice".to_string())),
        ("age".to_string(), ConstValue::U32(30)),
    ]);

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create struct constant (no explicit drop - destroyed via unit_end_values).
                    Instruction::Const { dest: ValueId(0), value: struct_const },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![struct_type],
        0,
        vec![],
        0,
        vec![ValueId(0)],  // Struct is tracked for cleanup.
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&unit, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_ctfe_struct_with_string_and_debuglog() {
    // Test that CTFE struct with string field is properly destroyed when debuglog is used.
    // This exactly mimics the failing test fixture.
    let struct_type = IrType::Struct(vec![
        ("name".to_string(), IrType::String),
        ("age".to_string(), IrType::U32),
    ]);

    let struct_const = ConstValue::Struct(vec![
        ("name".to_string(), ConstValue::String("Alice".to_string())),
        ("age".to_string(), ConstValue::U32(30)),
    ]);

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    // Create struct constant.
                    Instruction::Const { dest: ValueId(0), value: struct_const },
                    // DebugLog the struct.
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![struct_type],
        0,
        vec![],
        0,
        vec![ValueId(0)],  // Struct is tracked for cleanup.
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&unit, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}


// =========================================================================
// Tests for Error, Data, and ResultErr ConstValue variants
// =========================================================================

#[test]
fn test_const_error() {
    // Test ConstValue::Error - boxes an inner value.
    let error_const = ConstValue::Error(Box::new(ConstValue::String("test error message".to_string())));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: error_const },
                    // DebugLog to verify it can be printed.
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Error],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&unit, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_data() {
    // Test ConstValue::Data - boxes an inner value.
    let data_const = ConstValue::Data(Box::new(ConstValue::I32(42)));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: data_const },
                    // DebugLog to verify it can be printed.
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Data],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_result_err() {
    // Test ConstValue::ResultErr - creates a Result::Err from an Error.
    let result_err_const = ConstValue::ResultErr(Box::new(
        ConstValue::Error(Box::new(ConstValue::String("error in result".to_string())))
    ));

    let result_type = IrType::Result(Box::new(IrType::I32));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: result_err_const },
                    // DebugLog to verify it can be printed.
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![result_type],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_data_with_string() {
    // Test ConstValue::Data with a String inside.
    let data_const = ConstValue::Data(Box::new(ConstValue::String("boxed string".to_string())));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: data_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Data],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_error_with_i32() {
    // Test ConstValue::Error boxing an i32.
    let error_const = ConstValue::Error(Box::new(ConstValue::I32(42)));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: error_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Error],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_error_with_tuple() {
    // Test ConstValue::Error boxing a tuple.
    let error_const = ConstValue::Error(Box::new(ConstValue::Tuple(vec![
        ConstValue::I32(1),
        ConstValue::Bool(true),
    ])));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: error_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Error],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_data_with_tuple() {
    // Test ConstValue::Data boxing a tuple.
    let data_const = ConstValue::Data(Box::new(ConstValue::Tuple(vec![
        ConstValue::U32(100),
        ConstValue::U32(200),
    ])));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: data_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Data],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_result_err_with_i32() {
    // Test ConstValue::ResultErr with an Error containing i32.
    let result_err_const = ConstValue::ResultErr(Box::new(
        ConstValue::Error(Box::new(ConstValue::I32(999)))
    ));

    let result_type = IrType::Result(Box::new(IrType::String));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: result_err_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![result_type],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_data_with_bool() {
    // Test ConstValue::Data boxing a bool.
    let data_const = ConstValue::Data(Box::new(ConstValue::Bool(true)));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: data_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Data],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}

#[test]
fn test_const_error_with_unit() {
    // Test ConstValue::Error boxing Unit.
    let error_const = ConstValue::Error(Box::new(ConstValue::Unit));

    let unit = make_script_unit(
        vec![
            IrBlock { id: BlockId(0), params: vec![],
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: error_const },
                    Instruction::DebugLog { operand: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: None },
            },
        ],
        1,
        vec![IrType::Error],
        0,
        vec![],
        0,
        vec![ValueId(0)],
        vec![],
        None,
        vec![],
        vec![],
    );

    let mut interp = IrInterpreter::new();
    let mut env = ScriptEnvironment::new();

    let ret_type = IrType::Result(Box::new(IrType::Unit));
    let ret_tydesc = interp.tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = Destination { ptr: ret_buffer.as_mut_ptr(), tydesc: ret_tydesc };

    let completion = interp.execute_script_unit_in_env(&IrCodeUnit::from(unit.clone()), &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    env.destroy_live_values(interp.runtime_handle());
}
