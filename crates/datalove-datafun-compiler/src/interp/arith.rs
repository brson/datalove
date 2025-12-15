//! Arithmetic operations for the interpreter.
//!
//! Handles addition, subtraction, multiplication, division with automatic
//! type widening (u32 -> Int) and various error handling modes:
//! - Widening: u32 operations widen to Int
//! - Checked: return error on overflow/division-by-zero
//! - Optional: return None on overflow/division-by-zero

use datalove_rt::rtdt::{OptionTag, TyDescRef};
use datalove_rt::rtdt::layout::compute_option_layout;

use super::{InterpContext, InterpError, Value, Destination, ValueLocation};
use super::memory::destroy_value;
use super::types::{is_u32_value, is_int_value};
use super::alloc::{allocate_bool, allocate_f32, allocate_u32_raw, allocate_bigint};

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

/// Write Option<u32> result to destination or allocate new value.
pub(super) fn write_option_u32_result(ctx: &mut InterpContext<'_>, value: Option<u32>, dest: Option<Destination>) -> Result<Value, InterpError> {
    let u32_tydesc = ctx.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::U32);
    let option_tydesc = ctx.tydesc_table.create_option_from_inner_tydesc(u32_tydesc);
    let option_ref = unsafe { TyDescRef::from_ptr(option_tydesc) };
    let layout = unsafe { compute_option_layout(option_ref) };

    let dest_size = dest.map(|d| unsafe { (*d.tydesc).size });
    let use_dest = dest.is_some() && dest_size == Some(option_ref.size());

    if let (Some(d), true) = (dest, use_dest) {
        unsafe {
            match value {
                Some(v) => {
                    *(d.ptr as *mut u8) = OptionTag::Some as u8;
                    let payload_ptr = d.ptr.add(layout.payload_offset as usize);
                    *(payload_ptr as *mut u32) = v;
                }
                None => {
                    *(d.ptr as *mut u8) = OptionTag::None as u8;
                }
            }
        }
        Ok(Value { ptr: d.ptr, tydesc: d.tydesc, location: ValueLocation::Borrowed })
    } else {
        let rt_handle = ctx.runtime.handle();
        let ptr = unsafe {
            datalove_rt::c::dtlv_rti_mem_alloc_local(rt_handle, option_tydesc, 1)
        };
        if ptr.is_null() {
            return Err(InterpError::RuntimeError("Failed to allocate Option<u32>".to_string()));
        }
        unsafe {
            match value {
                Some(v) => {
                    *(ptr as *mut u8) = OptionTag::Some as u8;
                    let payload_ptr = ptr.add(layout.payload_offset as usize);
                    *(payload_ptr as *mut u32) = v;
                }
                None => {
                    *(ptr as *mut u8) = OptionTag::None as u8;
                }
            }
        }
        Ok(Value { ptr, tydesc: option_tydesc, location: ValueLocation::TempOwned })
    }
}

// ============================================================================
// Checked Arithmetic
// ============================================================================

/// Evaluate checked addition (returns error on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_add_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(*lhs) || !is_u32_value(*rhs) {
        return Err(InterpError::InvalidExpression("Checked addition requires u32 operands".to_string()));
    }
    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    match lhs_val.checked_add(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::Overflow),
    }
}

/// Evaluate checked subtraction (returns error on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_sub_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(*lhs) || !is_u32_value(*rhs) {
        return Err(InterpError::InvalidExpression("Checked subtraction requires u32 operands".to_string()));
    }
    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    match lhs_val.checked_sub(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::Overflow),
    }
}

/// Evaluate checked multiplication (returns error on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_mul_checked<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(*lhs) || !is_u32_value(*rhs) {
        return Err(InterpError::InvalidExpression("Checked multiplication requires u32 operands".to_string()));
    }
    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    match lhs_val.checked_mul(rhs_val) {
        Some(result) => write_u32_result(ctx, result, dest),
        None => Err(InterpError::Overflow),
    }
}

/// Evaluate checked division (returns error on division by zero).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
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

    // Both u32: use Rust checked_div.
    if is_u32_value(*lhs) && is_u32_value(*rhs) {
        let lhs_val = unsafe { *(lhs.ptr as *const u32) };
        let rhs_val = unsafe { *(rhs.ptr as *const u32) };
        match lhs_val.checked_div(rhs_val) {
            Some(result) => write_u32_result(ctx, result, dest),
            None => Err(InterpError::DivisionByZero),
        }
    } else {
        Err(InterpError::InvalidExpression("Checked division requires matching operand types".to_string()))
    }
}

// ============================================================================
// Optional Arithmetic
// ============================================================================

/// Evaluate optional addition (returns None on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_add_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(*lhs) || !is_u32_value(*rhs) {
        return Err(InterpError::InvalidExpression("Optional addition requires u32 operands".to_string()));
    }
    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    write_option_u32_result(ctx, lhs_val.checked_add(rhs_val), dest)
}

/// Evaluate optional subtraction (returns None on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_sub_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(*lhs) || !is_u32_value(*rhs) {
        return Err(InterpError::InvalidExpression("Optional subtraction requires u32 operands".to_string()));
    }
    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    write_option_u32_result(ctx, lhs_val.checked_sub(rhs_val), dest)
}

/// Evaluate optional multiplication (returns None on overflow).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_mul_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    if !is_u32_value(*lhs) || !is_u32_value(*rhs) {
        return Err(InterpError::InvalidExpression("Optional multiplication requires u32 operands".to_string()));
    }
    let lhs_val = unsafe { *(lhs.ptr as *const u32) };
    let rhs_val = unsafe { *(rhs.ptr as *const u32) };
    write_option_u32_result(ctx, lhs_val.checked_mul(rhs_val), dest)
}

/// Evaluate optional division (returns None on division by zero).
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_div_optional<'db>(
    ctx: &mut InterpContext<'db>,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
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

    // Both u32: use Rust checked_div and wrap in Option.
    if is_u32_value(*lhs) && is_u32_value(*rhs) {
        let lhs_val = unsafe { *(lhs.ptr as *const u32) };
        let rhs_val = unsafe { *(rhs.ptr as *const u32) };
        write_option_u32_result(ctx, lhs_val.checked_div(rhs_val), dest)
    } else {
        Err(InterpError::InvalidExpression("Optional division requires matching operand types".to_string()))
    }
}

// ============================================================================
// Comparison
// ============================================================================

/// Evaluate comparison operators.
///
/// Operands are borrowed (ref semantics) - caller manages their lifetime.
pub(super) fn eval_comparison<'db>(
    ctx: &mut InterpContext<'db>,
    op: crate::ast::BinOp,
    lhs: &Value,
    rhs: &Value,
    dest: Option<Destination>,
) -> Result<Value, InterpError> {
    use crate::ast::BinOp;

    // Both u32: direct comparison.
    if is_u32_value(*lhs) && is_u32_value(*rhs) {
        let lhs_val = unsafe { *(lhs.ptr as *const u32) };
        let rhs_val = unsafe { *(rhs.ptr as *const u32) };
        let result = match op {
            BinOp::Lt => lhs_val < rhs_val,
            BinOp::Le => lhs_val <= rhs_val,
            BinOp::Gt => lhs_val > rhs_val,
            BinOp::Ge => lhs_val >= rhs_val,
            BinOp::Eq => lhs_val == rhs_val,
            BinOp::Ne => lhs_val != rhs_val,
            _ => unreachable!(),
        };
        return write_bool_result(ctx, result, dest);
    }

    // Use generic runtime comparison (works for u32, Int, f32, etc).
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
