//! Lower AST to SSA IR.
//!
//! This module transforms typechecked AST into flat SSA IR suitable for
//! interpretation and codegen.

use rmx::prelude::*;
use std::collections::HashMap;
use salsa::plumbing::AsId;
use crate::ast::{self, Statement, ExprFun, ExprFunKind};
use crate::tycheck::TypecheckResult;
use crate::Db;
use super::*;

/// Parse an integer literal into a ConstValue based on the target type.
fn parse_int_const(text: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::U8 => text.parse::<u8>().map(ConstValue::U8).map_err(|_| ()),
        IrType::U16 => text.parse::<u16>().map(ConstValue::U16).map_err(|_| ()),
        IrType::U32 => text.parse::<u32>().map(ConstValue::U32).map_err(|_| ()),
        IrType::U64 => text.parse::<u64>().map(ConstValue::U64).map_err(|_| ()),
        IrType::I8 => text.parse::<i8>().map(ConstValue::I8).map_err(|_| ()),
        IrType::I16 => text.parse::<i16>().map(ConstValue::I16).map_err(|_| ()),
        IrType::I32 => text.parse::<i32>().map(ConstValue::I32).map_err(|_| ()),
        IrType::I64 => text.parse::<i64>().map(ConstValue::I64).map_err(|_| ()),
        IrType::Int => {
            // For now, parse as i64. TODO: Support bigint.
            text.parse::<i64>().map(ConstValue::I64).map_err(|_| ())
        }
        _ => Err(()),
    }
}

/// Parse a hex literal into a ConstValue based on the target type.
fn parse_hex_const(hex_str: &str, ty: &IrType) -> Result<ConstValue, ()> {
    match ty {
        IrType::U8 => u8::from_str_radix(hex_str, 16).map(ConstValue::U8).map_err(|_| ()),
        IrType::U16 => u16::from_str_radix(hex_str, 16).map(ConstValue::U16).map_err(|_| ()),
        IrType::U32 => u32::from_str_radix(hex_str, 16).map(ConstValue::U32).map_err(|_| ()),
        IrType::U64 => u64::from_str_radix(hex_str, 16).map(ConstValue::U64).map_err(|_| ()),
        IrType::I8 => i8::from_str_radix(hex_str, 16).map(ConstValue::I8).map_err(|_| ()),
        IrType::I16 => i16::from_str_radix(hex_str, 16).map(ConstValue::I16).map_err(|_| ()),
        IrType::I32 => i32::from_str_radix(hex_str, 16).map(ConstValue::I32).map_err(|_| ()),
        IrType::I64 => i64::from_str_radix(hex_str, 16).map(ConstValue::I64).map_err(|_| ()),
        IrType::Int => {
            // For now, parse as i64. TODO: Support bigint.
            i64::from_str_radix(hex_str, 16).map(ConstValue::I64).map_err(|_| ())
        }
        _ => Err(()),
    }
}

/// Context for lowering script units.
///
/// Tracks bindings available from previous units.
#[derive(Clone, Debug, Default)]
pub struct ScriptLowerContext {
    /// Available let bindings: name -> (unit_index, value_id).
    pub values: HashMap<String, (u32, ValueId)>,
    /// Available var bindings: name -> (unit_index, slot_id).
    pub slots: HashMap<String, (u32, SlotId)>,
    /// Available functions: name -> (unit_index, func_id).
    pub functions: HashMap<String, (u32, FuncId)>,
    /// Current unit index.
    pub current_unit: u32,
}

impl ScriptLowerContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add exports from a unit to the context.
    pub fn add_exports(&mut self, unit_index: u32, exports: &[(String, ExportBinding)]) {
        for (name, binding) in exports {
            match binding {
                ExportBinding::Value(v) => {
                    self.values.insert(name.clone(), (unit_index, *v));
                }
                ExportBinding::Slot(s) => {
                    self.slots.insert(name.clone(), (unit_index, *s));
                }
                ExportBinding::Function(func_id) => {
                    self.functions.insert(name.clone(), (unit_index, *func_id));
                }
            }
        }
    }
}

/// What kind of script unit we're lowering.
pub enum ScriptUnitKind<'db> {
    /// A sequence of statements.
    Fragment(Vec<Statement<'db>>),
    /// A single expression.
    Expr(ExprFun<'db>),
}

/// Context for lowering a single function or script unit.
pub struct LowerCtx<'db> {
    db: &'db dyn Db,
    /// Expression types from typechecker.
    expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
    /// Next ValueId to allocate.
    next_value: u32,
    /// Next SlotId to allocate.
    next_slot: u32,
    /// Next BlockId to allocate.
    next_block: u32,
    /// Blocks being built.
    blocks: Vec<IrBlock>,
    /// Current block being built.
    current_block: BlockId,
    /// Instructions for current block.
    current_instructions: Vec<Instruction>,
    /// Mapping from variable names to their operands.
    variables: HashMap<String, Operand>,
    /// Script context for external lookups (None for functions).
    script_ctx: Option<ScriptLowerContext>,
    /// Exports from this unit (only used for script units).
    exports: Vec<(String, ExportBinding)>,
    /// Functions defined in this script unit.
    functions: Vec<IrFunction>,
    /// Symbol table for function resolution.
    symbols: SymbolTable,
    /// Available functions: name -> FuncRef (for resolving calls).
    func_scope: HashMap<String, FuncRef>,
    /// Current unit index (for script units).
    current_unit: u32,
    /// Type for each ValueId.
    value_types: Vec<IrType>,
    /// Type for each SlotId.
    slot_types: Vec<IrType>,
}

impl<'db> LowerCtx<'db> {
    pub fn new(db: &'db dyn Db, expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>]) -> Self {
        Self {
            db,
            expr_types,
            next_value: 0,
            next_slot: 0,
            next_block: 1, // Block 0 is entry
            blocks: Vec::new(),
            current_block: BlockId(0),
            current_instructions: Vec::new(),
            variables: HashMap::new(),
            script_ctx: None,
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope: HashMap::new(),
            current_unit: 0,
            value_types: Vec::new(),
            slot_types: Vec::new(),
        }
    }

    /// Get the IrType for an expression from the typechecker.
    fn expr_type(&self, expr: ExprFun<'db>) -> IrType {
        let expr_id = expr.as_id();
        let index = expr_id.index() as usize;
        match self.expr_types.get(index).copied().flatten() {
            Some(ty) => IrType::from_tycheck(self.db, &ty),
            None => panic!(
                "Expression must have type from typechecker. Expression ID {} but expr_types.len() = {}",
                index, self.expr_types.len()
            ),
        }
    }

    /// Create a context for lowering a script unit.
    pub fn new_for_script(
        db: &'db dyn Db,
        expr_types: &'db [Option<crate::tycheck::TypeAndHeap<'db>>],
        script_ctx: ScriptLowerContext,
    ) -> Self {
        // Seed variables with external bindings from previous units.
        let mut variables = HashMap::new();
        for (name, (unit, value)) in &script_ctx.values {
            variables.insert(name.clone(), Operand::ExternalValue {
                unit: *unit,
                value: *value,
            });
        }
        for (name, (unit, slot)) in &script_ctx.slots {
            variables.insert(name.clone(), Operand::ExternalSlot {
                unit: *unit,
                slot: *slot,
            });
        }

        // Seed function scope with external functions.
        let mut func_scope = HashMap::new();
        for (name, (unit, func_id)) in &script_ctx.functions {
            func_scope.insert(name.clone(), FuncRef::External {
                unit: *unit,
                func: *func_id,
            });
        }

        let current_unit = script_ctx.current_unit;

        Self {
            db,
            expr_types,
            next_value: 0,
            next_slot: 0,
            next_block: 1,
            blocks: Vec::new(),
            current_block: BlockId(0),
            current_instructions: Vec::new(),
            variables,
            script_ctx: Some(script_ctx),
            exports: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            func_scope,
            current_unit,
            value_types: Vec::new(),
            slot_types: Vec::new(),
        }
    }

    /// Define a function in the current scope.
    fn define_func(&mut self, name: &str, param_count: usize) -> FuncId {
        let func_id = self.symbols.define_func(name.to_string(), param_count);
        self.func_scope.insert(name.to_string(), FuncRef::Local(func_id));
        func_id
    }

    /// Look up a function by name.
    fn lookup_func(&self, name: &str) -> Option<FuncRef> {
        self.func_scope.get(name).copied()
    }

    /// Allocate a fresh SSA value with known type.
    fn fresh_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.push(ty);
        id
    }

    /// Allocate a fresh mutable slot with known type.
    fn fresh_slot(&mut self, ty: IrType) -> SlotId {
        let id = SlotId(self.next_slot);
        self.next_slot += 1;
        self.slot_types.push(ty);
        id
    }

    /// Allocate a fresh block.
    fn fresh_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        id
    }

    /// Emit an instruction to the current block.
    fn emit(&mut self, instr: Instruction) {
        self.current_instructions.push(instr);
    }

    /// Finish current block with a terminator, start a new block.
    fn finish_block(&mut self, terminator: Terminator) -> BlockId {
        let block = IrBlock {
            id: self.current_block,
            instructions: std::mem::take(&mut self.current_instructions),
            terminator,
        };
        self.blocks.push(block);
        self.current_block
    }

    /// Start building a new block.
    fn start_block(&mut self, id: BlockId) {
        self.current_block = id;
        self.current_instructions.clear();
    }

    /// Bind a variable name to an operand.
    fn bind_var(&mut self, name: &str, operand: Operand) {
        self.variables.insert(name.to_string(), operand);
    }

    /// Look up a variable.
    fn lookup_var(&self, name: &str) -> Option<Operand> {
        self.variables.get(name).copied()
    }
}

/// Lower a function to IR.
///
/// For standalone function lowering (not in a script context).
pub fn lower_function<'db>(
    db: &'db dyn Db,
    tycheck_result: TypecheckResult<'db>,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let mut ctx = LowerCtx::new(db, tycheck_result.expr_types(db));
    let name = func.name(db).text(db).to_string();
    let param_count = func.params(db).len();

    // Define the function in the symbol table.
    let func_id = ctx.define_func(&name, param_count);

    lower_function_body(&mut ctx, func_id, func)
}

/// Lower a function body given an already-allocated FuncId.
fn lower_function_body<'db>(
    ctx: &mut LowerCtx<'db>,
    func_id: FuncId,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let name = func.name(ctx.db).text(ctx.db).to_string();

    // Allocate ValueIds for parameters.
    // TODO: Get parameter types from TypecheckResult instead of re-converting.
    // Currently we use Unit as a placeholder since convert_type_hint creates
    // tracked structs that can't be called outside a tracked function.
    let params: Vec<ValueId> = func.params(ctx.db)
        .iter()
        .map(|p| {
            let param_name = p.name(ctx.db).text(ctx.db).to_string();
            let id = ctx.fresh_value(IrType::Unit);  // TODO: Get actual param type
            ctx.bind_var(&param_name, Operand::Value(id));
            id
        })
        .collect();

    // Lower the function body.
    for stmt in func.body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }

    // If no explicit return, add implicit return unit.
    if ctx.current_instructions.is_empty()
        || !matches!(ctx.blocks.last().map(|b| &b.terminator), Some(Terminator::Return { .. }))
    {
        // Check if we already have a return as the last instruction.
        let needs_return = ctx.blocks.is_empty()
            || !matches!(ctx.blocks.last().unwrap().terminator, Terminator::Return { .. });
        if needs_return && ctx.current_instructions.len() > 0 {
            ctx.finish_block(Terminator::Return { value: None });
        } else if needs_return {
            ctx.finish_block(Terminator::Return { value: None });
        }
    }

    Ok(IrFunction {
        id: func_id,
        name,
        params,
        blocks: std::mem::take(&mut ctx.blocks),
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
    })
}

/// Lower a statement.
fn lower_statement<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, let_stmt.value(ctx.db))?;
            ctx.bind_var(&name, Operand::Value(value_id));
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name(ctx.db).text(ctx.db).to_string();
            // Get type from the initialization expression.
            let init_expr = var_stmt.value(ctx.db);
            let slot_type = ctx.expr_type(init_expr);
            let slot = ctx.fresh_slot(slot_type);
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.emit(Instruction::SlotStore {
                dest: SlotDest::Local(slot),
                value: Operand::Value(value_id),
            });
            ctx.bind_var(&name, Operand::Slot(slot));
            Ok(())
        }
        Statement::Set(set_stmt) => {
            let name = set_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, set_stmt.value(ctx.db))?;
            if let Some(Operand::Slot(slot)) = ctx.lookup_var(&name) {
                ctx.emit(Instruction::SlotStore {
                    dest: SlotDest::Local(slot),
                    value: Operand::Value(value_id),
                });
                Ok(())
            } else {
                Err(LowerError::VariableNotMutable(name))
            }
        }
        Statement::Ret(ret_stmt) => {
            let value = if let Some(expr) = ret_stmt.value(ctx.db) {
                Some(Operand::Value(lower_expression(ctx, expr)?))
            } else {
                None
            };
            ctx.finish_block(Terminator::Return { value });
            // Start a new unreachable block (code after return).
            let new_block = ctx.fresh_block();
            ctx.start_block(new_block);
            Ok(())
        }
        Statement::If(if_stmt) => {
            lower_if(ctx, *if_stmt)
        }
        Statement::Loop(loop_stmt) => {
            lower_loop(ctx, *loop_stmt)
        }
        Statement::Break(_) => {
            // TODO: Need loop context to know where to break to.
            Err(LowerError::NotImplemented("break".to_string()))
        }
        Statement::Continue(_) => {
            // TODO: Need loop context to know where to continue to.
            Err(LowerError::NotImplemented("continue".to_string()))
        }
        Statement::Fun(_) => {
            // Nested functions not supported in IR yet.
            Err(LowerError::NotImplemented("nested functions".to_string()))
        }
        Statement::Require(_) | Statement::Import(_) => {
            // These are module-level, not in function bodies.
            Ok(())
        }
        Statement::ParseError(_) => {
            Err(LowerError::ParseError)
        }
    }
}

/// Lower an if statement.
fn lower_if<'db>(
    ctx: &mut LowerCtx<'db>,
    if_stmt: ast::StmtIf<'db>,
) -> Result<(), LowerError> {
    let cond_id = lower_expression(ctx, if_stmt.condition(ctx.db))?;

    let then_block = ctx.fresh_block();
    let else_block = ctx.fresh_block();
    let merge_block = ctx.fresh_block();

    // Finish current block with branch.
    ctx.finish_block(Terminator::Branch {
        cond: Operand::Value(cond_id),
        then_block,
        else_block,
    });

    // Lower then branch.
    ctx.start_block(then_block);
    for stmt in if_stmt.then_body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }
    ctx.finish_block(Terminator::Goto(merge_block));

    // Lower else branch.
    ctx.start_block(else_block);
    if let Some(else_body) = if_stmt.else_body(ctx.db) {
        for stmt in else_body {
            lower_statement(ctx, stmt)?;
        }
    }
    ctx.finish_block(Terminator::Goto(merge_block));

    // Continue in merge block.
    ctx.start_block(merge_block);
    Ok(())
}

/// Lower a loop statement.
fn lower_loop<'db>(
    ctx: &mut LowerCtx<'db>,
    loop_stmt: ast::StmtLoop<'db>,
) -> Result<(), LowerError> {
    let loop_header = ctx.fresh_block();
    let loop_exit = ctx.fresh_block();

    // Jump to loop header.
    ctx.finish_block(Terminator::Goto(loop_header));

    // Lower loop body.
    ctx.start_block(loop_header);
    for stmt in loop_stmt.body(ctx.db) {
        lower_statement(ctx, stmt)?;
    }
    // Loop back to header.
    ctx.finish_block(Terminator::Goto(loop_header));

    // Continue after loop (unreachable unless break).
    ctx.start_block(loop_exit);
    Ok(())
}

/// Lower an expression, returning the ValueId holding the result.
fn lower_expression<'db>(
    ctx: &mut LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ValueId, LowerError> {
    match expr.expr(ctx.db) {
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            if let Some(operand) = ctx.lookup_var(name_str) {
                match operand {
                    Operand::Value(v) => {
                        // For SSA values, just return the existing ValueId.
                        Ok(v)
                    }
                    Operand::Slot(s) => {
                        // For slots, emit a load.
                        let slot_type = ctx.expr_type(expr);
                        let dest = ctx.fresh_value(slot_type);
                        ctx.emit(Instruction::SlotLoad { dest, slot: s });
                        Ok(dest)
                    }
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                        // External operands from previous script units.
                        // Copy into a local value.
                        let ext_type = ctx.expr_type(expr);
                        let dest = ctx.fresh_value(ext_type);
                        ctx.emit(Instruction::Copy { dest, src: operand });
                        Ok(dest)
                    }
                }
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
        }
        ExprFunKind::True(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(true),
            });
            Ok(dest)
        }
        ExprFunKind::False(_) => {
            let dest = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(false),
            });
            Ok(dest)
        }
        ExprFunKind::None(_) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapNone { dest });
            Ok(dest)
        }
        ExprFunKind::Int(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value(ctx.db).text(ctx.db);
            let const_value = parse_int_const(text, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::Hex(lit) => {
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type.clone());
            let text = lit.value(ctx.db).text(ctx.db);
            let hex_str = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
            let const_value = parse_hex_const(hex_str, &result_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const { dest, value: const_value });
            Ok(dest)
        }
        ExprFunKind::BinOp(binop) => {
            let lhs_id = lower_expression(ctx, binop.lhs(ctx.db))?;
            let rhs_id = lower_expression(ctx, binop.rhs(ctx.db))?;

            let ast_op = binop.op(ctx.db);
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            let op = match ast_op {
                ast::BinOp::Add => BinOp::Add,
                ast::BinOp::Sub => BinOp::Sub,
                ast::BinOp::Mul => BinOp::Mul,
                ast::BinOp::Div => BinOp::Div,
                ast::BinOp::Eq => BinOp::Eq,
                ast::BinOp::Ne => BinOp::Ne,
                ast::BinOp::Lt => BinOp::Lt,
                ast::BinOp::Le => BinOp::Le,
                ast::BinOp::Gt => BinOp::Gt,
                ast::BinOp::Ge => BinOp::Ge,
                // Checked/optional ops - emit as checked for now.
                ast::BinOp::AddChecked | ast::BinOp::AddOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Add,
                        lhs: Operand::Value(lhs_id),
                        rhs: Operand::Value(rhs_id),
                    });
                    return Ok(dest);
                }
                ast::BinOp::SubChecked | ast::BinOp::SubOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Sub,
                        lhs: Operand::Value(lhs_id),
                        rhs: Operand::Value(rhs_id),
                    });
                    return Ok(dest);
                }
                ast::BinOp::MulChecked | ast::BinOp::MulOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Mul,
                        lhs: Operand::Value(lhs_id),
                        rhs: Operand::Value(rhs_id),
                    });
                    return Ok(dest);
                }
                ast::BinOp::DivChecked | ast::BinOp::DivOptional => {
                    let overflow = ctx.fresh_value(IrType::Bool);
                    ctx.emit(Instruction::BinOpChecked {
                        dest,
                        overflow,
                        op: BinOp::Div,
                        lhs: Operand::Value(lhs_id),
                        rhs: Operand::Value(rhs_id),
                    });
                    return Ok(dest);
                }
            };

            ctx.emit(Instruction::BinOp {
                dest,
                op,
                lhs: Operand::Value(lhs_id),
                rhs: Operand::Value(rhs_id),
            });
            Ok(dest)
        }
        ExprFunKind::UnaryOp(unary) => {
            let operand_id = lower_expression(ctx, unary.operand(ctx.db))?;

            let op = match unary.op(ctx.db) {
                ast::UnaryOp::Neg => UnaryOp::Neg,
                ast::UnaryOp::NegOptional | ast::UnaryOp::NegResult => {
                    // TODO: Handle checked unary ops.
                    UnaryOp::Neg
                }
            };

            // TODO: Get actual operand type. For now assume I64 for arithmetic.
            let dest = ctx.fresh_value(IrType::I64);
            ctx.emit(Instruction::UnaryOp {
                dest,
                op,
                operand: Operand::Value(operand_id),
            });
            Ok(dest)
        }
        ExprFunKind::FunctionCall(call) => {
            let func_name = call.name(ctx.db).text(ctx.db).to_string();
            let args: Result<Vec<_>, _> = call.args(ctx.db)
                .iter()
                .map(|arg| lower_expression(ctx, *arg).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);

            // Resolve function reference.
            let func_ref = ctx.lookup_func(&func_name)
                .ok_or_else(|| LowerError::FunctionNotFound(func_name))?;

            ctx.emit(Instruction::Call {
                dest,
                func: func_ref,
                args: args?,
            });
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, some_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapSome {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ok_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapOk {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, er_expr.payload(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::WrapErr {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::TryOption(try_expr) => {
            let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            let is_some = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::UnwrapOption {
                dest,
                is_some,
                src: Operand::Value(src_id),
            });
            // TODO: Branch on is_some for early return.
            Ok(dest)
        }
        ExprFunKind::TryResult(try_expr) => {
            let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            let is_ok = ctx.fresh_value(IrType::Bool);
            ctx.emit(Instruction::UnwrapResult {
                dest,
                is_ok,
                src: Operand::Value(src_id),
            });
            // TODO: Branch on is_ok for early return.
            Ok(dest)
        }
        ExprFunKind::Tuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let fields = elements?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::Tuple(fields.len() as u32),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::AnonTuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let fields = elements?;
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::Pack {
                dest,
                ty: TypeRef::Tuple(fields.len() as u32),
                fields,
            });
            Ok(dest)
        }
        ExprFunKind::List(list) => {
            let elements: Result<Vec<_>, _> = list.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::ListNew {
                dest,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Set(set) => {
            let elements: Result<Vec<_>, _> = set.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::SetNew {
                dest,
                elements: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::Map(map) => {
            let entries: Result<Vec<_>, _> = map.entries(ctx.db)
                .iter()
                .map(|e| {
                    let k = lower_expression(ctx, e.key(ctx.db))?;
                    let v = lower_expression(ctx, e.value(ctx.db))?;
                    Ok((Operand::Value(k), Operand::Value(v)))
                })
                .collect();
            let result_type = ctx.expr_type(expr);
            let dest = ctx.fresh_value(result_type);
            ctx.emit(Instruction::MapNew {
                dest,
                entries: entries?,
            });
            Ok(dest)
        }
        _ => {
            // TODO: Handle remaining expression types.
            Err(LowerError::NotImplemented("expression type".to_string()))
        }
    }
}

/// Lower a script unit.
///
/// Script units are sequences of statements (fragment) or a single expression (expr).
/// They can reference values from previous units and export bindings to subsequent units.
pub fn lower_script_unit<'db>(
    db: &'db dyn Db,
    tycheck_result: TypecheckResult<'db>,
    script_ctx: ScriptLowerContext,
    kind: ScriptUnitKind<'db>,
) -> Result<IrScriptUnit, LowerError> {
    let mut ctx = LowerCtx::new_for_script(db, tycheck_result.expr_types(db), script_ctx);

    let result = match kind {
        ScriptUnitKind::Fragment(stmts) => {
            // Lower all statements.
            for stmt in &stmts {
                lower_statement_for_script(&mut ctx, stmt)?;
            }
            // Fragment units have no result value.
            None
        }
        ScriptUnitKind::Expr(expr) => {
            // Lower the expression and capture the result.
            let value_id = lower_expression(&mut ctx, expr)?;
            Some(value_id)
        }
    };

    // Finish the final block with UnitEnd.
    ctx.finish_block(Terminator::UnitEnd {
        result: result.map(Operand::Value),
    });

    Ok(IrScriptUnit {
        blocks: ctx.blocks,
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
        value_types: std::mem::take(&mut ctx.value_types),
        slot_types: std::mem::take(&mut ctx.slot_types),
        functions: ctx.functions,
        symbols: ctx.symbols,
        result,
        exports: ctx.exports,
    })
}

/// Lower a statement in script unit context.
///
/// This handles function definitions by lowering them and adding to the unit's functions.
fn lower_statement_for_script<'db>(
    ctx: &mut LowerCtx<'db>,
    stmt: &Statement<'db>,
) -> Result<(), LowerError> {
    match stmt {
        Statement::Let(let_stmt) => {
            let name = let_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, let_stmt.value(ctx.db))?;
            ctx.bind_var(&name, Operand::Value(value_id));
            // Export the binding.
            ctx.exports.push((name, ExportBinding::Value(value_id)));
            Ok(())
        }
        Statement::Var(var_stmt) => {
            let name = var_stmt.name(ctx.db).text(ctx.db).to_string();
            let init_expr = var_stmt.value(ctx.db);
            let slot_type = ctx.expr_type(init_expr);
            let slot = ctx.fresh_slot(slot_type);
            let value_id = lower_expression(ctx, init_expr)?;
            ctx.emit(Instruction::SlotStore {
                dest: SlotDest::Local(slot),
                value: Operand::Value(value_id),
            });
            ctx.bind_var(&name, Operand::Slot(slot));
            // Export the binding.
            ctx.exports.push((name, ExportBinding::Slot(slot)));
            Ok(())
        }
        Statement::Set(set_stmt) => {
            // Same as function lowering - no export needed for assignment.
            let name = set_stmt.name(ctx.db).text(ctx.db).to_string();
            let value_id = lower_expression(ctx, set_stmt.value(ctx.db))?;
            if let Some(operand) = ctx.lookup_var(&name) {
                match operand {
                    Operand::Slot(slot) => {
                        ctx.emit(Instruction::SlotStore {
                            dest: SlotDest::Local(slot),
                            value: Operand::Value(value_id),
                        });
                        Ok(())
                    }
                    Operand::ExternalSlot { unit, slot } => {
                        // Store to external slot in a previous unit.
                        ctx.emit(Instruction::SlotStore {
                            dest: SlotDest::External { unit, slot },
                            value: Operand::Value(value_id),
                        });
                        Ok(())
                    }
                    _ => Err(LowerError::VariableNotMutable(name)),
                }
            } else {
                Err(LowerError::VariableNotFound(name))
            }
        }
        Statement::Ret(ret_stmt) => {
            // In scripts, return means early return from the unit.
            let value = if let Some(expr) = ret_stmt.value(ctx.db) {
                Operand::Value(lower_expression(ctx, expr)?)
            } else {
                // Return unit value for bare `ret`.
                let unit_val = ctx.fresh_value(IrType::Unit);
                ctx.emit(Instruction::Const {
                    dest: unit_val,
                    value: ConstValue::Unit,
                });
                Operand::Value(unit_val)
            };
            ctx.finish_block(Terminator::UnitEarlyReturn { value });
            // Start a new unreachable block.
            let new_block = ctx.fresh_block();
            ctx.start_block(new_block);
            Ok(())
        }
        Statement::Fun(fun_stmt) => {
            // Define the function in the symbol table first (allows recursion).
            let func_name = fun_stmt.name(ctx.db).text(ctx.db).to_string();
            let param_count = fun_stmt.params(ctx.db).len();
            let func_id = ctx.define_func(&func_name, param_count);

            // Save current lowering state.
            let saved_blocks = std::mem::take(&mut ctx.blocks);
            let saved_instructions = std::mem::take(&mut ctx.current_instructions);
            let saved_current_block = ctx.current_block;
            let saved_next_block = ctx.next_block;
            let saved_next_value = ctx.next_value;
            let saved_next_slot = ctx.next_slot;
            let saved_variables = std::mem::take(&mut ctx.variables);

            // Reset for function body.
            ctx.current_block = BlockId(0);
            ctx.next_block = 1;
            ctx.next_value = 0;
            ctx.next_slot = 0;

            // Lower the function body.
            let func = lower_function_body(ctx, func_id, *fun_stmt)?;

            // Restore parent state.
            ctx.blocks = saved_blocks;
            ctx.current_instructions = saved_instructions;
            ctx.current_block = saved_current_block;
            ctx.next_block = saved_next_block;
            ctx.next_value = saved_next_value;
            ctx.next_slot = saved_next_slot;
            ctx.variables = saved_variables;

            // Add the function to the unit's functions.
            ctx.functions.push(func);

            // Export the function.
            ctx.exports.push((func_name, ExportBinding::Function(func_id)));
            Ok(())
        }
        Statement::If(if_stmt) => {
            lower_if(ctx, *if_stmt)
        }
        Statement::Loop(loop_stmt) => {
            lower_loop(ctx, *loop_stmt)
        }
        Statement::Break(_) => {
            Err(LowerError::NotImplemented("break".to_string()))
        }
        Statement::Continue(_) => {
            Err(LowerError::NotImplemented("continue".to_string()))
        }
        Statement::Require(_) | Statement::Import(_) => {
            // Module-level, handled elsewhere.
            Ok(())
        }
        Statement::ParseError(_) => {
            Err(LowerError::ParseError)
        }
    }
}

/// Errors that can occur during lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    VariableNotFound(String),
    VariableNotMutable(String),
    FunctionNotFound(String),
    InvalidLiteral(String),
    NotImplemented(String),
    ParseError,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::VariableNotFound(name) => write!(f, "variable not found: {}", name),
            LowerError::VariableNotMutable(name) => write!(f, "variable not mutable: {}", name),
            LowerError::FunctionNotFound(name) => write!(f, "function not found: {}", name),
            LowerError::InvalidLiteral(lit) => write!(f, "invalid literal: {}", lit),
            LowerError::NotImplemented(what) => write!(f, "not implemented: {}", what),
            LowerError::ParseError => write!(f, "parse error in source"),
        }
    }
}

impl std::error::Error for LowerError {}
