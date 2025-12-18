//! Arithmetic operations for the interpreter.
//!
//! Handles addition, subtraction, multiplication, division with automatic
//! type widening (u32 -> Int) and various error handling modes:
//! - Widening: u32 operations widen to Int
//! - Checked: return error on overflow/division-by-zero
//! - Optional: return None on overflow/division-by-zero

use datalove_rt::rtdt::{OptionTag, TyDescRef, TyTag};
use datalove_rt::rtdt::layout::compute_option_layout;

use super::{InterpContext, InterpError, Value, Destination, ValueLocation};
use super::memory::destroy_value;
use super::types::{is_int_value, is_fixed_int_value, get_type_tag};
use super::alloc::{
    allocate_bool, allocate_f32, allocate_u32_raw, allocate_bigint,
    allocate_u8, allocate_i8, allocate_u16, allocate_i16,
    allocate_i32, allocate_u64, allocate_i64,
};

// ============================================================================
// Result Writers
// ============================================================================

/// Write u32 result to destination or allocate new value.
pub(super) fn write_u32_result(ctx: &mut InterpContext<'_>, value: u32, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_u32_raw(ctx, value)
    }
}

/// Write f32 result to destination or allocate new value.
pub(super) fn write_f32_result(ctx: &mut InterpContext<'_>, value: f32, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut f32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_f32(ctx, value)
    }
}

/// Write bool result to destination or allocate new value.
pub(super) fn write_bool_result(ctx: &mut InterpContext<'_>, value: bool, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u8) = if value { 1 } else { 0 }; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_bool(ctx, value)
    }
}

/// Write u8 result to destination or allocate new value.
fn write_u8_result(ctx: &mut InterpContext<'_>, value: u8, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *d.ptr = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_u8(ctx, value)
    }
}

/// Write i8 result to destination or allocate new value.
fn write_i8_result(ctx: &mut InterpContext<'_>, value: i8, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut i8) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_i8(ctx, value)
    }
}

/// Write u16 result to destination or allocate new value.
fn write_u16_result(ctx: &mut InterpContext<'_>, value: u16, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u16) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_u16(ctx, value)
    }
}

/// Write i16 result to destination or allocate new value.
fn write_i16_result(ctx: &mut InterpContext<'_>, value: i16, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut i16) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_i16(ctx, value)
    }
}

/// Write i32 result to destination or allocate new value.
fn write_i32_result(ctx: &mut InterpContext<'_>, value: i32, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut i32) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_i32(ctx, value)
    }
}

/// Write u64 result to destination or allocate new value.
fn write_u64_result(ctx: &mut InterpContext<'_>, value: u64, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut u64) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_u64(ctx, value)
    }
}

/// Write i64 result to destination or allocate new value.
fn write_i64_result(ctx: &mut InterpContext<'_>, value: i64, dest: Option<Destination>) -> Result<Value, InterpError> {
    if let Some(d) = dest {
        unsafe { *(d.ptr as *mut i64) = value; }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        allocate_i64(ctx, value)
    }
}

/// Write Option result for any fixed-width integer type.
///
/// Writes the tag and payload based on the inner tydesc.
fn write_option_fixed_int_result(
    ctx: &mut InterpContext<'_>,
    has_value: bool,
    payload_ptr: *const u8,
    inner_tydesc: *const datalove_rt::rtdt::TyDesc,
) -> Result<Value, InterpError> {
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(inner_tydesc);
    let option_ref = unsafe { TyDescRef::from_ptr(option_tydesc) };
    let layout = compute_option_layout(option_ref);

    let rt_handle = ctx.runtime.handle();
    let ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, option_tydesc, 1)
    };
    if ptr.is_null() {
        return Err(InterpError::RuntimeError("Failed to allocate Option".to_string()));
    }

    unsafe {
        if has_value {
            *ptr = OptionTag::Some as u8;
            let dest_payload_ptr = ptr.add(layout.payload_offset as usize);
            let inner_size = (*inner_tydesc).size as usize;
            std::ptr::copy_nonoverlapping(payload_ptr, dest_payload_ptr, inner_size);
        } else {
            *ptr = OptionTag::None as u8;
        }
    }

    Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
}

// ============================================================================
// Checked Arithmetic
// ============================================================================

/// Evaluate checked addition (returns error on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64).
pub(super) fn eval_add_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
                    Some(r) => write_u8_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_add(b) {
                    Some(r) => write_i8_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_add(b) {
                    Some(r) => write_u16_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_add(b) {
                    Some(r) => write_i16_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_add(b) {
                    Some(r) => write_u32_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_add(b) {
                    Some(r) => write_i32_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_add(b) {
                    Some(r) => write_u64_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_add(b) {
                    Some(r) => write_i64_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked addition: {:?}", lhs_tag)
            )),
        }
    }
}

/// Evaluate checked subtraction (returns error on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64).
pub(super) fn eval_sub_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
                    Some(r) => write_u8_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_sub(b) {
                    Some(r) => write_i8_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_sub(b) {
                    Some(r) => write_u16_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_sub(b) {
                    Some(r) => write_i16_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_sub(b) {
                    Some(r) => write_u32_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_sub(b) {
                    Some(r) => write_i32_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_sub(b) {
                    Some(r) => write_u64_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_sub(b) {
                    Some(r) => write_i64_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked subtraction: {:?}", lhs_tag)
            )),
        }
    }
}

/// Evaluate checked multiplication (returns error on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64).
pub(super) fn eval_mul_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
                    Some(r) => write_u8_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_mul(b) {
                    Some(r) => write_i8_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_mul(b) {
                    Some(r) => write_u16_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_mul(b) {
                    Some(r) => write_i16_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_mul(b) {
                    Some(r) => write_u32_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_mul(b) {
                    Some(r) => write_i32_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_mul(b) {
                    Some(r) => write_u64_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_mul(b) {
                    Some(r) => write_i64_result(ctx, r, dest),
                    None => Err(InterpError::Overflow),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked multiplication: {:?}", lhs_tag)
            )),
        }
    }
}

/// Evaluate checked division (returns error on division by zero or overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64) and int.
pub(super) fn eval_div_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both int: use runtime checked division.
    if is_int_value(*lhs) && is_int_value(*rhs) {
        let result_int = allocate_bigint(ctx)?;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_int.ptr, result_int.tydesc,
            )
        };
        if status == datalove_rt::c::RtStatus::Ok {
            return Ok(result_int);
        } else {
            destroy_value(ctx, result_int);
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
                    Some(r) => write_u8_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                match a.checked_div(b) {
                    Some(r) => write_i8_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                match a.checked_div(b) {
                    Some(r) => write_u16_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                match a.checked_div(b) {
                    Some(r) => write_i16_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                match a.checked_div(b) {
                    Some(r) => write_u32_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                match a.checked_div(b) {
                    Some(r) => write_i32_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                match a.checked_div(b) {
                    Some(r) => write_u64_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                match a.checked_div(b) {
                    Some(r) => write_i64_result(ctx, r, dest),
                    None => Err(InterpError::DivisionByZero),
                }
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for checked division: {:?}", lhs_tag)
            )),
        }
    }
}

// ============================================================================
// Optional Arithmetic
// ============================================================================

/// Evaluate optional addition (returns None on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64).
pub(super) fn eval_add_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    _dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
                let result = a.checked_add(b);
                let mut val: u8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u8) }
                    None => (false, &val as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                let result = a.checked_add(b);
                let mut val: i8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i8 as *const u8) }
                    None => (false, &val as *const i8 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                let result = a.checked_add(b);
                let mut val: u16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u16 as *const u8) }
                    None => (false, &val as *const u16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                let result = a.checked_add(b);
                let mut val: i16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i16 as *const u8) }
                    None => (false, &val as *const i16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                let result = a.checked_add(b);
                let mut val: u32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u32 as *const u8) }
                    None => (false, &val as *const u32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                let result = a.checked_add(b);
                let mut val: i32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i32 as *const u8) }
                    None => (false, &val as *const i32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                let result = a.checked_add(b);
                let mut val: u64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u64 as *const u8) }
                    None => (false, &val as *const u64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                let result = a.checked_add(b);
                let mut val: i64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i64 as *const u8) }
                    None => (false, &val as *const i64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional addition: {:?}", lhs_tag)
            )),
        }
    }
}

/// Evaluate optional subtraction (returns None on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64).
pub(super) fn eval_sub_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    _dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
                let result = a.checked_sub(b);
                let mut val: u8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u8) }
                    None => (false, &val as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                let result = a.checked_sub(b);
                let mut val: i8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i8 as *const u8) }
                    None => (false, &val as *const i8 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                let result = a.checked_sub(b);
                let mut val: u16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u16 as *const u8) }
                    None => (false, &val as *const u16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                let result = a.checked_sub(b);
                let mut val: i16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i16 as *const u8) }
                    None => (false, &val as *const i16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                let result = a.checked_sub(b);
                let mut val: u32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u32 as *const u8) }
                    None => (false, &val as *const u32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                let result = a.checked_sub(b);
                let mut val: i32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i32 as *const u8) }
                    None => (false, &val as *const i32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                let result = a.checked_sub(b);
                let mut val: u64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u64 as *const u8) }
                    None => (false, &val as *const u64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                let result = a.checked_sub(b);
                let mut val: i64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i64 as *const u8) }
                    None => (false, &val as *const i64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional subtraction: {:?}", lhs_tag)
            )),
        }
    }
}

/// Evaluate optional multiplication (returns None on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64).
pub(super) fn eval_mul_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    _dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
                let result = a.checked_mul(b);
                let mut val: u8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u8) }
                    None => (false, &val as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                let result = a.checked_mul(b);
                let mut val: i8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i8 as *const u8) }
                    None => (false, &val as *const i8 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                let result = a.checked_mul(b);
                let mut val: u16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u16 as *const u8) }
                    None => (false, &val as *const u16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                let result = a.checked_mul(b);
                let mut val: i16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i16 as *const u8) }
                    None => (false, &val as *const i16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                let result = a.checked_mul(b);
                let mut val: u32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u32 as *const u8) }
                    None => (false, &val as *const u32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                let result = a.checked_mul(b);
                let mut val: i32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i32 as *const u8) }
                    None => (false, &val as *const i32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                let result = a.checked_mul(b);
                let mut val: u64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u64 as *const u8) }
                    None => (false, &val as *const u64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                let result = a.checked_mul(b);
                let mut val: i64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i64 as *const u8) }
                    None => (false, &val as *const i64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional multiplication: {:?}", lhs_tag)
            )),
        }
    }
}

/// Evaluate optional division (returns None on division by zero).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Supports all fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64) and int.
pub(super) fn eval_div_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    _dest: Option<Destination>,
) -> Result<Value, InterpError> {
    // Both int: use runtime optional division.
    if is_int_value(*lhs) && is_int_value(*rhs) {
        let result_int = allocate_bigint(ctx)?;
        let status = unsafe {
            datalove_rt::c::dtlv_rti_int_div_checked(
                ctx.runtime.handle(),
                lhs.ptr, lhs.tydesc,
                rhs.ptr, rhs.tydesc,
                result_int.ptr, result_int.tydesc,
            )
        };

        if status == datalove_rt::c::RtStatus::Ok {
            return Ok(result_int);
        } else {
            destroy_value(ctx, result_int);
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
                let result = a.checked_div(b);
                let mut val: u8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u8) }
                    None => (false, &val as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I8 => {
                let a = *(lhs.ptr as *const i8);
                let b = *(rhs.ptr as *const i8);
                let result = a.checked_div(b);
                let mut val: i8 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i8 as *const u8) }
                    None => (false, &val as *const i8 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U16 => {
                let a = *(lhs.ptr as *const u16);
                let b = *(rhs.ptr as *const u16);
                let result = a.checked_div(b);
                let mut val: u16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u16 as *const u8) }
                    None => (false, &val as *const u16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I16 => {
                let a = *(lhs.ptr as *const i16);
                let b = *(rhs.ptr as *const i16);
                let result = a.checked_div(b);
                let mut val: i16 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i16 as *const u8) }
                    None => (false, &val as *const i16 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U32 => {
                let a = *(lhs.ptr as *const u32);
                let b = *(rhs.ptr as *const u32);
                let result = a.checked_div(b);
                let mut val: u32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u32 as *const u8) }
                    None => (false, &val as *const u32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I32 => {
                let a = *(lhs.ptr as *const i32);
                let b = *(rhs.ptr as *const i32);
                let result = a.checked_div(b);
                let mut val: i32 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i32 as *const u8) }
                    None => (false, &val as *const i32 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::U64 => {
                let a = *(lhs.ptr as *const u64);
                let b = *(rhs.ptr as *const u64);
                let result = a.checked_div(b);
                let mut val: u64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const u64 as *const u8) }
                    None => (false, &val as *const u64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            TyTag::I64 => {
                let a = *(lhs.ptr as *const i64);
                let b = *(rhs.ptr as *const i64);
                let result = a.checked_div(b);
                let mut val: i64 = 0;
                let (has_value, payload_ptr) = match result {
                    Some(r) => { val = r; (true, &val as *const i64 as *const u8) }
                    None => (false, &val as *const i64 as *const u8)
                };
                write_option_fixed_int_result(ctx, has_value, payload_ptr, lhs.tydesc)
            }
            _ => Err(InterpError::InvalidExpression(
                format!("Unsupported type for optional division: {:?}", lhs_tag)
            )),
        }
    }
}

// ============================================================================
// Comparison
// ============================================================================

/// Evaluate comparison operators.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
/// Provides fast paths for all fixed-width integer types.
pub(super) fn eval_comparison<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::BinOp,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
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
            return write_bool_result(ctx, result, dest);
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

    write_bool_result(ctx, result, dest)
}
