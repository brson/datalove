//! Checked and optional arithmetic operations.
//!
//! - Checked: return error on overflow/division-by-zero
//! - Optional: return OptionNone on overflow/division-by-zero

use datalove_rt::rtdt::TyTag;

use super::{InterpContext, InterpError, Value, Destination};
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

/// Checked addition: error on overflow.
pub(super) fn eval_add_checked<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_add(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_add(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_add(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_add(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_add(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_add(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_add(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_add(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked addition: {:?}", lhs_tag)
            )),
        }
    }
}

/// Checked subtraction: error on underflow.
pub(super) fn eval_sub_checked<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_sub(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_sub(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_sub(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_sub(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_sub(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_sub(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_sub(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_sub(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked subtraction: {:?}", lhs_tag)
            )),
        }
    }
}

/// Checked multiplication: error on overflow.
pub(super) fn eval_mul_checked<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_mul(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_mul(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_mul(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_mul(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_mul(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_mul(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_mul(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_mul(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked multiplication: {:?}", lhs_tag)
            )),
        }
    }
}

/// Checked division: error on division by zero.
pub(super) fn eval_div_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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
            return Ok(());
        } else {
            return Err(InterpError::DivisionByZero);
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_div(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_div(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_div(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_div(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_div(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_div(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_div(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_div(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked division: {:?}", lhs_tag)
            )),
        }
    }
}

/// Optional addition: OptionNone on overflow.
pub(super) fn eval_add_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_add(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_add(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_add(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_add(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_add(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_add(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_add(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_add(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional addition: {:?}", lhs_tag)
            )),
        }
    }
}

/// Optional subtraction: OptionNone on underflow.
pub(super) fn eval_sub_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_sub(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_sub(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_sub(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_sub(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_sub(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_sub(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_sub(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_sub(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional subtraction: {:?}", lhs_tag)
            )),
        }
    }
}

/// Optional multiplication: OptionNone on overflow.
pub(super) fn eval_mul_optional<'db>(
    _ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_mul(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_mul(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_mul(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_mul(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_mul(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_mul(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_mul(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_mul(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional multiplication: {:?}", lhs_tag)
            )),
        }
    }
}

/// Optional division: OptionNone on division by zero.
pub(super) fn eval_div_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Destination,
) -> Result<(), InterpError> {
    // Both int: use runtime optional division.
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
            return Ok(());
        } else {
            return Err(InterpError::OptionNone);
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

    unsafe {
        match lhs_tag {
            TyTag::U8 => {
                let a = *(lhs.ptr as *const u8);
                let b = *(rhs.ptr as *const u8);
                match a.checked_div(b) {
                    Some(r) => write_u8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_div(b) {
                    Some(r) => write_i8_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_div(b) {
                    Some(r) => write_u16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_div(b) {
                    Some(r) => write_i16_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_div(b) {
                    Some(r) => write_u32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_div(b) {
                    Some(r) => write_i32_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_div(b) {
                    Some(r) => write_u64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_div(b) {
                    Some(r) => write_i64_result(r, dest),
                    None => Err(InterpError::OptionNone),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional division: {:?}", lhs_tag)
            )),
        }
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
