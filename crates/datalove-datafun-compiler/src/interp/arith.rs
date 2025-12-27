//! Checked and optional arithmetic operations.
//!
//! - Checked: early-return Result::Err on overflow/division-by-zero
//! - Optional: early-return Option::None on overflow/division-by-zero

use datalove_rt::rtdt::TyTag;

use super::{InterpContext, InterpError, Value, Destination, EvalResult};
use super::types::{is_int_value, is_fixed_int_value, get_type_tag};

pub(super) fn write_f32_result(value: f32, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut f32) = value; }
    Ok(())
}

fn write_bool_result(value: bool, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut u8) = if value { 1 } else { 0 }; }
    Ok(())
}

fn write_u8_result(value: u8, dest: Destination) -> Result<(), InterpError> {
    unsafe { *dest.ptr = value; }
    Ok(())
}

fn write_i8_result(value: i8, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut i8) = value; }
    Ok(())
}

fn write_u16_result(value: u16, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut u16) = value; }
    Ok(())
}

fn write_i16_result(value: i16, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut i16) = value; }
    Ok(())
}

fn write_u32_result(value: u32, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut u32) = value; }
    Ok(())
}

fn write_i32_result(value: i32, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut i32) = value; }
    Ok(())
}

fn write_u64_result(value: u64, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut u64) = value; }
    Ok(())
}

fn write_i64_result(value: i64, dest: Destination) -> Result<(), InterpError> {
    unsafe { *(dest.ptr as *mut i64) = value; }
    Ok(())
}

/// Write Option::None to return_dest for optional operator early return.
fn write_option_none_early_return(return_dest: Destination) {
    use datalove_rt::rtdt::OptionTag;
    unsafe {
        *(return_dest.ptr as *mut u8) = OptionTag::None as u8;
    }
}

/// Write Result::Err with error string to return_dest for checked operator early return.
pub(super) fn write_result_err_early_return(
    ctx: &mut InterpContext<'_>,
    return_dest: Destination,
    error_msg: &str,
) -> Result<(), InterpError> {
    use datalove_rt::rtdt::{TyDescRef, TyTag, ResultTag, layout::compute_result_layout};
    use crate::datalit::tycheck::Type;

    let return_tydesc_ref = unsafe { TyDescRef::from_ptr(return_dest.tydesc) };

    if return_tydesc_ref.type_tag() != TyTag::Result {
        return Err(InterpError::RuntimeError(
            format!("Expected Result return type for checked operator, got {:?}", return_tydesc_ref.type_tag())
        ));
    }

    let layout = compute_result_layout(return_tydesc_ref);

    // Write Err tag.
    unsafe {
        *(return_dest.ptr as *mut u8) = ResultTag::Err as u8;
    }

    // Allocate and create error string.
    let string_tydesc = ctx.tydesc_table.get_or_create(&Type::String);
    let rt_handle = ctx.runtime.handle();
    let string_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, string_tydesc, 1)
    };

    if string_ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate error string".to_string()));
    }

    let status = unsafe {
        datalove_rt::c::dtlv_rti_string_create_local(rt_handle, string_ptr, string_tydesc)
    };
    if status != datalove_rt::c::RtStatus::Ok {
        return Err(InterpError::RuntimeError("Failed to create error string".to_string()));
    }

    if !error_msg.is_empty() {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_string_push_bytes_local(
                rt_handle,
                string_ptr,
                string_tydesc,
                error_msg.as_ptr(),
                error_msg.len() as u32,
            )
        };
        if status != datalove_rt::c::RtStatus::Ok {
            return Err(InterpError::RuntimeError("Failed to push error string bytes".to_string()));
        }
    }

    // Write Error (Data structure) to payload area.
    let payload_ptr = unsafe { return_dest.ptr.add(layout.payload_offset as usize) };
    unsafe {
        let error_data = datalove_rt::rtdt::Data::from_pointers(string_tydesc, string_ptr);
        std::ptr::write(payload_ptr as *mut datalove_rt::rtdt::Data, error_data);
    }

    Ok(())
}

/// Checked addition: early-return Result::Err on overflow.
pub(super) fn eval_add_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Checked addition requires fixed-width integer operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in checked addition: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! checked_add {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_add(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_result_err_early_return(ctx, return_dest, "overflow")?;
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => checked_add!(u8, write_u8_result),
        TyTag::I8 => checked_add!(i8, write_i8_result),
        TyTag::U16 => checked_add!(u16, write_u16_result),
        TyTag::I16 => checked_add!(i16, write_i16_result),
        TyTag::U32 => checked_add!(u32, write_u32_result),
        TyTag::I32 => checked_add!(i32, write_i32_result),
        TyTag::U64 => checked_add!(u64, write_u64_result),
        TyTag::I64 => checked_add!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for checked addition: {:?}", lhs_tag)
        )),
    }
}

/// Checked subtraction: early-return Result::Err on underflow.
pub(super) fn eval_sub_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Checked subtraction requires fixed-width integer operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in checked subtraction: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! checked_sub {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_sub(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_result_err_early_return(ctx, return_dest, "overflow")?;
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => checked_sub!(u8, write_u8_result),
        TyTag::I8 => checked_sub!(i8, write_i8_result),
        TyTag::U16 => checked_sub!(u16, write_u16_result),
        TyTag::I16 => checked_sub!(i16, write_i16_result),
        TyTag::U32 => checked_sub!(u32, write_u32_result),
        TyTag::I32 => checked_sub!(i32, write_i32_result),
        TyTag::U64 => checked_sub!(u64, write_u64_result),
        TyTag::I64 => checked_sub!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for checked subtraction: {:?}", lhs_tag)
        )),
    }
}

/// Checked multiplication: early-return Result::Err on overflow.
pub(super) fn eval_mul_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Checked multiplication requires fixed-width integer operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in checked multiplication: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! checked_mul {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_mul(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_result_err_early_return(ctx, return_dest, "overflow")?;
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => checked_mul!(u8, write_u8_result),
        TyTag::I8 => checked_mul!(i8, write_i8_result),
        TyTag::U16 => checked_mul!(u16, write_u16_result),
        TyTag::I16 => checked_mul!(i16, write_i16_result),
        TyTag::U32 => checked_mul!(u32, write_u32_result),
        TyTag::I32 => checked_mul!(i32, write_i32_result),
        TyTag::U64 => checked_mul!(u64, write_u64_result),
        TyTag::I64 => checked_mul!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for checked multiplication: {:?}", lhs_tag)
        )),
    }
}

/// Checked division: early-return Result::Err on division by zero.
pub(super) fn eval_div_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    // Both int: use runtime checked division.
    if is_int_value(*lhs) && is_int_value(*rhs) {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                dest.ptr, dest.tydesc,
            )
        };
        if status == datalove_rt::c::RtStatus::Ok {
            return Ok(EvalResult::Ok);
        } else {
            write_result_err_early_return(ctx, return_dest, "division by zero")?;
            return Ok(EvalResult::EarlyReturn);
        }
    }

    // Fixed-width integers.
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Checked division requires fixed-width integer or int operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in checked division: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! checked_div {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_div(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_result_err_early_return(ctx, return_dest, "division by zero")?;
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => checked_div!(u8, write_u8_result),
        TyTag::I8 => checked_div!(i8, write_i8_result),
        TyTag::U16 => checked_div!(u16, write_u16_result),
        TyTag::I16 => checked_div!(i16, write_i16_result),
        TyTag::U32 => checked_div!(u32, write_u32_result),
        TyTag::I32 => checked_div!(i32, write_i32_result),
        TyTag::U64 => checked_div!(u64, write_u64_result),
        TyTag::I64 => checked_div!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for checked division: {:?}", lhs_tag)
        )),
    }
}

/// Optional addition: early-return Option::None on overflow.
pub(super) fn eval_add_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Optional addition requires fixed-width integer operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in optional addition: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! optional_add {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_add(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_option_none_early_return(return_dest);
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => optional_add!(u8, write_u8_result),
        TyTag::I8 => optional_add!(i8, write_i8_result),
        TyTag::U16 => optional_add!(u16, write_u16_result),
        TyTag::I16 => optional_add!(i16, write_i16_result),
        TyTag::U32 => optional_add!(u32, write_u32_result),
        TyTag::I32 => optional_add!(i32, write_i32_result),
        TyTag::U64 => optional_add!(u64, write_u64_result),
        TyTag::I64 => optional_add!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for optional addition: {:?}", lhs_tag)
        )),
    }
}

/// Optional subtraction: early-return Option::None on underflow.
pub(super) fn eval_sub_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Optional subtraction requires fixed-width integer operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in optional subtraction: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! optional_sub {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_sub(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_option_none_early_return(return_dest);
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => optional_sub!(u8, write_u8_result),
        TyTag::I8 => optional_sub!(i8, write_i8_result),
        TyTag::U16 => optional_sub!(u16, write_u16_result),
        TyTag::I16 => optional_sub!(i16, write_i16_result),
        TyTag::U32 => optional_sub!(u32, write_u32_result),
        TyTag::I32 => optional_sub!(i32, write_i32_result),
        TyTag::U64 => optional_sub!(u64, write_u64_result),
        TyTag::I64 => optional_sub!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for optional subtraction: {:?}", lhs_tag)
        )),
    }
}

/// Optional multiplication: early-return Option::None on overflow.
pub(super) fn eval_mul_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Optional multiplication requires fixed-width integer operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in optional multiplication: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! optional_mul {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_mul(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_option_none_early_return(return_dest);
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => optional_mul!(u8, write_u8_result),
        TyTag::I8 => optional_mul!(i8, write_i8_result),
        TyTag::U16 => optional_mul!(u16, write_u16_result),
        TyTag::I16 => optional_mul!(i16, write_i16_result),
        TyTag::U32 => optional_mul!(u32, write_u32_result),
        TyTag::I32 => optional_mul!(i32, write_i32_result),
        TyTag::U64 => optional_mul!(u64, write_u64_result),
        TyTag::I64 => optional_mul!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for optional multiplication: {:?}", lhs_tag)
        )),
    }
}

/// Optional division: early-return Option::None on division by zero.
pub(super) fn eval_div_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
    return_dest: Destination,
) -> Result<EvalResult, InterpError> {
    // Both int: use runtime optional division.
    if is_int_value(*lhs) && is_int_value(*rhs) {
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                _ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                dest.ptr, dest.tydesc,
            )
        };

        if status == datalove_rt::c::RtStatus::Ok {
            return Ok(EvalResult::Ok);
        } else {
            write_option_none_early_return(return_dest);
            return Ok(EvalResult::EarlyReturn);
        }
    }

    // Fixed-width integers.
    if !is_fixed_int_value(*lhs) || !is_fixed_int_value(*rhs) {
        return Err(InterpError::InvalidExpression(
            "Optional division requires fixed-width integer or int operands".to_string()
        ));
    }

    let lhs_tag = get_type_tag(*lhs);
    let rhs_tag = get_type_tag(*rhs);
    if lhs_tag != rhs_tag {
        return Err(InterpError::InvalidExpression(
            format!("Type mismatch in optional division: {:?} vs {:?}", lhs_tag, rhs_tag)
        ));
    }

    macro_rules! optional_div {
        ($ty:ty, $write_fn:ident) => {{
            let a = unsafe { *(lhs.ptr as *const $ty) };
            let b = unsafe { *(rhs.ptr as *const $ty) };
            match a.checked_div(b) {
                Some(r) => {
                    $write_fn(r, dest)?;
                    Ok(EvalResult::Ok)
                }
                None => {
                    write_option_none_early_return(return_dest);
                    Ok(EvalResult::EarlyReturn)
                }
            }
        }};
    }

    match lhs_tag {
        TyTag::U8 => optional_div!(u8, write_u8_result),
        TyTag::I8 => optional_div!(i8, write_i8_result),
        TyTag::U16 => optional_div!(u16, write_u16_result),
        TyTag::I16 => optional_div!(i16, write_i16_result),
        TyTag::U32 => optional_div!(u32, write_u32_result),
        TyTag::I32 => optional_div!(i32, write_i32_result),
        TyTag::U64 => optional_div!(u64, write_u64_result),
        TyTag::I64 => optional_div!(i64, write_i64_result),
        _ => Err(InterpError::InvalidExpression(
            format!("Unsupported type for optional division: {:?}", lhs_tag)
        )),
    }
}

/// Evaluate comparison operators (Lt, Le, Gt, Ge, Eq, Ne).
pub(super) fn eval_comparison<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::BinOp,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
    use crate::ast::BinOp;

    // Fast path for same-type fixed-width integers.
    if is_fixed_int_value(*lhs) && is_fixed_int_value(*rhs) {
        let lhs_tag = get_type_tag(*lhs);
        let rhs_tag = get_type_tag(*rhs);

        if lhs_tag == rhs_tag {
            let result = unsafe {
                match lhs_tag {
                    TyTag::U8 => {
                        let a = *(lhs.ptr as *const u8);
                        let b = *(rhs.ptr as *const u8);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::I8 => {
                        let a = *(lhs.ptr as *const i8);
                        let b = *(rhs.ptr as *const i8);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::U16 => {
                        let a = *(lhs.ptr as *const u16);
                        let b = *(rhs.ptr as *const u16);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::I16 => {
                        let a = *(lhs.ptr as *const i16);
                        let b = *(rhs.ptr as *const i16);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::U32 => {
                        let a = *(lhs.ptr as *const u32);
                        let b = *(rhs.ptr as *const u32);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::I32 => {
                        let a = *(lhs.ptr as *const i32);
                        let b = *(rhs.ptr as *const i32);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::U64 => {
                        let a = *(lhs.ptr as *const u64);
                        let b = *(rhs.ptr as *const u64);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    TyTag::I64 => {
                        let a = *(lhs.ptr as *const i64);
                        let b = *(rhs.ptr as *const i64);
                        match op {
                            BinOp::Lt => a < b,
                            BinOp::Le => a <= b,
                            BinOp::Gt => a > b,
                            BinOp::Ge => a >= b,
                            BinOp::Eq => a == b,
                            BinOp::Ne => a != b,
                            _ => unreachable!(),
                        }
                    }
                    _ => unreachable!(),
                }
            };
            return write_bool_result(result, dest);
        }
    }

    // Use generic runtime comparison (works for Int, f32, mixed types, etc).
    use datalove_rt::c::RtOrdering;

    let ordering = unsafe {
        datalove_rt::c::dtlv_rti_cmp_total_local(
            ctx.runtime.handle(),
            lhs.ptr,
            lhs.tydesc,
            rhs.ptr,
            rhs.tydesc,
        )
    };

    let result = match (op, ordering) {
        (BinOp::Lt, RtOrdering::Less) => true,
        (BinOp::Gt, RtOrdering::Greater) => true,
        (BinOp::Le, RtOrdering::Less | RtOrdering::Equal) => true,
        (BinOp::Ge, RtOrdering::Greater | RtOrdering::Equal) => true,
        (BinOp::Eq, RtOrdering::Equal) => true,
        (BinOp::Ne, RtOrdering::Less | RtOrdering::Greater) => true,
        (_, RtOrdering::Error) => {
            return Err(InterpError::RuntimeError("Comparison failed: type mismatch".into()));
        }
        _ => false,
    };

    write_bool_result(result, dest)
}
