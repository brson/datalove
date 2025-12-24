//! Literal value allocation and writing.
//!
//! Functions for allocating and writing literal values (integers, floats,
//! strings) from parsed AST nodes to runtime values.

use crate::ast;

use super::{InterpContext, InterpError, Value, Destination, ValueOwnership};
use super::alloc::write_bigint_to_ptr;

/// Write inline integer literal to destination.
pub(super) fn write_inline_int_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    int_expr: &ast::ExprInt<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let value_str = int_expr.value(ctx.db).as_str(ctx.db);

    // Determine the destination type and parse accordingly.
    let type_tag = unsafe { (*dest.tydesc).type_tag };
    match type_tag {
        datalove_rt::rtdt::TyTag::U8 => {
            let value: u8 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u8: {}", e)))?;
            unsafe { *(dest.ptr as *mut u8) = value; }
        }
        datalove_rt::rtdt::TyTag::I8 => {
            let value: i8 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i8: {}", e)))?;
            unsafe { *(dest.ptr as *mut i8) = value; }
        }
        datalove_rt::rtdt::TyTag::U16 => {
            let value: u16 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u16: {}", e)))?;
            unsafe { *(dest.ptr as *mut u16) = value; }
        }
        datalove_rt::rtdt::TyTag::I16 => {
            let value: i16 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i16: {}", e)))?;
            unsafe { *(dest.ptr as *mut i16) = value; }
        }
        datalove_rt::rtdt::TyTag::U32 => {
            let value: u32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u32: {}", e)))?;
            unsafe { *(dest.ptr as *mut u32) = value; }
        }
        datalove_rt::rtdt::TyTag::I32 => {
            let value: i32 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i32: {}", e)))?;
            unsafe { *(dest.ptr as *mut i32) = value; }
        }
        datalove_rt::rtdt::TyTag::U64 => {
            let value: u64 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse u64: {}", e)))?;
            unsafe { *(dest.ptr as *mut u64) = value; }
        }
        datalove_rt::rtdt::TyTag::I64 => {
            let value: i64 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse i64: {}", e)))?;
            unsafe { *(dest.ptr as *mut i64) = value; }
        }
        datalove_rt::rtdt::TyTag::Int => {
            let value: i128 = value_str.parse()
                .map_err(|e| InterpError::RuntimeError(format!("Failed to parse int: {}", e)))?;
            let int_ptr = dest.ptr as *mut datalove_rt::rtdt::Int;
            write_bigint_to_ptr(ctx.runtime.handle(), int_ptr, value);
        }
        _ => {
            return Err(InterpError::RuntimeError(
                format!("Cannot write integer to destination type {:?}", type_tag)
            ));
        }
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

/// Write Option::None to a destination.
pub(super) fn write_option_none_to_dest(dest: Destination) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, OptionTag};

    let dest_tydesc = unsafe { TyDescRef::from_ptr(dest.tydesc) };

    // Verify the destination type is Option.
    if dest_tydesc.type_tag() != TyTag::Option {
        return Err(InterpError::RuntimeError(
            format!("Cannot write @none to non-Option type: {:?}", dest_tydesc.type_tag())
        ));
    }

    // Write None tag to destination.
    unsafe {
        *(dest.ptr as *mut u8) = OptionTag::None as u8;
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

/// Write a boolean value to destination.
pub(super) fn write_bool_to_dest(dest: Destination, value: bool) -> Result<Value, InterpError> {
    unsafe { *dest.ptr = if value { 1 } else { 0 }; }
    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

/// Write an f32 value to destination.
pub(super) fn write_f32_to_dest(dest: Destination, value: f32) -> Result<Value, InterpError> {
    unsafe { *(dest.ptr as *mut f32) = value; }
    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

/// Write a u32 value to destination (for hex literals).
pub(super) fn write_u32_to_dest(dest: Destination, value: u32) -> Result<Value, InterpError> {
    unsafe { *(dest.ptr as *mut u32) = value; }
    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

/// Write an inline string literal to destination.
pub(super) fn write_string_to_dest<'db>(
    ctx: &mut InterpContext<'db>,
    string_expr: &ast::ExprString<'db>,
    dest: Destination,
) -> Result<Value, InterpError> {
    let string_value_raw = string_expr.value(ctx.db).as_str(ctx.db);

    // Strip quotes if present.
    let string_value = if string_value_raw.starts_with('"') && string_value_raw.ends_with('"') {
        &string_value_raw[1..string_value_raw.len()-1]
    } else {
        string_value_raw
    };

    let rt_handle = ctx.runtime.handle();

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(
            rt_handle,
            dest.ptr,
            dest.tydesc,
        )
    };

    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create string at dest".to_string()));
    }

    if !string_value.is_empty() {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                dest.ptr,
                dest.tydesc,
                string_value.as_ptr(),
                string_value.len() as u32,
            )
        };

        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to push string bytes".to_string()));
        }
    }

    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, ownership: ValueOwnership::Borrowed })
}

/// Write Result::Er from an error payload value.
///
/// Takes ownership of the error payload and wraps it in Er.
/// Requires a destination to provide the Result type.
pub(super) fn write_result_er_from_value<'db>(
    ctx: &mut InterpContext<'db>,
    payload: Value,
    dest: Destination,
) -> Result<Value, InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, ResultTag};
    use datalove_rt::rtdt::layout::compute_result_layout;
    use super::memory::destroy_value;

    // Verify payload is an error type.
    let payload_tag = unsafe { (*payload.tydesc).type_tag };
    if payload_tag != TyTag::Error {
        destroy_value(ctx, payload);
        return Err(InterpError::RuntimeError(
            format!("Er payload must be error type, got {:?}", payload_tag)
        ));
    }

    // Verify destination is Result type.
    let dest_tag = unsafe { (*dest.tydesc).type_tag };
    if dest_tag != TyTag::Result {
        destroy_value(ctx, payload);
        return Err(InterpError::RuntimeError(
            format!("er requires Result destination, got {:?}", dest_tag)
        ));
    }

    let result_tydesc = dest.tydesc;
    let result_ref = unsafe { TyDescRef::from_ptr(result_tydesc) };
    let layout = compute_result_layout(result_ref);

    // Write Er tag.
    unsafe { *(dest.ptr as *mut u8) = ResultTag::Err as u8; }

    // Clone error payload to Result error area.
    let error_ptr = unsafe { dest.ptr.add(layout.payload_offset as usize) };
    let clone_status = unsafe {
        datalove_rt::c::dtlv_rti_clone_local(
            ctx.runtime.handle(),
            payload.ptr,
            payload.tydesc,
            error_ptr,
            payload.tydesc,
        )
    };

    // Clean up original payload.
    destroy_value(ctx, payload);

    if clone_status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to clone error for Er".to_string()));
    }

    Ok(Value { ptr: dest.ptr, tydesc: result_tydesc, ownership: ValueOwnership::Borrowed })
}
