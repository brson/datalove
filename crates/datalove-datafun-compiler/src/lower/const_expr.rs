//! Compile-time constant expression evaluation.
//!
//! Evaluates expressions at compile time. Simple literals are extracted directly.
//! Complex expressions are lowered to IR and evaluated via a pluggable `CtfeEvaluator`.

use std::collections::HashMap;
use datalove_datafun_ast::ast::{self, ExprFun, ExprFunKind};
use datalove_datafun_ir::{
    ConstValue, IrType, IrScriptUnit, IrBlock, BlockId, ValueId,
    Operand, Terminator, Instruction, SymbolTable, BinOp, UnaryOp,
};
use super::context::LowerCtx;
use super::LowerError;

/// Evaluate a constant expression at compile time.
///
/// - Simple literals are extracted directly without needing an evaluator.
/// - Const name references look up previously computed values.
/// - Complex expressions require a `CtfeEvaluator` to be configured.
pub fn eval_const_expr<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ConstValue, LowerError> {
    // For simple literals, extract directly.
    if let Some(value) = try_eval_literal(ctx, expr) {
        return value;
    }

    // For const references, look up the previously computed value.
    if let ExprFunKind::Name(name) = expr.expr(ctx.db) {
        let name_str = name.text(ctx.db);
        if let Some((_, value)) = ctx.lookup_const(name_str) {
            return Ok(value.clone());
        }
        return Err(LowerError::NotImplemented(format!(
            "non-const variable '{}' in const expression",
            name_str
        )));
    }

    // For complex expressions, lower to IR and use the evaluator.
    let ir_type = ctx.expr_type(expr);
    let unit = lower_const_expr_to_unit(ctx, expr)?;

    // Get the evaluator from context.
    let evaluator = ctx.ctfe_evaluator()
        .ok_or_else(|| LowerError::NotImplemented(
            "complex const expressions require a CTFE evaluator".to_string()
        ))?;

    evaluator.borrow_mut()
        .evaluate(&unit, &ir_type)
        .map_err(|e| LowerError::NotImplemented(format!("CTFE error: {}", e)))
}

/// Evaluate a constant expression without a full LowerCtx.
///
/// This is used for module-level const evaluation where we don't have access
/// to a CTFE evaluator (tracked functions can't take trait objects).
///
/// Supports:
/// - Simple literals (int, float, bool, string, none)
/// - References to previously evaluated consts
///
/// Does NOT support complex expressions (binary ops, function calls, etc.).
pub fn eval_const_expr_simple<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
) -> Result<ConstValue, LowerError> {
    match expr.expr(db) {
        ExprFunKind::True(_) => Ok(ConstValue::Bool(true)),
        ExprFunKind::False(_) => Ok(ConstValue::Bool(false)),
        ExprFunKind::None(_) => Ok(ConstValue::OptionNone),

        ExprFunKind::Int(int_expr) => {
            let text = int_expr.value.text(db);
            super::literal::parse_int_const(text, ir_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
        }

        ExprFunKind::Float(float_expr) => {
            let text = float_expr.value.text(db);
            super::literal::parse_float_const(text, ir_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
        }

        ExprFunKind::String(s) => {
            Ok(ConstValue::String(s.value.as_str(db).to_string()))
        }

        ExprFunKind::Name(name) => {
            let name_str = name.text(db);
            if let Some((_, value)) = resolved_consts.get(name_str) {
                Ok(value.clone())
            } else {
                Err(LowerError::NotImplemented(format!(
                    "non-const variable '{}' in const expression",
                    name_str
                )))
            }
        }

        _ => Err(LowerError::NotImplemented(
            "complex const expressions not supported in modules (no CTFE evaluator)".to_string()
        )),
    }
}

/// Evaluate a constant expression using the CTFE evaluator.
///
/// This handles complex expressions that can't be evaluated as simple literals.
/// Used for module-level const evaluation where we have access to the evaluator
/// outside of tracked salsa functions.
pub fn eval_const_expr_with_evaluator<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
    evaluator: std::rc::Rc<std::cell::RefCell<dyn datalove_datafun_ir::CtfeEvaluator>>,
) -> Result<ConstValue, LowerError> {
    // Lower the expression to a minimal IR unit.
    let unit = lower_const_expr_to_unit_standalone(db, expr, ir_type, resolved_consts)?;

    // Evaluate using the CTFE evaluator.
    evaluator.borrow_mut()
        .evaluate(&unit, ir_type)
        .map_err(|e| LowerError::NotImplemented(format!("CTFE error: {}", e)))
}

/// Lower a const expression to a minimal IrScriptUnit without needing LowerCtx.
///
/// Used for module-level const evaluation outside of tracked functions.
fn lower_const_expr_to_unit_standalone<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
    resolved_consts: &HashMap<String, (IrType, ConstValue)>,
) -> Result<IrScriptUnit, LowerError> {
    let mut mini_ctx = StandaloneMiniLowerCtx::new(db, ir_type.clone(), resolved_consts);
    let result_value = mini_ctx.lower_expr(expr)?;
    mini_ctx.finish(result_value, ir_type.clone())
}

/// Minimal lowering context for standalone const expression evaluation.
struct StandaloneMiniLowerCtx<'a, 'db> {
    db: &'db dyn salsa::Database,
    result_type: IrType,
    resolved_consts: &'a HashMap<String, (IrType, ConstValue)>,
    instructions: Vec<Instruction>,
    next_value: u32,
    value_types: Vec<IrType>,
}

impl<'a, 'db> StandaloneMiniLowerCtx<'a, 'db> {
    fn new(db: &'db dyn salsa::Database, result_type: IrType, resolved_consts: &'a HashMap<String, (IrType, ConstValue)>) -> Self {
        Self {
            db,
            result_type,
            resolved_consts,
            instructions: Vec::new(),
            next_value: 0,
            value_types: Vec::new(),
        }
    }

    fn fresh_value(&mut self, ir_type: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.push(ir_type);
        id
    }

    fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr);
    }

    fn lower_expr(&mut self, expr: ExprFun<'db>) -> Result<ValueId, LowerError> {
        // Use result_type for type context when needed.
        let expr_type = self.result_type.clone();

        match expr.expr(self.db) {
            ExprFunKind::True(_) => {
                let dest = self.fresh_value(IrType::Bool);
                self.emit(Instruction::Const { dest, value: ConstValue::Bool(true) });
                Ok(dest)
            }
            ExprFunKind::False(_) => {
                let dest = self.fresh_value(IrType::Bool);
                self.emit(Instruction::Const { dest, value: ConstValue::Bool(false) });
                Ok(dest)
            }
            ExprFunKind::None(_) => {
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value: ConstValue::OptionNone });
                Ok(dest)
            }
            ExprFunKind::Int(int_expr) => {
                let text = int_expr.value.text(self.db);
                let value = super::literal::parse_int_const(text, &expr_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::Float(float_expr) => {
                let text = float_expr.value.text(self.db);
                let value = super::literal::parse_float_const(text, &expr_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::String(s) => {
                let value = ConstValue::String(s.value.as_str(self.db).to_string());
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::Name(name) => {
                let name_str = name.text(self.db);
                if let Some((ty, value)) = self.resolved_consts.get(name_str) {
                    let dest = self.fresh_value(ty.clone());
                    self.emit(Instruction::Const { dest, value: value.clone() });
                    Ok(dest)
                } else {
                    Err(LowerError::NotImplemented(format!(
                        "non-const variable '{}' in const expression",
                        name_str
                    )))
                }
            }
            ExprFunKind::BinOp(binop) => {
                let lhs = self.lower_expr(binop.lhs)?;
                let rhs = self.lower_expr(binop.rhs)?;
                let lhs_type = self.value_types[lhs.0 as usize].clone();
                let dest = self.fresh_value(lhs_type);

                let ir_op = convert_binop(binop.op)?;
                self.emit(Instruction::BinOp {
                    dest,
                    op: ir_op,
                    lhs: Operand::Value(lhs),
                    rhs: Operand::Value(rhs),
                });
                Ok(dest)
            }
            ExprFunKind::UnaryOp(unop) => {
                let operand = self.lower_expr(unop.operand)?;
                let operand_type = self.value_types[operand.0 as usize].clone();
                let dest = self.fresh_value(operand_type);

                let ir_op = convert_unaryop(unop.op)?;
                self.emit(Instruction::UnaryOp {
                    dest,
                    op: ir_op,
                    operand: Operand::Value(operand),
                });
                Ok(dest)
            }
            _ => Err(LowerError::NotImplemented(
                "unsupported expression in const evaluation".to_string()
            )),
        }
    }

    fn finish(self, result: ValueId, _result_type: IrType) -> Result<IrScriptUnit, LowerError> {
        let block = IrBlock {
            id: BlockId(0),
            params: Vec::new(),
            instructions: self.instructions,
            terminator: Terminator::UnitEnd {
                result: Some(Operand::Value(result)),
            },
        };

        Ok(IrScriptUnit {
            blocks: vec![block],
            value_count: self.next_value,
            slot_count: 0,
            value_types: self.value_types,
            slot_types: Vec::new(),
            tracked_values: Vec::new(),
            tracked_slots: Vec::new(),
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            result: Some(result),
            exports: Vec::new(),
        })
    }
}

/// Try to evaluate a literal expression directly without the interpreter.
fn try_eval_literal<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Option<Result<ConstValue, LowerError>> {
    match expr.expr(ctx.db) {
        ExprFunKind::True(_) => Some(Ok(ConstValue::Bool(true))),
        ExprFunKind::False(_) => Some(Ok(ConstValue::Bool(false))),
        ExprFunKind::None(_) => Some(Ok(ConstValue::OptionNone)),

        ExprFunKind::Int(int_expr) => {
            let ir_type = ctx.expr_type(expr);
            let text = int_expr.value.text(ctx.db);
            Some(
                super::literal::parse_int_const(text, &ir_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
            )
        }

        ExprFunKind::Float(float_expr) => {
            let ir_type = ctx.expr_type(expr);
            let text = float_expr.value.text(ctx.db);
            Some(
                super::literal::parse_float_const(text, &ir_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
            )
        }

        ExprFunKind::String(s) => {
            Some(Ok(ConstValue::String(s.value.as_str(ctx.db).to_string())))
        }

        _ => None, // Not a simple literal.
    }
}

/// Lower a const expression to a minimal IrScriptUnit for execution.
fn lower_const_expr_to_unit<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<IrScriptUnit, LowerError> {
    let result_type = ctx.expr_type(expr);

    // Create a mini lowering context for this expression.
    let mut mini_ctx = MiniLowerCtx::new(ctx);

    // Lower the expression.
    let result_value = mini_ctx.lower_expr(expr)?;

    // Build the script unit.
    mini_ctx.finish(result_value, result_type)
}

/// Minimal lowering context for const expression evaluation.
struct MiniLowerCtx<'a, 'db> {
    parent: &'a LowerCtx<'db>,
    value_types: Vec<IrType>,
    instructions: Vec<Instruction>,
    next_value: u32,
}

impl<'a, 'db> MiniLowerCtx<'a, 'db> {
    fn new(parent: &'a LowerCtx<'db>) -> Self {
        Self {
            parent,
            value_types: Vec::new(),
            instructions: Vec::new(),
            next_value: 0,
        }
    }

    fn fresh_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.push(ty);
        id
    }

    fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr);
    }

    fn lower_expr(&mut self, expr: ExprFun<'db>) -> Result<ValueId, LowerError> {
        let expr_type = self.parent.expr_type(expr);

        match expr.expr(self.parent.db) {
            // Literals - emit Const instructions.
            ExprFunKind::True(_) => {
                let dest = self.fresh_value(IrType::Bool);
                self.emit(Instruction::Const { dest, value: ConstValue::Bool(true) });
                Ok(dest)
            }
            ExprFunKind::False(_) => {
                let dest = self.fresh_value(IrType::Bool);
                self.emit(Instruction::Const { dest, value: ConstValue::Bool(false) });
                Ok(dest)
            }
            ExprFunKind::None(_) => {
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value: ConstValue::OptionNone });
                Ok(dest)
            }
            ExprFunKind::Int(int_expr) => {
                let text = int_expr.value.text(self.parent.db);
                let value = super::literal::parse_int_const(text, &expr_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::Float(float_expr) => {
                let text = float_expr.value.text(self.parent.db);
                let value = super::literal::parse_float_const(text, &expr_type)
                    .map_err(|_| LowerError::InvalidLiteral(text.to_string()))?;
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::String(s) => {
                let value = ConstValue::String(s.value.as_str(self.parent.db).to_string());
                let dest = self.fresh_value(expr_type);
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }

            // Const reference - substitute with the previously computed value.
            ExprFunKind::Name(name) => {
                let name_str = name.text(self.parent.db);
                if let Some((_, value)) = self.parent.lookup_const(name_str) {
                    let dest = self.fresh_value(expr_type);
                    self.emit(Instruction::Const { dest, value: value.clone() });
                    Ok(dest)
                } else {
                    Err(LowerError::NotImplemented(format!(
                        "non-const variable '{}' in const expression",
                        name_str
                    )))
                }
            }

            // Binary operation.
            ExprFunKind::BinOp(binop) => {
                let lhs = self.lower_expr(binop.lhs)?;
                let rhs = self.lower_expr(binop.rhs)?;
                let dest = self.fresh_value(expr_type);

                let ir_op = convert_binop(binop.op)?;
                self.emit(Instruction::BinOp {
                    dest,
                    op: ir_op,
                    lhs: Operand::Value(lhs),
                    rhs: Operand::Value(rhs),
                });
                Ok(dest)
            }

            // Unary operation.
            ExprFunKind::UnaryOp(unop) => {
                let operand = self.lower_expr(unop.operand)?;
                let dest = self.fresh_value(expr_type);

                let ir_op = convert_unaryop(unop.op)?;
                self.emit(Instruction::UnaryOp {
                    dest,
                    op: ir_op,
                    operand: Operand::Value(operand),
                });
                Ok(dest)
            }

            // Function calls not supported yet.
            ExprFunKind::FunctionCall(_) => {
                Err(LowerError::NotImplemented(
                    "function calls in const expressions not yet supported".to_string()
                ))
            }

            _ => Err(LowerError::NotImplemented(format!(
                "const evaluation for expression type not yet supported: {:?}",
                std::mem::discriminant(&expr.expr(self.parent.db))
            ))),
        }
    }

    fn finish(self, result: ValueId, result_type: IrType) -> Result<IrScriptUnit, LowerError> {
        let block = IrBlock {
            id: BlockId(0),
            params: Vec::new(),
            instructions: self.instructions,
            terminator: Terminator::UnitEnd {
                result: Some(Operand::Value(result)),
            },
        };

        Ok(IrScriptUnit {
            blocks: vec![block],
            value_count: self.next_value,
            slot_count: 0,
            value_types: self.value_types,
            slot_types: Vec::new(),
            tracked_values: Vec::new(),
            tracked_slots: Vec::new(),
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            result: Some(result),
            exports: Vec::new(),
        })
    }
}

/// Convert AST BinOp to IR BinOp.
fn convert_binop(ast_op: ast::BinOp) -> Result<BinOp, LowerError> {
    match ast_op {
        ast::BinOp::Add => Ok(BinOp::Add),
        ast::BinOp::Sub => Ok(BinOp::Sub),
        ast::BinOp::Mul => Ok(BinOp::Mul),
        ast::BinOp::Div => Ok(BinOp::Div),
        ast::BinOp::Eq => Ok(BinOp::Eq),
        ast::BinOp::Ne => Ok(BinOp::Ne),
        ast::BinOp::Lt => Ok(BinOp::Lt),
        ast::BinOp::Le => Ok(BinOp::Le),
        ast::BinOp::Gt => Ok(BinOp::Gt),
        ast::BinOp::Ge => Ok(BinOp::Ge),
        ast::BinOp::And => Ok(BinOp::LogicAnd),
        ast::BinOp::Or => Ok(BinOp::LogicOr),
        ast::BinOp::Xor => Ok(BinOp::LogicXor),
        // Checked/optional ops not yet supported in simple CTFE.
        ast::BinOp::AddChecked | ast::BinOp::SubChecked |
        ast::BinOp::MulChecked | ast::BinOp::DivChecked |
        ast::BinOp::AddOptional | ast::BinOp::SubOptional |
        ast::BinOp::MulOptional | ast::BinOp::DivOptional => {
            Err(LowerError::NotImplemented(
                "checked/optional arithmetic in const expressions not yet supported".to_string()
            ))
        }
    }
}

/// Convert AST UnaryOp to IR UnaryOp.
fn convert_unaryop(ast_op: ast::UnaryOp) -> Result<UnaryOp, LowerError> {
    match ast_op {
        ast::UnaryOp::Neg => Ok(UnaryOp::Neg),
        ast::UnaryOp::Not => Ok(UnaryOp::Not),
        ast::UnaryOp::NegOptional | ast::UnaryOp::NegResult => {
            Err(LowerError::NotImplemented(
                "optional/result negation in const expressions not yet supported".to_string()
            ))
        }
    }
}

// ============================================================================
// Phase 2: Evaluate Consts (Not Memoized)
// ============================================================================

use std::cell::RefCell;
use std::rc::Rc;
use salsa::plumbing::AsId;
use datalove_datafun_ast::ast::Statement;
use datalove_datafun_ir::{
    ConstBindingGraph, ResolvedConsts,
    ConstEvalError, CtfeEvaluator, CtfeError,
};
use datalove_datafun_tycheck::Type;

/// Evaluate all const bindings from a ConstBindingGraph.
///
/// This is Phase 2 of the 3-phase CTFE memoization pipeline.
/// NOT memoized because it requires a trait object (CtfeEvaluator).
///
/// Evaluates consts in topological order (dependencies before dependents).
/// Simple literals are extracted directly; complex expressions use the evaluator.
pub fn evaluate_consts<'db>(
    db: &'db dyn salsa::Database,
    graph: &ConstBindingGraph,
    statements: &[Statement<'db>],
    expr_types: &[Option<Type<'db>>],
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<ResolvedConsts, ConstEvalError> {
    let mut resolved = ResolvedConsts::new();

    for binding in &graph.bindings {
        // Find the expression for this binding.
        let expr = statements.iter()
            .find_map(|s| match s {
                Statement::Const(c) if c.value.as_id() == binding.stmt_id => Some(c.value),
                _ => None,
            });

        let expr = match expr {
            Some(e) => e,
            None => {
                // This shouldn't happen if the graph was built correctly.
                panic!("const binding expression not found: {}", binding.name);
            }
        };

        // Try simple literal extraction first.
        if let Some(value) = try_extract_literal(db, expr, &binding.ir_type) {
            resolved.insert(binding.stmt_id, binding.name.clone(), value);
            continue;
        }

        // Check for const references - substitute with already-evaluated values.
        if let ExprFunKind::Name(name) = expr.expr(db) {
            let name_str = name.text(db);
            if let Some(value) = resolved.get_by_name(name_str) {
                resolved.insert(binding.stmt_id, binding.name.clone(), value.clone());
                continue;
            }
            // Not a const reference we've evaluated - fall through to CTFE.
        }

        // Lower to mini-unit and evaluate.
        let unit = lower_const_expr_standalone(db, expr, &binding.ir_type, &resolved)?;

        let value = evaluator.borrow_mut()
            .evaluate(&unit, &binding.ir_type)
            .map_err(|e| match e {
                CtfeError::InterpError(msg) if msg.contains("gas") => {
                    ConstEvalError::GasExpired { binding_name: binding.name.clone() }
                }
                _ => {
                    // Other interpreter errors are unexpected.
                    panic!("unexpected CTFE error evaluating const '{}': {}", binding.name, e);
                }
            })?;

        resolved.insert(binding.stmt_id, binding.name.clone(), value);
    }

    Ok(resolved)
}

/// Try to extract a literal value directly without interpreter.
fn try_extract_literal<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    ir_type: &IrType,
) -> Option<ConstValue> {
    match expr.expr(db) {
        ExprFunKind::True(_) => Some(ConstValue::Bool(true)),
        ExprFunKind::False(_) => Some(ConstValue::Bool(false)),
        ExprFunKind::None(_) => Some(ConstValue::OptionNone),

        ExprFunKind::Int(int_expr) => {
            let text = int_expr.value.text(db);
            super::literal::parse_int_const(text, ir_type).ok()
        }

        ExprFunKind::Float(float_expr) => {
            let text = float_expr.value.text(db);
            super::literal::parse_float_const(text, ir_type).ok()
        }

        ExprFunKind::String(s) => {
            Some(ConstValue::String(s.value.as_str(db).to_string()))
        }

        _ => None,
    }
}

/// Lower a const expression to IrScriptUnit without needing LowerCtx.
///
/// Uses pre-resolved const values for name references.
fn lower_const_expr_standalone<'db>(
    db: &'db dyn salsa::Database,
    expr: ExprFun<'db>,
    result_type: &IrType,
    resolved: &ResolvedConsts,
) -> Result<IrScriptUnit, ConstEvalError> {
    let mut ctx = StandaloneLowerCtx::new(db, resolved);
    let result_value = ctx.lower_expr(expr, result_type)?;
    Ok(ctx.finish(result_value))
}

/// Standalone lowering context for Phase 2 const evaluation.
///
/// Unlike MiniLowerCtx, this doesn't depend on LowerCtx.
struct StandaloneLowerCtx<'a, 'db> {
    db: &'db dyn salsa::Database,
    resolved: &'a ResolvedConsts,
    value_types: Vec<IrType>,
    instructions: Vec<Instruction>,
    next_value: u32,
}

impl<'a, 'db> StandaloneLowerCtx<'a, 'db> {
    fn new(db: &'db dyn salsa::Database, resolved: &'a ResolvedConsts) -> Self {
        Self {
            db,
            resolved,
            value_types: Vec::new(),
            instructions: Vec::new(),
            next_value: 0,
        }
    }

    fn fresh_value(&mut self, ty: IrType) -> ValueId {
        let id = ValueId(self.next_value);
        self.next_value += 1;
        self.value_types.push(ty);
        id
    }

    fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr);
    }

    fn lower_expr(&mut self, expr: ExprFun<'db>, expected_type: &IrType) -> Result<ValueId, ConstEvalError> {
        match expr.expr(self.db) {
            // Literals.
            ExprFunKind::True(_) => {
                let dest = self.fresh_value(IrType::Bool);
                self.emit(Instruction::Const { dest, value: ConstValue::Bool(true) });
                Ok(dest)
            }
            ExprFunKind::False(_) => {
                let dest = self.fresh_value(IrType::Bool);
                self.emit(Instruction::Const { dest, value: ConstValue::Bool(false) });
                Ok(dest)
            }
            ExprFunKind::None(_) => {
                let dest = self.fresh_value(expected_type.clone());
                self.emit(Instruction::Const { dest, value: ConstValue::OptionNone });
                Ok(dest)
            }
            ExprFunKind::Int(int_expr) => {
                let text = int_expr.value.text(self.db);
                let value = super::literal::parse_int_const(text, expected_type)
                    .map_err(|_| ConstEvalError::GasExpired {
                        binding_name: format!("invalid literal: {}", text),
                    })?;
                let dest = self.fresh_value(expected_type.clone());
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::Float(float_expr) => {
                let text = float_expr.value.text(self.db);
                let value = super::literal::parse_float_const(text, expected_type)
                    .map_err(|_| ConstEvalError::GasExpired {
                        binding_name: format!("invalid literal: {}", text),
                    })?;
                let dest = self.fresh_value(expected_type.clone());
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }
            ExprFunKind::String(s) => {
                let value = ConstValue::String(s.value.as_str(self.db).to_string());
                let dest = self.fresh_value(expected_type.clone());
                self.emit(Instruction::Const { dest, value });
                Ok(dest)
            }

            // Const reference.
            ExprFunKind::Name(name) => {
                let name_str = name.text(self.db);
                if let Some(value) = self.resolved.get_by_name(name_str) {
                    let dest = self.fresh_value(expected_type.clone());
                    self.emit(Instruction::Const { dest, value: value.clone() });
                    Ok(dest)
                } else {
                    Err(ConstEvalError::DependencyFailed {
                        binding_name: name_str.to_string(),
                        dependency: name_str.to_string(),
                    })
                }
            }

            // Binary operation.
            ExprFunKind::BinOp(binop) => {
                // For standalone lowering, we don't have type info for subexpressions.
                // Use the expected type for both operands as a simplification.
                // This works for homogeneous ops but may fail for others.
                let lhs = self.lower_expr(binop.lhs, expected_type)?;
                let rhs = self.lower_expr(binop.rhs, expected_type)?;
                let dest = self.fresh_value(expected_type.clone());

                let ir_op = convert_binop(binop.op).map_err(|_| {
                    ConstEvalError::GasExpired {
                        binding_name: "unsupported binop".to_string(),
                    }
                })?;
                self.emit(Instruction::BinOp {
                    dest,
                    op: ir_op,
                    lhs: Operand::Value(lhs),
                    rhs: Operand::Value(rhs),
                });
                Ok(dest)
            }

            // Unary operation.
            ExprFunKind::UnaryOp(unop) => {
                let operand = self.lower_expr(unop.operand, expected_type)?;
                let dest = self.fresh_value(expected_type.clone());

                let ir_op = convert_unaryop(unop.op).map_err(|_| {
                    ConstEvalError::GasExpired {
                        binding_name: "unsupported unaryop".to_string(),
                    }
                })?;
                self.emit(Instruction::UnaryOp {
                    dest,
                    op: ir_op,
                    operand: Operand::Value(operand),
                });
                Ok(dest)
            }

            _ => Err(ConstEvalError::GasExpired {
                binding_name: "unsupported expression type".to_string(),
            }),
        }
    }

    fn finish(self, result: ValueId) -> IrScriptUnit {
        let block = IrBlock {
            id: BlockId(0),
            params: Vec::new(),
            instructions: self.instructions,
            terminator: Terminator::UnitEnd {
                result: Some(Operand::Value(result)),
            },
        };

        IrScriptUnit {
            blocks: vec![block],
            value_count: self.next_value,
            slot_count: 0,
            value_types: self.value_types,
            slot_types: Vec::new(),
            tracked_values: Vec::new(),
            tracked_slots: Vec::new(),
            unit_end_values: Vec::new(),
            unit_end_slots: Vec::new(),
            functions: Vec::new(),
            symbols: SymbolTable::new(),
            result: Some(result),
            exports: Vec::new(),
        }
    }
}
