use super::*;
use cranelift_object::{ObjectBuilder, ObjectModule};
use datalove_datafun_ir::{BinOp, IrBlock, IrCodeUnit, CodeUnitId, CodeUnitContext, FunctionContext, ConstValue, Terminator, SymbolTable};

fn create_test_isa() -> std::sync::Arc<dyn TargetIsa> {
    use cranelift_codegen::isa;
    use cranelift_codegen::settings::{self, Configurable};
    use target_lexicon::Triple;

    let mut settings_builder = settings::builder();
    settings_builder.set("opt_level", "speed").unwrap();
    let flags = settings::Flags::new(settings_builder);

    isa::lookup(Triple::host())
        .unwrap()
        .finish(flags)
        .unwrap()
}

fn create_test_module(isa: std::sync::Arc<dyn TargetIsa>) -> ObjectModule {
    let obj_builder = ObjectBuilder::new(
        isa,
        "test",
        cranelift_module::default_libcall_names(),
    ).unwrap();
    ObjectModule::new(obj_builder)
}

/// Helper to create a function code unit for tests.
fn make_func_unit(
    name: &str,
    return_type: IrType,
    blocks: Vec<IrBlock>,
    value_types: Vec<IrType>,
) -> IrCodeUnit {
    IrCodeUnit {
        id: CodeUnitId(0),
        name: name.into(),
        blocks,
        value_count: value_types.len() as u32,
        slot_count: 0,
        value_types,
        slot_types: vec![],
        tracked_slots: vec![],
        const_values: vec![],
        symbols: SymbolTable::default(),
        context: CodeUnitContext::Function(FunctionContext {
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            return_type,
            tracked_params: vec![],
        type_param_count: 0,
        descriptor_table: Vec::new(),
        inline: false,
        }),
        nested_units: vec![],
    }
}

#[test]
fn test_compile_const_i32() {
    let isa = create_test_isa();
    let mut module = create_test_module(isa.clone());

    // Create a function: fn foo() -> i32 { 42 }
    let code_unit = make_func_unit(
        "test_const",
        IrType::I32,
        vec![
            IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I32(42),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(0))),
                },
            },
        ],
        vec![IrType::I32],
    );

    let mut tydescs = TyDescEmitter::new();
    let runtime = RuntimeImports::declare(&mut module, isa.default_call_conv()).expect("runtime imports");
    let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, runtime, &mut tydescs, None);
    let result = compiler.compile();
    assert!(result.is_ok(), "compile failed: {:?}", result.err());
}

// test_compile_binop_add removed: i32 + i32 -> i32 is invalid IR.
// Fixed-width integer arithmetic uses widening (-> Int) or checked ops (BinOpChecked).

#[test]
fn test_compile_comparison() {
    let isa = create_test_isa();
    let mut module = create_test_module(isa.clone());

    // Create a function: fn foo() -> bool { 10 < 32 }
    let code_unit = make_func_unit(
        "test_cmp",
        IrType::Bool,
        vec![
            IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I32(10),
                    },
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::I32(32),
                    },
                    Instruction::BinOp {
                        dest: ValueId(2),
                        op: BinOp::Lt,
                        lhs: Operand::Value(ValueId(0)),
                        rhs: Operand::Value(ValueId(1)),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(2))),
                },
            },
        ],
        vec![IrType::I32, IrType::I32, IrType::Bool],
    );

    let mut tydescs = TyDescEmitter::new();
    let runtime = RuntimeImports::declare(&mut module, isa.default_call_conv()).expect("runtime imports");
    let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, runtime, &mut tydescs, None);
    let result = compiler.compile();
    assert!(result.is_ok(), "compile failed: {:?}", result.err());
}

#[test]
fn test_compile_unary_neg() {
    let isa = create_test_isa();
    let mut module = create_test_module(isa.clone());

    // Create a function: fn foo() -> i32 { -42 }
    let code_unit = make_func_unit(
        "test_neg",
        IrType::I32,
        vec![
            IrBlock { id: BlockId(0), params: vec![], instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::I32(42),
                    },
                    Instruction::UnaryOp {
                        dest: ValueId(1),
                        op: datalove_datafun_ir::UnaryOp::Neg,
                        operand: Operand::Value(ValueId(0)),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            },
        ],
        vec![IrType::I32, IrType::I32],
    );

    let mut tydescs = TyDescEmitter::new();
    let runtime = RuntimeImports::declare(&mut module, isa.default_call_conv()).expect("runtime imports");
    let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, runtime, &mut tydescs, None);
    let result = compiler.compile();
    assert!(result.is_ok(), "compile failed: {:?}", result.err());
}

#[test]
fn test_compile_branch() {
    let isa = create_test_isa();
    let mut module = create_test_module(isa.clone());

    // Create a function with a branch:
    // fn foo() -> i32 {
    //     if true { 1 } else { 2 }
    // }
    let code_unit = IrCodeUnit {
        id: CodeUnitId(0),
        name: "test_branch".into(),
        blocks: vec![
            IrBlock {
                id: BlockId(0),
                params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(0),
                        value: ConstValue::Bool(true),
                    },
                ],
                terminator: Terminator::Branch {
                    cond: Operand::Value(ValueId(0)),
                    then_block: BlockId(1),
                    then_args: vec![],
                    else_block: BlockId(2),
                    else_args: vec![],
                },
            },
            IrBlock {
                id: BlockId(1),
                params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(1),
                        value: ConstValue::I32(1),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(1))),
                },
            },
            IrBlock {
                id: BlockId(2),
                params: vec![],
                instructions: vec![
                    Instruction::Const {
                        dest: ValueId(2),
                        value: ConstValue::I32(2),
                    },
                ],
                terminator: Terminator::Return {
                    value: Some(Operand::Value(ValueId(2))),
                },
            },
        ],
        value_count: 3,
        slot_count: 0,
        value_types: vec![IrType::Bool, IrType::I32, IrType::I32],
        slot_types: vec![],
        tracked_slots: vec![],
        const_values: vec![],
        symbols: SymbolTable::default(),
        context: CodeUnitContext::Function(FunctionContext {
            params: vec![],
            param_modes: vec![],
            param_types: vec![],
            return_type: IrType::I32,
            tracked_params: vec![],
        type_param_count: 0,
        descriptor_table: Vec::new(),
        inline: false,
        }),
        nested_units: vec![],
    };

    let mut tydescs = TyDescEmitter::new();
    let runtime = RuntimeImports::declare(&mut module, isa.default_call_conv()).expect("runtime imports");
    let compiler = FunctionCompiler::new(&code_unit, isa.as_ref(), &mut module, runtime, &mut tydescs, None);
    let result = compiler.compile();
    assert!(result.is_ok(), "compile failed: {:?}", result.err());
}
