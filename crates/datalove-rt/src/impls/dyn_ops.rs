//! Operators on values whose type only a descriptor says.
//!
//! A generic bounded to `float` may add, subtract, multiply, divide and compare
//! its type parameter. Erasure compiles one body, so the machine code cannot
//! hold an `fadd` for one call and an `fadd.f32` for another: which float it is
//! is not known until the call supplies it. The operation comes here with the
//! descriptors instead, and the tag says what to do.
//!
//! That is the same bargain every collection here already makes. A list of a
//! type parameter has its elements walked by a runtime that reads their size
//! off a descriptor rather than by code that knew it; this reads an operator's
//! operands the same way.
//!
//! The operands may arrive wrapped, because a type parameter is a `data` where
//! it is written, and the result goes back wrapped if that is what the
//! destination is. Both are undone and redone here so that no backend has to
//! know which case it is looking at.

use datalove_rtdt as rtdt;
use rtdt::{DynOp, DynUnOp, TyTag};

use crate::c::{LocalRtHandle, RtStatus};

/// A float and which width it is.
enum Float {
    F32(f32),
    F64(f64),
}

/// Read a float that may have arrived wrapped.
///
/// A float is small enough that a `data` holds it in its own words rather than
/// on the heap, so there is no address to borrow and the value is read out
/// instead. The descriptor comes back with it, because packing the answer needs
/// to say what the answer is.
unsafe fn read_float(
    value: *const u8,
    tydesc: *const rtdt::TyDesc,
) -> core::option::Option<(Float, *const rtdt::TyDesc)> {
    unsafe {
        match (*tydesc).type_tag {
            TyTag::F32 => core::option::Option::Some((Float::F32(*(value as *const f32)), tydesc)),
            TyTag::F64 => core::option::Option::Some((Float::F64(*(value as *const f64)), tydesc)),
            TyTag::Data => {
                let data = &*(value as *const rtdt::Data);
                let inner = data.tydesc();
                match data.tytag() {
                    TyTag::F32 => data.as_f32().map(|v| (Float::F32(v), inner)),
                    TyTag::F64 => data.as_f64().map(|v| (Float::F64(v), inner)),
                    _ => core::option::Option::None,
                }
            }
            _ => core::option::Option::None,
        }
    }
}

/// Apply `op` to two floats whose type is read off their descriptors.
///
/// Both operands must be the same float type; the call site has already been
/// told they are, because a bound gives one type parameter one type.
pub unsafe fn float_binop(
    rt: LocalRtHandle,
    op: DynOp,
    lhs: *const u8,
    lhs_tydesc: *const rtdt::TyDesc,
    rhs: *const u8,
    rhs_tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let core::option::Option::Some((lhs, lhs_ty)) = read_float(lhs, lhs_tydesc) else {
            return RtStatus::Error;
        };
        let core::option::Option::Some((rhs, _)) = read_float(rhs, rhs_tydesc) else {
            return RtStatus::Error;
        };

        if op.gives_bool() {
            let answer = match (lhs, rhs) {
                (Float::F32(a), Float::F32(b)) => compare(op, a, b),
                (Float::F64(a), Float::F64(b)) => compare(op, a, b),
                // One type parameter is one type, so the two always agree.
                _ => return RtStatus::Error,
            };
            // A comparison gives a bool, which is never a type parameter and so
            // never wrapped.
            *(out as *mut bool) = answer;
            return RtStatus::Ok;
        }

        // The answer has the operands' type, and goes wherever the destination
        // wants it: wrapped where that is a type parameter, plain otherwise.
        match (lhs, rhs) {
            (Float::F32(a), Float::F32(b)) => {
                let value = arithmetic(op, a, b);
                place(rt, &value as *const f32 as *const u8, lhs_ty, out, out_tydesc)
            }
            (Float::F64(a), Float::F64(b)) => {
                let value = arithmetic(op, a, b);
                place(rt, &value as *const f64 as *const u8, lhs_ty, out, out_tydesc)
            }
            _ => RtStatus::Error,
        }
    }
}

/// Put a freshly computed value where the destination wants it, wrapping it if
/// the destination is a `data`.
unsafe fn place(
    rt: LocalRtHandle,
    value: *const u8,
    value_tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        if (*out_tydesc).type_tag == TyTag::Data {
            return crate::impls::boxing::data_from_local(rt, value, value_tydesc, out);
        }
        let size = (*value_tydesc).size as usize;
        std::ptr::copy_nonoverlapping(value, out, size);
        RtStatus::Ok
    }
}

fn arithmetic<T>(op: DynOp, lhs: T, rhs: T) -> T
where
    T: std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<Output = T>
        + std::ops::Div<Output = T>,
{
    match op {
        DynOp::Add => lhs + rhs,
        DynOp::Sub => lhs - rhs,
        DynOp::Mul => lhs * rhs,
        DynOp::Div => lhs / rhs,
        // The caller asked whether the operator gives a bool before coming here.
        _ => unreachable!("a comparison is not arithmetic"),
    }
}

fn compare<T: PartialOrd + PartialEq>(op: DynOp, lhs: T, rhs: T) -> bool {
    match op {
        DynOp::Eq => lhs == rhs,
        DynOp::Ne => lhs != rhs,
        DynOp::Lt => lhs < rhs,
        DynOp::Le => lhs <= rhs,
        DynOp::Gt => lhs > rhs,
        DynOp::Ge => lhs >= rhs,
        _ => unreachable!("arithmetic is not a comparison"),
    }
}

/// Apply a one-operand `op` to a float whose type is read off its descriptor.
pub unsafe fn float_unop(
    rt: LocalRtHandle,
    op: DynUnOp,
    value: *const u8,
    value_tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let core::option::Option::Some((x, x_ty)) = read_float(value, value_tydesc) else {
            return RtStatus::Error;
        };
        match x {
            Float::F32(v) => {
                let answer = unary_f32(op, v);
                place(rt, &answer as *const f32 as *const u8, x_ty, out, out_tydesc)
            }
            Float::F64(v) => {
                let answer = unary_f64(op, v);
                place(rt, &answer as *const f64 as *const u8, x_ty, out, out_tydesc)
            }
        }
    }
}

fn unary_f32(op: DynUnOp, x: f32) -> f32 {
    match op {
        DynUnOp::Abs => x.abs(),
        DynUnOp::Sqrt => x.sqrt(),
        DynUnOp::Floor => x.floor(),
        DynUnOp::Ceil => x.ceil(),
        DynUnOp::Round => x.round(),
        DynUnOp::Trunc => x.trunc(),
        DynUnOp::Fract => x.fract(),
        DynUnOp::Recip => x.recip(),
        DynUnOp::Signum => x.signum(),
        DynUnOp::Neg => -x,
    }
}

fn unary_f64(op: DynUnOp, x: f64) -> f64 {
    match op {
        DynUnOp::Abs => x.abs(),
        DynUnOp::Sqrt => x.sqrt(),
        DynUnOp::Floor => x.floor(),
        DynUnOp::Ceil => x.ceil(),
        DynUnOp::Round => x.round(),
        DynUnOp::Trunc => x.trunc(),
        DynUnOp::Fract => x.fract(),
        DynUnOp::Recip => x.recip(),
        DynUnOp::Signum => x.signum(),
        DynUnOp::Neg => -x,
    }
}
