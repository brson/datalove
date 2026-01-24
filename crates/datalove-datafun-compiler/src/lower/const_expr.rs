//! Compile-time constant expression evaluation.
//!
//! Evaluates expressions that can be computed at compile time for const bindings.

use datalove_datafun_ast::ast::{ExprFun, ExprFunKind, BinOp, UnaryOp};
use datalove_datafun_ir::ConstValue;
use super::context::LowerCtx;
use super::LowerError;

/// Evaluate a constant expression at compile time.
///
/// Currently supports:
/// - Integer, float, bool, and string literals
/// - Simple arithmetic on constants
/// - References to other const bindings
/// - Option/Result wrappers (Some, None, Ok, Err)
/// - Tuples and lists
///
/// Returns an error if the expression cannot be evaluated at compile time.
pub fn eval_const_expr<'db>(
    ctx: &LowerCtx<'db>,
    expr: ExprFun<'db>,
) -> Result<ConstValue, LowerError> {
    match expr.expr(ctx.db) {
        // Function calls - not yet supported.
        ExprFunKind::FunctionCall(_) => {
            Err(LowerError::NotImplemented(
                "function calls in const expressions not yet supported".to_string()
            ))
        }

        // Bool literals.
        ExprFunKind::True(_) => Ok(ConstValue::Bool(true)),
        ExprFunKind::False(_) => Ok(ConstValue::Bool(false)),

        // Integer literals.
        ExprFunKind::Int(int_expr) => {
            let ir_type = ctx.expr_type(expr);
            let text = int_expr.value.text(ctx.db);
            super::literal::parse_int_const(text, &ir_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
        }

        // Float literals.
        ExprFunKind::Float(float_expr) => {
            let ir_type = ctx.expr_type(expr);
            let text = float_expr.value.text(ctx.db);
            super::literal::parse_float_const(text, &ir_type)
                .map_err(|_| LowerError::InvalidLiteral(text.to_string()))
        }

        // String literals.
        ExprFunKind::String(s) => {
            Ok(ConstValue::String(s.value.as_str(ctx.db).to_string()))
        }

        // Reference to another const binding.
        ExprFunKind::Name(name) => {
            let name_str = name.text(ctx.db);
            if let Some((_, value)) = ctx.lookup_const(name_str) {
                Ok(value.clone())
            } else {
                Err(LowerError::NotImplemented(format!(
                    "non-const variable '{}' in const expression",
                    name_str
                )))
            }
        }

        // Binary operations on constants.
        ExprFunKind::BinOp(binop) => {
            let lhs_val = eval_const_expr(ctx, binop.lhs)?;
            let rhs_val = eval_const_expr(ctx, binop.rhs)?;
            eval_const_binop(&lhs_val, &rhs_val, binop.op)
        }

        // Unary operations on constants.
        ExprFunKind::UnaryOp(unop) => {
            let operand_val = eval_const_expr(ctx, unop.operand)?;
            eval_const_unaryop(&operand_val, unop.op)
        }

        // None literal.
        ExprFunKind::None(_) => Ok(ConstValue::OptionNone),

        // Some wrapper.
        ExprFunKind::Some(some_expr) => {
            let inner_val = eval_const_expr(ctx, some_expr.payload)?;
            Ok(ConstValue::OptionSome(Box::new(inner_val)))
        }

        // Ok wrapper.
        ExprFunKind::Ok(ok_expr) => {
            let inner_val = eval_const_expr(ctx, ok_expr.payload)?;
            Ok(ConstValue::ResultOk(Box::new(inner_val)))
        }

        // Err wrapper.
        ExprFunKind::Er(er_expr) => {
            let inner_val = eval_const_expr(ctx, er_expr.payload)?;
            Ok(ConstValue::ResultErr(Box::new(inner_val)))
        }

        // Error wrapper.
        ExprFunKind::Error(error_expr) => {
            let inner_val = eval_const_expr(ctx, error_expr.value)?;
            // Convert inner value to string for Error type.
            let msg = match inner_val {
                ConstValue::String(s) => s,
                other => format!("{:?}", other),
            };
            Ok(ConstValue::Error(msg))
        }

        // Tuple literal (datafun style).
        ExprFunKind::Tuple(tuple) => {
            let mut values = Vec::with_capacity(tuple.elements.len());
            for elem in &tuple.elements {
                values.push(eval_const_expr(ctx, *elem)?);
            }
            Ok(ConstValue::Tuple(values))
        }

        // Anonymous tuple literal.
        ExprFunKind::AnonTuple(tuple) => {
            let mut values = Vec::with_capacity(tuple.elements.len());
            for elem in &tuple.elements {
                values.push(eval_const_expr(ctx, *elem)?);
            }
            Ok(ConstValue::Tuple(values))
        }

        // List literal.
        ExprFunKind::List(list) => {
            let mut values = Vec::with_capacity(list.elements.len());
            for elem in &list.elements {
                values.push(eval_const_expr(ctx, *elem)?);
            }
            Ok(ConstValue::List(values))
        }

        _ => Err(LowerError::NotImplemented(format!(
            "const evaluation for expression type not yet supported: {:?}",
            std::mem::discriminant(&expr.expr(ctx.db))
        ))),
    }
}

/// Evaluate a binary operation on constant values.
pub fn eval_const_binop(
    lhs: &ConstValue,
    rhs: &ConstValue,
    op: BinOp,
) -> Result<ConstValue, LowerError> {
    match (lhs, rhs, op) {
        // u32 operations.
        (ConstValue::U32(l), ConstValue::U32(r), BinOp::Add) => Ok(ConstValue::U32(l.wrapping_add(*r))),
        (ConstValue::U32(l), ConstValue::U32(r), BinOp::AddChecked) => {
            l.checked_add(*r)
                .map(ConstValue::U32)
                .ok_or_else(|| LowerError::InvalidLiteral("u32 overflow in const".to_string()))
        }
        (ConstValue::U32(l), ConstValue::U32(r), BinOp::Sub) => Ok(ConstValue::U32(l.wrapping_sub(*r))),
        (ConstValue::U32(l), ConstValue::U32(r), BinOp::SubChecked) => {
            l.checked_sub(*r)
                .map(ConstValue::U32)
                .ok_or_else(|| LowerError::InvalidLiteral("u32 underflow in const".to_string()))
        }
        (ConstValue::U32(l), ConstValue::U32(r), BinOp::Mul) => Ok(ConstValue::U32(l.wrapping_mul(*r))),
        (ConstValue::U32(l), ConstValue::U32(r), BinOp::MulChecked) => {
            l.checked_mul(*r)
                .map(ConstValue::U32)
                .ok_or_else(|| LowerError::InvalidLiteral("u32 overflow in const".to_string()))
        }

        // i32 operations.
        (ConstValue::I32(l), ConstValue::I32(r), BinOp::Add) => Ok(ConstValue::I32(l.wrapping_add(*r))),
        (ConstValue::I32(l), ConstValue::I32(r), BinOp::AddChecked) => {
            l.checked_add(*r)
                .map(ConstValue::I32)
                .ok_or_else(|| LowerError::InvalidLiteral("i32 overflow in const".to_string()))
        }
        (ConstValue::I32(l), ConstValue::I32(r), BinOp::Sub) => Ok(ConstValue::I32(l.wrapping_sub(*r))),
        (ConstValue::I32(l), ConstValue::I32(r), BinOp::SubChecked) => {
            l.checked_sub(*r)
                .map(ConstValue::I32)
                .ok_or_else(|| LowerError::InvalidLiteral("i32 underflow in const".to_string()))
        }
        (ConstValue::I32(l), ConstValue::I32(r), BinOp::Mul) => Ok(ConstValue::I32(l.wrapping_mul(*r))),
        (ConstValue::I32(l), ConstValue::I32(r), BinOp::MulChecked) => {
            l.checked_mul(*r)
                .map(ConstValue::I32)
                .ok_or_else(|| LowerError::InvalidLiteral("i32 overflow in const".to_string()))
        }

        // Bool operations.
        (ConstValue::Bool(l), ConstValue::Bool(r), BinOp::And) => Ok(ConstValue::Bool(*l && *r)),
        (ConstValue::Bool(l), ConstValue::Bool(r), BinOp::Or) => Ok(ConstValue::Bool(*l || *r)),

        _ => Err(LowerError::NotImplemented(format!(
            "const binop {:?} on {:?} and {:?}",
            op, lhs, rhs
        ))),
    }
}

/// Evaluate a unary operation on a constant value.
pub fn eval_const_unaryop(
    operand: &ConstValue,
    op: UnaryOp,
) -> Result<ConstValue, LowerError> {
    match (operand, op) {
        (ConstValue::Bool(b), UnaryOp::Not) => Ok(ConstValue::Bool(!*b)),
        (ConstValue::I32(n), UnaryOp::Neg) => Ok(ConstValue::I32(-*n)),
        (ConstValue::I64(n), UnaryOp::Neg) => Ok(ConstValue::I64(-*n)),
        (ConstValue::F32(n), UnaryOp::Neg) => Ok(ConstValue::F32(-*n)),
        (ConstValue::F64(n), UnaryOp::Neg) => Ok(ConstValue::F64(-*n)),
        _ => Err(LowerError::NotImplemented(format!(
            "const unaryop {:?} on {:?}",
            op, operand
        ))),
    }
}
