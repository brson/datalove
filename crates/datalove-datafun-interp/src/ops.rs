//! Binary and unary operations.
//!
//! Supports fixed-width integers (u8-u64, i8-i64), bigints (Int), floats (F32),
//! and booleans. Includes widening arithmetic (fixed-width operands to Int result)
//! and checked operations with overflow detection.

use datalove_datafun_ir::{BinOp, IrType, UnaryOp};
use datalove_rt::rtdt;

use crate::error::InterpError;
use crate::value::{Destination, Value};
use crate::IrInterpreter;

impl IrInterpreter {
    /// Check if a type tag is a fixed-width integer (u8-u64, i8-i64).
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
                | rtdt::TyTag::F32
                | rtdt::TyTag::F64
        )
    }

    /// Widen a fixed-width integer value to an Int in a stack-allocated buffer.
    ///
    /// Returns the widened Int representation. The caller is responsible for
    /// destroying the Int (freeing its limbs) after use.
    pub(crate) unsafe fn widen_to_int(
        &self,
        src: &Value,
        int_buf: &mut rtdt::Int,
    ) -> Result<(), InterpError> {
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
                _ => {
                    return Err(InterpError::TypeMismatch(format!(
                        "Cannot widen type {:?} to Int",
                        type_tag
                    )))
                }
            };

            let rt_handle = self.runtime.handle();

            if magnitude == 0 {
                int_buf.data = std::ptr::null();
                int_buf.size_and_sign = 0;
                int_buf.capacity = 0;
            } else if magnitude <= u32::MAX as u64 {
                // Fits in one limb.
                let limb_ptr =
                    datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, 4, 4, 1) as *mut u32;
                *limb_ptr = magnitude as u32;
                int_buf.data = limb_ptr;
                int_buf.size_and_sign = if is_negative { -1 } else { 1 };
                int_buf.capacity = 1;
            } else {
                // Needs two limbs (for u64/i64 values > u32::MAX).
                let limb_ptr =
                    datalove_rt::c::dtlv_rti_mem_alloc_raw_local(rt_handle, 4, 4, 2) as *mut u32;
                // Low limb first (little-endian limb order).
                *limb_ptr = magnitude as u32;
                *limb_ptr.add(1) = (magnitude >> 32) as u32;
                int_buf.data = limb_ptr;
                int_buf.size_and_sign = if is_negative { -2 } else { 2 };
                int_buf.capacity = 2;
            }
        }

        Ok(())
    }

    /// Destroy a temporary Int's limb allocation.
    pub(crate) unsafe fn destroy_temp_int(&self, int_buf: &rtdt::Int) {
        unsafe {
            if !int_buf.data.is_null() && int_buf.capacity > 0 {
                datalove_rt::c::dtlv_rti_mem_free_raw_local(
                    self.runtime.handle(),
                    4, // align
                    4, // elem_size
                    int_buf.capacity,
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
    ) -> Result<(), InterpError> {
        // Macro to generate binop implementations for integer types.
        macro_rules! int_binop {
            ($tag:ident, $ty:ty, $lhs:expr, $rhs:expr, $dest:expr, $op:expr) => {
                if (*$lhs.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($lhs.ptr as *const $ty);
                    let b = *($rhs.ptr as *const $ty);
                    match $op {
                        BinOp::Add => {
                            *($dest.ptr as *mut $ty) = a.wrapping_add(b);
                            return Ok(());
                        }
                        BinOp::Sub => {
                            *($dest.ptr as *mut $ty) = a.wrapping_sub(b);
                            return Ok(());
                        }
                        BinOp::Mul => {
                            *($dest.ptr as *mut $ty) = a.wrapping_mul(b);
                            return Ok(());
                        }
                        BinOp::Div => {
                            if b == 0 {
                                return Err(InterpError::DivisionByZero);
                            }
                            *($dest.ptr as *mut $ty) = a.wrapping_div(b);
                            return Ok(());
                        }
                        BinOp::Mod => {
                            if b == 0 {
                                return Err(InterpError::DivisionByZero);
                            }
                            *($dest.ptr as *mut $ty) = a.wrapping_rem(b);
                            return Ok(());
                        }
                        BinOp::Lt => {
                            *($dest.ptr as *mut bool) = a < b;
                            return Ok(());
                        }
                        BinOp::Le => {
                            *($dest.ptr as *mut bool) = a <= b;
                            return Ok(());
                        }
                        BinOp::Gt => {
                            *($dest.ptr as *mut bool) = a > b;
                            return Ok(());
                        }
                        BinOp::Ge => {
                            *($dest.ptr as *mut bool) = a >= b;
                            return Ok(());
                        }
                        BinOp::Eq => {
                            *($dest.ptr as *mut bool) = a == b;
                            return Ok(());
                        }
                        BinOp::Ne => {
                            *($dest.ptr as *mut bool) = a != b;
                            return Ok(());
                        }
                        BinOp::BitAnd => {
                            *($dest.ptr as *mut $ty) = a & b;
                            return Ok(());
                        }
                        BinOp::BitOr => {
                            *($dest.ptr as *mut $ty) = a | b;
                            return Ok(());
                        }
                        BinOp::BitXor => {
                            *($dest.ptr as *mut $ty) = a ^ b;
                            return Ok(());
                        }
                        BinOp::Shl => {
                            *($dest.ptr as *mut $ty) = a.wrapping_shl(b as u32);
                            return Ok(());
                        }
                        BinOp::Shr => {
                            *($dest.ptr as *mut $ty) = a.wrapping_shr(b as u32);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            };
        }

        unsafe {
            let lhs_tag = (*lhs.tydesc).type_tag;
            let dest_tag = (*dest.tydesc).type_tag;

            // Widening arithmetic: fixed-width int operands -> Int result.
            // This is triggered when dest is Int but operands are fixed-width ints.
            if dest_tag == rtdt::TyTag::Int && Self::is_fixed_width_int(lhs_tag) {
                use datalove_rt::c::RtStatus;

                // Allocate temporary Ints on the stack for widened operands.
                let mut lhs_int = std::mem::MaybeUninit::<rtdt::Int>::uninit();
                let mut rhs_int = std::mem::MaybeUninit::<rtdt::Int>::uninit();

                self.widen_to_int(lhs, lhs_int.assume_init_mut())?;
                self.widen_to_int(rhs, rhs_int.assume_init_mut())?;

                let lhs_int = lhs_int.assume_init();
                let rhs_int = rhs_int.assume_init();

                let rt_handle = self.runtime.handle();
                let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

                let lhs_ptr = &lhs_int as *const rtdt::Int as *mut u8;
                let rhs_ptr = &rhs_int as *const rtdt::Int as *mut u8;

                let status = match op {
                    BinOp::Add => datalove_rt::c::dtlv_rti_int_add(
                        rt_handle,
                        lhs_ptr,
                        int_tydesc,
                        rhs_ptr,
                        int_tydesc,
                        dest.ptr,
                        int_tydesc,
                    ),
                    BinOp::Sub => datalove_rt::c::dtlv_rti_int_sub(
                        rt_handle,
                        lhs_ptr,
                        int_tydesc,
                        rhs_ptr,
                        int_tydesc,
                        dest.ptr,
                        int_tydesc,
                    ),
                    BinOp::Mul => datalove_rt::c::dtlv_rti_int_mul(
                        rt_handle,
                        lhs_ptr,
                        int_tydesc,
                        rhs_ptr,
                        int_tydesc,
                        dest.ptr,
                        int_tydesc,
                    ),
                    _ => {
                        // Clean up temporaries before returning error.
                        self.destroy_temp_int(&lhs_int);
                        self.destroy_temp_int(&rhs_int);
                        return Err(InterpError::TypeMismatch(format!(
                            "widening arithmetic not supported for {:?}",
                            op
                        )));
                    }
                };

                // Clean up temporary Int allocations.
                self.destroy_temp_int(&lhs_int);
                self.destroy_temp_int(&rhs_int);

                if status != RtStatus::Ok {
                    return Err(InterpError::RuntimeError(format!(
                        "widening Int {:?} operation failed",
                        op
                    )));
                }
                return Ok(());
            }

            // Same-type operations: fixed-width int operands with fixed-width int result.
            let tag = lhs_tag;

            // Try all integer types.
            int_binop!(I8, i8, lhs, rhs, dest, op);
            int_binop!(I16, i16, lhs, rhs, dest, op);
            int_binop!(I32, i32, lhs, rhs, dest, op);
            int_binop!(I64, i64, lhs, rhs, dest, op);
            int_binop!(U8, u8, lhs, rhs, dest, op);
            int_binop!(U16, u16, lhs, rhs, dest, op);
            int_binop!(U32, u32, lhs, rhs, dest, op);
            int_binop!(U64, u64, lhs, rhs, dest, op);

            // Bigint operations via runtime (when operands are already Int).
            if tag == rtdt::TyTag::Int {
                use datalove_rt::c::RtStatus;

                let rt_handle = self.runtime.handle();
                let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

                let status = match op {
                    BinOp::Add => datalove_rt::c::dtlv_rti_int_add(
                        rt_handle,
                        lhs.ptr,
                        int_tydesc,
                        rhs.ptr,
                        int_tydesc,
                        dest.ptr,
                        int_tydesc,
                    ),
                    BinOp::Sub => datalove_rt::c::dtlv_rti_int_sub(
                        rt_handle,
                        lhs.ptr,
                        int_tydesc,
                        rhs.ptr,
                        int_tydesc,
                        dest.ptr,
                        int_tydesc,
                    ),
                    BinOp::Mul => datalove_rt::c::dtlv_rti_int_mul(
                        rt_handle,
                        lhs.ptr,
                        int_tydesc,
                        rhs.ptr,
                        int_tydesc,
                        dest.ptr,
                        int_tydesc,
                    ),
                    BinOp::Div => {
                        let status = datalove_rt::c::dtlv_rti_int_div_checked(
                            rt_handle,
                            lhs.ptr,
                            int_tydesc,
                            rhs.ptr,
                            int_tydesc,
                            dest.ptr,
                            int_tydesc,
                        );
                        if status != RtStatus::Ok {
                            return Err(InterpError::DivisionByZero);
                        }
                        return Ok(());
                    }
                    // Int comparison operations via runtime.
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        use datalove_rt::c::RtOrdering;
                        let cmp = datalove_rt::c::dtlv_rti_cmp_local(
                            rt_handle,
                            lhs.ptr,
                            int_tydesc,
                            rhs.ptr,
                            int_tydesc,
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
                        return Ok(());
                    }
                    _ => {
                        return Err(InterpError::TypeMismatch(format!(
                            "unsupported Int binop {:?}",
                            op
                        )))
                    }
                };

                if status != RtStatus::Ok {
                    return Err(InterpError::RuntimeError(format!(
                        "Int {:?} operation failed",
                        op
                    )));
                }
                return Ok(());
            }

            // F32 operations.
            if tag == rtdt::TyTag::F32 {
                let a = *(lhs.ptr as *const f32);
                let b = *(rhs.ptr as *const f32);
                match op {
                    BinOp::Add => {
                        *(dest.ptr as *mut f32) = a + b;
                        return Ok(());
                    }
                    BinOp::Sub => {
                        *(dest.ptr as *mut f32) = a - b;
                        return Ok(());
                    }
                    BinOp::Mul => {
                        *(dest.ptr as *mut f32) = a * b;
                        return Ok(());
                    }
                    BinOp::Div => {
                        *(dest.ptr as *mut f32) = a / b;
                        return Ok(());
                    }
                    BinOp::Lt => {
                        *(dest.ptr as *mut bool) = a < b;
                        return Ok(());
                    }
                    BinOp::Le => {
                        *(dest.ptr as *mut bool) = a <= b;
                        return Ok(());
                    }
                    BinOp::Gt => {
                        *(dest.ptr as *mut bool) = a > b;
                        return Ok(());
                    }
                    BinOp::Ge => {
                        *(dest.ptr as *mut bool) = a >= b;
                        return Ok(());
                    }
                    BinOp::Eq => {
                        *(dest.ptr as *mut bool) = a == b;
                        return Ok(());
                    }
                    BinOp::Ne => {
                        *(dest.ptr as *mut bool) = a != b;
                        return Ok(());
                    }
                    _ => {}
                }
            }

            // F64 operations.
            if tag == rtdt::TyTag::F64 {
                let a = *(lhs.ptr as *const f64);
                let b = *(rhs.ptr as *const f64);
                match op {
                    BinOp::Add => {
                        *(dest.ptr as *mut f64) = a + b;
                        return Ok(());
                    }
                    BinOp::Sub => {
                        *(dest.ptr as *mut f64) = a - b;
                        return Ok(());
                    }
                    BinOp::Mul => {
                        *(dest.ptr as *mut f64) = a * b;
                        return Ok(());
                    }
                    BinOp::Div => {
                        *(dest.ptr as *mut f64) = a / b;
                        return Ok(());
                    }
                    BinOp::Lt => {
                        *(dest.ptr as *mut bool) = a < b;
                        return Ok(());
                    }
                    BinOp::Le => {
                        *(dest.ptr as *mut bool) = a <= b;
                        return Ok(());
                    }
                    BinOp::Gt => {
                        *(dest.ptr as *mut bool) = a > b;
                        return Ok(());
                    }
                    BinOp::Ge => {
                        *(dest.ptr as *mut bool) = a >= b;
                        return Ok(());
                    }
                    BinOp::Eq => {
                        *(dest.ptr as *mut bool) = a == b;
                        return Ok(());
                    }
                    BinOp::Ne => {
                        *(dest.ptr as *mut bool) = a != b;
                        return Ok(());
                    }
                    _ => {}
                }
            }

            // Boolean operations.
            if tag == rtdt::TyTag::Bool {
                let a = *(lhs.ptr as *const bool);
                let b = *(rhs.ptr as *const bool);
                match op {
                    BinOp::And | BinOp::LogicAnd => {
                        *(dest.ptr as *mut bool) = a && b;
                        return Ok(());
                    }
                    BinOp::Or | BinOp::LogicOr => {
                        *(dest.ptr as *mut bool) = a || b;
                        return Ok(());
                    }
                    BinOp::LogicXor => {
                        *(dest.ptr as *mut bool) = a ^ b;
                        return Ok(());
                    }
                    BinOp::Eq => {
                        *(dest.ptr as *mut bool) = a == b;
                        return Ok(());
                    }
                    BinOp::Ne => {
                        *(dest.ptr as *mut bool) = a != b;
                        return Ok(());
                    }
                    _ => {}
                }
            }

            Err(InterpError::TypeMismatch(format!(
                "unsupported binop {:?} for type {:?}",
                op, tag
            )))
        }
    }

    pub(crate) fn execute_unaryop(
        &mut self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
    ) -> Result<(), InterpError> {
        // Macro to generate unaryop implementations for signed integer types.
        macro_rules! signed_int_unaryop {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $op:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    match $op {
                        UnaryOp::Neg => {
                            *($dest.ptr as *mut $ty) = a.wrapping_neg();
                            return Ok(());
                        }
                        UnaryOp::BitNot => {
                            *($dest.ptr as *mut $ty) = !a;
                            return Ok(());
                        }
                        UnaryOp::Not | UnaryOp::LogicNot => {}
                    }
                }
            };
        }

        // Macro for unsigned integers (only BitNot).
        macro_rules! unsigned_int_unaryop {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $op:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    match $op {
                        UnaryOp::BitNot => {
                            *($dest.ptr as *mut $ty) = !a;
                            return Ok(());
                        }
                        UnaryOp::Neg | UnaryOp::Not | UnaryOp::LogicNot => {}
                    }
                }
            };
        }

        unsafe {
            let tag = (*src.tydesc).type_tag;

            // Signed integer negation and bitnot.
            signed_int_unaryop!(I8, i8, src, dest, op);
            signed_int_unaryop!(I16, i16, src, dest, op);
            signed_int_unaryop!(I32, i32, src, dest, op);
            signed_int_unaryop!(I64, i64, src, dest, op);

            // Unsigned integer bitnot.
            unsigned_int_unaryop!(U8, u8, src, dest, op);
            unsigned_int_unaryop!(U16, u16, src, dest, op);
            unsigned_int_unaryop!(U32, u32, src, dest, op);
            unsigned_int_unaryop!(U64, u64, src, dest, op);

            // F32 negation.
            if tag == rtdt::TyTag::F32 && op == UnaryOp::Neg {
                let a = *(src.ptr as *const f32);
                *(dest.ptr as *mut f32) = -a;
                return Ok(());
            }

            // F64 negation.
            if tag == rtdt::TyTag::F64 && op == UnaryOp::Neg {
                let a = *(src.ptr as *const f64);
                *(dest.ptr as *mut f64) = -a;
                return Ok(());
            }

            // Boolean not.
            if tag == rtdt::TyTag::Bool && (op == UnaryOp::Not || op == UnaryOp::LogicNot) {
                let a = *(src.ptr as *const bool);
                *(dest.ptr as *mut bool) = !a;
                return Ok(());
            }

            // Bigint negation.
            if tag == rtdt::TyTag::Int && op == UnaryOp::Neg {
                use datalove_rt::c::RtStatus;

                let rt_handle = self.runtime.handle();
                let int_tydesc = self.tydesc_table.get_or_create(&IrType::Int);

                let status = datalove_rt::c::dtlv_rti_int_neg(
                    rt_handle,
                    src.ptr,
                    int_tydesc,
                    dest.ptr,
                    int_tydesc,
                );
                if status != RtStatus::Ok {
                    return Err(InterpError::RuntimeError("Int negation failed".to_string()));
                }
                return Ok(());
            }

            Err(InterpError::TypeMismatch(format!(
                "unsupported unaryop {:?} for type {:?}",
                op, tag
            )))
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
    ) -> Result<(), InterpError> {
        // Macro to generate checked binop implementations for integer types.
        macro_rules! checked_int_binop {
            ($tag:ident, $ty:ty, $lhs:expr, $rhs:expr, $dest:expr, $overflow:expr, $op:expr) => {
                if (*$lhs.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($lhs.ptr as *const $ty);
                    let b = *($rhs.ptr as *const $ty);
                    let (result, overflowed) = match $op {
                        BinOp::Add => a.overflowing_add(b),
                        BinOp::Sub => a.overflowing_sub(b),
                        BinOp::Mul => a.overflowing_mul(b),
                        BinOp::Div => {
                            // checked_div returns None on div-by-zero or overflow.
                            match a.checked_div(b) {
                                Some(r) => (r, false),
                                None => (0 as $ty, true),
                            }
                        }
                        _ => {
                            return Err(InterpError::TypeMismatch(format!(
                                "checked binop only supports Add/Sub/Mul/Div, got {:?}",
                                $op
                            )))
                        }
                    };
                    *($dest.ptr as *mut $ty) = result;
                    *($overflow.ptr as *mut bool) = overflowed;
                    return Ok(());
                }
            };
        }

        unsafe {
            checked_int_binop!(I8, i8, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I16, i16, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I32, i32, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(I64, i64, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U8, u8, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U16, u16, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U32, u32, lhs, rhs, dest, overflow_dest, op);
            checked_int_binop!(U64, u64, lhs, rhs, dest, overflow_dest, op);

            let tag = (*lhs.tydesc).type_tag;
            Err(InterpError::TypeMismatch(format!(
                "unsupported checked binop {:?} for type {:?}",
                op, tag
            )))
        }
    }

    /// Execute checked unary operation (e.g., negation with overflow detection).
    pub(crate) fn execute_unaryop_checked(
        &self,
        op: UnaryOp,
        src: &Value,
        dest: Destination,
        overflow_dest: Destination,
    ) -> Result<(), InterpError> {
        // Only negation can overflow for signed integers.
        if op != UnaryOp::Neg {
            return Err(InterpError::TypeMismatch(format!(
                "checked unaryop only supports Neg, got {:?}",
                op
            )));
        }

        // Macro to generate checked negation for signed integer types.
        macro_rules! checked_signed_neg {
            ($tag:ident, $ty:ty, $src:expr, $dest:expr, $overflow:expr) => {
                if (*$src.tydesc).type_tag == rtdt::TyTag::$tag {
                    let a = *($src.ptr as *const $ty);
                    let (result, overflowed) = a.overflowing_neg();
                    *($dest.ptr as *mut $ty) = result;
                    *($overflow.ptr as *mut bool) = overflowed;
                    return Ok(());
                }
            };
        }

        unsafe {
            checked_signed_neg!(I8, i8, src, dest, overflow_dest);
            checked_signed_neg!(I16, i16, src, dest, overflow_dest);
            checked_signed_neg!(I32, i32, src, dest, overflow_dest);
            checked_signed_neg!(I64, i64, src, dest, overflow_dest);

            let tag = (*src.tydesc).type_tag;
            Err(InterpError::TypeMismatch(format!(
                "checked negation only supported for signed integers, got {:?}",
                tag
            )))
        }
    }
}
