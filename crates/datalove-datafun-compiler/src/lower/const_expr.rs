//! Compile-time constant expression evaluation.
//!
//! Evaluates expressions at compile time. Simple literals are extracted directly.
//! Complex expressions are lowered to IR and evaluated via a pluggable `CtfeEvaluator`.

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
