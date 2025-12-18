//! Widening arithmetic operations.
//!
//! Handles arithmetic operations with automatic type widening:
//! - Fixed-width integer operands are widened to Int (bigint) for add/sub/mul/div
//! - f32 operands use native float operations
//! - Int operands use runtime bigint operations
//!
//! Also handles unary negation for Int, and checked negation variants
//! for fixed-width integers.

use crate::ast::{BinOp, UnaryOp};

use super::{InterpContext, InterpError, Value, Destination, ValueLocation};
use super::memory::destroy_value;
use super::types::{is_int_value, is_f32_value, is_fixed_int_value, get_type_tag};
use super::alloc::{allocate_bigint, widen_fixed_int_to_int};
use super::arith::{
    write_f32_result,
    eval_add_checked, eval_sub_checked, eval_mul_checked, eval_div_checked,
    eval_add_optional, eval_sub_optional, eval_mul_optional, eval_div_optional,
    eval_comparison,
};

/// Evaluate addition with automatic widening to int.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_add<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: add and return f32.
    if is_f32_value(*lhs) && is_f32_value(*rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        return write_f32_result(ctx, a + b, dest);
    }

    // Both fixed-width ints of same type: widen to Int and add.
    if is_fixed_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let lhs_tag = get_type_tag(*lhs);
        let rhs_tag = get_type_tag(*rhs);
        if lhs_tag != rhs_tag {
            return Err(InterpError::InvalidExpression(
                format!("Type mismatch in addition: {:?} vs {:?}", lhs_tag, rhs_tag)
            ));
        }

        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let rhs_int = match widen_fixed_int_to_int(ctx, *rhs) {
            Ok(v) => v,
            Err(e) => {
                destroy_value(ctx, lhs_int);
                return Err(e);
            }
        };

        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = match allocate_bigint(ctx) {
                Ok(v) => v,
                Err(e) => {
                    destroy_value(ctx, lhs_int);
                    destroy_value(ctx, rhs_int);
                    return Err(e);
                }
            };
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs_int.ptr, lhs_int.tydesc,
                rhs_int.ptr, rhs_int.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    // Both Int: add directly.
    else if is_int_value(*lhs) && is_int_value(*rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = allocate_bigint(ctx)?;
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_ptr, result_tydesc,
            )
        };

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    // Mixed fixed-int and Int: widen fixed-int side.
    else if is_fixed_int_value(*lhs) && is_int_value(*rhs) {
        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;

        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = allocate_bigint(ctx)?;
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs_int.ptr, lhs_int.tydesc,
                rhs.ptr, rhs.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;

        let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
            (d.ptr, d.tydesc, true)
        } else {
            let result_int = allocate_bigint(ctx)?;
            (result_int.ptr, result_int.tydesc, false)
        };

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_add(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs_int.ptr, rhs_int.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value {
                ptr: result_ptr,
                tydesc: result_tydesc,
                location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
            })
        } else {
            if !is_borrowed {
                let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
                destroy_value(ctx, result_val);
            }
            Err(InterpError::RuntimeError("Int addition failed".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression(
            "Unsupported types for addition".to_string()
        ))
    }
}

/// Evaluate subtraction with automatic widening to int.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_sub<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: subtract and return f32.
    if is_f32_value(*lhs) && is_f32_value(*rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        return write_f32_result(ctx, a - b, dest);
    }

    let get_result = |ctx: &mut InterpContext<'db>| -> Result<(*mut u8, *const datalove_rt::rtdt::TyDesc, bool), InterpError> {
        if let Some(d) = dest {
            Ok((d.ptr, d.tydesc, true))
        } else {
            let v = allocate_bigint(ctx)?;
            Ok((v.ptr, v.tydesc, false))
        }
    };

    // Both fixed-width ints of same type: widen to Int and subtract.
    if is_fixed_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let lhs_tag = get_type_tag(*lhs);
        let rhs_tag = get_type_tag(*rhs);
        if lhs_tag != rhs_tag {
            return Err(InterpError::InvalidExpression(
                format!("Type mismatch in subtraction: {:?} vs {:?}", lhs_tag, rhs_tag)
            ));
        }

        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;

        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs_int.ptr, lhs_int.tydesc,
                rhs_int.ptr, rhs_int.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_int_value(*rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_ptr, result_tydesc,
            )
        };

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_fixed_int_value(*lhs) && is_int_value(*rhs) {
        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs_int.ptr, lhs_int.tydesc,
                rhs.ptr, rhs.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, lhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_sub(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs_int.ptr, rhs_int.tydesc,
                result_ptr, result_tydesc,
            )
        };

        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression("Unsupported types for subtraction".to_string()))
    }
}

/// Evaluate multiplication with automatic widening to int.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_mul<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: multiply and return f32.
    if is_f32_value(*lhs) && is_f32_value(*rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        return write_f32_result(ctx, a * b, dest);
    }

    let get_result = |ctx: &mut InterpContext<'db>| -> Result<(*mut u8, *const datalove_rt::rtdt::TyDesc, bool), InterpError> {
        if let Some(d) = dest { Ok((d.ptr, d.tydesc, true)) } else { let v = allocate_bigint(ctx)?; Ok((v.ptr, v.tydesc, false)) }
    };

    // Both fixed-width ints of same type: widen to Int and multiply.
    if is_fixed_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let lhs_tag = get_type_tag(*lhs);
        let rhs_tag = get_type_tag(*rhs);
        if lhs_tag != rhs_tag {
            return Err(InterpError::InvalidExpression(
                format!("Type mismatch in multiplication: {:?} vs {:?}", lhs_tag, rhs_tag)
            ));
        }

        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_int_value(*rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_fixed_int_value(*lhs) && is_int_value(*rhs) {
        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_mul(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression("Unsupported types for multiplication".to_string()))
    }
}

/// Evaluate division with automatic widening to int.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_div<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both f32: divide and return f32.
    if is_f32_value(*lhs) && is_f32_value(*rhs) {
        let a = unsafe { *(lhs.ptr as *const f32) };
        let b = unsafe { *(rhs.ptr as *const f32) };
        return write_f32_result(ctx, a / b, dest);
    }

    let get_result = |ctx: &mut InterpContext<'db>| -> Result<(*mut u8, *const datalove_rt::rtdt::TyDesc, bool), InterpError> {
        if let Some(d) = dest { Ok((d.ptr, d.tydesc, true)) } else { let v = allocate_bigint(ctx)?; Ok((v.ptr, v.tydesc, false)) }
    };

    // Both fixed-width ints of same type: widen to Int and divide.
    if is_fixed_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let lhs_tag = get_type_tag(*lhs);
        let rhs_tag = get_type_tag(*rhs);
        if lhs_tag != rhs_tag {
            return Err(InterpError::InvalidExpression(
                format!("Type mismatch in division: {:?} vs {:?}", lhs_tag, rhs_tag)
            ));
        }

        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;

        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_int_value(*rhs) {
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_fixed_int_value(*lhs) && is_int_value(*rhs) {
        let lhs_int = widen_fixed_int_to_int(ctx, *lhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs_int.ptr, lhs_int.tydesc, rhs.ptr, rhs.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, lhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else if is_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let rhs_int = widen_fixed_int_to_int(ctx, *rhs)?;
        let (result_ptr, result_tydesc, is_borrowed) = get_result(ctx)?;
        let status = unsafe { datalove_rt::c::dtlv_rti_int_div_checked(ctx.runtime.handle(), lhs.ptr, lhs.tydesc, rhs_int.ptr, rhs_int.tydesc, result_ptr, result_tydesc) };
        destroy_value(ctx, rhs_int);

        if status == datalove_rt::c::RtStatus::Ok {
            Ok(Value { ptr: result_ptr, tydesc: result_tydesc, location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned } })
        } else {
            if !is_borrowed { destroy_value(ctx, Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned }); }
            Err(InterpError::RuntimeError("Int division failed (possibly division by zero)".to_string()))
        }
    }
    else {
        Err(InterpError::InvalidExpression("Unsupported types for division".to_string()))
    }
}

/// Execute a binary operation.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn execute_binop<'db>(
    ctx: &mut InterpContext<'db>,
    op: BinOp,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    match op {
        // Bare operators: widen to Int.
        BinOp::Add => eval_add(ctx, lhs, rhs, dest),
        BinOp::Sub => eval_sub(ctx, lhs, rhs, dest),
        BinOp::Mul => eval_mul(ctx, lhs, rhs, dest),
        BinOp::Div => eval_div(ctx, lhs, rhs, dest),

        // Checked operators: preserve type, early-return on overflow.
        BinOp::AddChecked => eval_add_checked(ctx, lhs, rhs, dest),
        BinOp::SubChecked => eval_sub_checked(ctx, lhs, rhs, dest),
        BinOp::MulChecked => eval_mul_checked(ctx, lhs, rhs, dest),
        BinOp::DivChecked => eval_div_checked(ctx, lhs, rhs, dest),

        // Comparison operators.
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne => {
            eval_comparison(ctx, op, lhs, rhs, dest)
        }

        // Optional operators: preserve type, early-return on overflow/div0.
        BinOp::AddOptional => eval_add_optional(ctx, lhs, rhs, dest),
        BinOp::SubOptional => eval_sub_optional(ctx, lhs, rhs, dest),
        BinOp::MulOptional => eval_mul_optional(ctx, lhs, rhs, dest),
        BinOp::DivOptional => eval_div_optional(ctx, lhs, rhs, dest),
    }
}

/// Execute a unary operation.
///
/// Operand is borrowed (ref semantics) - caller manages its lifetime.
pub(super) fn execute_unop<'db>(
    ctx: &mut InterpContext<'db>,
    op: UnaryOp,
    operand: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    match op {
        UnaryOp::Neg => eval_neg(ctx, operand, dest),
        UnaryOp::NegOptional => eval_neg_optional(ctx, operand, dest),
        UnaryOp::NegResult => eval_neg_result(ctx, operand, dest),
    }
}

/// Evaluate negation for Int type.
///
/// Operand is borrowed (ref semantics) - caller manages its lifetime.
pub(super) fn eval_neg<'db>(
    ctx: &mut InterpContext<'db>,
    operand: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Only Int (bigint) supports bare negation.
    if !is_int_value(*operand) {
        return Err(InterpError::InvalidExpression(
            "Negation only supports Int type".to_string()
        ));
    }

    let (result_ptr, result_tydesc, is_borrowed) = if let Some(d) = dest {
        (d.ptr, d.tydesc, true)
    } else {
        let result_int = allocate_bigint(ctx)?;
        (result_int.ptr, result_int.tydesc, false)
    };

    let status = unsafe {
        datalove_rt::c::dtlv_rti_int_neg(
            ctx.runtime.handle(),
            operand.ptr,
            operand.tydesc,
            result_ptr,
            result_tydesc,
        )
    };

    if status == datalove_rt::c::RtStatus::Ok {
        Ok(Value {
            ptr: result_ptr,
            tydesc: result_tydesc,
            location: if is_borrowed { ValueLocation::Borrowed } else { ValueLocation::TempOwned },
        })
    } else {
        if !is_borrowed {
            let result_val = Value { ptr: result_ptr, tydesc: result_tydesc, location: ValueLocation::TempOwned };
            destroy_value(ctx, result_val);
        }
        Err(InterpError::RuntimeError("Int negation failed".to_string()))
    }
}

/// Evaluate optional negation (-?x).
///
/// Performs checked negation on signed integers.
/// Returns the raw negated value on success, or OptionNone error on overflow.
/// Operand is borrowed (ref semantics) - caller manages its lifetime.
pub(super) fn eval_neg_optional<'db>(
    ctx: &mut InterpContext<'db>,
    operand: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let type_tag = unsafe { (*operand.tydesc).type_tag };
    let operand_tydesc = operand.tydesc;

    let raw_value = unsafe { *(operand.ptr as *const u32) };

    let negated_result: Option<u32> = match type_tag {
        TyTag::I8 => {
            let val = raw_value as i8;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I16 => {
            let val = raw_value as i16;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I32 => {
            let val = raw_value as i32;
            val.checked_neg().map(|r| r as u32)
        }
        _ => {
            return Err(InterpError::InvalidExpression(
                format!("Optional negation not supported for type {:?}", type_tag)
            ));
        }
    };

    match negated_result {
        Some(result) => write_typed_int_result(ctx, result, operand_tydesc, dest),
        None => Err(InterpError::OptionNone),
    }
}

/// Evaluate result negation (-!x).
///
/// Performs checked negation on fixed-width integers.
/// Returns the raw negated value on success, or ResultErr with "overflow" on overflow.
/// Operand is borrowed (ref semantics) - caller manages its lifetime.
pub(super) fn eval_neg_result<'db>(
    ctx: &mut InterpContext<'db>,
    operand: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::TyTag;

    let type_tag = unsafe { (*operand.tydesc).type_tag };
    let operand_tydesc = operand.tydesc;

    let raw_value = unsafe { *(operand.ptr as *const u32) };

    let negated_result: Option<u32> = match type_tag {
        TyTag::I8 => {
            let val = raw_value as i8;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I16 => {
            let val = raw_value as i16;
            val.checked_neg().map(|r| (r as i32) as u32)
        }
        TyTag::I32 => {
            let val = raw_value as i32;
            val.checked_neg().map(|r| r as u32)
        }
        TyTag::U8 => {
            let val = raw_value as u8;
            val.checked_neg().map(|r| r as u32)
        }
        TyTag::U16 => {
            let val = raw_value as u16;
            val.checked_neg().map(|r| r as u32)
        }
        TyTag::U32 => {
            raw_value.checked_neg()
        }
        _ => {
            return Err(InterpError::InvalidExpression(
                format!("Result negation not supported for type {:?}", type_tag)
            ));
        }
    };

    match negated_result {
        Some(result) => write_typed_int_result(ctx, result, operand_tydesc, dest),
        None => {
            let err_string = allocate_error_string(ctx, "overflow")?;
            Err(InterpError::ResultErr {
                tydesc: err_string.tydesc,
                ptr: err_string.ptr,
            })
        }
    }
}

/// Write a typed integer result to destination or allocate new value.
///
/// Preserves the original type (i8, i16, i32, u8, u16, u32) from the tydesc.
fn write_typed_int_result(
    ctx: &mut InterpContext<'_>,
    value: u32,
    tydesc: *const datalove_rt::rtdt::TyDesc,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        let ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(ctx.runtime.handle(), tydesc, 1)
        };

        if ptr.is_null() {
            return Err(InterpError::RuntimeError("Failed to allocate integer".to_string()));
        }

        unsafe { *(ptr as *mut u32) = value; }
        Ok(Value { ptr, tydesc, location: ValueLocation::TempOwned })
    }
}

/// Allocate a string value with the given content for use as an error.
fn allocate_error_string<'db>(
    ctx: &mut InterpContext<'db>,
    content: &str,
) -> Result<Value, InterpError> {
    use crate::datalit::tycheck::Type;

    let tydesc_ptr = ctx.tydesc_table.get_or_create(&Type::String);

    let rt_handle = ctx.runtime.handle();
    let string_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, tydesc_ptr, 1)
    };

    if string_ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate error string".to_string()));
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            string_ptr,
            tydesc_ptr,
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create error string".to_string()));
    }

    if !content.is_empty() {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                string_ptr,
                tydesc_ptr,
                content.as_ptr(),
                content.len() as u32,
            )
        };

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to push error string bytes".to_string()));
        }
    }

    Ok(Value {
        ptr: string_ptr,
        tydesc: tydesc_ptr,
        location: ValueLocation::TempOwned,
    })
}
