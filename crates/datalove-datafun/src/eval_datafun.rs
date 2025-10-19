//! Evaluators for datafun expressions.

use rmx::prelude::*;
use crate::ast::*;
use crate::interp::{InterpContext, InterpResult, InterpError};
use crate::value::Value;

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
            crate::eval_datalit::eval_datalit(ctx, datalit_expr, expected)
        }

        ExprFunKind::Name(name) => eval_name(ctx, name),

        ExprFunKind::BinOp(binop) => eval_binop(ctx, binop),

        ExprFunKind::FunctionCall(call) => eval_function_call(ctx, call),

        ExprFunKind::Tuple(tuple) => eval_tuple(ctx, tuple),

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
            let rt_handle = Box::as_mut(&mut ctx.rt) as *mut _ as rt::LocalRtHandle;

            // Clone the value.
            let status = unsafe {
                rt::dtlv_rti_clone_local(
                    rt_handle,
                    ptr,
                    tydesc,
                    new_value.as_mut_ptr(),
                )
            };

            if status != rt::RtStatus::Ok {
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

/// Evaluate a try-option operator (?).
/// If the operand is None, returns early with ReturnNone.
/// If the operand is Some(value), returns the unwrapped value.
fn eval_try_option<'db>(
    ctx: &mut InterpContext<'db>,
    try_op: crate::ast::ExprTryOption<'db>,
) -> InterpResult {
    use datalove_rtdt as rtdt;

    let operand = try_op.operand(ctx.db);
    let operand_value = eval_expr(ctx, operand)?;

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

    match option_tag {
        rtdt::OptionTag::None => {
            // Early return with None.
            Err(InterpError::ReturnNone)
        }
        rtdt::OptionTag::Some => {
            // Extract the Some value using the same pattern as if-destructuring.
            let layout = unsafe { rtdt::layout::compute_option_layout(tydesc) };
            let payload_ptr = unsafe { ptr.add(layout.payload_offset as usize) };

            // Get the inner type descriptor.
            let inner_tydesc = unsafe { (*tydesc).type_info.option.inner_tydesc };

            // Create a Value for the payload (clones it).
            InterpContext::value_from_ptr(&mut ctx.rt, payload_ptr, inner_tydesc)
        }
    }
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
    let operand_value = eval_expr(ctx, operand)?;

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
            let layout = unsafe { rtdt::layout::compute_result_layout(tydesc) };
            let payload_ptr = unsafe { ptr.add(layout.payload_offset as usize) };

            // The error payload is of type Error (same layout as Data).
            // Error contains tydesc + value pointer.
            let error_ptr = payload_ptr as *const rtdt::Error;
            let error_tydesc = unsafe { (*error_ptr).tydesc() };
            let error_value_ptr = unsafe { (*error_ptr).value_ptr() };

            // Create a Value for the error (clones it).
            let error_value = InterpContext::value_from_ptr(&mut ctx.rt, error_value_ptr, error_tydesc)?;

            Err(InterpError::ReturnError(error_value))
        }
        rtdt::ResultTag::Ok => {
            // Extract the Ok value using the same pattern as if-destructuring.
            let layout = unsafe { rtdt::layout::compute_result_layout(tydesc) };
            let payload_ptr = unsafe { ptr.add(layout.payload_offset as usize) };

            // Get the Ok type descriptor.
            let ok_tydesc = unsafe { (*tydesc).type_info.result.ok_tydesc };

            // Create a Value for the payload (clones it).
            InterpContext::value_from_ptr(&mut ctx.rt, payload_ptr, ok_tydesc)
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
    if ctx.call_depth >= crate::interp::MAX_CALL_DEPTH {
        return Err(InterpError::StackOverflow);
    }

    // Increment call depth.
    ctx.call_depth += 1;

    // Look up the function definition (need to copy since we'll borrow ctx mutably later).
    let func = *ctx.functions.get(&name)
        .ok_or_else(|| InterpError::UnresolvedName(name.as_str(ctx.db).to_string()))?;

    let params = func.params(ctx.db);
    let body = func.body(ctx.db);

    // Evaluate arguments with expected parameter types.
    let mut arg_values = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        // Get the expected type from the parameter if available.
        let expected_type = if i < params.len() {
            let type_hint = params[i].type_hint(ctx.db);
            crate::eval_datalit::convert_type_hint_tracked(ctx.db, type_hint)
        } else {
            None
        };

        let value = eval_expr_with_expected(ctx, *arg, expected_type)?;
        arg_values.push(value);
    }

    // Save parameter names that we're about to shadow.
    let mut shadowed_vars: Vec<(bct::text::InternedText<'db>, Option<Value>)> = Vec::new();
    for param in params {
        let param_name = param.name(ctx.db);
        let old_value = ctx.variables.remove(&param_name);
        shadowed_vars.push((param_name, old_value));
    }

    // Bind parameters to argument values.
    for (param, arg_value) in params.iter().zip(arg_values.into_iter()) {
        ctx.variables.insert(param.name(ctx.db), arg_value);
    }

    // Execute function body.
    let mut result = Err(InterpError::RuntimeError(
        "Function did not return a value".to_string(),
    ));

    // Get the function's return type for handling early returns and automatic coercion.
    let return_type_hint = func.return_type(ctx.db);
    let return_type = return_type_hint.and_then(|type_hint| {
        crate::eval_datalit::convert_type_hint_tracked(ctx.db, type_hint)
    });

    // Set expected return type for automatic coercion in return statements.
    let old_return_type = ctx.expected_return_type;
    ctx.expected_return_type = return_type;

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
                    let layout = unsafe { datalove_rtdt::layout::compute_result_layout(tydesc) };
                    unsafe {
                        *(err_result.as_mut_ptr()) = datalove_rtdt::ResultTag::Err as u8;
                    }

                    // Write Error payload at the correct offset.
                    let payload_ptr = unsafe { err_result.as_mut_ptr().add(layout.payload_offset as usize) };
                    let error_ptr = payload_ptr as *mut datalove_rtdt::Error;

                    // Extract the inner value's tydesc and ptr.
                    // We need to write a Data/Error structure containing (tydesc, value_ptr).
                    let (inner_tydesc, inner_ptr): (*const datalove_rtdt::TyDesc, *const u8) = match &error_value {
                        Value::Bool(b) => {
                            // For inline values, we need to allocate them on the heap first.
                            // This is complex, so for now we'll leave it unimplemented.
                            result = Err(InterpError::NotImplemented(
                                "Error value handling for inline types not yet implemented".to_string()
                            ));
                            break;
                        }
                        Value::U32(_) | Value::F32(_) => {
                            result = Err(InterpError::NotImplemented(
                                "Error value handling for inline types not yet implemented".to_string()
                            ));
                            break;
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

    // Restore shadowed variables.
    // First, free the parameter values.
    for (param_name, _) in &shadowed_vars {
        if let Some(mut param_value) = ctx.variables.remove(param_name) {
            unsafe {
                param_value.free(&mut ctx.rt);
            }
        }
    }

    // Then restore old values.
    for (param_name, old_value) in shadowed_vars {
        if let Some(old_val) = old_value {
            ctx.variables.insert(param_name, old_val);
        }
    }

    // Restore expected return type.
    ctx.expected_return_type = old_return_type;

    // Decrement call depth before returning.
    ctx.call_depth -= 1;

    result
}

/// Evaluate a tuple expression.
///
/// TODO: This is a simplified implementation that needs proper type tracking.
fn eval_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    tuple: crate::ast::ExprTuple<'db>,
) -> InterpResult {
    // For now, return an error indicating tuples are not yet fully supported.
    // The typechecker allows them, but runtime evaluation needs more work.
    Err(InterpError::RuntimeError(
        "Datafun tuple evaluation not yet fully implemented".to_string()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_table::TypeTable;
    use crate::tycheck::type_check;
    use rmx::prelude::*;

    #[test]
    fn test_variable_reference_u32() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @42\nlet y = x"));
        let script = crate::parser::parse(&db, source);

        // Type check the script.
        let tycheck_result = type_check(&db, script);
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
        let source = bct::input::Source::new(&db, S("let x = @10\nlet y = x + @5"));
        let script = crate::parser::parse(&db, source);

        // Type check the script.
        let tycheck_result = type_check(&db, script);
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
            Value::U32(y) => {
                assert_eq!(*y, 15);
            }
            _ => panic!("Expected U32 value for y"),
        }
    }

    #[test]
    fn test_multiple_variable_references() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let a = @100\nlet b = a\nlet c = a + b"));
        let script = crate::parser::parse(&db, source);

        // Type check the script.
        let tycheck_result = type_check(&db, script);
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
            Value::U32(c) => {
                assert_eq!(*c, 200);
            }
            _ => panic!("Expected U32 value for c"),
        }
    }

    // Tests for complex datalit expressions enabled by direct token parsing

    #[test]
    fn test_eval_datalit_tuple() {
        let db = crate::Database::default();
        let source = bct::input::Source::new(&db, S("let x = @(1, 2, 3)"));
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
        let script = crate::parser::parse(&db, source);

        let tycheck_result = type_check(&db, script);
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
