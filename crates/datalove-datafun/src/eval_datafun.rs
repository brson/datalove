//! Evaluators for datafun expressions.

use rmx::prelude::*;
use crate::ast::*;
use crate::interp::{InterpContext, InterpResult, InterpError};
use crate::value::Value;

/// Evaluate a datafun expression.
pub fn eval_expr<'db>(ctx: &mut InterpContext<'db>, expr: ExprFun<'db>) -> InterpResult {
    match expr.expr(ctx.db) {
        ExprFunKind::Datalit(datalit_expr) => {
            crate::eval_datalit::eval_datalit(ctx, datalit_expr)
        }

        ExprFunKind::Name(name) => eval_name(ctx, name),

        ExprFunKind::BinOp(binop) => eval_binop(ctx, binop),

        ExprFunKind::ParseError(err) => {
            let message = err.message(ctx.db);
            Err(InterpError::RuntimeError(
                format!("Parse error: {}", message.as_str(ctx.db)),
            ))
        }
    }
}

/// Evaluate a name (variable reference).
fn eval_name<'db>(
    ctx: &mut InterpContext<'db>,
    name: bct::text::InternedText<'db>,
) -> InterpResult {
    // Look up the variable.
    ctx.lookup_variable(name)
        .map(|v| {
            // TODO: We need to clone the value here, but we don't have a
            // proper clone implementation yet. For now, return an error.
            Err(InterpError::NotImplemented(
                "variable reference (value cloning)".to_string(),
            ))
        })
        .unwrap_or_else(|e| Err(e))
}

/// Evaluate a binary operation.
fn eval_binop<'db>(ctx: &mut InterpContext<'db>, binop: ExprBinOp<'db>) -> InterpResult {
    let op = binop.op(ctx.db);
    let lhs = binop.lhs(ctx.db);
    let rhs = binop.rhs(ctx.db);

    // Evaluate operands.
    let lhs_value = eval_expr(ctx, lhs)?;
    let rhs_value = eval_expr(ctx, rhs)?;

    use BinOp::*;

    match op {
        // Basic arithmetic.
        Add => eval_add(ctx, lhs_value, rhs_value),
        Sub => eval_sub(ctx, lhs_value, rhs_value),
        Mul => eval_mul(ctx, lhs_value, rhs_value),
        Div => eval_div(ctx, lhs_value, rhs_value),

        // Checked arithmetic.
        AddChecked => eval_add_checked(ctx, lhs_value, rhs_value),
        SubChecked => eval_sub_checked(ctx, lhs_value, rhs_value),
        MulChecked => eval_mul_checked(ctx, lhs_value, rhs_value),
        DivChecked => eval_div_checked(ctx, lhs_value, rhs_value),

        // Optional arithmetic.
        AddOptional => eval_add_optional(ctx, lhs_value, rhs_value),
        SubOptional => eval_sub_optional(ctx, lhs_value, rhs_value),
        MulOptional => eval_mul_optional(ctx, lhs_value, rhs_value),
        DivOptional => eval_div_optional(ctx, lhs_value, rhs_value),

        // Saturating arithmetic.
        AddSaturating => eval_add_saturating(ctx, lhs_value, rhs_value),
        SubSaturating => eval_sub_saturating(ctx, lhs_value, rhs_value),
        MulSaturating => eval_mul_saturating(ctx, lhs_value, rhs_value),
        DivSaturating => eval_div_saturating(ctx, lhs_value, rhs_value),

        // Comparison operators.
        Lt => eval_lt(ctx, lhs_value, rhs_value),
        Gt => eval_gt(ctx, lhs_value, rhs_value),
        Le => eval_le(ctx, lhs_value, rhs_value),
        Ge => eval_ge(ctx, lhs_value, rhs_value),
        Eq => eval_eq(ctx, lhs_value, rhs_value),
        Ne => eval_ne(ctx, lhs_value, rhs_value),
    }
}

/// Evaluate addition.
fn eval_add(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping addition.
            Ok(Value::from_u32(a.wrapping_add(b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a + b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for addition".to_string(),
        )),
    }
}

/// Evaluate subtraction.
fn eval_sub(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping subtraction.
            Ok(Value::from_u32(a.wrapping_sub(b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a - b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for subtraction".to_string(),
        )),
    }
}

/// Evaluate multiplication.
fn eval_mul(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping multiplication.
            Ok(Value::from_u32(a.wrapping_mul(b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a * b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for multiplication".to_string(),
        )),
    }
}

/// Evaluate division.
fn eval_div(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            if b == 0 {
                Err(InterpError::DivisionByZero)
            } else {
                Ok(Value::from_u32(a / b))
            }
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a / b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for division".to_string(),
        )),
    }
}

/// Evaluate checked addition.
fn eval_add_checked(ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Return Result<u32>.
            match a.checked_add(b) {
                Some(result) => {
                    // TODO: Create Result::Ok value.
                    Err(InterpError::NotImplemented("Result type".to_string()))
                }
                None => {
                    // TODO: Create Result::Err value.
                    Err(InterpError::NotImplemented("Result type".to_string()))
                }
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for checked addition".to_string(),
        )),
    }
}

/// Evaluate checked subtraction.
fn eval_sub_checked(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("checked subtraction".to_string()))
}

/// Evaluate checked multiplication.
fn eval_mul_checked(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("checked multiplication".to_string()))
}

/// Evaluate checked division.
fn eval_div_checked(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("checked division".to_string()))
}

/// Evaluate optional addition.
fn eval_add_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional addition".to_string()))
}

/// Evaluate optional subtraction.
fn eval_sub_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional subtraction".to_string()))
}

/// Evaluate optional multiplication.
fn eval_mul_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional multiplication".to_string()))
}

/// Evaluate optional division.
fn eval_div_optional(_ctx: &mut InterpContext<'_>, _lhs: Value, _rhs: Value) -> InterpResult {
    Err(InterpError::NotImplemented("optional division".to_string()))
}

/// Evaluate saturating addition.
fn eval_add_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_u32(a.saturating_add(b))),
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating addition".to_string(),
        )),
    }
}

/// Evaluate saturating subtraction.
fn eval_sub_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_u32(a.saturating_sub(b))),
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating subtraction".to_string(),
        )),
    }
}

/// Evaluate saturating multiplication.
fn eval_mul_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_u32(a.saturating_mul(b))),
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating multiplication".to_string(),
        )),
    }
}

/// Evaluate saturating division.
fn eval_div_saturating(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            if b == 0 {
                Err(InterpError::DivisionByZero)
            } else {
                Ok(Value::from_u32(a / b))
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for saturating division".to_string(),
        )),
    }
}

/// Evaluate less than.
fn eval_lt(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a < b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a < b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate greater than.
fn eval_gt(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a > b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a > b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate less than or equal.
fn eval_le(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a <= b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a <= b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate greater than or equal.
fn eval_ge(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a >= b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a >= b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    }
}

/// Evaluate equality.
fn eval_eq(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::from_bool(a == b)),
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a == b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a == b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for equality".to_string(),
        )),
    }
}

/// Evaluate inequality.
fn eval_ne(_ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value) -> InterpResult {
    match (lhs, rhs) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::from_bool(a != b)),
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a != b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a != b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for inequality".to_string(),
        )),
    }
}
