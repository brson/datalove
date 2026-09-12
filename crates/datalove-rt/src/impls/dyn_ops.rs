//! Operators on values whose type only a descriptor says.
//!
//! A generic bounded to `float` or `fixedint` may operate on its type
//! parameter. Erasure compiles one body, so the machine code cannot hold an
//! `fadd.f32` for one call and an `fadd.f64` for another: which type it is is
//! not known until the call supplies it. The operation comes here with the
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
//!
//! Integer arithmetic is done in 128 bits and narrowed back on the way out.
//! Every fixed width this language has fits twice over in that, so the wide
//! sum is exact and the narrowing is where overflow is noticed — the same
//! answer a width-specific `overflowing_add` would give, decided once instead
//! of ten times.

use datalove_rtdt as rtdt;
use rtdt::anypack::Tag;
use rtdt::{DynOp, DynUnOp, TyTag};

use crate::c::{LocalRtHandle, RtStatus};

/// A number and the kind of arithmetic its type wants.
///
/// Signed and unsigned are kept apart because their comparison and division
/// differ; the two float widths are kept apart because rounding does.
#[derive(Copy, Clone)]
enum Num {
    Signed(i128),
    Unsigned(u128),
    F32(f32),
    F64(f64),
}

/// The raw bits of a scalar, wherever this particular value keeps them.
unsafe fn scalar_bits(value: *const u8, tag: TyTag) -> u64 {
    unsafe {
        match tag {
            TyTag::U8 | TyTag::I8 => *value as u64,
            TyTag::U16 | TyTag::I16 => *(value as *const u16) as u64,
            TyTag::U32 | TyTag::I32 | TyTag::F32 => *(value as *const u32) as u64,
            _ => *(value as *const u64),
        }
    }
}

/// Read the bits out of a wrapped value.
///
/// A narrow scalar and a 64-bit one ride in the `data`'s own words; anything
/// that cannot be packed that way, which among the numbers is `index` and
/// `offset`, sits behind a pointer instead.
unsafe fn wrapped_bits(data: &rtdt::Data) -> u64 {
    unsafe {
        match data.tag() {
            Tag::TwoPointers => scalar_bits(data.value_ptr(), data.tytag()),
            _ => data.inline_bits(),
        }
    }
}

/// Interpret bits as the number its tag says they are.
fn from_bits(bits: u64, tag: TyTag) -> core::option::Option<Num> {
    core::option::Option::Some(match tag {
        TyTag::U8 => Num::Unsigned(bits as u8 as u128),
        TyTag::U16 => Num::Unsigned(bits as u16 as u128),
        TyTag::U32 => Num::Unsigned(bits as u32 as u128),
        TyTag::U64 => Num::Unsigned(bits as u128),
        TyTag::Index => Num::Unsigned(bits as rtdt::IndexRepr as u128),
        TyTag::I8 => Num::Signed(bits as u8 as i8 as i128),
        TyTag::I16 => Num::Signed(bits as u16 as i16 as i128),
        TyTag::I32 => Num::Signed(bits as u32 as i32 as i128),
        TyTag::I64 => Num::Signed(bits as i64 as i128),
        TyTag::Offset => Num::Signed(bits as rtdt::OffsetRepr as i128),
        TyTag::F32 => Num::F32(f32::from_bits(bits as u32)),
        TyTag::F64 => Num::F64(f64::from_bits(bits)),
        _ => return core::option::Option::None,
    })
}

/// Read a number that may have arrived wrapped.
///
/// The descriptor of what was really there comes back with it, because a
/// wrapped operand's static descriptor says only `data` and packing the answer
/// needs to say what the answer is.
unsafe fn read_num(
    value: *const u8,
    tydesc: *const rtdt::TyDesc,
) -> core::option::Option<(Num, TyTag, *const rtdt::TyDesc)> {
    unsafe {
        let tag = (*tydesc).type_tag;
        if tag == TyTag::Data {
            let data = &*(value as *const rtdt::Data);
            let inner_tag = data.tytag();
            let num = from_bits(wrapped_bits(data), inner_tag)?;
            return core::option::Option::Some((num, inner_tag, data.tydesc()));
        }
        let num = from_bits(scalar_bits(value, tag), tag)?;
        core::option::Option::Some((num, tag, tydesc))
    }
}

/// How wide a value of this tag is, in bytes.
fn width(tag: TyTag) -> usize {
    match tag {
        TyTag::U8 | TyTag::I8 => 1,
        TyTag::U16 | TyTag::I16 => 2,
        TyTag::U32 | TyTag::I32 | TyTag::F32 => 4,
        _ => 8,
    }
}

/// Narrow a wide signed result to `tag`'s width.
///
/// The bits come back wrapped whether or not they fit, because that is what a
/// width-specific `overflowing_add` gives and the caller may want it; the flag
/// says which happened.
fn pack_signed(value: i128, tag: TyTag) -> (u64, bool) {
    macro_rules! narrow {
        ($t:ty) => {{
            let fits = value >= <$t>::MIN as i128 && value <= <$t>::MAX as i128;
            (value as $t as u64, !fits)
        }};
    }
    match tag {
        TyTag::I8 => {
            let fits = value >= i8::MIN as i128 && value <= i8::MAX as i128;
            (value as i8 as u8 as u64, !fits)
        }
        TyTag::I16 => {
            let fits = value >= i16::MIN as i128 && value <= i16::MAX as i128;
            (value as i16 as u16 as u64, !fits)
        }
        TyTag::I32 => {
            let fits = value >= i32::MIN as i128 && value <= i32::MAX as i128;
            (value as i32 as u32 as u64, !fits)
        }
        TyTag::I64 => narrow!(i64),
        TyTag::Offset => narrow!(rtdt::OffsetRepr),
        _ => unreachable!("{:?} is not a signed integer", tag),
    }
}

/// Narrow a wide unsigned result to `tag`'s width, as `pack_signed` does.
fn pack_unsigned(value: u128, tag: TyTag) -> (u64, bool) {
    macro_rules! narrow {
        ($t:ty) => {{
            let fits = value <= <$t>::MAX as u128;
            (value as $t as u64, !fits)
        }};
    }
    match tag {
        TyTag::U8 => narrow!(u8),
        TyTag::U16 => narrow!(u16),
        TyTag::U32 => narrow!(u32),
        TyTag::U64 => narrow!(u64),
        TyTag::Index => narrow!(rtdt::IndexRepr),
        _ => unreachable!("{:?} is not an unsigned integer", tag),
    }
}

/// Narrow a result that may have gone below zero into an unsigned width.
///
/// The bits come back wrapped either way, as `pack_signed`'s do: a negative
/// value taken to the width is that width's two's complement, which is the
/// answer the instruction would have given.
fn pack_unsigned_wide(value: i128, tag: TyTag) -> (u64, bool) {
    macro_rules! narrow {
        ($t:ty) => {{
            let fits = value >= 0 && value <= <$t>::MAX as i128;
            (value as u64 as $t as u64, !fits)
        }};
    }
    match tag {
        TyTag::U8 => narrow!(u8),
        TyTag::U16 => narrow!(u16),
        TyTag::U32 => narrow!(u32),
        TyTag::U64 => narrow!(u64),
        TyTag::Index => narrow!(rtdt::IndexRepr),
        _ => unreachable!("{:?} is not an unsigned integer", tag),
    }
}

/// Put a freshly computed value where the destination wants it, wrapping it if
/// the destination is a `data`.
unsafe fn place_bits(
    rt: LocalRtHandle,
    bits: u64,
    tag: TyTag,
    tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        // Written through a pointer of the right width rather than copied from
        // the low bytes, so that the byte order is the machine's own.
        let mut buf = [0u8; 8];
        let value = buf.as_mut_ptr();
        match width(tag) {
            1 => *value = bits as u8,
            2 => *(value as *mut u16) = bits as u16,
            4 => *(value as *mut u32) = bits as u32,
            _ => *(value as *mut u64) = bits,
        }

        if (*out_tydesc).type_tag == TyTag::Data {
            return crate::impls::boxing::data_from_local_tagged(
                rt, value, tag, tydesc, out);
        }
        std::ptr::copy_nonoverlapping(value as *const u8, out, width(tag));
        RtStatus::Ok
    }
}

/// The bits of a float, ready for `place_bits`.
fn float_bits(value: Num) -> u64 {
    match value {
        Num::F32(v) => v.to_bits() as u64,
        Num::F64(v) => v.to_bits(),
        _ => unreachable!("not a float"),
    }
}

/// Apply `op` to two numbers whose type is read off their descriptors.
///
/// Both operands are the same type: the call site was told so, because a bound
/// gives one type parameter one type.
///
/// Only the comparisons and the float arithmetic get here. A fixed-width
/// integer has no bare `+`, so a `T is fixedint` cannot ask for one either, and
/// its arithmetic goes through `dyn_binop_checked` instead.
pub unsafe fn dyn_binop(
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
        let core::option::Option::Some((lhs, tag, lhs_ty)) = read_num(lhs, lhs_tydesc) else {
            return RtStatus::Error;
        };
        let core::option::Option::Some((rhs, _, _)) = read_num(rhs, rhs_tydesc) else {
            return RtStatus::Error;
        };

        if op.gives_bool() {
            let answer = match (lhs, rhs) {
                (Num::Signed(a), Num::Signed(b)) => compare(op, a, b),
                (Num::Unsigned(a), Num::Unsigned(b)) => compare(op, a, b),
                (Num::F32(a), Num::F32(b)) => compare(op, a, b),
                (Num::F64(a), Num::F64(b)) => compare(op, a, b),
                // One type parameter is one type, so the two always agree.
                _ => return RtStatus::Error,
            };
            // A comparison gives a bool, which is never a type parameter and so
            // never wrapped.
            *(out as *mut bool) = answer;
            return RtStatus::Ok;
        }

        let answer = match (lhs, rhs) {
            (Num::F32(a), Num::F32(b)) => Num::F32(arithmetic(op, a, b)),
            (Num::F64(a), Num::F64(b)) => Num::F64(arithmetic(op, a, b)),
            // An integer's bare arithmetic is refused at the call site, so
            // nothing asks for it here.
            _ => return RtStatus::Error,
        };
        place_bits(rt, float_bits(answer), tag, lhs_ty, out, out_tydesc)
    }
}

/// Apply a checked `op` to two integers whose type is read off their
/// descriptors, reporting whether the answer fit.
///
/// The result is written either way, wrapped where it did not fit, which is
/// what the width-specific instruction does and what the caller's overflow
/// branch expects to be able to drop.
pub unsafe fn dyn_binop_checked(
    rt: LocalRtHandle,
    op: DynOp,
    lhs: *const u8,
    lhs_tydesc: *const rtdt::TyDesc,
    rhs: *const u8,
    rhs_tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
    overflow_out: *mut bool,
) -> RtStatus {
    unsafe {
        let core::option::Option::Some((lhs, tag, lhs_ty)) = read_num(lhs, lhs_tydesc) else {
            return RtStatus::Error;
        };
        let core::option::Option::Some((rhs, _, _)) = read_num(rhs, rhs_tydesc) else {
            return RtStatus::Error;
        };

        let (bits, overflowed) = match (lhs, rhs) {
            (Num::Signed(a), Num::Signed(b)) => match op {
                DynOp::Div if b == 0 => (0, true),
                // The one division that overflows: the most negative value has
                // no positive counterpart to become. A division that fails
                // gives zero rather than a wrapped answer, which is what the
                // width-specific `checked_div` gives.
                DynOp::Div => match pack_signed(a / b, tag) {
                    (_, true) => (0, true),
                    fitted => fitted,
                },
                _ => pack_signed(arithmetic(op, a, b), tag),
            },
            (Num::Unsigned(a), Num::Unsigned(b)) => match op {
                DynOp::Div if b == 0 => (0, true),
                // Taken signed, because this is the one unsigned operation
                // whose answer can be below the bottom of the range and the
                // wide subtraction would go below zero too.
                DynOp::Sub => pack_unsigned_wide(a as i128 - b as i128, tag),
                _ => pack_unsigned(arithmetic(op, a, b), tag),
            },
            // Checked arithmetic is for the fixed-width integers alone.
            _ => return RtStatus::Error,
        };

        *overflow_out = overflowed;
        place_bits(rt, bits, tag, lhs_ty, out, out_tydesc)
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

/// Apply a one-operand `op` to a value whose type is read off its descriptor.
pub unsafe fn dyn_unop(
    rt: LocalRtHandle,
    op: DynUnOp,
    value: *const u8,
    value_tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    unsafe {
        let core::option::Option::Some((x, tag, x_ty)) = read_num(value, value_tydesc) else {
            return RtStatus::Error;
        };
        let answer = match x {
            Num::F32(v) => Num::F32(unary_f32(op, v)),
            Num::F64(v) => Num::F64(unary_f64(op, v)),
            // An integer's one-operand work is the rider's, not an operator's.
            _ => return RtStatus::Error,
        };
        place_bits(rt, float_bits(answer), tag, x_ty, out, out_tydesc)
    }
}

/// Negate an integer whose type is read off its descriptor, reporting whether
/// the answer fit.
///
/// This is the whole of `dyn_unop`'s integer half: the only one-operand
/// operator a fixed-width integer has is negation, and it is always checked
/// because the most negative value has no counterpart and zero is the only
/// unsigned value that survives.
pub unsafe fn dyn_neg_checked(
    rt: LocalRtHandle,
    value: *const u8,
    value_tydesc: *const rtdt::TyDesc,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
    overflow_out: *mut bool,
) -> RtStatus {
    unsafe {
        let core::option::Option::Some((x, tag, x_ty)) = read_num(value, value_tydesc) else {
            return RtStatus::Error;
        };
        let (bits, overflowed) = match x {
            Num::Signed(v) => pack_signed(-v, tag),
            Num::Unsigned(v) => {
                if v == 0 {
                    (0, false)
                } else {
                    // Wraps to the width's complement, as the instruction does.
                    let (bits, _) = pack_unsigned(v.wrapping_neg(), tag);
                    (bits, true)
                }
            }
            _ => return RtStatus::Error,
        };
        *overflow_out = overflowed;
        place_bits(rt, bits, tag, x_ty, out, out_tydesc)
    }
}

/// Make a constant at the type a descriptor names.
///
/// The descriptor comes from the call site, because a type parameter that
/// appears only in a native's return type has no argument to bring it. See
/// `NativeContext::descriptor_shapes`.
pub unsafe fn dyn_const(
    rt: LocalRtHandle,
    which: rtdt::DynConst,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    use rtdt::DynConst;

    unsafe {
        let tag = (*value_tydesc).type_tag;

        // Written as the widest of each signedness and narrowed, so that the
        // bounds are read off the target type in one place rather than listed
        // per width twice over.
        let bits = match which {
            DynConst::Zero => 0,
            DynConst::One => match signedness(tag) {
                core::option::Option::Some(true) => pack_signed(1, tag).0,
                core::option::Option::Some(false) => pack_unsigned(1, tag).0,
                core::option::Option::None => return RtStatus::Error,
            },
            DynConst::MinValue => match limits(tag) {
                core::option::Option::Some((low, _)) => low,
                core::option::Option::None => return RtStatus::Error,
            },
            DynConst::MaxValue => match limits(tag) {
                core::option::Option::Some((_, high)) => high,
                core::option::Option::None => return RtStatus::Error,
            },
        };

        place_bits(rt, bits, tag, value_tydesc, out, out_tydesc)
    }
}

/// Make a float constant at the width a descriptor names.
///
/// The counterpart of `dyn_const` for the floats. Written out per width rather
/// than computed from one, because the two are different types and the
/// constants of each are its own.
pub unsafe fn dyn_float_const(
    rt: LocalRtHandle,
    which: rtdt::DynFloatConst,
    out: *mut u8,
    out_tydesc: *const rtdt::TyDesc,
    value_tydesc: *const rtdt::TyDesc,
) -> RtStatus {
    use rtdt::DynFloatConst as K;

    unsafe {
        let tag = (*value_tydesc).type_tag;
        let bits = match tag {
            TyTag::F32 => {
                let v: f32 = match which {
                    K::Zero => 0.0,
                    K::One => 1.0,
                    K::Nan => f32::NAN,
                    K::Infinity => f32::INFINITY,
                    K::NegInfinity => f32::NEG_INFINITY,
                    K::MinValue => f32::MIN,
                    K::MaxValue => f32::MAX,
                    K::MinPositive => f32::MIN_POSITIVE,
                    K::Epsilon => f32::EPSILON,
                    K::Pi => std::f32::consts::PI,
                    K::E => std::f32::consts::E,
                };
                v.to_bits() as u64
            }
            TyTag::F64 => {
                let v: f64 = match which {
                    K::Zero => 0.0,
                    K::One => 1.0,
                    K::Nan => f64::NAN,
                    K::Infinity => f64::INFINITY,
                    K::NegInfinity => f64::NEG_INFINITY,
                    K::MinValue => f64::MIN,
                    K::MaxValue => f64::MAX,
                    K::MinPositive => f64::MIN_POSITIVE,
                    K::Epsilon => f64::EPSILON,
                    K::Pi => std::f64::consts::PI,
                    K::E => std::f64::consts::E,
                };
                v.to_bits()
            }
            _ => return RtStatus::Error,
        };

        place_bits(rt, bits, tag, value_tydesc, out, out_tydesc)
    }
}

/// Whether a tag is a signed integer, or none when it is not an integer.
fn signedness(tag: TyTag) -> core::option::Option<bool> {
    core::option::Option::Some(match tag {
        TyTag::I8 | TyTag::I16 | TyTag::I32 | TyTag::I64 | TyTag::Offset => true,
        TyTag::U8 | TyTag::U16 | TyTag::U32 | TyTag::U64 | TyTag::Index => false,
        _ => return core::option::Option::None,
    })
}

/// The smallest and largest value of a fixed-width integer, as its own bits.
fn limits(tag: TyTag) -> core::option::Option<(u64, u64)> {
    macro_rules! signed {
        ($t:ty) => {
            (pack_signed(<$t>::MIN as i128, tag).0, pack_signed(<$t>::MAX as i128, tag).0)
        };
    }
    macro_rules! unsigned {
        ($t:ty) => { (0, pack_unsigned(<$t>::MAX as u128, tag).0) };
    }
    core::option::Option::Some(match tag {
        TyTag::I8 => signed!(i8),
        TyTag::I16 => signed!(i16),
        TyTag::I32 => signed!(i32),
        TyTag::I64 => signed!(i64),
        TyTag::Offset => signed!(rtdt::OffsetRepr),
        TyTag::U8 => unsigned!(u8),
        TyTag::U16 => unsigned!(u16),
        TyTag::U32 => unsigned!(u32),
        TyTag::U64 => unsigned!(u64),
        TyTag::Index => unsigned!(rtdt::IndexRepr),
        _ => return core::option::Option::None,
    })
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
