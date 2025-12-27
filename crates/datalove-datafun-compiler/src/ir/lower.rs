//! Lower AST to SSA IR.
//!
//! This module transforms typechecked AST into flat SSA IR suitable for
//! interpretation and codegen.

use rmx::prelude::*;
use std::collections::HashMap;
use crate::ast::{self, Statement, ExprFun, ExprFunKind};
use crate::Db;
use super::*;

/// Context for lowering a single function.
pub struct LowerCtx<'db> {
    db: &'db dyn Db,
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
}

impl<'db> LowerCtx<'db> {
    pub fn new(db: &'db dyn Db) -> Self {
        Self {
            db,
            next_value: 0,
            next_slot: 0,
            next_block: 1, // Block 0 is entry
            blocks: Vec::new(),
            current_block: BlockId(0),
            current_instructions: Vec::new(),
            variables: HashMap::new(),
        }
    }

    /// Allocate a fresh SSA value.
    fn fresh_value(&mut self) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        id
    }

    /// Allocate a fresh mutable slot.
    fn fresh_slot(&mut self) -> SlotId {
        let id = SlotId(self.next_slot);
        self.next_slot += 1;
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
pub fn lower_function<'db>(
    db: &'db dyn Db,
    func: ast::StmtFun<'db>,
) -> Result<IrFunction, LowerError> {
    let mut ctx = LowerCtx::new(db);

    let name = func.name(db).text(db).to_string();

    // Allocate ValueIds for parameters.
    let params: Vec<ValueId> = func.params(db)
        .iter()
        .map(|p| {
            let id = ctx.fresh_value();
            let param_name = p.name(db).text(db).to_string();
            ctx.bind_var(&param_name, Operand::Value(id));
            id
        })
        .collect();

    // Lower the function body.
    for stmt in func.body(db) {
        lower_statement(&mut ctx, stmt)?;
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
        name,
        params,
        blocks: ctx.blocks,
        value_count: ctx.next_value,
        slot_count: ctx.next_slot,
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
            let slot = ctx.fresh_slot();
            let value_id = lower_expression(ctx, var_stmt.value(ctx.db))?;
            ctx.emit(Instruction::SlotStore {
                slot,
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
                    slot,
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
                        let dest = ctx.fresh_value();
                        ctx.emit(Instruction::SlotLoad { dest, slot: s });
                        Ok(dest)
                    }
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => {
                        // External operands from previous script units.
                        // Copy into a local value.
                        let dest = ctx.fresh_value();
                        ctx.emit(Instruction::Copy { dest, src: operand });
                        Ok(dest)
                    }
                }
            } else {
                Err(LowerError::VariableNotFound(name_str.to_string()))
            }
        }
        ExprFunKind::True(_) => {
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(true),
            });
            Ok(dest)
        }
        ExprFunKind::False(_) => {
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::Bool(false),
            });
            Ok(dest)
        }
        ExprFunKind::None(_) => {
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::WrapNone { dest });
            Ok(dest)
        }
        ExprFunKind::Int(lit) => {
            let dest = ctx.fresh_value();
            let text = lit.value(ctx.db).text(ctx.db);
            // Parse as i64 for now.
            let value: i64 = text.parse().map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::I64(value),
            });
            Ok(dest)
        }
        ExprFunKind::Hex(lit) => {
            let dest = ctx.fresh_value();
            let text = lit.value(ctx.db).text(ctx.db);
            // Remove 0x prefix and parse.
            let hex_str = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
            let value = u64::from_str_radix(hex_str, 16)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
            ctx.emit(Instruction::Const {
                dest,
                value: ConstValue::U64(value),
            });
            Ok(dest)
        }
        ExprFunKind::BinOp(binop) => {
            let lhs_id = lower_expression(ctx, binop.lhs(ctx.db))?;
            let rhs_id = lower_expression(ctx, binop.rhs(ctx.db))?;
            let dest = ctx.fresh_value();

            let op = match binop.op(ctx.db) {
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
                    let overflow = ctx.fresh_value();
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
                    let overflow = ctx.fresh_value();
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
                    let overflow = ctx.fresh_value();
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
                    let overflow = ctx.fresh_value();
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
            let dest = ctx.fresh_value();

            let op = match unary.op(ctx.db) {
                ast::UnaryOp::Neg => UnaryOp::Neg,
                ast::UnaryOp::NegOptional | ast::UnaryOp::NegResult => {
                    // TODO: Handle checked unary ops.
                    UnaryOp::Neg
                }
            };

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
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::Call {
                dest,
                func: func_name,
                args: args?,
            });
            Ok(dest)
        }
        ExprFunKind::Some(some_expr) => {
            let inner_id = lower_expression(ctx, some_expr.payload(ctx.db))?;
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::WrapSome {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Ok(ok_expr) => {
            let inner_id = lower_expression(ctx, ok_expr.payload(ctx.db))?;
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::WrapOk {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::Er(er_expr) => {
            let inner_id = lower_expression(ctx, er_expr.payload(ctx.db))?;
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::WrapErr {
                dest,
                inner: Operand::Value(inner_id),
            });
            Ok(dest)
        }
        ExprFunKind::TryOption(try_expr) => {
            let src_id = lower_expression(ctx, try_expr.operand(ctx.db))?;
            let dest = ctx.fresh_value();
            let is_some = ctx.fresh_value();
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
            let dest = ctx.fresh_value();
            let is_ok = ctx.fresh_value();
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
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::Pack {
                dest,
                ty: "tuple".to_string(),
                fields: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::AnonTuple(tuple) => {
            let elements: Result<Vec<_>, _> = tuple.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let dest = ctx.fresh_value();
            ctx.emit(Instruction::Pack {
                dest,
                ty: "anon_tuple".to_string(),
                fields: elements?,
            });
            Ok(dest)
        }
        ExprFunKind::List(list) => {
            let elements: Result<Vec<_>, _> = list.elements(ctx.db)
                .iter()
                .map(|e| lower_expression(ctx, *e).map(|v| Operand::Value(v)))
                .collect();
            let dest = ctx.fresh_value();
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
            let dest = ctx.fresh_value();
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
            let dest = ctx.fresh_value();
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

/// Errors that can occur during lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    VariableNotFound(String),
    VariableNotMutable(String),
    InvalidLiteral(String),
    NotImplemented(String),
    ParseError,
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::VariableNotFound(name) => write!(f, "variable not found: {}", name),
            LowerError::VariableNotMutable(name) => write!(f, "variable not mutable: {}", name),
            LowerError::InvalidLiteral(lit) => write!(f, "invalid literal: {}", lit),
            LowerError::NotImplemented(what) => write!(f, "not implemented: {}", what),
            LowerError::ParseError => write!(f, "parse error in source"),
        }
    }
}

impl std::error::Error for LowerError {}
