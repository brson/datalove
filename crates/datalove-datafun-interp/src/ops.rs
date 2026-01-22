//! Binary and unary operations.
//!
//! Supports fixed-width integers (u8-u64, i8-i64), bigints (Int), floats (F32),
//! and booleans. Includes widening arithmetic (fixed-width operands to Int result)
//! and checked operations with overflow detection.

use datalove_datafun_ir::{BinOp, IrType, UnaryOp};
use datalove_rtdt as rtdt;

use crate::error::InterpError;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

/// Trait for checked integer operations (overflow detection).
trait CheckedIntOps: Sized + Default {
    fn overflowing_add_impl(self, rhs: Self) -> (Self, bool);
    fn overflowing_sub_impl(self, rhs: Self) -> (Self, bool);
    fn overflowing_mul_impl(self, rhs: Self) -> (Self, bool);
    fn checked_div_impl(self, rhs: Self) -> (Self, bool);
}

macro_rules! impl_checked_int_ops {
    ($ty:ty) => {
        impl CheckedIntOps for $ty {
            fn overflowing_add_impl(self, rhs: Self) -> (Self, bool) {
                self.overflowing_add(rhs)
            }
            fn overflowing_sub_impl(self, rhs: Self) -> (Self, bool) {
                self.overflowing_sub(rhs)
            }
            fn overflowing_mul_impl(self, rhs: Self) -> (Self, bool) {
                self.overflowing_mul(rhs)
            }
            fn checked_div_impl(self, rhs: Self) -> (Self, bool) {
                match self.checked_div(rhs) {
                    Some(r) => (r, false),
                    None => (0, true),
                }
            }
        }
    };
}

impl_checked_int_ops!(i8);
impl_checked_int_ops!(i16);
impl_checked_int_ops!(i32);
impl_checked_int_ops!(i64);
impl_checked_int_ops!(u8);
impl_checked_int_ops!(u16);
impl_checked_int_ops!(u32);
impl_checked_int_ops!(u64);
// IsizeRepr/UsizeRepr are type aliases to i32/u32 or i64/u64 - covered above.

/// Generate a binop function for a fixed-width integer type.
///
/// Only comparisons and bitwise operations are valid here. Arithmetic operations
/// (Add, Sub, Mul, Div, Mod) on fixed-width integers are either:
/// - Widened to Int (bigint) for regular `+`, `-`, `*`, `/`
/// - Handled via BinOpChecked for `+!`, `-!`, `*!`, `/!` and `+?`, `-?`, `*?`, `/?`
macro_rules! impl_int_binop {
    ($fname:ident, $ty:ty) => {
        fn $fname(
            op: BinOp,
            lhs: &Value,
            rhs: &Value,
            dest: Destination,
        ) {
            unsafe {
                let a = *(lhs.ptr as *const $ty);
                let b = *(rhs.ptr as *const $ty);
                match op {
                    // Arithmetic ops are unreachable - they widen to Int or use checked ops.
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        unreachable!(
                            "fixed-width arithmetic should use widening or checked ops, not BinOp"
                        )
                    }
                    // Comparisons are valid.
                    BinOp::Lt => *(dest.ptr as *mut bool) = a < b,
                    BinOp::Le => *(dest.ptr as *mut bool) = a <= b,
                    BinOp::Gt => *(dest.ptr as *mut bool) = a > b,
                    BinOp::Ge => *(dest.ptr as *mut bool) = a >= b,
                    BinOp::Eq => *(dest.ptr as *mut bool) = a == b,
                    BinOp::Ne => *(dest.ptr as *mut bool) = a != b,
                    // Bitwise operations are valid.
                    BinOp::BitAnd => *(dest.ptr as *mut $ty) = a & b,
                    BinOp::BitOr => *(dest.ptr as *mut $ty) = a | b,
                    BinOp::BitXor => *(dest.ptr as *mut $ty) = a ^ b,
                    BinOp::Shl => *(dest.ptr as *mut $ty) = a.wrapping_shl(b as u32),
                    BinOp::Shr => *(dest.ptr as *mut $ty) = a.wrapping_shr(b as u32),
                    // Type checker ensures only valid ops reach here.
                    _ => unreachable!("unsupported integer binop {:?}", op),
                }
            }
        }
    };
}

impl IrInterpreter {
    // Generate binop functions for each integer type.
    impl_int_binop!(execute_binop_i8, i8);
    impl_int_binop!(execute_binop_i16, i16);
    impl_int_binop!(execute_binop_i32, i32);
    impl_int_binop!(execute_binop_i64, i64);
    impl_int_binop!(execute_binop_u8, u8);
    impl_int_binop!(execute_binop_u16, u16);
    impl_int_binop!(execute_binop_u32, u32);
    impl_int_binop!(execute_binop_u64, u64);
    impl_int_binop!(execute_binop_usize, rtdt::UsizeRepr);
    impl_int_binop!(execute_binop_isize, rtdt::IsizeRepr);

    /// Check if a type tag is a fixed-width integer (u8-u64, i8-i64, usize, isize).
    pub(crate) fn is_fixed_width_int(tag: rtdt::TyTag) -> bool {
        matches!(
            tag,
            rtdt::TyTag::U8
                | rtdt::TyTag::U16
                | rtdt::TyTag::U32
                | rtdt::TyTag::U64
                | rtdt::TyTag::I8
                | rtdt::TyTag::I16
                | rtdt::TyTag::I32
                | rtdt::TyTag::I64
                | rtdt::TyTag::Usize
                | rtdt::TyTag::Isize
        )
    }

    /// Check if a type tag is a copy type (can be duplicated without ownership transfer).
    pub(crate) fn is_copy_type_tag(tag: rtdt::TyTag) -> bool {
        matches!(
            tag,
            rtdt::TyTag::Bool
                | rtdt::TyTag::U8
                | rtdt::TyTag::U16
                | rtdt::TyTag::U32
                | rtdt::TyTag::U64
                | rtdt::TyTag::I8
                | rtdt::TyTag::I16
                | rtdt::TyTag::I32
                | rtdt::TyTag::I64
                | rtdt::TyTag::Usize
                | rtdt::TyTag::Isize
                | rtdt::TyTag::F32
                | rtdt::TyTag::F64
        )
    }

    /// Widen a fixed-width integer value to an Int in a stack-allocated buffer.
    ///
    /// The caller is responsible for destroying the Int (freeing its limbs) after use.
    ///
    /// Panics if src is not a fixed-width integer type (compiler bug).
    pub(crate) unsafe fn widen_to_int(&self, src: &Value, int_buf: &mut rtdt::Int) {
        unsafe {
            let type_tag = (*src.tydesc).type_tag;

            // Extract magnitude and sign from the fixed-width integer.
            let (magnitude, is_negative): (u64, bool) = match type_tag {
                rtdt::TyTag::U8 => (*(src.ptr as *const u8) as u64, false),
                rtdt::TyTag::U16 => (*(src.ptr as *const u16) as u64, false),
                rtdt::TyTag::U32 => (*(src.ptr as *const u32) as u64, false),
                rtdt::TyTag::U64 => (*(src.ptr as *const u64), false),
                rtdt::TyTag::I8 => {
                    let v = *(src.ptr as *const i8);
                    if v < 0 {
                        ((-(v as i64)) as u64, true)
                    } else {
                        (v as u64, false)
                    }
                }
                rtdt::TyTag::I16 => {
                    let v = *(src.ptr as *const i16);
                    if v < 0 {
                        ((-(v as i64)) as u64, true)
                    } else {
                        (v as u64, false)
                    }
                }
                rtdt::TyTag::I32 => {
                    let v = *(src.ptr as *const i32);
                    if v < 0 {
                        ((-(v as i64)) as u64, true)
                    } else {
                        (v as u64, false)
                    }
                }
                rtdt::TyTag::I64 => {
                    let v = *(src.ptr as *const i64);
                    if v == i64::MIN {
                        // Special case: i64::MIN cannot be negated without overflow.
                        // Its magnitude is 2^63 = 0x8000_0000_0000_0000.
                        (0x8000_0000_0000_0000u64, true)
                    } else if v < 0 {
                        ((-v) as u64, true)
                    } else {
                        (v as u64, false)
                    }
                }
                rtdt::TyTag::Usize => {
                    (*(src.ptr as *const rtdt::UsizeRepr) as u64, false)
                }
                rtdt::TyTag::Isize => {
                    let v = *(src.ptr as *const rtdt::IsizeRepr);
                    #[cfg(not(feature = "index-64"))]
                    {
                        if v < 0 {
                            ((-(v as i64)) as u64, true)
                        } else {
                            (v as u64, false)
                        }
                    }
                    #[cfg(feature = "index-64")]
                    {
                        if v == i64::MIN {
                            (0x8000_0000_0000_0000u64, true)
                        } else if v < 0 {
                            ((-v) as u64, true)
                        } else {
                            (v as u64, false)
                        }
                    }
                }
                _ => panic!("widen_to_int: cannot widen type {:?} to Int", type_tag),
            };

            let rt_handle = self.runtime.handle();

            if magnitude == 0 {
                int_buf.data = std::ptr::null();
                int_buf.size_and_sign = 0;
                int_buf.capacity = rtdt::Usize::ZERO;
            } else if magnitude <= u32::MAX as u64 {
                // Fits in one limb.
                let limb_ptr =
                    datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, 4, 4, 1) as *mut u32;
                *limb_ptr = magnitude as u32;
                int_buf.data = limb_ptr;
                int_buf.size_and_sign = if is_negative { -1 } else { 1 };
                int_buf.capacity = rtdt::Usize(1);
            } else {
                // Needs two limbs (for u64/i64 values > u32::MAX).
                let limb_ptr =
                    datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, 4, 4, 2) as *mut u32;
                // Low limb first (little-endian limb order).
                *limb_ptr = magnitude as u32;
                *limb_ptr.add(1) = (magnitude >> 32) as u32;
                int_buf.data = limb_ptr;
                int_buf.size_and_sign = if is_negative { -2 } else { 2 };
                int_buf.capacity = rtdt::Usize(2);
            }
        }
    }

    /// Destroy a temporary Int's limb allocation.
    pub(crate) unsafe fn destroy_temp_int(&self, int_buf: &rtdt::Int) {
        unsafe {
            if !int_buf.data.is_null() && int_buf.capacity > rtdt::Usize::ZERO {
                datalove_rt::c::dtlv_rti_mem_free_raw_local(
                    self.runtime.handle(),
                    4, // align
                    4, // elem_size
                    int_buf.capacity.0,
                    int_buf.data as *mut u8,
                );
            }
        }
    }

    pub(crate) fn execute_binop(
        &mut self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) {
        unsafe {
            let lhs_tag = (*lhs.tydesc).type_tag;
            let dest_tag = (*dest.tydesc).type_tag;

            // Widening arithmetic: fixed-width int operands -> Int result.
            // This is triggered when dest is Int but operands are fixed-width ints.
            if dest_tag == rtdt::TyTag::Int && Self::is_fixed_width_int(lhs_tag) {
                self.execute_binop_widening(op, lhs, rhs, dest);
                return;
            }

            // Dispatch on operand type.
            match lhs_tag {
                rtdt::TyTag::I8 => Self::execute_binop_i8(op, lhs, rhs, dest),
                rtdt::TyTag::I16 => Self::execute_binop_i16(op, lhs, rhs, dest),
                rtdt::TyTag::I32 => Self::execute_binop_i32(op, lhs, rhs, dest),
                rtdt::TyTag::I64 => Self::execute_binop_i64(op, lhs, rhs, dest),
                rtdt::TyTag::U8 => Self::execute_binop_u8(op, lhs, rhs, dest),
                rtdt::TyTag::U16 => Self::execute_binop_u16(op, lhs, rhs, dest),
                rtdt::TyTag::U32 => Self::execute_binop_u32(op, lhs, rhs, dest),
                rtdt::TyTag::U64 => Self::execute_binop_u64(op, lhs, rhs, dest),
                rtdt::TyTag::Usize => Self::execute_binop_usize(op, lhs, rhs, dest),
                rtdt::TyTag::Isize => Self::execute_binop_isize(op, lhs, rhs, dest),
                rtdt::TyTag::Int => self.execute_binop_bigint(op, lhs, rhs, dest),
                rtdt::TyTag::F32 => Self::execute_binop_f32(op, lhs, rhs, dest),
                rtdt::TyTag::F64 => Self::execute_binop_f64(op, lhs, rhs, dest),
                rtdt::TyTag::Bool => Self::execute_binop_bool(op, lhs, rhs, dest),
                // Type checker ensures only valid types reach here.
                _ => unreachable!("unsupported binop {:?} for type {:?}", op, lhs_tag),
            }
        }
    }

    /// Widening arithmetic: fixed-width int operands -> Int result.
    fn execute_binop_widening(
        &mut self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) {
        use datalove_rt::c::RtStatus;

        unsafe {
            // Allocate temporary Ints on the stack for widened operands.
            let mut lhs_int = std::mem::MaybeUninit::<rtdt::Int>::uninit();
            let mut rhs_int = std::mem::MaybeUninit::<rtdt::Int>::uninit();

            self.widen_to_int(lhs, lhs_int.assume_init_mut());
            self.widen_to_int(rhs, rhs_int.assume_init_mut());

            let lhs_int = lhs_int.assume_init();
            let rhs_int = rhs_int.assume_init();

            let rt_handle = self.runtime.handle();
            let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

            let lhs_ptr = &lhs_int as *const rtdt::Int as *mut u8;
            let rhs_ptr = &rhs_int as *const rtdt::Int as *mut u8;

            let status = match op {
                BinOp::Add => datalove_rt::c::dtlv_rti_int_add(
                    rt_handle, lhs_ptr, int_tydesc, rhs_ptr, int_tydesc, dest.ptr, int_tydesc,
                ),
                BinOp::Sub => datalove_rt::c::dtlv_rti_int_sub(
                    rt_handle, lhs_ptr, int_tydesc, rhs_ptr, int_tydesc, dest.ptr, int_tydesc,
                ),
                BinOp::Mul => datalove_rt::c::dtlv_rti_int_mul(
                    rt_handle, lhs_ptr, int_tydesc, rhs_ptr, int_tydesc, dest.ptr, int_tydesc,
                ),
                // Only Add/Sub/Mul widen; type checker ensures this.
                _ => unreachable!("widening arithmetic not supported for {:?}", op),
            };

            self.destroy_temp_int(&lhs_int);
            self.destroy_temp_int(&rhs_int);

            if status != RtStatus::Ok {
                unreachable!("widening Int {:?} operation failed", op);
            }
        }
    }

    /// Bigint operations via runtime.
    fn execute_binop_bigint(
        &mut self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) {
        use datalove_rt::c::RtStatus;

        unsafe {
            let rt_handle = self.runtime.handle();
            let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

            let status = match op {
                BinOp::Add => datalove_rt::c::dtlv_rti_int_add(
                    rt_handle, lhs.ptr, int_tydesc, rhs.ptr, int_tydesc, dest.ptr, int_tydesc,
                ),
                BinOp::Sub => datalove_rt::c::dtlv_rti_int_sub(
                    rt_handle, lhs.ptr, int_tydesc, rhs.ptr, int_tydesc, dest.ptr, int_tydesc,
                ),
                BinOp::Mul => datalove_rt::c::dtlv_rti_int_mul(
                    rt_handle, lhs.ptr, int_tydesc, rhs.ptr, int_tydesc, dest.ptr, int_tydesc,
                ),
                BinOp::Div => {
                    unreachable!();
                }
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    use datalove_rt::c::RtOrdering;
                    let cmp = datalove_rt::c::dtlv_rti_cmp_local(
                        rt_handle, lhs.ptr, int_tydesc, rhs.ptr, int_tydesc,
                    );
                    let result = match op {
                        BinOp::Eq => cmp == RtOrdering::Equal,
                        BinOp::Ne => cmp != RtOrdering::Equal,
                        BinOp::Lt => cmp == RtOrdering::Less,
                        BinOp::Le => cmp == RtOrdering::Less || cmp == RtOrdering::Equal,
                        BinOp::Gt => cmp == RtOrdering::Greater,
                        BinOp::Ge => cmp == RtOrdering::Greater || cmp == RtOrdering::Equal,
                        _ => unreachable!(),
                    };
                    *(dest.ptr as *mut bool) = result;
                    return;
                }
                // Type checker ensures only arithmetic and comparisons reach here.
                _ => unreachable!("unsupported Int binop {:?}", op),
            };

            if status != RtStatus::Ok {
                unreachable!("unexpected runtime failure {:?}", status);
            }
        }
    }

    /// F32 binary operations.
    fn execute_binop_f32(
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) {
        unsafe {
            let a = *(lhs.ptr as *const f32);
            let b = *(rhs.ptr as *const f32);
            match op {
                BinOp::Add => *(dest.ptr as *mut f32) = a + b,
                BinOp::Sub => *(dest.ptr as *mut f32) = a - b,
                BinOp::Mul => *(dest.ptr as *mut f32) = a * b,
                BinOp::Div => *(dest.ptr as *mut f32) = a / b,
                BinOp::Lt => *(dest.ptr as *mut bool) = a < b,
                BinOp::Le => *(dest.ptr as *mut bool) = a <= b,
                BinOp::Gt => *(dest.ptr as *mut bool) = a > b,
                BinOp::Ge => *(dest.ptr as *mut bool) = a >= b,
                BinOp::Eq => *(dest.ptr as *mut bool) = a == b,
                BinOp::Ne => *(dest.ptr as *mut bool) = a != b,
                // Type checker ensures only valid ops reach here.
                _ => unreachable!("unsupported F32 binop {:?}", op),
            }
        }
    }

    /// F64 binary operations.
    fn execute_binop_f64(
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) {
        unsafe {
            let a = *(lhs.ptr as *const f64);
            let b = *(rhs.ptr as *const f64);
            match op {
                BinOp::Add => *(dest.ptr as *mut f64) = a + b,
                BinOp::Sub => *(dest.ptr as *mut f64) = a - b,
                BinOp::Mul => *(dest.ptr as *mut f64) = a * b,
                BinOp::Div => *(dest.ptr as *mut f64) = a / b,
                BinOp::Lt => *(dest.ptr as *mut bool) = a < b,
                BinOp::Le => *(dest.ptr as *mut bool) = a <= b,
                BinOp::Gt => *(dest.ptr as *mut bool) = a > b,
                BinOp::Ge => *(dest.ptr as *mut bool) = a >= b,
                BinOp::Eq => *(dest.ptr as *mut bool) = a == b,
                BinOp::Ne => *(dest.ptr as *mut bool) = a != b,
                // Type checker ensures only valid ops reach here.
                _ => unreachable!("unsupported F64 binop {:?}", op),
            }
        }
    }

    /// Boolean binary operations.
    fn execute_binop_bool(
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
    ) {
        unsafe {
            let a = *(lhs.ptr as *const bool);
            let b = *(rhs.ptr as *const bool);
            match op {
                BinOp::And | BinOp::LogicAnd => *(dest.ptr as *mut bool) = a && b,
                BinOp::Or | BinOp::LogicOr => *(dest.ptr as *mut bool) = a || b,
                BinOp::LogicXor => *(dest.ptr as *mut bool) = a ^ b,
                BinOp::Eq => *(dest.ptr as *mut bool) = a == b,
                BinOp::Ne => *(dest.ptr as *mut bool) = a != b,
                // Type checker ensures only valid ops reach here.
                _ => unreachable!("unsupported Bool binop {:?}", op),
            }
        }
    }

    pub(crate) fn execute_unaryop(
        &mut self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) {
        unsafe {
            let tag = (*src.tydesc).type_tag;

            // Dispatch on operand type.
            match tag {
                rtdt::TyTag::I8 => Self::execute_unaryop_signed::<i8>(op, src, dest),
                rtdt::TyTag::I16 => Self::execute_unaryop_signed::<i16>(op, src, dest),
                rtdt::TyTag::I32 => Self::execute_unaryop_signed::<i32>(op, src, dest),
                rtdt::TyTag::I64 => Self::execute_unaryop_signed::<i64>(op, src, dest),
                rtdt::TyTag::Isize => Self::execute_unaryop_signed::<rtdt::IsizeRepr>(op, src, dest),
                rtdt::TyTag::U8 => Self::execute_unaryop_unsigned::<u8>(op, src, dest),
                rtdt::TyTag::U16 => Self::execute_unaryop_unsigned::<u16>(op, src, dest),
                rtdt::TyTag::U32 => Self::execute_unaryop_unsigned::<u32>(op, src, dest),
                rtdt::TyTag::U64 => Self::execute_unaryop_unsigned::<u64>(op, src, dest),
                rtdt::TyTag::Usize => Self::execute_unaryop_unsigned::<rtdt::UsizeRepr>(op, src, dest),
                rtdt::TyTag::Int => self.execute_unaryop_bigint(op, src, dest),
                rtdt::TyTag::F32 => Self::execute_unaryop_f32(op, src, dest),
                rtdt::TyTag::F64 => Self::execute_unaryop_f64(op, src, dest),
                rtdt::TyTag::Bool => Self::execute_unaryop_bool(op, src, dest),
                // Type checker ensures only valid types reach here.
                _ => unreachable!("unsupported unaryop {:?} for type {:?}", op, tag),
            }
        }
    }

    /// Signed integer unary operations.
    fn execute_unaryop_signed<T>(
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    )
    where
        T: Copy + std::ops::Neg<Output = T> + std::ops::Not<Output = T>,
    {
        unsafe {
            let a = *(src.ptr as *const T);
            match op {
                UnaryOp::Neg => *(dest.ptr as *mut T) = -a,
                UnaryOp::BitNot => *(dest.ptr as *mut T) = !a,
                // Type checker ensures only valid ops reach here.
                _ => unreachable!("unsupported signed int unaryop {:?}", op),
            }
        }
    }

    /// Unsigned integer unary operations.
    fn execute_unaryop_unsigned<T>(
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    )
    where
        T: Copy + std::ops::Not<Output = T>,
    {
        unsafe {
            let a = *(src.ptr as *const T);
            match op {
                UnaryOp::BitNot => *(dest.ptr as *mut T) = !a,
                // Type checker ensures only valid ops reach here.
                _ => unreachable!("unsupported unsigned int unaryop {:?}", op),
            }
        }
    }

    /// Bigint unary operations.
    fn execute_unaryop_bigint(
        &mut self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) {
        use datalove_rt::c::RtStatus;

        unsafe {
            match op {
                UnaryOp::Neg => {
                    let rt_handle = self.runtime.handle();
                    let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);
                    let status = datalove_rt::c::dtlv_rti_int_neg(
                        rt_handle, src.ptr, int_tydesc, dest.ptr, int_tydesc,
                    );
                    if status != RtStatus::Ok {
                        unreachable!("Int negation failed");
                    }
                }
                // Type checker ensures only Neg reaches here.
                _ => unreachable!("unsupported Int unaryop {:?}", op),
            }
        }
    }

    /// F32 unary operations.
    fn execute_unaryop_f32(
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) {
        unsafe {
            let a = *(src.ptr as *const f32);
            match op {
                UnaryOp::Neg => *(dest.ptr as *mut f32) = -a,
                // Type checker ensures only Neg reaches here.
                _ => unreachable!("unsupported F32 unaryop {:?}", op),
            }
        }
    }

    /// F64 unary operations.
    fn execute_unaryop_f64(
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) {
        unsafe {
            let a = *(src.ptr as *const f64);
            match op {
                UnaryOp::Neg => *(dest.ptr as *mut f64) = -a,
                // Type checker ensures only Neg reaches here.
                _ => unreachable!("unsupported F64 unaryop {:?}", op),
            }
        }
    }

    /// Boolean unary operations.
    fn execute_unaryop_bool(
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) {
        unsafe {
            let a = *(src.ptr as *const bool);
            match op {
                UnaryOp::Not | UnaryOp::LogicNot => *(dest.ptr as *mut bool) = !a,
                // Type checker ensures only Not/LogicNot reach here.
                _ => unreachable!("unsupported Bool unaryop {:?}", op),
            }
        }
    }

    /// Execute checked arithmetic operation.
    pub(crate) fn execute_binop_checked(
        &self,
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
        overflow_dest: Destination,
    ) {
        unsafe {
            let tag = (*lhs.tydesc).type_tag;

            match tag {
                rtdt::TyTag::I8 => Self::execute_binop_checked_int::<i8>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::I16 => Self::execute_binop_checked_int::<i16>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::I32 => Self::execute_binop_checked_int::<i32>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::I64 => Self::execute_binop_checked_int::<i64>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::Isize => Self::execute_binop_checked_int::<rtdt::IsizeRepr>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::U8 => Self::execute_binop_checked_int::<u8>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::U16 => Self::execute_binop_checked_int::<u16>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::U32 => Self::execute_binop_checked_int::<u32>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::U64 => Self::execute_binop_checked_int::<u64>(op, lhs, rhs, dest, overflow_dest),
                rtdt::TyTag::Usize => Self::execute_binop_checked_int::<rtdt::UsizeRepr>(op, lhs, rhs, dest, overflow_dest),
                // Checked ops only apply to fixed-width ints; type checker ensures this.
                _ => unreachable!("unsupported checked binop {:?} for type {:?}", op, tag),
            }
        }
    }

    /// Checked integer binop implementation using generics.
    fn execute_binop_checked_int<T>(
        op: BinOp,
        lhs: &Value,
        rhs: &Value,
        dest: Destination,
        overflow_dest: Destination,
    )
    where
        T: Copy + CheckedIntOps,
    {
        unsafe {
            let a = *(lhs.ptr as *const T);
            let b = *(rhs.ptr as *const T);
            let (result, overflowed) = match op {
                BinOp::Add => a.overflowing_add_impl(b),
                BinOp::Sub => a.overflowing_sub_impl(b),
                BinOp::Mul => a.overflowing_mul_impl(b),
                BinOp::Div => a.checked_div_impl(b),
                // Only Add/Sub/Mul/Div have checked variants; type checker ensures this.
                _ => unreachable!("checked binop only supports Add/Sub/Mul/Div, got {:?}", op),
            };
            *(dest.ptr as *mut T) = result;
            *(overflow_dest.ptr as *mut bool) = overflowed;
        }
    }

    /// Execute checked unary operation (e.g., negation with overflow detection).
    pub(crate) fn execute_unaryop_checked(
        &self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
        overflow_dest: Destination,
    ) {
        // Only negation can overflow for signed integers; type checker ensures this.
        if op != UnaryOp::Neg {
            unreachable!("checked unaryop only supports Neg, got {:?}", op);
        }

        unsafe {
            let tag = (*src.tydesc).type_tag;

            match tag {
                rtdt::TyTag::I8 => Self::execute_checked_neg::<i8>(src, dest, overflow_dest),
                rtdt::TyTag::I16 => Self::execute_checked_neg::<i16>(src, dest, overflow_dest),
                rtdt::TyTag::I32 => Self::execute_checked_neg::<i32>(src, dest, overflow_dest),
                rtdt::TyTag::I64 => Self::execute_checked_neg::<i64>(src, dest, overflow_dest),
                rtdt::TyTag::Isize => Self::execute_checked_neg::<rtdt::IsizeRepr>(src, dest, overflow_dest),
                // Checked negation only for signed ints; type checker ensures this.
                _ => unreachable!("checked negation only supported for signed integers, got {:?}", tag),
            }
        }
    }

    /// Checked negation for signed integers.
    fn execute_checked_neg<T>(
        src: &Value,
        dest: Destination,
        overflow_dest: Destination,
    )
    where
        T: Copy + CheckedNeg,
    {
        unsafe {
            let a = *(src.ptr as *const T);
            let (result, overflowed) = a.overflowing_neg_impl();
            *(dest.ptr as *mut T) = result;
            *(overflow_dest.ptr as *mut bool) = overflowed;
        }
    }
}

/// Trait for checked negation.
trait CheckedNeg: Sized {
    fn overflowing_neg_impl(self) -> (Self, bool);
}

macro_rules! impl_checked_neg {
    ($ty:ty) => {
        impl CheckedNeg for $ty {
            fn overflowing_neg_impl(self) -> (Self, bool) {
                self.overflowing_neg()
            }
        }
    };
}

impl_checked_neg!(i8);
impl_checked_neg!(i16);
impl_checked_neg!(i32);
impl_checked_neg!(i64);
// IsizeRepr is a type alias to i32 or i64 - covered above.
