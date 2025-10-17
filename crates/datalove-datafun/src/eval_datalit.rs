//! Evaluators for datalit expressions.

use rmx::prelude::*;
use datalove_rt;
use datalove_rtdt as rtdt;
use crate::datalit;
use crate::interp::{InterpContext, InterpResult, InterpError};
use crate::value::Value;

/// Evaluate a datalit expression.
pub fn eval_datalit<'db>(
    ctx: &mut InterpContext<'db>,
    expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    let expr_and_heap = expr.expr(ctx.db);
    let expr_inner = expr_and_heap.expr(ctx.db);

    use datalit::ast::Expr;

    match expr_inner {
        Expr::True => Ok(Value::from_bool(true)),

        Expr::False => Ok(Value::from_bool(false)),

        Expr::Int(int_expr) => eval_int(ctx, int_expr),

        Expr::Float(float_expr) => eval_float(ctx, float_expr),

        Expr::String(string_expr) => eval_string(ctx, string_expr, expr),

        Expr::AnonTuple(tuple_expr) => eval_anon_tuple(ctx, tuple_expr, expr),

        Expr::NamedTuple(tuple_expr) => eval_named_tuple(ctx, tuple_expr, expr),

        Expr::AnonStruct(struct_expr) => eval_anon_struct(ctx, struct_expr, expr),

        Expr::NamedStruct(struct_expr) => eval_named_struct(ctx, struct_expr, expr),

        Expr::AnonEnum(enum_expr) => eval_anon_enum(ctx, enum_expr, expr),

        Expr::NamedEnum(enum_expr) => eval_named_enum(ctx, enum_expr, expr),

        Expr::List(list_expr) => eval_list(ctx, list_expr, expr),

        Expr::Map(map_expr) => eval_map(ctx, map_expr, expr),

        Expr::Set(set_expr) => eval_set(ctx, set_expr, expr),

        Expr::None => eval_none(ctx, expr),

        Expr::Data(data_expr) => eval_data(ctx, data_expr),

        Expr::Err(err_expr) => eval_error(ctx, err_expr),

        Expr::ParseError(err) => {
            let message = err.message(ctx.db);
            Err(InterpError::RuntimeError(
                format!("Parse error: {}", message.as_str(ctx.db)),
            ))
        }
    }
}

/// Evaluate an integer literal.
fn eval_int<'db>(
    _ctx: &mut InterpContext<'db>,
    int_expr: datalit::ast::ExprInt<'db>,
) -> InterpResult {
    let value_str = int_expr.value(_ctx.db).as_str(_ctx.db);

    // Try to parse as u32 first (most common case).
    if let Ok(value) = value_str.parse::<u32>() {
        return Ok(Value::from_u32(value));
    }

    // TODO: Implement bigint support.
    Err(InterpError::NotImplemented("bigint support".to_string()))
}

/// Evaluate a float literal.
fn eval_float<'db>(
    ctx: &mut InterpContext<'db>,
    float_expr: datalit::ast::ExprFloat<'db>,
) -> InterpResult {
    let value_str = float_expr.value(ctx.db).as_str(ctx.db);

    // Parse as f32.
    match value_str.parse::<f32>() {
        Ok(value) => Ok(Value::from_f32(value)),
        Err(_) => Err(InterpError::RuntimeError(
            format!("Invalid float literal: {}", value_str),
        )),
    }
}

/// Evaluate a string literal.
fn eval_string<'db>(
    ctx: &mut InterpContext<'db>,
    string_expr: datalit::ast::ExprString<'db>,
    full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    let value_str = string_expr.value(ctx.db).as_str(ctx.db);

    // Get the type descriptor for this string.
    let tydesc = ctx.type_table.get_expr_type(full_expr);
    if tydesc.is_null() {
        return Err(InterpError::TypeError("No type for string".to_string()));
    }

    // Parse the string literal to remove quotes and handle escape sequences.
    // The value_str includes the quotes, e.g., "\"hello\"".
    let unquoted = if value_str.len() >= 2 && value_str.starts_with('"') && value_str.ends_with('"') {
        &value_str[1..value_str.len()-1]
    } else {
        value_str
    };

    unsafe {
        // Allocate heap storage for the string struct.
        let ptr = ctx.rt.alloc.alloc(
            std::mem::size_of::<rtdt::String>() as u32,
            std::mem::align_of::<rtdt::String>() as u32,
            1,
        ) as *mut rtdt::String;

        let rt_handle = Box::as_mut(&mut ctx.rt) as *mut _ as datalove_rt::LocalRtHandle;

        // Initialize the string.
        let status = datalove_rt::dtlv_rti_string_create_local(
            rt_handle,
            ptr as *mut u8,
            tydesc,
        );

        if status != datalove_rt::RtStatus::Ok {
            ctx.rt.alloc.free(
                std::mem::size_of::<rtdt::String>() as u32,
                std::mem::align_of::<rtdt::String>() as u32,
                1,
                ptr as *mut u8,
            );
            return Err(InterpError::RuntimeError(
                "Failed to create string".to_string(),
            ));
        }

        // Push bytes into the string.
        let bytes = unquoted.as_bytes();
        let status = datalove_rt::dtlv_rti_string_push_bytes_local(
            rt_handle,
            ptr as *mut u8,
            tydesc,
            bytes.as_ptr(),
            bytes.len() as u32,
        );

        if status != datalove_rt::RtStatus::Ok {
            // Call destroy_local to free the string's buffer before freeing the struct.
            datalove_rt::dtlv_rti_string_destroy_local(
                rt_handle,
                ptr as *mut u8,
                tydesc,
            );
            ctx.rt.alloc.free(
                std::mem::size_of::<rtdt::String>() as u32,
                std::mem::align_of::<rtdt::String>() as u32,
                1,
                ptr as *mut u8,
            );
            return Err(InterpError::RuntimeError(
                "Failed to push bytes to string".to_string(),
            ));
        }

        Ok(Value::String { ptr, tydesc })
    }
}

/// Evaluate an anonymous tuple.
fn eval_anon_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    tuple_expr: datalit::ast::ExprAnonTuple<'db>,
    full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    let tydesc = ctx.type_table.get_expr_type(full_expr);
    if tydesc.is_null() {
        return Err(InterpError::TypeError("No type for tuple".to_string()));
    }

    // Allocate tuple storage.
    let mut value = unsafe { Value::alloc_tuple(&mut ctx.rt, tydesc) };

    // Evaluate each element and store it.
    let elements = tuple_expr.elements(ctx.db);

    if let Value::Tuple { ptr, .. } = &value {
        let tuple_ptr = *ptr;

        // Get field layout info.
        let tuple_info = unsafe { (*tydesc).type_info.tuple };
        let num_fields = unsafe { tuple_info.num_fields };
        let fields = unsafe { tuple_info.fields };

        if elements.len() != num_fields as usize {
            return Err(InterpError::RuntimeError(
                "Tuple element count mismatch".to_string(),
            ));
        }

        for (i, elem) in elements.iter().enumerate() {
            let elem_value = eval_datalit(ctx, *elem)?;
            let field_info = unsafe { &*fields.add(i) };
            let field_offset = field_info.offset;

            // Copy the value into the tuple field.
            // TODO: Need proper value copying logic.
            unsafe {
                write_value_at_offset(ctx, elem_value, tuple_ptr, field_offset)?;
            }
        }
    }

    Ok(value)
}

/// Evaluate a named tuple.
fn eval_named_tuple<'db>(
    ctx: &mut InterpContext<'db>,
    tuple_expr: datalit::ast::ExprNamedTuple<'db>,
    full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // Named tuples are evaluated the same as anonymous tuples,
    // but use a different type.
    let tydesc = ctx.type_table.get_expr_type(full_expr);
    if tydesc.is_null() {
        return Err(InterpError::TypeError("No type for named tuple".to_string()));
    }

    let mut value = unsafe { Value::alloc_tuple(&mut ctx.rt, tydesc) };

    let elements = tuple_expr.elements(ctx.db);

    if let Value::Tuple { ptr, .. } = &value {
        let tuple_ptr = *ptr;
        let tuple_info = unsafe { (*tydesc).type_info.tuple };
        let num_fields = unsafe { tuple_info.num_fields };
        let fields = unsafe { tuple_info.fields };

        if elements.len() != num_fields as usize {
            return Err(InterpError::RuntimeError(
                "Tuple element count mismatch".to_string(),
            ));
        }

        for (i, elem) in elements.iter().enumerate() {
            let elem_value = eval_datalit(ctx, *elem)?;
            let field_info = unsafe { &*fields.add(i) };
            let field_offset = field_info.offset;

            unsafe {
                write_value_at_offset(ctx, elem_value, tuple_ptr, field_offset)?;
            }
        }
    }

    Ok(value)
}

/// Evaluate an anonymous struct.
fn eval_anon_struct<'db>(
    ctx: &mut InterpContext<'db>,
    _struct_expr: datalit::ast::ExprAnonStruct<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement struct evaluation.
    Err(InterpError::NotImplemented("struct evaluation".to_string()))
}

/// Evaluate a named struct.
fn eval_named_struct<'db>(
    ctx: &mut InterpContext<'db>,
    _struct_expr: datalit::ast::ExprNamedStruct<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement struct evaluation.
    Err(InterpError::NotImplemented("struct evaluation".to_string()))
}

/// Evaluate an anonymous enum.
fn eval_anon_enum<'db>(
    ctx: &mut InterpContext<'db>,
    _enum_expr: datalit::ast::ExprAnonEnum<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement enum evaluation.
    Err(InterpError::NotImplemented("enum evaluation".to_string()))
}

/// Evaluate a named enum.
fn eval_named_enum<'db>(
    ctx: &mut InterpContext<'db>,
    _enum_expr: datalit::ast::ExprNamedEnum<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement enum evaluation.
    Err(InterpError::NotImplemented("enum evaluation".to_string()))
}

/// Evaluate a list.
fn eval_list<'db>(
    ctx: &mut InterpContext<'db>,
    _list_expr: datalit::ast::ExprList<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement list evaluation.
    Err(InterpError::NotImplemented("list evaluation".to_string()))
}

/// Evaluate a map.
fn eval_map<'db>(
    ctx: &mut InterpContext<'db>,
    _map_expr: datalit::ast::ExprMap<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement map evaluation.
    Err(InterpError::NotImplemented("map evaluation".to_string()))
}

/// Evaluate a set.
fn eval_set<'db>(
    ctx: &mut InterpContext<'db>,
    _set_expr: datalit::ast::ExprSet<'db>,
    _full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    // TODO: Implement set evaluation.
    Err(InterpError::NotImplemented("set evaluation".to_string()))
}

/// Evaluate None.
fn eval_none<'db>(
    ctx: &mut InterpContext<'db>,
    full_expr: datalit::ast::ExprFull<'db>,
) -> InterpResult {
    let tydesc = ctx.type_table.get_expr_type(full_expr);
    if tydesc.is_null() {
        return Err(InterpError::TypeError("No type for None".to_string()));
    }

    // Allocate option storage.
    let value = unsafe { Value::alloc_option(&mut ctx.rt, tydesc) };

    // Set tag to None.
    if let Value::Option { ptr, .. } = &value {
        unsafe {
            *(*ptr) = rtdt::OptionTag::None as u8;
        }
    }

    Ok(value)
}

/// Evaluate a data expression.
fn eval_data<'db>(
    ctx: &mut InterpContext<'db>,
    _data_expr: datalit::ast::ExprData<'db>,
) -> InterpResult {
    // TODO: Implement data evaluation.
    Err(InterpError::NotImplemented("data evaluation".to_string()))
}

/// Evaluate an error expression.
fn eval_error<'db>(
    ctx: &mut InterpContext<'db>,
    _err_expr: datalit::ast::ExprErr<'db>,
) -> InterpResult {
    // TODO: Implement error evaluation.
    Err(InterpError::NotImplemented("error evaluation".to_string()))
}

/// Write a value at a specific offset in memory.
unsafe fn write_value_at_offset(
    _ctx: &mut InterpContext<'_>,
    value: Value,
    base_ptr: *mut u8,
    offset: u32,
) -> Result<(), InterpError> {
    let field_ptr = unsafe { base_ptr.add(offset as usize) };

    match value {
        Value::Bool(b) => {
            unsafe { *(field_ptr as *mut bool) = b };
            Ok(())
        }
        Value::U32(v) => {
            unsafe { *(field_ptr as *mut u32) = v };
            Ok(())
        }
        Value::F32(v) => {
            unsafe { *(field_ptr as *mut f32) = v };
            Ok(())
        }
        _ => {
            // TODO: Handle copying heap-allocated values.
            Err(InterpError::NotImplemented(
                "copying heap-allocated values".to_string(),
            ))
        }
    }
}
