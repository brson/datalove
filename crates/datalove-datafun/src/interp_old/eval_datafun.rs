//! Evaluators for datafun expressions.

use rmx::prelude::*;
use std::collections::HashMap;
use crate::ast::*;
use crate::interp_old::interp::{InterpContext, InterpResult, InterpError};
use crate::interp_old::value::Value;
use datalove_rt::{self as rt, c::{LocalRtHandle, RtStatus}};

/// Evaluate a datafun expression.
pub fn eval_expr<'db>(ctx: &mut InterpContext<'db>, expr: ExprFun<'db>) -> InterpResult {
    eval_expr_with_expected(ctx, expr, None)
}

/// Evaluate a datafun expression with an optional expected type.
pub fn eval_expr_with_expected<'db>(
    ctx: &mut InterpContext<'db>,
    expr: ExprFun<'db>,
    expected: Option<crate::datalit::tycheck::TypeAndHeap<'db>>,
) -> InterpResult {
    match expr.expr(ctx.db) {
        ExprFunKind::Datalit(datalit_expr) => {
            crate::interp_old::eval_datalit::eval_datalit(ctx, datalit_expr, expected)
        }

        ExprFunKind::Name(name) => eval_name(ctx, name),

        ExprFunKind::BinOp(binop) => eval_binop(ctx, expr, binop),

        ExprFunKind::UnaryOp(unaryop) => eval_unaryop(ctx, expr, unaryop),

        ExprFunKind::FunctionCall(call) => eval_function_call(ctx, call),

        ExprFunKind::Tuple(tuple) => eval_tuple(ctx, expr, tuple),

        ExprFunKind::TryOption(try_op) => eval_try_option(ctx, try_op),

        ExprFunKind::TryResult(try_op) => eval_try_result(ctx, try_op),

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
    use datalove_rt as rt;

    // Look up the variable and extract the necessary information.
    // We need to do this in two stages to avoid borrow checker issues.

    // Stage 1: Determine the value type and extract ptr/tydesc if needed.
    enum ValueCloneInfo {
        Bool(bool),
        U32(u32),
        F32(f32),
        HeapAllocated {
            ptr: *const u8,
            tydesc: *const datalove_rtdt::TyDesc,
            kind: HeapValueKind,
        },
        Unimplemented,
    }

    #[derive(Debug)]
    enum HeapValueKind {
        Int,
        String,
        Tuple,
        Struct,
        Enum,
        List,
        Map,
        Set,
        Option,
        Result,
    }

    let value_info = {
        let value = ctx.lookup_variable(name)?;

        match value {
            Value::Bool(b) => ValueCloneInfo::Bool(*b),
            Value::U32(n) => ValueCloneInfo::U32(*n),
            Value::F32(f) => ValueCloneInfo::F32(*f),

            Value::Int { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Int,
            },

            Value::String { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::String,
            },

            Value::Tuple { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Tuple,
            },

            Value::Struct { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Struct,
            },

            Value::Enum { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Enum,
            },

            Value::List { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::List,
            },

            Value::Map { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Map,
            },

            Value::Set { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Set,
            },

            Value::Option { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Option,
            },

            Value::Result { ptr, tydesc } => ValueCloneInfo::HeapAllocated {
                ptr: *ptr as *const u8,
                tydesc: *tydesc,
                kind: HeapValueKind::Result,
            },

            Value::Data { .. } | Value::Error { .. } => ValueCloneInfo::Unimplemented,
        }
    };

    // Stage 2: Clone the value.
    match value_info {
        ValueCloneInfo::Bool(b) => Ok(Value::Bool(b)),
        ValueCloneInfo::U32(n) => Ok(Value::U32(n)),
        ValueCloneInfo::F32(f) => Ok(Value::F32(f)),

        ValueCloneInfo::HeapAllocated { ptr, tydesc, kind } => {
            // Allocate new value based on kind.
            let mut new_value = unsafe {
                match kind {
                    HeapValueKind::Int => Value::alloc_int(&mut ctx.rt, tydesc),
                    HeapValueKind::String => Value::alloc_string(&mut ctx.rt, tydesc),
                    HeapValueKind::Tuple => Value::alloc_tuple(&mut ctx.rt, tydesc),
                    HeapValueKind::Struct => Value::alloc_struct(&mut ctx.rt, tydesc),
                    HeapValueKind::Enum => Value::alloc_enum(&mut ctx.rt, tydesc),
                    HeapValueKind::List => Value::alloc_list(&mut ctx.rt, tydesc),
                    HeapValueKind::Map => Value::alloc_map(&mut ctx.rt, tydesc),
                    HeapValueKind::Set => Value::alloc_set(&mut ctx.rt, tydesc),
                    HeapValueKind::Option => Value::alloc_option(&mut ctx.rt, tydesc),
                    HeapValueKind::Result => Value::alloc_result(&mut ctx.rt, tydesc),
                }
            };

            // Get handle to runtime.
            let rt_handle = Box::as_mut(&mut ctx.rt) as *mut _ as rt::c::LocalRtHandle;

            // Clone the value.
            let status = unsafe {
                rt::c::dtlv_rti_clone_local(
                    rt_handle,
                    ptr,
                    tydesc,
                    new_value.as_mut_ptr(),
                )
            };

            if status != rt::c::RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    format!("Failed to clone {:?} value", kind),
                ));
            }

            Ok(new_value)
        }

        ValueCloneInfo::Unimplemented => {
            Err(InterpError::NotImplemented(
                "cloning Data/Error values".to_string(),
            ))
        }
    }
}

/// Evaluate a binary operation.
fn eval_binop<'db>(ctx: &mut InterpContext<'db>, expr: ExprFun<'db>, binop: ExprBinOp<'db>) -> InterpResult {
    let op = binop.op(ctx.db);
    let lhs = binop.lhs(ctx.db);
    let rhs = binop.rhs(ctx.db);

    // Evaluate operands.
    let lhs_value = eval_expr(ctx, lhs)?;
    let rhs_value = eval_expr(ctx, rhs)?;

    // For checked/optional operators, get the operand type descriptor and construct
    // the Result<T>/Option<T> type descriptor from it.
    // NOTE: We can't get this from the type table because BinOp types aren't stored there
    // (to avoid salsa context issues). Instead, we construct it on-demand from the operand type.
    let lhs_tydesc = get_expr_tydesc(ctx, lhs);

    use BinOp::*;

    match op {
        // Basic arithmetic.
        Add => eval_add(ctx, lhs_value, rhs_value),
        Sub => eval_sub(ctx, lhs_value, rhs_value),
        Mul => eval_mul(ctx, lhs_value, rhs_value),
        Div => eval_div(ctx, lhs_value, rhs_value),

        // Checked arithmetic.
        AddChecked => eval_add_checked(ctx, lhs_value, rhs_value, lhs_tydesc),
        SubChecked => eval_sub_checked(ctx, lhs_value, rhs_value, lhs_tydesc),
        MulChecked => eval_mul_checked(ctx, lhs_value, rhs_value, lhs_tydesc),
        DivChecked => eval_div_checked(ctx, lhs_value, rhs_value, lhs_tydesc),

        // Optional arithmetic.
        AddOptional => eval_add_optional(ctx, lhs_value, rhs_value, lhs_tydesc),
        SubOptional => eval_sub_optional(ctx, lhs_value, rhs_value, lhs_tydesc),
        MulOptional => eval_mul_optional(ctx, lhs_value, rhs_value, lhs_tydesc),
        DivOptional => eval_div_optional(ctx, lhs_value, rhs_value, lhs_tydesc),

        // Comparison operators.
        Lt => eval_lt(ctx, lhs_value, rhs_value),
        Gt => eval_gt(ctx, lhs_value, rhs_value),
        Le => eval_le(ctx, lhs_value, rhs_value),
        Ge => eval_ge(ctx, lhs_value, rhs_value),
        Eq => eval_eq(ctx, lhs_value, rhs_value),
        Ne => eval_ne(ctx, lhs_value, rhs_value),
    }
}

/// Evaluate unary operation.
fn eval_unaryop<'db>(ctx: &mut InterpContext<'db>, expr: ExprFun<'db>, unaryop: ExprUnaryOp<'db>) -> InterpResult {
    let op = unaryop.op(ctx.db);
    let operand = unaryop.operand(ctx.db);

    // Evaluate operand.
    let operand_value = eval_expr(ctx, operand)?;

    // Get the type descriptor for the unary operation expression.
    let result_tydesc = ctx.type_table.get_datafun_expr_type(expr);

    use UnaryOp::*;

    match op {
        // Bare negation (for bigints).
        Neg => eval_neg(ctx, operand_value),

        // Optional negation.
        NegOptional => eval_neg_optional(ctx, operand_value, result_tydesc),

        // Result negation.
        NegResult => eval_neg_result(ctx, operand_value, result_tydesc),
    }
}

/// Evaluate bare negation (for bigints).
fn eval_neg(ctx: &mut InterpContext<'_>, mut operand: Value) -> InterpResult {
    let result = match &operand {
        Value::Int { ptr, tydesc } => {
            // Allocate result Int.
            let result = unsafe { Value::alloc_int(&mut ctx.rt, *tydesc) };
            if let Value::Int { ptr: result_ptr, tydesc: result_tydesc } = result {
                let status = unsafe {
                    rt::c::dtlv_rti_int_neg(
                        Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                        *ptr as *const u8,
                        *tydesc,
                        result_ptr as *mut u8,
                        result_tydesc,
                    )
                };
                if status == RtStatus::Ok {
                    Ok(result)
                } else {
                    Err(InterpError::RuntimeError("Int negation failed".to_string()))
                }
            } else {
                Err(InterpError::RuntimeError("Failed to allocate Int".to_string()))
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported type for bare negation".to_string(),
        )),
    };

    // Free consumed operand.
    unsafe {
        operand.free(&mut ctx.rt);
    }

    result
}

/// Evaluate optional negation.
fn eval_neg_optional(ctx: &mut InterpContext<'_>, operand: Value, operand_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    use datalove_rtdt as rtdt;

    match operand {
        Value::U32(u) => {
            // Default to I32 if type descriptor is null (for backward compatibility).
            let (tytag, option_tydesc) = if operand_tydesc.is_null() {
                (rtdt::TyTag::I32, make_option_tydesc_i32(ctx))
            } else {
                // operand_tydesc is Option<T>, extract T
                let inner_tydesc = unsafe { (*operand_tydesc).type_info.option.inner_tydesc };
                (unsafe { (*inner_tydesc).type_tag }, operand_tydesc)
            };

            match tytag {
                rtdt::TyTag::I8 => {
                    let a = (u as i32) as i8;
                    match a.checked_neg() {
                        Some(result) => create_option_some(ctx, Value::U32((result as i32) as u32), option_tydesc),
                        None => create_option_none(ctx, option_tydesc),
                    }
                }
                rtdt::TyTag::I16 => {
                    let a = (u as i32) as i16;
                    match a.checked_neg() {
                        Some(result) => create_option_some(ctx, Value::U32((result as i32) as u32), option_tydesc),
                        None => create_option_none(ctx, option_tydesc),
                    }
                }
                rtdt::TyTag::I32 => {
                    let a = u as i32;
                    match a.checked_neg() {
                        Some(result) => create_option_some(ctx, Value::U32(result as u32), option_tydesc),
                        None => create_option_none(ctx, option_tydesc),
                    }
                }
                _ => Err(InterpError::TypeError(format!("Unsupported type {:?} for optional negation (only signed integers supported)", tytag))),
            }
        }
        _ => Err(InterpError::TypeError("Unsupported type for optional negation".to_string())),
    }
}

/// Evaluate result negation.
fn eval_neg_result(ctx: &mut InterpContext<'_>, operand: Value, operand_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    use datalove_rtdt as rtdt;

    match operand {
        Value::U32(u) => {
            // Default to I32 if type descriptor is null (for backward compatibility).
            let (tytag, result_tydesc) = if operand_tydesc.is_null() {
                (rtdt::TyTag::I32, make_result_tydesc_i32(ctx))
            } else {
                // operand_tydesc is Result<T>, extract T
                let inner_tydesc = unsafe { (*operand_tydesc).type_info.result.ok_tydesc };
                (unsafe { (*inner_tydesc).type_tag }, operand_tydesc)
            };

            match tytag {
                rtdt::TyTag::I8 => {
                    let a = (u as i32) as i8;
                    match a.checked_neg() {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I16 => {
                    let a = (u as i32) as i16;
                    match a.checked_neg() {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I32 => {
                    let a = u as i32;
                    match a.checked_neg() {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                _ => Err(InterpError::TypeError(format!("Unsupported type {:?} for result negation (only signed integers supported)", tytag))),
            }
        }
        _ => Err(InterpError::TypeError("Unsupported type for result negation".to_string())),
    }
}

/// Evaluate addition.
fn eval_add(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping addition.
            Ok(Value::from_u32(a.wrapping_add(*b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a + b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            // Allocate result Int.
            let mut result = unsafe { Value::alloc_int(&mut ctx.rt, *a_tydesc) };
            if let Value::Int { ptr: result_ptr, tydesc: result_tydesc } = result {
                let status = unsafe {
                    rt::c::dtlv_rti_int_add(
                        Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                        *a_ptr as *const u8,
                        *a_tydesc,
                        *b_ptr as *const u8,
                        *b_tydesc,
                        result_ptr as *mut u8,
                        result_tydesc,
                    )
                };
                if status == RtStatus::Ok {
                    Ok(result)
                } else {
                    unsafe { result.free(&mut ctx.rt); }
                    Err(InterpError::RuntimeError("Int addition failed".to_string()))
                }
            } else {
                Err(InterpError::RuntimeError("Failed to allocate Int".to_string()))
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for addition".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate subtraction.
fn eval_sub(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping subtraction.
            Ok(Value::from_u32(a.wrapping_sub(*b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a - b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            // Allocate result Int.
            let mut result = unsafe { Value::alloc_int(&mut ctx.rt, *a_tydesc) };
            if let Value::Int { ptr: result_ptr, tydesc: result_tydesc } = result {
                let status = unsafe {
                    rt::c::dtlv_rti_int_sub(
                        Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                        *a_ptr as *const u8,
                        *a_tydesc,
                        *b_ptr as *const u8,
                        *b_tydesc,
                        result_ptr as *mut u8,
                        result_tydesc,
                    )
                };
                if status == RtStatus::Ok {
                    Ok(result)
                } else {
                    unsafe { result.free(&mut ctx.rt); }
                    Err(InterpError::RuntimeError("Int subtraction failed".to_string()))
                }
            } else {
                Err(InterpError::RuntimeError("Failed to allocate Int".to_string()))
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for subtraction".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate multiplication.
fn eval_mul(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // Wrapping multiplication.
            Ok(Value::from_u32(a.wrapping_mul(*b)))
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a * b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            // Allocate result Int.
            let mut result = unsafe { Value::alloc_int(&mut ctx.rt, *a_tydesc) };
            if let Value::Int { ptr: result_ptr, tydesc: result_tydesc } = result {
                let status = unsafe {
                    rt::c::dtlv_rti_int_mul(
                        Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                        *a_ptr as *const u8,
                        *a_tydesc,
                        *b_ptr as *const u8,
                        *b_tydesc,
                        result_ptr as *mut u8,
                        result_tydesc,
                    )
                };
                if status == RtStatus::Ok {
                    Ok(result)
                } else {
                    unsafe { result.free(&mut ctx.rt); }
                    Err(InterpError::RuntimeError("Int multiplication failed".to_string()))
                }
            } else {
                Err(InterpError::RuntimeError("Failed to allocate Int".to_string()))
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for multiplication".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate division.
fn eval_div(_ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            if *b == 0 {
                Err(InterpError::DivisionByZero)
            } else {
                Ok(Value::from_u32(a / b))
            }
        }
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_f32(a / b)),
        _ => Err(InterpError::TypeError(
            "Unsupported types for division".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut _ctx.rt);
        rhs.free(&mut _ctx.rt);
    }

    result
}

/// Evaluate checked addition.
fn eval_add_checked(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value, inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    use datalove_rtdt as rtdt;

    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // All expression types should be populated in the type table.
            let tytag = unsafe { (*inner_tydesc).type_tag };

            match tytag {
                rtdt::TyTag::U8 => {
                    let result_tydesc = make_result_tydesc_u8(ctx);
                    let a = *a as u8;
                    let b = *b as u8;
                    match a.checked_add(b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I8 => {
                    let result_tydesc = make_result_tydesc_i8(ctx);
                    let a = (*a as i32) as i8;
                    let b = (*b as i32) as i8;
                    match a.checked_add(b) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U16 => {
                    let result_tydesc = make_result_tydesc_u16(ctx);
                    let a = *a as u16;
                    let b = *b as u16;
                    match a.checked_add(b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I16 => {
                    let result_tydesc = make_result_tydesc_i16(ctx);
                    let a = (*a as i32) as i16;
                    let b = (*b as i32) as i16;
                    match a.checked_add(b) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I32 => {
                    let result_tydesc = make_result_tydesc_i32(ctx);
                    let a = *a as i32;
                    let b = *b as i32;
                    match a.checked_add(b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U32 => {
                    let result_tydesc = make_result_tydesc_u32(ctx);
                    match a.checked_add(*b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                _ => Err(InterpError::TypeError(
                    format!("Unsupported type {:?} for checked addition", tytag),
                )),
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for checked addition".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate checked subtraction.
fn eval_sub_checked(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value, inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    use datalove_rtdt as rtdt;

    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // All expression types should be populated in the type table.
            let tytag = unsafe { (*inner_tydesc).type_tag };

            match tytag {
                rtdt::TyTag::U8 => {
                    let result_tydesc = make_result_tydesc_u8(ctx);
                    let a = *a as u8;
                    let b = *b as u8;
                    match a.checked_sub(b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I8 => {
                    let result_tydesc = make_result_tydesc_i8(ctx);
                    let a = (*a as i32) as i8;
                    let b = (*b as i32) as i8;
                    match a.checked_sub(b) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U16 => {
                    let result_tydesc = make_result_tydesc_u16(ctx);
                    let a = *a as u16;
                    let b = *b as u16;
                    match a.checked_sub(b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I16 => {
                    let result_tydesc = make_result_tydesc_i16(ctx);
                    let a = (*a as i32) as i16;
                    let b = (*b as i32) as i16;
                    match a.checked_sub(b) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I32 => {
                    let result_tydesc = make_result_tydesc_i32(ctx);
                    let a = *a as i32;
                    let b = *b as i32;
                    match a.checked_sub(b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U32 => {
                    let result_tydesc = make_result_tydesc_u32(ctx);
                    match a.checked_sub(*b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                _ => Err(InterpError::TypeError(
                    format!("Unsupported type {:?} for checked subtraction", tytag),
                )),
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for checked subtraction".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate checked multiplication.
fn eval_mul_checked(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value, inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    use datalove_rtdt as rtdt;

    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // All expression types should be populated in the type table.
            let tytag = unsafe { (*inner_tydesc).type_tag };
            match tytag {
                rtdt::TyTag::U8 => {
                    let result_tydesc = make_result_tydesc_u8(ctx);
                    match (*a as u8).checked_mul(*b as u8) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I8 => {
                    let result_tydesc = make_result_tydesc_i8(ctx);
                    match ((*a as i32) as i8).checked_mul((*b as i32) as i8) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U16 => {
                    let result_tydesc = make_result_tydesc_u16(ctx);
                    match (*a as u16).checked_mul(*b as u16) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I16 => {
                    let result_tydesc = make_result_tydesc_i16(ctx);
                    match ((*a as i32) as i16).checked_mul((*b as i32) as i16) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I32 => {
                    let result_tydesc = make_result_tydesc_i32(ctx);
                    match (*a as i32).checked_mul(*b as i32) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U32 => {
                    let result_tydesc = make_result_tydesc_u32(ctx);
                    match a.checked_mul(*b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result), result_tydesc),
                        None => create_result_overflow_err(ctx, result_tydesc),
                    }
                }
                _ => Err(InterpError::TypeError(format!("Unsupported type {:?} for checked multiplication", tytag))),
            }
        }
        _ => Err(InterpError::TypeError("Unsupported types for checked multiplication".to_string())),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate checked division.
fn eval_div_checked(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value, inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    use datalove_rtdt as rtdt;

    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => {
            // All expression types should be populated in the type table.
            let tytag = unsafe { (*inner_tydesc).type_tag };
            match tytag {
                rtdt::TyTag::U8 => {
                    let result_tydesc = make_result_tydesc_u8(ctx);
                    match (*a as u8).checked_div(*b as u8) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_divzero_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I8 => {
                    let result_tydesc = make_result_tydesc_i8(ctx);
                    match ((*a as i32) as i8).checked_div((*b as i32) as i8) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_divzero_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U16 => {
                    let result_tydesc = make_result_tydesc_u16(ctx);
                    match (*a as u16).checked_div(*b as u16) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_divzero_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I16 => {
                    let result_tydesc = make_result_tydesc_i16(ctx);
                    match ((*a as i32) as i16).checked_div((*b as i32) as i16) {
                        Some(result) => create_result_ok(ctx, Value::U32((result as i32) as u32), result_tydesc),
                        None => create_result_divzero_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::I32 => {
                    let result_tydesc = make_result_tydesc_i32(ctx);
                    match (*a as i32).checked_div(*b as i32) {
                        Some(result) => create_result_ok(ctx, Value::U32(result as u32), result_tydesc),
                        None => create_result_divzero_err(ctx, result_tydesc),
                    }
                }
                rtdt::TyTag::U32 => {
                    let result_tydesc = make_result_tydesc_u32(ctx);
                    match a.checked_div(*b) {
                        Some(result) => create_result_ok(ctx, Value::U32(result), result_tydesc),
                        None => create_result_divzero_err(ctx, result_tydesc),
                    }
                }
                _ => Err(InterpError::TypeError(format!("Unsupported type {:?} for checked division", tytag))),
            }
        }
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let result_result_tydesc = make_result_tydesc_int(ctx);

            // Allocate result Int.
            let mut result = unsafe { Value::alloc_int(&mut ctx.rt, *a_tydesc) };
            if let Value::Int { ptr: result_ptr, tydesc: result_tydesc } = result {
                let status = unsafe {
                    rt::c::dtlv_rti_int_div_checked(
                        Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                        *a_ptr as *const u8,
                        *a_tydesc,
                        *b_ptr as *const u8,
                        *b_tydesc,
                        result_ptr as *mut u8,
                        result_tydesc,
                    )
                };
                if status == RtStatus::Ok {
                    create_result_ok(ctx, result, result_result_tydesc)
                } else {
                    // RT function returns Error for division by zero.
                    // Free the allocated result before returning error.
                    unsafe { result.free(&mut ctx.rt); }
                    create_result_divzero_err(ctx, result_result_tydesc)
                }
            } else {
                Err(InterpError::RuntimeError("Failed to allocate Int".to_string()))
            }
        }
        _ => Err(InterpError::TypeError("Unsupported types for checked division".to_string())),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate optional addition.
fn eval_add_optional(ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value, _inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            let option_tydesc = make_option_tydesc_u32(ctx);
            match a.checked_add(b) {
                Some(result) => {
                    create_option_some(ctx, Value::U32(result), option_tydesc)
                }
                None => {
                    create_option_none(ctx, option_tydesc)
                }
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for optional addition".to_string(),
        )),
    }
}

/// Evaluate optional subtraction.
fn eval_sub_optional(ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value, _inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            let option_tydesc = make_option_tydesc_u32(ctx);
            match a.checked_sub(b) {
                Some(result) => {
                    create_option_some(ctx, Value::U32(result), option_tydesc)
                }
                None => {
                    create_option_none(ctx, option_tydesc)
                }
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for optional subtraction".to_string(),
        )),
    }
}

/// Evaluate optional multiplication.
fn eval_mul_optional(ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value, _inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            let option_tydesc = make_option_tydesc_u32(ctx);
            match a.checked_mul(b) {
                Some(result) => {
                    create_option_some(ctx, Value::U32(result), option_tydesc)
                }
                None => {
                    create_option_none(ctx, option_tydesc)
                }
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for optional multiplication".to_string(),
        )),
    }
}

/// Evaluate optional division.
fn eval_div_optional(ctx: &mut InterpContext<'_>, lhs: Value, rhs: Value, _inner_tydesc: *const datalove_rtdt::TyDesc) -> InterpResult {
    match (lhs, rhs) {
        (Value::U32(a), Value::U32(b)) => {
            let option_tydesc = make_option_tydesc_u32(ctx);
            match a.checked_div(b) {
                Some(result) => {
                    create_option_some(ctx, Value::U32(result), option_tydesc)
                }
                None => {
                    create_option_none(ctx, option_tydesc)
                }
            }
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for optional division".to_string(),
        )),
    }
}

/// Evaluate less than.
fn eval_lt(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a < b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a < b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let cmp_result = unsafe {
                rt::c::dtlv_rti_int_cmp(
                    Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                    *a_ptr as *const u8,
                    *a_tydesc,
                    *b_ptr as *const u8,
                    *b_tydesc,
                )
            };
            Ok(Value::from_bool(cmp_result < 0))
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate greater than.
fn eval_gt(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a > b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a > b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let cmp_result = unsafe {
                rt::c::dtlv_rti_int_cmp(
                    Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                    *a_ptr as *const u8,
                    *a_tydesc,
                    *b_ptr as *const u8,
                    *b_tydesc,
                )
            };
            Ok(Value::from_bool(cmp_result > 0))
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate less than or equal.
fn eval_le(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a <= b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a <= b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let cmp_result = unsafe {
                rt::c::dtlv_rti_int_cmp(
                    Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                    *a_ptr as *const u8,
                    *a_tydesc,
                    *b_ptr as *const u8,
                    *b_tydesc,
                )
            };
            Ok(Value::from_bool(cmp_result <= 0))
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate greater than or equal.
fn eval_ge(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a >= b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a >= b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let cmp_result = unsafe {
                rt::c::dtlv_rti_int_cmp(
                    Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                    *a_ptr as *const u8,
                    *a_tydesc,
                    *b_ptr as *const u8,
                    *b_tydesc,
                )
            };
            Ok(Value::from_bool(cmp_result >= 0))
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for comparison".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate equality.
fn eval_eq(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::from_bool(a == b)),
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a == b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a == b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let cmp_result = unsafe {
                rt::c::dtlv_rti_int_cmp(
                    Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                    *a_ptr as *const u8,
                    *a_tydesc,
                    *b_ptr as *const u8,
                    *b_tydesc,
                )
            };
            Ok(Value::from_bool(cmp_result == 0))
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for equality".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate inequality.
fn eval_ne(ctx: &mut InterpContext<'_>, mut lhs: Value, mut rhs: Value) -> InterpResult {
    let result = match (&lhs, &rhs) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::from_bool(a != b)),
        (Value::U32(a), Value::U32(b)) => Ok(Value::from_bool(a != b)),
        (Value::F32(a), Value::F32(b)) => Ok(Value::from_bool(a != b)),
        (Value::Int { ptr: a_ptr, tydesc: a_tydesc }, Value::Int { ptr: b_ptr, tydesc: b_tydesc }) => {
            let cmp_result = unsafe {
                rt::c::dtlv_rti_int_cmp(
                    Box::as_mut(&mut ctx.rt) as *mut _ as LocalRtHandle,
                    *a_ptr as *const u8,
                    *a_tydesc,
                    *b_ptr as *const u8,
                    *b_tydesc,
                )
            };
            Ok(Value::from_bool(cmp_result != 0))
        }
        _ => Err(InterpError::TypeError(
            "Unsupported types for inequality".to_string(),
        )),
    };

    // Free consumed operands.
    unsafe {
        lhs.free(&mut ctx.rt);
        rhs.free(&mut ctx.rt);
    }

    result
}

/// Evaluate a try-option operator (?).
/// If the operand is None, returns early with ReturnNone.
/// If the operand is Some(value), returns the unwrapped value.
fn eval_try_option<'db>(
    ctx: &mut InterpContext<'db>,
    try_op: crate::ast::ExprTryOption<'db>,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    let operand = try_op.operand(ctx.db);
    let mut operand_value = eval_expr(ctx, operand)?;

    // The operand must be an Option value.
    let (ptr, tydesc) = match operand_value {
        Value::Option { ptr, tydesc } => (ptr, tydesc),
        _ => {
            return Err(InterpError::TypeError(
                "Try-option operator (?) requires Option type".to_string(),
            ));
        }
    };

    // Read the OptionTag.
    let tag = unsafe { *(ptr as *const u8) as u8 };
    let option_tag = if tag == rtdt::OptionTag::Some as u8 {
        rtdt::OptionTag::Some
    } else {
        rtdt::OptionTag::None
    };

    let result = match option_tag {
        rtdt::OptionTag::None => {
            // Early return with None.
            Err(InterpError::ReturnNone)
        }
        rtdt::OptionTag::Some => {
            // Extract the Some value using the same pattern as if-destructuring.
            let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(tydesc)) };
            let payload_ptr = unsafe { ptr.add(layout.payload_offset as usize) };

            // Get the inner type descriptor.
            let inner_tydesc = unsafe { (*tydesc).type_info.option.inner_tydesc };

            // Create a Value for the payload (clones it).
            InterpContext::value_from_ptr(&mut ctx.rt, payload_ptr, inner_tydesc)
        }
    };

    // Free the operand value.
    unsafe { operand_value.free(&mut ctx.rt); }

    result
}

/// Evaluate a try-result operator (!).
/// If the operand is Err, returns early with ReturnError.
/// If the operand is Ok(value), returns the unwrapped value.
fn eval_try_result<'db>(
    ctx: &mut InterpContext<'db>,
    try_op: crate::ast::ExprTryResult<'db>,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    let operand = try_op.operand(ctx.db);
    let mut operand_value = eval_expr(ctx, operand)?;

    // The operand must be a Result value.
    let (ptr, tydesc) = match operand_value {
        Value::Result { ptr, tydesc } => (ptr, tydesc),
        _ => {
            return Err(InterpError::TypeError(
                "Try-result operator (!) requires Result type".to_string(),
            ));
        }
    };

    // Read the ResultTag.
    let tag = unsafe { *(ptr as *const u8) as u8 };
    let result_tag = if tag == rtdt::ResultTag::Ok as u8 {
        rtdt::ResultTag::Ok
    } else {
        rtdt::ResultTag::Err
    };

    match result_tag {
        rtdt::ResultTag::Err => {
            // Extract the Error value and return early.
            let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(tydesc)) };
            let payload_ptr = unsafe { ptr.add(layout.payload_offset as usize) };

            // The error payload is of type Error (same layout as Data).
            // Error contains tydesc + value pointer.
            let error_ptr = payload_ptr as *const rtdt::Error;
            let error_tydesc = unsafe { (*error_ptr).tydesc() };
            let error_value_ptr = unsafe { (*error_ptr).value_ptr() };

            // Create a Value for the error (clones it).
            let error_value = InterpContext::value_from_ptr(&mut ctx.rt, error_value_ptr, error_tydesc)?;

            // TODO: Free the Result value - but this causes issues with Error payloads.
            // The Error contains a pointer that needs special handling.

            Err(InterpError::ReturnError(error_value))
        }
        rtdt::ResultTag::Ok => {
            // Extract the Ok value using the same pattern as if-destructuring.
            let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(tydesc)) };
            let payload_ptr = unsafe { ptr.add(layout.payload_offset as usize) };

            // Get the Ok type descriptor.
            let ok_tydesc = unsafe { (*tydesc).type_info.result.ok_tydesc };

            // Create a Value for the payload (clones it).
            let result = InterpContext::value_from_ptr(&mut ctx.rt, payload_ptr, ok_tydesc)?;

            // Free the Result value now that we've extracted the Ok payload.
            unsafe { operand_value.free(&mut ctx.rt); }

            Ok(result)
        }
    }
}

/// Evaluate a function call.
fn eval_function_call<'db>(
    ctx: &mut InterpContext<'db>,
    call: crate::ast::ExprFunctionCall<'db>,
) -> InterpResult {
    let name = call.name(ctx.db);
    let args = call.args(ctx.db);

    // Check stack depth to prevent overflow.
    if ctx.call_depth() >= crate::interp_old::interp::MAX_CALL_DEPTH {
        return Err(InterpError::StackOverflow);
    }

    // Look up the function definition from current frame.
    let func = ctx.lookup_function(name)
        .ok_or_else(|| InterpError::UnresolvedName(name.as_str(ctx.db).to_string()))?;

    let params = func.params(ctx.db);
    let body = func.body(ctx.db);

    // Evaluate arguments with expected parameter types (in the current frame).
    let mut arg_values = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        // Get the expected type from the parameter if available.
        let expected_type = if i < params.len() {
            let type_hint = params[i].type_hint(ctx.db);
            crate::interp_old::eval_datalit::convert_type_hint_tracked(ctx.db, type_hint)
        } else {
            None
        };

        let value = eval_expr_with_expected(ctx, *arg, expected_type)?;
        arg_values.push(value);
    }

    // Create new stack frame for function call.
    // Copy functions and module_functions from current frame to maintain visibility.
    let mut new_frame = crate::interp_old::interp::StackFrame {
        variables: HashMap::new(),
        functions: ctx.clone_current_functions(),
        module_functions: ctx.clone_current_module_functions(),
        expected_return_type: None,
    };

    // Get the function's return type for handling early returns and automatic coercion.
    let return_type_hint = func.return_type(ctx.db);
    let return_type = return_type_hint.and_then(|type_hint| {
        crate::interp_old::eval_datalit::convert_type_hint_tracked(ctx.db, type_hint)
    });
    new_frame.expected_return_type = return_type;

    // Bind parameters to argument values in the new frame.
    for (param, arg_value) in params.iter().zip(arg_values.into_iter()) {
        new_frame.variables.insert(param.name(ctx.db), arg_value);
    }

    // Push the new frame.
    ctx.push_frame(new_frame);

    // Execute function body.
    let mut result = Err(InterpError::RuntimeError(
        "Function did not return a value".to_string(),
    ));

    for stmt in body {
        match ctx.exec_stmt(stmt) {
            Ok(()) => {
                // Continue executing statements.
            }
            Err(InterpError::Return(value)) => {
                // Got a return value.
                result = Ok(value);
                break;
            }
            Err(InterpError::ReturnNone) => {
                // Early return from try-option operator (?).
                // Create a None value of the function's return type.
                if let Some(rt_type) = return_type {
                    let tydesc = ctx.tydesc_table.get_or_create(rt_type.ty(ctx.db));
                    let mut none_value = unsafe { Value::alloc_option(&mut ctx.rt, tydesc) };
                    // Write None tag.
                    unsafe {
                        *(none_value.as_mut_ptr()) = datalove_rtdt::OptionTag::None as u8;
                    }
                    result = Ok(none_value);
                    break;
                } else {
                    result = Err(InterpError::RuntimeError(
                        "Cannot create None return value: function has no type annotation".to_string()
                    ));
                    break;
                }
            }
            Err(InterpError::ReturnError(mut error_value)) => {
                // Early return from try-result operator (!).
                // Create an Err value wrapping the error.
                if let Some(rt_type) = return_type {
                    let tydesc = ctx.tydesc_table.get_or_create(rt_type.ty(ctx.db));
                    let mut err_result = unsafe { Value::alloc_result(&mut ctx.rt, tydesc) };

                    // Write Err tag.
                    let layout = unsafe { datalove_rtdt::layout::compute_result_layout(datalove_rtdt::TyDescRef::from_ptr(tydesc)) };
                    unsafe {
                        *(err_result.as_mut_ptr()) = datalove_rtdt::ResultTag::Err as u8;
                    }

                    // Write Error payload at the correct offset.
                    let payload_ptr = unsafe { err_result.as_mut_ptr().add(layout.payload_offset as usize) };
                    let error_ptr = payload_ptr as *mut datalove_rtdt::Error;

                    // Extract the inner value's tydesc and ptr.
                    // We need to write a Data/Error structure containing (tydesc, value_ptr).
                    let (inner_tydesc, inner_ptr): (*const datalove_rtdt::TyDesc, *const u8) = match &error_value {
                        Value::Bool(_) => {
                            // For inline values, allocate on heap to get a pointer.
                            let tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::Bool);
                            let ptr = unsafe { alloc_scalar_value(&mut ctx.rt, &error_value, tydesc) };
                            (tydesc, ptr)
                        }
                        Value::U32(_) => {
                            let tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::U32);
                            let ptr = unsafe { alloc_scalar_value(&mut ctx.rt, &error_value, tydesc) };
                            (tydesc, ptr)
                        }
                        Value::F32(_) => {
                            let tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::F32);
                            let ptr = unsafe { alloc_scalar_value(&mut ctx.rt, &error_value, tydesc) };
                            (tydesc, ptr)
                        }
                        Value::Int { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::String { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Tuple { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Struct { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Enum { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::List { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Map { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Set { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Option { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Result { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Data { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                        Value::Error { ptr, tydesc } => (*tydesc, *ptr as *const u8),
                    };

                    // Write the Error structure as Data (same layout).
                    unsafe {
                        std::ptr::write(
                            error_ptr as *mut datalove_rtdt::Data,
                            datalove_rtdt::Data::from_pointers(inner_tydesc, inner_ptr)
                        );
                    }

                    // Transfer ownership - the error_value is now owned by the Result.
                    // We must not free it.
                    std::mem::forget(error_value);

                    result = Ok(err_result);
                    break;
                } else {
                    result = Err(InterpError::RuntimeError(
                        "Cannot create Err return value: function has no type annotation".to_string()
                    ));
                    break;
                }
            }
            Err(e) => {
                // Propagate other errors.
                result = Err(e);
                break;
            }
        }
    }

    // Pop the frame and free all its variables.
    let mut frame = ctx.pop_frame();
    for (_, mut value) in frame.variables.drain() {
        unsafe {
            value.free(&mut ctx.rt);
        }
    }

    result
}

/// Evaluate a tuple expression.
fn eval_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    tuple_expr: ExprFun<'db>,
    tuple: crate::ast::ExprTuple<'db>,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    let elements = tuple.elements(ctx.db);

    // First, evaluate all element expressions.
    let mut element_values = Vec::new();
    for elem in elements {
        let value = eval_expr(ctx, *elem)?;
        element_values.push(value);
    }

    // Try to get the tuple type descriptor from the type table.
    let tuple_tydesc = ctx.type_table.get_datafun_expr_type(tuple_expr);

    // If we don't have a pre-computed tuple type (e.g., because some elements
    // are calls to imported functions), we need to build it from the element types.
    let tuple_tydesc = if !tuple_tydesc.is_null() {
        tuple_tydesc
    } else {
        // Build tuple type from element values.
        let mut element_tydescs = Vec::new();
        for elem_value in &element_values {
            let elem_tydesc = elem_value.get_tydesc();
            // For primitive inline values, we need to create their tydescs.
            let elem_tydesc = if !elem_tydesc.is_null() {
                elem_tydesc
            } else {
                match elem_value {
                    Value::Bool(_) => {
                        use crate::datalit::tycheck::Type;
                        ctx.tydesc_table.get_or_create(&Type::Bool)
                    }
                    Value::U32(_) => {
                        use crate::datalit::tycheck::Type;
                        ctx.tydesc_table.get_or_create(&Type::U32)
                    }
                    Value::F32(_) => {
                        use crate::datalit::tycheck::Type;
                        ctx.tydesc_table.get_or_create(&Type::F32)
                    }
                    _ => {
                        return Err(InterpError::RuntimeError(
                            "Cannot determine type of tuple element".to_string()
                        ));
                    }
                }
            };
            element_tydescs.push(elem_tydesc);
        }

        // Create tuple type descriptor.
        ctx.tydesc_table.get_or_create_tuple(&element_tydescs)
    };

    // Allocate memory for the tuple.
    let mut tuple_value = unsafe { Value::alloc_tuple(&mut ctx.rt, tuple_tydesc) };

    // Get the tuple pointer.
    let tuple_ptr = match &mut tuple_value {
        Value::Tuple { ptr, .. } => *ptr,
        _ => unreachable!("alloc_tuple should return a Tuple value"),
    };

    // Compute the layout to get field offsets.
    let layout = unsafe { rtdt::layout::compute_tuple_layout(rtdt::TyDescRef::from_ptr(tuple_tydesc)) };

    // Copy each element into the tuple at the correct offset.
    for (i, mut elem_value) in element_values.into_iter().enumerate() {
        let field_offset = layout.field_offsets[i];
        let field_dest = unsafe { tuple_ptr.add(field_offset as usize) };

        // Copy the element value into the tuple field.
        match &mut elem_value {
            Value::Bool(b) => {
                unsafe {
                    *(field_dest as *mut bool) = *b;
                }
            }
            Value::U32(n) => {
                unsafe {
                    *(field_dest as *mut u32) = *n;
                }
            }
            Value::F32(f) => {
                unsafe {
                    *(field_dest as *mut f32) = *f;
                }
            }
            Value::Int { ptr, tydesc } => {
                // Transfer ownership of the pointer.
                unsafe {
                    *(field_dest as *mut *mut rtdt::Int) = *ptr;
                }
                // Prevent double-free by nulling out the source.
                *ptr = std::ptr::null_mut();
            }
            Value::String { ptr, tydesc } => {
                // Transfer ownership of the pointer.
                unsafe {
                    *(field_dest as *mut *mut rtdt::String) = *ptr;
                }
                // Prevent double-free by nulling out the source.
                *ptr = std::ptr::null_mut();
            }
            Value::Tuple { ptr, tydesc } => {
                // Get the field tydesc from the tuple type info.
                let field_tydesc = unsafe {
                    let tuple_info = (*tuple_tydesc).type_info.tuple;
                    let field_info = &*tuple_info.fields.add(i);
                    field_info.tydesc
                };
                let size = unsafe { (*field_tydesc).size };
                // Copy the entire tuple data.
                unsafe {
                    std::ptr::copy_nonoverlapping(*ptr, field_dest, size as usize);
                }
                // Prevent double-free by nulling out the source.
                *ptr = std::ptr::null_mut();
            }
            Value::Struct { ptr, tydesc } => {
                let field_tydesc = unsafe {
                    let tuple_info = (*tuple_tydesc).type_info.tuple;
                    let field_info = &*tuple_info.fields.add(i);
                    field_info.tydesc
                };
                let size = unsafe { (*field_tydesc).size };
                unsafe {
                    std::ptr::copy_nonoverlapping(*ptr, field_dest, size as usize);
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Enum { ptr, tydesc } => {
                let field_tydesc = unsafe {
                    let tuple_info = (*tuple_tydesc).type_info.tuple;
                    let field_info = &*tuple_info.fields.add(i);
                    field_info.tydesc
                };
                let size = unsafe { (*field_tydesc).size };
                unsafe {
                    std::ptr::copy_nonoverlapping(*ptr, field_dest, size as usize);
                }
                *ptr = std::ptr::null_mut();
            }
            Value::List { ptr, tydesc } => {
                unsafe {
                    *(field_dest as *mut *mut rtdt::List) = *ptr;
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Map { ptr, tydesc } => {
                unsafe {
                    *(field_dest as *mut *mut rtdt::Map) = *ptr;
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Set { ptr, tydesc } => {
                unsafe {
                    *(field_dest as *mut *mut rtdt::Set) = *ptr;
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Option { ptr, tydesc } => {
                let field_tydesc = unsafe {
                    let tuple_info = (*tuple_tydesc).type_info.tuple;
                    let field_info = &*tuple_info.fields.add(i);
                    field_info.tydesc
                };
                let size = unsafe { (*field_tydesc).size };
                unsafe {
                    std::ptr::copy_nonoverlapping(*ptr, field_dest, size as usize);
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Result { ptr, tydesc } => {
                let field_tydesc = unsafe {
                    let tuple_info = (*tuple_tydesc).type_info.tuple;
                    let field_info = &*tuple_info.fields.add(i);
                    field_info.tydesc
                };
                let size = unsafe { (*field_tydesc).size };
                unsafe {
                    std::ptr::copy_nonoverlapping(*ptr, field_dest, size as usize);
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Data { ptr, tydesc } => {
                unsafe {
                    *(field_dest as *mut *mut rtdt::Data) = *ptr;
                }
                *ptr = std::ptr::null_mut();
            }
            Value::Error { ptr, tydesc } => {
                unsafe {
                    *(field_dest as *mut *mut rtdt::Error) = *ptr;
                }
                *ptr = std::ptr::null_mut();
            }
        }
    }

    Ok(tuple_value)
}

/// Helper function to create an Option::Some value.
fn create_option_some(
    ctx: &mut InterpContext<'_>,
    payload: Value,
    option_tydesc: *const datalove_rtdt::TyDesc,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    if option_tydesc.is_null() {
        return Err(InterpError::RuntimeError(
            "Cannot create Option value: missing type descriptor".to_string(),
        ));
    }

    let mut option_value = unsafe { Value::alloc_option(&mut ctx.rt, option_tydesc) };
    let layout = unsafe { rtdt::layout::compute_option_layout(rtdt::TyDescRef::from_ptr(option_tydesc)) };

    // Write Some tag.
    unsafe {
        *(option_value.as_mut_ptr()) = rtdt::OptionTag::Some as u8;
    }

    // Write payload at the correct offset.
    let payload_ptr = unsafe { option_value.as_mut_ptr().add(layout.payload_offset as usize) };

    // Write the payload value.
    unsafe {
        write_value_to_ptr(&mut ctx.rt, payload, payload_ptr)?;
    }

    Ok(option_value)
}

/// Helper function to create an Option::None value.
fn create_option_none(
    ctx: &mut InterpContext<'_>,
    option_tydesc: *const datalove_rtdt::TyDesc,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    if option_tydesc.is_null() {
        return Err(InterpError::RuntimeError(
            "Cannot create Option value: missing type descriptor".to_string(),
        ));
    }

    let mut option_value = unsafe { Value::alloc_option(&mut ctx.rt, option_tydesc) };

    // Write None tag.
    unsafe {
        *(option_value.as_mut_ptr()) = rtdt::OptionTag::None as u8;
    }

    Ok(option_value)
}

/// Helper function to create a Result::Ok value.
fn create_result_ok(
    ctx: &mut InterpContext<'_>,
    payload: Value,
    result_tydesc: *const datalove_rtdt::TyDesc,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    if result_tydesc.is_null() {
        return Err(InterpError::RuntimeError(
            "Cannot create Result value: missing type descriptor".to_string(),
        ));
    }

    let mut result_value = unsafe { Value::alloc_result(&mut ctx.rt, result_tydesc) };
    let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(result_tydesc)) };

    // Write Ok tag.
    unsafe {
        *(result_value.as_mut_ptr()) = rtdt::ResultTag::Ok as u8;
    }

    // Write payload at the correct offset.
    let payload_ptr = unsafe { result_value.as_mut_ptr().add(layout.payload_offset as usize) };

    // Write the payload value.
    unsafe {
        write_value_to_ptr(&mut ctx.rt, payload, payload_ptr)?;
    }

    Ok(result_value)
}

/// Helper function to create a Result::Err value with an overflow error message.
fn create_result_overflow_err(
    ctx: &mut InterpContext<'_>,
    result_tydesc: *const datalove_rtdt::TyDesc,
) -> InterpResult {
    create_result_err_with_string(ctx, result_tydesc, "overflow")
}

/// Helper function to create a Result::Err value with a division by zero error message.
fn create_result_divzero_err(
    ctx: &mut InterpContext<'_>,
    result_tydesc: *const datalove_rtdt::TyDesc,
) -> InterpResult {
    create_result_err_with_string(ctx, result_tydesc, "division by zero")
}

/// Helper function to create a Result::Err value with a string error message.
fn create_result_err_with_string(
    ctx: &mut InterpContext<'_>,
    result_tydesc: *const datalove_rtdt::TyDesc,
    error_msg: &str,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    if result_tydesc.is_null() {
        return Err(InterpError::RuntimeError(
            "Cannot create Result value: missing type descriptor".to_string(),
        ));
    }

    let mut result_value = unsafe { Value::alloc_result(&mut ctx.rt, result_tydesc) };
    let layout = unsafe { rtdt::layout::compute_result_layout(rtdt::TyDescRef::from_ptr(result_tydesc)) };

    // Write Err tag.
    unsafe {
        *(result_value.as_mut_ptr()) = rtdt::ResultTag::Err as u8;
    }

    // Create a String value for the error message.
    let string_tydesc = ctx.tydesc_table.get_or_create(
        &crate::datalit::tycheck::Type::String
    );

    let error_string = unsafe {
        // alloc_string already initializes the String struct properly.
        let mut string_value = Value::alloc_string(&mut ctx.rt, string_tydesc);

        let rt_handle = ctx.rt.as_mut() as *mut _ as datalove_rt::c::LocalRtHandle;

        // Push the error message bytes into the string.
        let status = datalove_rt::c::dtlv_rti_string_push_bytes_local(
            rt_handle,
            string_value.as_mut_ptr(),
            string_tydesc,
            error_msg.as_ptr(),
            error_msg.len() as u32,
        );
        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError(
                "Failed to push error message to string".to_string(),
            ));
        }
        string_value
    };

    // Write Error payload (which is a Data structure with tydesc + value_ptr).
    let payload_ptr = unsafe { result_value.as_mut_ptr().add(layout.payload_offset as usize) };
    let error_ptr = payload_ptr as *mut rtdt::Error;

    let (inner_tydesc, inner_ptr) = match error_string {
        Value::String { ptr, tydesc } => (tydesc, ptr as *const u8),
        _ => unreachable!(),
    };

    unsafe {
        std::ptr::write(
            error_ptr as *mut rtdt::Data,
            rtdt::Data::from_pointers(inner_tydesc, inner_ptr)
        );
    }

    // Transfer ownership - the error_string is now owned by the Result.
    std::mem::forget(error_string);

    Ok(result_value)
}

/// Allocate a scalar value on the heap and return its pointer.
///
/// This is used for inline types (Bool, U32, F32, etc.) that need to be
/// stored in Error structures which expect heap pointers.
unsafe fn alloc_scalar_value(
    rt: &mut datalove_rt::impls::rt_local::RtLocal,
    value: &Value,
    tydesc: *const datalove_rtdt::TyDesc,
) -> *const u8 {
    use datalove_rtdt as rtdt;

    let size = unsafe { (*tydesc).size };
    let align = unsafe { (*tydesc).align };
    let ptr = unsafe { rt.alloc.alloc(size, align, 1) };

    // Write the scalar value to the allocated memory.
    match value {
        Value::Bool(b) => {
            unsafe { *(ptr as *mut bool) = *b; }
        }
        Value::U32(u) => {
            unsafe { *(ptr as *mut u32) = *u; }
        }
        Value::F32(f) => {
            unsafe { *(ptr as *mut f32) = *f; }
        }
        _ => {
            // This should never happen - only called for inline types.
            panic!("alloc_scalar_value called with non-scalar value");
        }
    }

    ptr as *const u8
}

/// Helper function to write a Value to a memory location.
unsafe fn write_value_to_ptr(
    rt: &mut datalove_rt::impls::rt_local::RtLocal,
    value: Value,
    dest_ptr: *mut u8,
) -> Result<(), InterpError> {
    match value {
        Value::Bool(b) => {
            unsafe { *(dest_ptr as *mut bool) = b; }
        }
        Value::U32(u) => {
            unsafe { *(dest_ptr as *mut u32) = u; }
        }
        Value::F32(f) => {
            unsafe { *(dest_ptr as *mut f32) = f; }
        }
        Value::Int { ptr, tydesc } => {
            // Clone the Int value to the destination.
            let rt_handle = rt as *mut _ as datalove_rt::c::LocalRtHandle;
            let status = unsafe { datalove_rt::c::dtlv_rti_clone_local(
                rt_handle,
                ptr as *const u8,
                tydesc,
                dest_ptr,
            ) };
            if status != datalove_rt::c::RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to clone Int value".to_string(),
                ));
            }
        }
        Value::String { ptr, tydesc } => {
            let rt_handle = rt as *mut _ as datalove_rt::c::LocalRtHandle;
            let status = unsafe { datalove_rt::c::dtlv_rti_clone_local(
                rt_handle,
                ptr as *const u8,
                tydesc,
                dest_ptr,
            ) };
            if status != datalove_rt::c::RtStatus::Ok {
                return Err(InterpError::RuntimeError(
                    "Failed to clone String value".to_string(),
                ));
            }
        }
        _ => {
            return Err(InterpError::NotImplemented(
                format!("Writing {:?} to pointer not yet implemented", value),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp_old::type_table::TypeTable;
    use crate::tycheck::type_check;
    use rmx::prelude::*;

    /// Tracked compile helper that does full parse + typecheck pipeline.
    #[salsa::tracked]
    fn compile_for_test<'db>(
        db: &'db dyn crate::Db,
        source: bct::input::Source,
    ) -> (crate::ast::Script<'db>, crate::tycheck::TypecheckResult<'db>) {
        let parse_result = crate::parser::parse(db, source);
        let tycheck_result = crate::tycheck::type_check(
            db,
            source,
            parse_result.script,
        );
        (parse_result.script, tycheck_result)
    }

    #[test]
    fn test_variable_reference_u32() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @42\nlet y = x"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        // Build type table.
        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        // Create interpreter context.
        let mut ctx = InterpContext::new(&db, type_table);

        // Execute the script.
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        // Verify that y has the same value as x.
        let x_name = bct::text::InternedText::new(&db, S("x"));
        let y_name = bct::text::InternedText::new(&db, S("y"));

        let x_value = ctx.lookup_variable(x_name).expect("x not found");
        let y_value = ctx.lookup_variable(y_name).expect("y not found");

        match (x_value, y_value) {
            (Value::U32(x), Value::U32(y)) => {
                assert_eq!(x, y, "x and y should have the same value");
                assert_eq!(*x, 42);
            }
            _ => panic!("Expected U32 values for x and y"),
        }
    }

    #[test]
    fn test_variable_reference_in_expression() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x: @int = @10\nlet y = x + : @int / @5"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        // Build type table.
        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        // Create interpreter context.
        let mut ctx = InterpContext::new(&db, type_table);

        // Execute the script.
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        // Verify that y = x + 5 = 15.
        let y_name = bct::text::InternedText::new(&db, S("y"));
        let y_value = ctx.lookup_variable(y_name).expect("y not found");

        match y_value {
            Value::Int { .. } => {
                // Successfully got int value.
            }
            _ => panic!("Expected Int value for y"),
        }
    }

    #[test]
    fn test_multiple_variable_references() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let a: @int = @100\nlet b = a\nlet c = a + b"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        // Build type table.
        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        // Create interpreter context.
        let mut ctx = InterpContext::new(&db, type_table);

        // Execute the script.
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        // Verify that c = a + b = 100 + 100 = 200.
        let c_name = bct::text::InternedText::new(&db, S("c"));
        let c_value = ctx.lookup_variable(c_name).expect("c not found");

        match c_value {
            Value::Int { .. } => {
                // Successfully got int value.
            }
            _ => panic!("Expected Int value for c"),
        }
    }

    // Tests for complex datalit expressions enabled by direct token parsing

    #[test]
    fn test_eval_datalit_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a tuple value.
        match x_value {
            Value::Tuple { .. } => {
                // Successfully created tuple
            }
            _ => panic!("Expected Tuple value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[1, 2, 3]"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a list value.
        match x_value {
            Value::List { .. } => {
                // Successfully created list
            }
            _ => panic!("Expected List value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_map() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @map { @1 = @10, @2 = @20 }"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a map value.
        match x_value {
            Value::Map { .. } => {
                // Successfully created map
            }
            _ => panic!("Expected Map value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_nested_tuple_in_list() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @[(1, 2), (3, 4)]"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a list value containing tuples.
        match x_value {
            Value::List { .. } => {
                // Successfully created nested structure
            }
            _ => panic!("Expected List value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_nested_list_in_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@1, @2, @3], @100)"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a tuple value with nested list.
        match x_value {
            Value::Tuple { .. } => {
                // Successfully created nested tuple with list
            }
            _ => panic!("Expected Tuple value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_set() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @set { @1, @2, @3 }"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a set value.
        match x_value {
            Value::Set { .. } => {
                // Successfully created set
            }
            _ => panic!("Expected Set value for x"),
        }
    }

    #[test]
    fn test_eval_datalit_deeply_nested() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(@[@(@1, @2)], @[@(@3, @4)])"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let x_value = ctx.lookup_variable(x_name).expect("x not found");

        // Verify it's a tuple with deeply nested structure.
        match x_value {
            Value::Tuple { .. } => {
                // Successfully created deeply nested structure
            }
            _ => panic!("Expected Tuple value for x"),
        }
    }

    #[test]
    fn test_eval_variable_reference_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)\nlet y = x"));
        let (script, tycheck_result) = compile_for_test(&db, source);
        assert_eq!(tycheck_result.errors(&db).len(), 0, "Type errors found");

        let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(&db);
        let type_table = TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
            .expect("Failed to build type table");

        let mut ctx = InterpContext::new(&db, type_table);
        let result = ctx.execute(script);
        assert!(result.is_ok(), "Failed to execute script: {:?}", result);

        let x_name = bct::text::InternedText::new(&db, S("x"));
        let y_name = bct::text::InternedText::new(&db, S("y"));

        let x_value = ctx.lookup_variable(x_name).expect("x not found");
        let y_value = ctx.lookup_variable(y_name).expect("y not found");

        // Both should be tuples.
        match (x_value, y_value) {
            (Value::Tuple { .. }, Value::Tuple { .. }) => {
                // Successfully cloned tuple value
            }
            _ => panic!("Expected Tuple values for x and y"),
        }
    }
}

/// Get the type descriptor for a datafun expression.
///
/// This looks up the type in the type table first, and if not found,
/// tries to synthesize it for literals.
fn get_expr_tydesc<'db>(ctx: &InterpContext<'db>, expr: ExprFun<'db>) -> *const datalove_rtdt::TyDesc {
    use datalove_rtdt as rtdt;

    // First try the type table.
    let tydesc = ctx.type_table.get_datafun_expr_type(expr);
    if !tydesc.is_null() {
        return tydesc;
    }

    // For literals, we can infer the type from the expression kind.
    match expr.expr(ctx.db) {
        ExprFunKind::Datalit(datalit_expr) => {
            // datalit_expr is ExprFull from the datalit AST.
            ctx.type_table.get_expr_type(datalit_expr)
        }
        _ => std::ptr::null(),
    }
}

/// Construct an Option<T> type descriptor from an inner type.
///
/// Currently hardcoded for u32. Extend as needed for other types.
fn make_option_tydesc_u32<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    // Get the tydesc for u32.
    let u32_ty = crate::datalit::tycheck::Type::U32;
    let u32_tydesc = ctx.tydesc_table.get_or_create(&u32_ty);

    // Construct Option<u32> from it.
    ctx.tydesc_table.create_option_from_inner_tydesc(u32_tydesc)
}

/// Construct a Result<T> type descriptor from an inner type.
///
/// Currently hardcoded for u32. Extend as needed for other types.
fn make_result_tydesc_u32<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    // Get the tydesc for u32.
    let u32_ty = crate::datalit::tycheck::Type::U32;
    let u32_tydesc = ctx.tydesc_table.get_or_create(&u32_ty);

    // Construct Result<u32> from it.
    ctx.tydesc_table.create_result_from_inner_tydesc(u32_tydesc)
}

/// Helper to create Option<u8> tydesc.
fn make_option_tydesc_u8<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let u8_ty = crate::datalit::tycheck::Type::U8;
    let u8_tydesc = ctx.tydesc_table.get_or_create(&u8_ty);
    ctx.tydesc_table.create_option_from_inner_tydesc(u8_tydesc)
}

/// Helper to create Result<u8> tydesc.
fn make_result_tydesc_u8<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let u8_ty = crate::datalit::tycheck::Type::U8;
    let u8_tydesc = ctx.tydesc_table.get_or_create(&u8_ty);
    ctx.tydesc_table.create_result_from_inner_tydesc(u8_tydesc)
}

/// Helper to create Option<i8> tydesc.
fn make_option_tydesc_i8<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let i8_ty = crate::datalit::tycheck::Type::I8;
    let i8_tydesc = ctx.tydesc_table.get_or_create(&i8_ty);
    ctx.tydesc_table.create_option_from_inner_tydesc(i8_tydesc)
}

/// Helper to create Result<i8> tydesc.
fn make_result_tydesc_i8<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let i8_ty = crate::datalit::tycheck::Type::I8;
    let i8_tydesc = ctx.tydesc_table.get_or_create(&i8_ty);
    ctx.tydesc_table.create_result_from_inner_tydesc(i8_tydesc)
}

/// Helper to create Option<u16> tydesc.
fn make_option_tydesc_u16<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let u16_ty = crate::datalit::tycheck::Type::U16;
    let u16_tydesc = ctx.tydesc_table.get_or_create(&u16_ty);
    ctx.tydesc_table.create_option_from_inner_tydesc(u16_tydesc)
}

/// Helper to create Result<u16> tydesc.
fn make_result_tydesc_u16<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let u16_ty = crate::datalit::tycheck::Type::U16;
    let u16_tydesc = ctx.tydesc_table.get_or_create(&u16_ty);
    ctx.tydesc_table.create_result_from_inner_tydesc(u16_tydesc)
}

/// Helper to create Option<i16> tydesc.
fn make_option_tydesc_i16<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let i16_ty = crate::datalit::tycheck::Type::I16;
    let i16_tydesc = ctx.tydesc_table.get_or_create(&i16_ty);
    ctx.tydesc_table.create_option_from_inner_tydesc(i16_tydesc)
}

/// Helper to create Result<i16> tydesc.
fn make_result_tydesc_i16<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let i16_ty = crate::datalit::tycheck::Type::I16;
    let i16_tydesc = ctx.tydesc_table.get_or_create(&i16_ty);
    ctx.tydesc_table.create_result_from_inner_tydesc(i16_tydesc)
}

/// Helper to create Option<i32> tydesc.
fn make_option_tydesc_i32<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let i32_ty = crate::datalit::tycheck::Type::I32;
    let i32_tydesc = ctx.tydesc_table.get_or_create(&i32_ty);
    ctx.tydesc_table.create_option_from_inner_tydesc(i32_tydesc)
}

/// Helper to create Result<i32> tydesc.
fn make_result_tydesc_i32<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let i32_ty = crate::datalit::tycheck::Type::I32;
    let i32_tydesc = ctx.tydesc_table.get_or_create(&i32_ty);
    ctx.tydesc_table.create_result_from_inner_tydesc(i32_tydesc)
}

/// Create Result<Int> type descriptor.
fn make_result_tydesc_int<'db>(
    ctx: &mut InterpContext<'db>,
) -> *const datalove_rtdt::TyDesc {
    let int_ty = crate::datalit::tycheck::Type::Int;
    let int_tydesc = ctx.tydesc_table.get_or_create(&int_ty);
    ctx.tydesc_table.create_result_from_inner_tydesc(int_tydesc)
}

/// Check if an Int value is zero.
unsafe fn is_int_zero(int_ptr: *const datalove_rtdt::Int) -> bool {
    unsafe {
        (*int_ptr).size_and_sign == 0
    }
}
