use crate::*;
use datalove_rt::rtdt;
use datalove_datafun_ir::{IrType, IrBlock, Terminator, FuncRef, FuncId, TypeRef, SlotId, SlotDest};

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

/// Helper to create a simple function for testing.
fn make_add_function() -> IrFunction {
    // fn add(a: i64, b: i64) -> i64 { a + b }
    IrFunction {
        id: FuncId(0),
        name: "add".to_string(),
        params: vec![ValueId(0), ValueId(1)],  // a, b
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(2))),
                },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    }
}

#[test]
fn test_simple_function_call() {
    // Create the add function.
    let add_fn = make_add_function();

    // Create main function that calls add(10, 20).
    // fn main() -> i64 { add(10, 20) }
    let main_fn = IrFunction {
        id: FuncId(1),
        name: "main".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
                        dest: ValueId(2),
                        func: FuncRef::Local(FuncId(0)),  // add function
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    // Create context with both functions.
    let functions = vec![add_fn, main_fn.clone()];
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
    interp.call_in_context(&main_fn, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();

    // Verify result is 30.
    assert_eq!(result_storage, 30);
}

#[test]
fn test_nested_function_calls() {
    // fn double(x: i64) -> i64 { x + x }
    let double_fn = IrFunction {
        id: FuncId(0),
        name: "double".to_string(),
        params: vec![ValueId(0)],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::BinOp {
                        dest: ValueId(1),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            },
        ],
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    // fn quadruple(x: i64) -> i64 { double(double(x)) }
    let quadruple_fn = IrFunction {
        id: FuncId(1),
        name: "quadruple".to_string(),
        params: vec![ValueId(0)],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    // First call: double(x)
                    Instruction::Call {
                        dest: ValueId(1),
                        func: FuncRef::Local(FuncId(0)),
                        args: vec![Operand::Value(ValueId(0))],
                    },
                    // Second call: double(result)
                    Instruction::Call {
                        dest: ValueId(2),
                        func: FuncRef::Local(FuncId(0)),
                        args: vec![Operand::Value(ValueId(1))],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(2))),
                },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    // Create context with both functions.
    let functions = vec![double_fn, quadruple_fn.clone()];
    let ctx = ExecutionContext::new(&functions);

    // Create argument value: 5.
    let mut interp = IrInterpreter::new();
    let mut arg_storage = 5i64;
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
    interp.call_in_context(&quadruple_fn, vec![arg], ret_dest, &ctx, &registry, &mut frames).unwrap();

    // Verify result is 20 (5 * 2 * 2).
    assert_eq!(result_storage, 20);
}

/// Helper to run a function and get an i64 result.
fn run_i64_function(func: &IrFunction) -> i64 {
    let functions = [func.clone()];
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
    interp.call_in_context(func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    result
}

/// Helper to run a function and get a u32 result.
fn run_u32_function(func: &IrFunction) -> u32 {
    let functions = [func.clone()];
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
    interp.call_in_context(func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    result
}

/// Helper to run a function and get a bool result.
fn run_bool_function(func: &IrFunction) -> bool {
    let functions = [func.clone()];
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
    interp.call_in_context(func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    result
}

// =========================================================================
// Const tests for all types
// =========================================================================

#[test]
fn test_const_u8() {
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::U8],
        slot_types: vec![],
    };

    let functions = [func.clone()];
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
    interp.call_in_context(&func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    assert_eq!(result, 42);
}

#[test]
fn test_const_i32() {
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I32],
        slot_types: vec![],
    };

    let functions = [func.clone()];
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
    interp.call_in_context(&func, vec![], ret_dest, &ctx, &registry, &mut frames).unwrap();
    assert_eq!(result, -12345);
}

#[test]
fn test_const_bool() {
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

// =========================================================================
// BinOp tests
// =========================================================================

#[test]
fn test_binop_sub() {
    // fn test() -> i64 { 100 - 42 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(100) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(42) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Sub,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 58);
}

#[test]
fn test_binop_mul() {
    // fn test() -> i64 { 7 * 6 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(7) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(6) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Mul,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_binop_div() {
    // fn test() -> i64 { 84 / 2 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(84) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Div,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_binop_mod() {
    // fn test() -> i64 { 47 % 5 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(47) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(5) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Mod,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 2);
}

#[test]
fn test_binop_eq() {
    // fn test() -> bool { 42 == 42 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_binop_ne() {
    // fn test() -> bool { 1 != 2 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_binop_lt() {
    // fn test() -> bool { 1 < 2 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_binop_bitand() {
    // fn test() -> i64 { 0b1100 & 0b1010 } = 0b1000 = 8
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 8);
}

#[test]
fn test_binop_bitor() {
    // fn test() -> i64 { 0b1100 | 0b1010 } = 0b1110 = 14
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 14);
}

#[test]
fn test_binop_shl() {
    // fn test() -> i64 { 1 << 4 } = 16
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 16);
}

// =========================================================================
// BinOpChecked tests
// =========================================================================

#[test]
fn test_binop_checked_no_overflow() {
    // fn test() -> (i64, bool) { checked_add(10, 20) }
    // Returns (30, false) - no overflow.
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 30);
}

#[test]
fn test_binop_checked_overflow() {
    // fn test() -> bool { let (_, overflow) = checked_add(i64::MAX, 1); overflow }
    // Returns true - overflow occurred.
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

// =========================================================================
// UnaryOp tests
// =========================================================================

#[test]
fn test_unaryop_neg() {
    // fn test() -> i64 { -42 }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), -42);
}

#[test]
fn test_unaryop_not() {
    // fn test() -> bool { !true }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::Bool, IrType::Bool],
        slot_types: vec![],
    };

    assert!(!run_bool_function(&func));
}

#[test]
fn test_unaryop_bitnot() {
    // fn test() -> u32 { ~0u32 } = 0xFFFFFFFF
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::U32, IrType::U32],
        slot_types: vec![],
    };

    assert_eq!(run_u32_function(&func), 0xFFFFFFFF);
}

// =========================================================================
// SlotStore/SlotLoad tests
// =========================================================================

#[test]
fn test_slot_store_load() {
    // var x = 10; x = x + 5; ret x
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    // var x = 10
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(0)) },
                    // x + 5
                    Instruction::SlotLoad { dest: ValueId(1), slot: SlotId(0) },
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(5) },
                    Instruction::BinOp {
                        dest: ValueId(3),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(1)),
                        rhs: Operand::Value(ValueId(2)),
                    },
                    // x = result
                    Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(3)) },
                    // ret x
                    Instruction::SlotLoad { dest: ValueId(4), slot: SlotId(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        value_count: 5,
        slot_count: 1,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![IrType::I64],
    };

    assert_eq!(run_i64_function(&func), 15);
}

#[test]
fn test_slot_multiple_updates() {
    // var x = 1; x = x * 2; x = x * 2; x = x * 2; ret x
    // 1 -> 2 -> 4 -> 8
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                    Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(0)) },
                    // x = x * 2
                    Instruction::SlotLoad { dest: ValueId(1), slot: SlotId(0) },
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                    Instruction::BinOp { dest: ValueId(3), op: BinOp::Mul, lhs: Operand::Value(ValueId(1)), rhs: Operand::Value(ValueId(2)) },
                    Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(3)) },
                    // x = x * 2
                    Instruction::SlotLoad { dest: ValueId(4), slot: SlotId(0) },
                    Instruction::Const { dest: ValueId(5), value: ConstValue::I64(2) },
                    Instruction::BinOp { dest: ValueId(6), op: BinOp::Mul, lhs: Operand::Value(ValueId(4)), rhs: Operand::Value(ValueId(5)) },
                    Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(6)) },
                    // x = x * 2
                    Instruction::SlotLoad { dest: ValueId(7), slot: SlotId(0) },
                    Instruction::Const { dest: ValueId(8), value: ConstValue::I64(2) },
                    Instruction::BinOp { dest: ValueId(9), op: BinOp::Mul, lhs: Operand::Value(ValueId(7)), rhs: Operand::Value(ValueId(8)) },
                    Instruction::SlotStore { dest: SlotDest::Local(SlotId(0)), value: Operand::Value(ValueId(9)) },
                    // ret x
                    Instruction::SlotLoad { dest: ValueId(10), slot: SlotId(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(10))) },
            },
        ],
        value_count: 11,
        slot_count: 1,
        value_types: vec![IrType::I64; 11],
        slot_types: vec![IrType::I64],
    };

    assert_eq!(run_i64_function(&func), 8);
}

// =========================================================================
// Control flow tests
// =========================================================================

#[test]
fn test_branch_true() {
    // fn test() -> i64 { if true { 1 } else { 2 } }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(true) },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::Bool, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 1);
}

#[test]
fn test_branch_false() {
    // fn test() -> i64 { if false { 1 } else { 2 } }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::Bool(false) },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(2) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(2))) },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::Bool, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 2);
}

#[test]
fn test_goto_chain() {
    // block0 -> block1 -> block2 (return)
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Goto(BlockId(1)),
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(2) },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Goto(BlockId(2)),
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const { dest: ValueId(3), value: ConstValue::I64(3) },
                    Instruction::BinOp {
                        dest: ValueId(4),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(2)),
                        rhs: Operand::Value(ValueId(3)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        value_count: 5,
        slot_count: 0,
        value_types: vec![IrType::I64; 5],
        slot_types: vec![],
    };

    // 1 + 2 + 3 = 6
    assert_eq!(run_i64_function(&func), 6);
}

// =========================================================================
// Copy/Move tests
// =========================================================================

#[test]
fn test_copy() {
    // fn test() -> i64 { let a = 42; let b = a; b }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::Copy { dest: ValueId(1), src: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_move() {
    // fn test() -> i64 { let a = 42; let b = move a; b }
    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(42) },
                    Instruction::Move { dest: ValueId(1), src: Operand::Value(ValueId(0)) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(1))) },
            },
        ],
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 42);
}

// =========================================================================
// Pack/Unpack tests
// =========================================================================

#[test]
fn test_pack_tuple() {
    // fn test() -> (i64, i64) { (10, 20) }
    // Returns packed tuple, then extract first element.
    let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64]);

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Pack {
                        dest: ValueId(2),
                        ty: TypeRef::Tuple(0),
                        fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                    },
                    Instruction::TupleIndex {
                        dest: ValueId(3),
                        base: Operand::Value(ValueId(2)),
                        index: 0,
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, tuple_ty, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 10);
}

#[test]
fn test_unpack_tuple() {
    // fn test() -> i64 { let (a, b) = (10, 20); a + b }
    let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64]);

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
                    // a + b
                    Instruction::BinOp {
                        dest: ValueId(5),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(3)),
                        rhs: Operand::Value(ValueId(4)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
        ],
        value_count: 6,
        slot_count: 0,
        value_types: vec![
            IrType::I64, IrType::I64, tuple_ty,
            IrType::I64, IrType::I64, IrType::I64
        ],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 30);
}

#[test]
fn test_tuple_index_second() {
    // fn test() -> i64 { let t = (10, 20, 30); t.1 }
    let tuple_ty = IrType::Tuple(vec![IrType::I64, IrType::I64, IrType::I64]);

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(30) },
                    Instruction::Pack {
                        dest: ValueId(3),
                        ty: TypeRef::Tuple(0),
                        fields: vec![
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                            Operand::Value(ValueId(2)),
                        ],
                    },
                    Instruction::TupleIndex {
                        dest: ValueId(4),
                        base: Operand::Value(ValueId(3)),
                        index: 1,
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        value_count: 5,
        slot_count: 0,
        value_types: vec![
            IrType::I64, IrType::I64, IrType::I64, tuple_ty, IrType::I64
        ],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 20);
}

// =========================================================================
// Option tests
// =========================================================================

#[test]
fn test_wrap_some_unwrap() {
    // fn test() -> i64 { let opt = Some(42); opt.unwrap() }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_is_some() {
    // fn test() -> bool { let opt = Some(42); opt.is_some() }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_is_none() {
    // fn test() -> bool { let opt: ?i64 = none; !opt.is_some() }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 4,
        slot_count: 0,
        value_types: vec![opt_ty, IrType::I64, IrType::Bool, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_option_branch() {
    // fn test() -> i64 { if Some(42).is_some() { 1 } else { 0 } }
    let opt_ty = IrType::Option(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const { dest: ValueId(4), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const { dest: ValueId(5), value: ConstValue::I64(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
        ],
        value_count: 6,
        slot_count: 0,
        value_types: vec![IrType::I64, opt_ty, IrType::I64, IrType::Bool, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 1);
}

// =========================================================================
// Result tests
// =========================================================================

#[test]
fn test_wrap_ok_unwrap() {
    // fn test() -> i64 { let res = Ok(42); res.unwrap() }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 5,
        slot_count: 0,
        value_types: vec![IrType::I64, res_ty, IrType::I64, IrType::Error, IrType::Bool],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 42);
}

#[test]
fn test_is_ok() {
    // fn test() -> bool { let res = Ok(42); res.is_ok() }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 5,
        slot_count: 0,
        value_types: vec![IrType::I64, res_ty, IrType::I64, IrType::Error, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_is_err() {
    // fn test() -> bool { let res: !i64 = err; !res.is_ok() }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 6,
        slot_count: 0,
        value_types: vec![IrType::Unit, res_ty, IrType::I64, IrType::Error, IrType::Bool, IrType::Bool],
        slot_types: vec![],
    };

    assert!(run_bool_function(&func));
}

#[test]
fn test_result_branch() {
    // fn test() -> i64 { if Ok(42).is_ok() { 1 } else { 0 } }
    let res_ty = IrType::Result(Box::new(IrType::I64));

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const { dest: ValueId(5), value: ConstValue::I64(1) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const { dest: ValueId(6), value: ConstValue::I64(0) },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(6))) },
            },
        ],
        value_count: 7,
        slot_count: 0,
        value_types: vec![IrType::I64, res_ty, IrType::I64, IrType::Error, IrType::Bool, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 1);
}

// =========================================================================
// Struct tests
// =========================================================================

#[test]
fn test_pack_struct() {
    // fn test() -> i64 { let s = { x: 10, y: 20 }; s.x }
    let struct_ty = IrType::Struct(vec![
        ("x".to_string(), IrType::I64),
        ("y".to_string(), IrType::I64),
    ]);

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Pack {
                        dest: ValueId(2),
                        ty: TypeRef::AnonStruct(0),
                        fields: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
                    },
                    Instruction::FieldAccess {
                        dest: ValueId(3),
                        base: Operand::Value(ValueId(2)),
                        field_index: 0,
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(3))) },
            },
        ],
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, struct_ty, IrType::I64],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 10);
}

#[test]
fn test_field_access_second() {
    // fn test() -> i64 { let s = { a: 10, b: 20, c: 30 }; s.b }
    let struct_ty = IrType::Struct(vec![
        ("a".to_string(), IrType::I64),
        ("b".to_string(), IrType::I64),
        ("c".to_string(), IrType::I64),
    ]);

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const { dest: ValueId(0), value: ConstValue::I64(10) },
                    Instruction::Const { dest: ValueId(1), value: ConstValue::I64(20) },
                    Instruction::Const { dest: ValueId(2), value: ConstValue::I64(30) },
                    Instruction::Pack {
                        dest: ValueId(3),
                        ty: TypeRef::AnonStruct(0),
                        fields: vec![
                            Operand::Value(ValueId(0)),
                            Operand::Value(ValueId(1)),
                            Operand::Value(ValueId(2)),
                        ],
                    },
                    Instruction::FieldAccess {
                        dest: ValueId(4),
                        base: Operand::Value(ValueId(3)),
                        field_index: 1,
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(4))) },
            },
        ],
        value_count: 5,
        slot_count: 0,
        value_types: vec![
            IrType::I64, IrType::I64, IrType::I64, struct_ty, IrType::I64
        ],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 20);
}

#[test]
fn test_unpack_struct() {
    // fn test() -> i64 { let { x, y } = { x: 10, y: 20 }; x + y }
    let struct_ty = IrType::Struct(vec![
        ("x".to_string(), IrType::I64),
        ("y".to_string(), IrType::I64),
    ]);

    let func = IrFunction {
        id: FuncId(0),
        name: "test".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
                    // x + y
                    Instruction::BinOp {
                        dest: ValueId(5),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(3)),
                        rhs: Operand::Value(ValueId(4)),
                    },
                ],
                terminator: Terminator::Return { value: Some(Operand::Value(ValueId(5))) },
            },
        ],
        value_count: 6,
        slot_count: 0,
        value_types: vec![
            IrType::I64, IrType::I64, struct_ty,
            IrType::I64, IrType::I64, IrType::I64
        ],
        slot_types: vec![],
    };

    assert_eq!(run_i64_function(&func), 30);
}

// ==========================================================================
// Cross-unit reference tests
// ==========================================================================

#[test]
fn test_crossunit_external_value() {
    // Unit 0: let x = 42
    let unit0 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(42),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![("x".to_string(), datalove_datafun_ir::ExportBinding::Value(ValueId(0)))],
    };

    // Unit 1: return x (from unit 0)
    let unit1 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: Some(ValueId(0)),
        exports: vec![],
    };

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
    let unit0 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(10),
                    },
                    Instruction::SlotStore {
                        dest: SlotDest::Local(SlotId(0)),
                        value: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        value_count: 1,
        slot_count: 1,
        value_types: vec![IrType::I64],
        slot_types: vec![IrType::I64],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![("y".to_string(), datalove_datafun_ir::ExportBinding::Slot(SlotId(0)))],
    };

    // Unit 1: return y (from unit 0's slot)
    let unit1 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
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
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: Some(ValueId(0)),
        exports: vec![],
    };

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
    // Unit 0: fn double(x: i64) -> i64 { x + x }
    let double_fn = IrFunction {
        id: FuncId(0),
        name: "double".to_string(),
        params: vec![ValueId(0)],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::BinOp {
                        dest: ValueId(1),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            },
        ],
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    let unit0 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        value_count: 0,
        slot_count: 0,
        value_types: vec![],
        slot_types: vec![],
        functions: vec![double_fn],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![("double".to_string(), datalove_datafun_ir::ExportBinding::Function(FuncId(0)))],
    };

    // Unit 1: return double(7)
    let unit1 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(7),
                    },
                    Instruction::Call {
                        dest: ValueId(1),
                        func: FuncRef::External { unit: 0, func: FuncId(0) },
                        args: vec![Operand::Value(ValueId(0))],
                    },
                ],
                terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(1))) },
            },
        ],
        value_count: 2,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: Some(ValueId(1)),
        exports: vec![],
    };

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

    // Execute unit 1 (returns double(7)).
    let mut result: i64 = 0;
    let expr_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let expr_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: expr_tydesc,
    };
    let completion = interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest, Some(expr_dest)).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    assert_eq!(result, 14);  // 7 + 7 = 14
}

#[test]
fn test_crossunit_chain() {
    // Unit 0: let a = 5
    let unit0 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I64(5),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![("a".to_string(), datalove_datafun_ir::ExportBinding::Value(ValueId(0)))],
    };

    // Unit 1: let b = a + 3
    let unit1 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    // Load a from unit 0.
                    Instruction::Copy {
                        dest: ValueId(0),
                        src: Operand::ExternalValue { unit: 0, value: ValueId(0) },
                    },
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::I64(3),
                    },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Add,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::UnitEnd { result: None },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: None,
        exports: vec![("b".to_string(), datalove_datafun_ir::ExportBinding::Value(ValueId(2)))],
    };

    // Unit 2: return b
    let unit2 = IrScriptUnit {
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    // Load b from unit 1.
                    Instruction::Copy {
                        dest: ValueId(0),
                        src: Operand::ExternalValue { unit: 1, value: ValueId(2) },
                    },
                ],
                terminator: Terminator::UnitEnd { result: Some(Operand::Value(ValueId(0))) },
            },
        ],
        value_count: 1,
        slot_count: 0,
        value_types: vec![IrType::I64],
        slot_types: vec![],
        functions: vec![],
        symbols: datalove_datafun_ir::SymbolTable::new(),
        result: Some(ValueId(0)),
        exports: vec![],
    };

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
    let completion = interp.execute_script_unit_in_env(&unit0, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);
    let completion = interp.execute_script_unit_in_env(&unit1, &mut env, ret_dest, None).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    // Execute unit 2 (returns a + b).
    let mut result: i64 = 0;
    let expr_tydesc = interp.tydesc_table.get_or_create(&IrType::I64);
    let expr_dest = Destination {
        ptr: &mut result as *mut i64 as *mut u8,
        tydesc: expr_tydesc,
    };
    let completion = interp.execute_script_unit_in_env(&unit2, &mut env, ret_dest, Some(expr_dest)).unwrap();
    assert_eq!(completion, super::UnitCompletion::Normal);

    assert_eq!(result, 8);  // 5 + 3 = 8
}

// =========================================================================
// Phi node tests
// =========================================================================

#[test]
fn test_phi_true_branch() {
    // Test: if true { 10 } else { 20 }
    // Expected: 10
    //
    // block0:
    //     v0 = const true
    //     branch v0, block1, block2
    // block1:
    //     v1 = const 10
    //     goto block3
    // block2:
    //     v2 = const 20
    //     goto block3
    // block3:
    //     v3 = phi [(block1, v1), (block2, v2)]
    //     return v3
    let func = IrFunction {
        id: FuncId(0),
        name: "test_phi_true".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::Bool(true),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::I64(10),
                    },
                ],
                terminator: Terminator::Goto(BlockId(3)),
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(2),
                        value: ConstValue::I64(20),
                    },
                ],
                terminator: Terminator::Goto(BlockId(3)),
            },
            IrBlock {
                id: BlockId(3),
                instructions: vec![
                    Instruction::Phi {
                        dest: ValueId(3),
                        incoming: vec![
                            (BlockId(1), Operand::Value(ValueId(1))),
                            (BlockId(2), Operand::Value(ValueId(2))),
                        ],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(3))),
                },
            },
        ],
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::Bool, IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    let result = run_i64_function(&func);
    assert_eq!(result, 10);
}

#[test]
fn test_phi_false_branch() {
    // Test: if false { 10 } else { 20 }
    // Expected: 20
    let func = IrFunction {
        id: FuncId(0),
        name: "test_phi_false".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::Bool(false),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::I64(10),
                    },
                ],
                terminator: Terminator::Goto(BlockId(3)),
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(2),
                        value: ConstValue::I64(20),
                    },
                ],
                terminator: Terminator::Goto(BlockId(3)),
            },
            IrBlock {
                id: BlockId(3),
                instructions: vec![
                    Instruction::Phi {
                        dest: ValueId(3),
                        incoming: vec![
                            (BlockId(1), Operand::Value(ValueId(1))),
                            (BlockId(2), Operand::Value(ValueId(2))),
                        ],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(3))),
                },
            },
        ],
        value_count: 4,
        slot_count: 0,
        value_types: vec![IrType::Bool, IrType::I64, IrType::I64, IrType::I64],
        slot_types: vec![],
    };

    let result = run_i64_function(&func);
    assert_eq!(result, 20);
}

#[test]
fn test_phi_nested_if() {
    // Test: if true { if false { 1 } else { 2 } } else { 3 }
    // Expected: 2
    //
    // block0:
    //     v0 = const true
    //     branch v0, block1, block4
    // block1:
    //     v1 = const false
    //     branch v1, block2, block3
    // block2:
    //     v2 = const 1
    //     goto block5
    // block3:
    //     v3 = const 2
    //     goto block5
    // block4:
    //     v4 = const 3
    //     goto block6
    // block5:
    //     v5 = phi [(block2, v2), (block3, v3)]
    //     goto block6
    // block6:
    //     v6 = phi [(block5, v5), (block4, v4)]
    //     return v6
    let func = IrFunction {
        id: FuncId(0),
        name: "test_phi_nested".to_string(),
        params: vec![],
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::Bool(true),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1),
                    else_block: BlockId(4),
                },
            },
            IrBlock {
                id: BlockId(1),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::Bool(false),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(1)),
                    then_block: BlockId(2),
                    else_block: BlockId(3),
                },
            },
            IrBlock {
                id: BlockId(2),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(2),
                        value: ConstValue::I64(1),
                    },
                ],
                terminator: Terminator::Goto(BlockId(5)),
            },
            IrBlock {
                id: BlockId(3),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(3),
                        value: ConstValue::I64(2),
                    },
                ],
                terminator: Terminator::Goto(BlockId(5)),
            },
            IrBlock {
                id: BlockId(4),
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(4),
                        value: ConstValue::I64(3),
                    },
                ],
                terminator: Terminator::Goto(BlockId(6)),
            },
            IrBlock {
                id: BlockId(5),
                instructions: vec![
                    Instruction::Phi {
                        dest: ValueId(5),
                        incoming: vec![
                            (BlockId(2), Operand::Value(ValueId(2))),
                            (BlockId(3), Operand::Value(ValueId(3))),
                        ],
                    },
                ],
                terminator: Terminator::Goto(BlockId(6)),
            },
            IrBlock {
                id: BlockId(6),
                instructions: vec![
                    Instruction::Phi {
                        dest: ValueId(6),
                        incoming: vec![
                            (BlockId(5), Operand::Value(ValueId(5))),
                            (BlockId(4), Operand::Value(ValueId(4))),
                        ],
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(6))),
                },
            },
        ],
        value_count: 7,
        slot_count: 0,
        value_types: vec![
            IrType::Bool, IrType::Bool,
            IrType::I64, IrType::I64, IrType::I64, IrType::I64, IrType::I64,
        ],
        slot_types: vec![],
    };

    let result = run_i64_function(&func);
    assert_eq!(result, 2);
}
