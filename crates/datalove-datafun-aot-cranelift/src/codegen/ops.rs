//! Binary and unary operation instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{BinOp, IrType, Operand, UnaryOp, ValueId};

use crate::types::PTR_TYPE;
use crate::AotError;

use super::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile a binary operation.
    pub(super) fn compile_binop(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<(), AotError> {
        // Get the type of the result to determine how to compile.
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Check operand types for Int operations.
        let lhs_ty = self.get_operand_type(lhs)?;
        let rhs_ty = self.get_operand_type(rhs)?;

        // Int (bigint) comparisons require runtime calls.
        // Comparison result is Bool, but operands are Int.
        let is_comparison = matches!(
            op,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne
        );
        if is_comparison && matches!(lhs_ty, IrType::Int) && matches!(rhs_ty, IrType::Int) {
            return self.compile_int_cmp(builder, dest, op, lhs, rhs);
        }

        // Int (bigint) arithmetic operations require runtime calls.
        // We require both operands to also be Int (widening from fixed-width not yet supported).
        if matches!(dest_ty, IrType::Int) {
            if matches!(lhs_ty, IrType::Int) && matches!(rhs_ty, IrType::Int) {
                return self.compile_int_binop(builder, dest, op, lhs, rhs);
            } else {
                return Err(AotError::Unsupported(format!(
                    "Int BinOp with widening from fixed-width types not yet supported: {:?} {:?} {:?}",
                    lhs_ty, op, rhs_ty
                )));
            }
        }

        let lhs_val = self.get_operand_value(builder, lhs)?;
        let rhs_val = self.get_operand_value(builder, rhs)?;

        // For comparison ops, check operand type since dest is bool.
        // For arithmetic ops, check dest type.
        let type_to_check = if is_comparison { &lhs_ty } else { dest_ty };

        let is_signed = matches!(
            type_to_check,
            IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64
        );
        let is_float = matches!(type_to_check, IrType::F32);

        let cl_val = if is_float {
            match op {
                BinOp::Add => builder.ins().fadd(lhs_val, rhs_val),
                BinOp::Sub => builder.ins().fsub(lhs_val, rhs_val),
                BinOp::Mul => builder.ins().fmul(lhs_val, rhs_val),
                BinOp::Div => builder.ins().fdiv(lhs_val, rhs_val),
                BinOp::Eq => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::Equal, lhs_val, rhs_val)
                }
                BinOp::Ne => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::NotEqual, lhs_val, rhs_val)
                }
                BinOp::Lt => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::LessThan, lhs_val, rhs_val)
                }
                BinOp::Le => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::LessThanOrEqual, lhs_val, rhs_val)
                }
                BinOp::Gt => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::GreaterThan, lhs_val, rhs_val)
                }
                BinOp::Ge => {
                    builder.ins().fcmp(cl_ir::condcodes::FloatCC::GreaterThanOrEqual, lhs_val, rhs_val)
                }
                _ => {
                    return Err(AotError::Unsupported(format!(
                        "float binop not supported: {:?}",
                        op
                    )));
                }
            }
        } else {
            match op {
                BinOp::Add => builder.ins().iadd(lhs_val, rhs_val),
                BinOp::Sub => builder.ins().isub(lhs_val, rhs_val),
                BinOp::Mul => builder.ins().imul(lhs_val, rhs_val),
                BinOp::Div => {
                    if is_signed {
                        builder.ins().sdiv(lhs_val, rhs_val)
                    } else {
                        builder.ins().udiv(lhs_val, rhs_val)
                    }
                }
                BinOp::Mod => {
                    if is_signed {
                        builder.ins().srem(lhs_val, rhs_val)
                    } else {
                        builder.ins().urem(lhs_val, rhs_val)
                    }
                }
                BinOp::Eq => {
                    builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, lhs_val, rhs_val)
                }
                BinOp::Ne => {
                    builder.ins().icmp(cl_ir::condcodes::IntCC::NotEqual, lhs_val, rhs_val)
                }
                BinOp::Lt => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedLessThan
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedLessThan
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::Le => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedLessThanOrEqual
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedLessThanOrEqual
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::Gt => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedGreaterThan
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedGreaterThan
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::Ge => {
                    let cc = if is_signed {
                        cl_ir::condcodes::IntCC::SignedGreaterThanOrEqual
                    } else {
                        cl_ir::condcodes::IntCC::UnsignedGreaterThanOrEqual
                    };
                    builder.ins().icmp(cc, lhs_val, rhs_val)
                }
                BinOp::And => {
                    // Logical AND - both operands are booleans.
                    builder.ins().band(lhs_val, rhs_val)
                }
                BinOp::Or => {
                    // Logical OR.
                    builder.ins().bor(lhs_val, rhs_val)
                }
                BinOp::BitAnd => builder.ins().band(lhs_val, rhs_val),
                BinOp::BitOr => builder.ins().bor(lhs_val, rhs_val),
                BinOp::BitXor => builder.ins().bxor(lhs_val, rhs_val),
                BinOp::Shl => builder.ins().ishl(lhs_val, rhs_val),
                BinOp::Shr => {
                    if is_signed {
                        builder.ins().sshr(lhs_val, rhs_val)
                    } else {
                        builder.ins().ushr(lhs_val, rhs_val)
                    }
                }
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile a unary operation.
    pub(super) fn compile_unaryop(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        op: UnaryOp,
        operand: &Operand,
    ) -> Result<(), AotError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Int (bigint) negation requires runtime call.
        if matches!(dest_ty, IrType::Int) && matches!(op, UnaryOp::Neg) {
            return self.compile_int_neg(builder, dest, operand);
        }

        let val = self.get_operand_value(builder, operand)?;
        let is_float = matches!(dest_ty, IrType::F32);

        let cl_val = match op {
            UnaryOp::Neg => {
                if is_float {
                    builder.ins().fneg(val)
                } else {
                    builder.ins().ineg(val)
                }
            }
            UnaryOp::Not => {
                // Logical NOT on boolean (i8).
                let one = builder.ins().iconst(cl_types::I8, 1);
                builder.ins().bxor(val, one)
            }
            UnaryOp::BitNot => {
                builder.ins().bnot(val)
            }
        };

        self.values.insert(dest, cl_val);
        Ok(())
    }

    /// Compile a binary operation on Int (bigint) via runtime call.
    fn compile_int_binop(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<(), AotError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("Int BinOp requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("Int BinOp requires runtime handle".into())
        })?;

        // Select the runtime function based on the operation.
        let func_id = match op {
            BinOp::Add => runtime.int_add,
            BinOp::Sub => runtime.int_sub,
            BinOp::Mul => runtime.int_mul,
            BinOp::Div => runtime.int_div,
            _ => {
                return Err(AotError::Unsupported(format!(
                    "Int binop not yet supported: {:?}",
                    op
                )));
            }
        };

        // Get pointers to operands.
        let lhs_ptr = self.get_operand_ptr(builder, lhs)?;
        let rhs_ptr = self.get_operand_ptr(builder, rhs)?;

        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for Int BinOp result".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let result_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Int TyDesc.
        let int_tydesc_id = self.tydesc_emitter.get(&IrType::Int).ok_or_else(|| {
            AotError::Codegen("TyDesc not found for Int".into())
        })?;
        let int_tydesc_gv = self.module.declare_data_in_func(int_tydesc_id, builder.func);
        let int_tydesc_ptr = builder.ins().global_value(PTR_TYPE, int_tydesc_gv);

        // Call runtime function: (rt, a_in, a_tydesc, b_in, b_tydesc, result_out, result_tydesc) -> status
        let func_ref = self.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(func_ref, &[
            rt_handle,
            lhs_ptr,
            int_tydesc_ptr,
            rhs_ptr,
            int_tydesc_ptr,
            result_ptr,
            int_tydesc_ptr,
        ]);

        // Store pointer to result.
        self.values.insert(dest, result_ptr);
        Ok(())
    }

    /// Compile Int (bigint) negation via runtime call.
    fn compile_int_neg(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        operand: &Operand,
    ) -> Result<(), AotError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("Int Neg requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("Int Neg requires runtime handle".into())
        })?;

        // Get pointer to operand.
        let operand_ptr = self.get_operand_ptr(builder, operand)?;

        // Get frame slot and destination address.
        let frame_slot = self.frame_slot.ok_or_else(|| {
            AotError::Codegen("no frame slot for Int Neg result".into())
        })?;
        let dest_offset = self.layout.value_offset(dest.0);
        let result_ptr = builder.ins().stack_addr(PTR_TYPE, frame_slot, dest_offset as i32);

        // Get Int TyDesc.
        let int_tydesc_id = self.tydesc_emitter.get(&IrType::Int).ok_or_else(|| {
            AotError::Codegen("TyDesc not found for Int".into())
        })?;
        let int_tydesc_gv = self.module.declare_data_in_func(int_tydesc_id, builder.func);
        let int_tydesc_ptr = builder.ins().global_value(PTR_TYPE, int_tydesc_gv);

        // Call runtime function: (rt, a_in, a_tydesc, result_out, result_tydesc) -> status
        let func_ref = self.module.declare_func_in_func(runtime.int_neg, builder.func);
        builder.ins().call(func_ref, &[
            rt_handle,
            operand_ptr,
            int_tydesc_ptr,
            result_ptr,
            int_tydesc_ptr,
        ]);

        // Store pointer to result.
        self.values.insert(dest, result_ptr);
        Ok(())
    }

    /// Compile Int (bigint) comparison via runtime call.
    ///
    /// Calls `dtlv_rti_cmp_local` which returns `RtOrdering`:
    /// - 1 = Less
    /// - 2 = Equal
    /// - 3 = Greater
    fn compile_int_cmp(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<(), AotError> {
        // Get runtime imports and handle.
        let runtime = self.runtime.ok_or_else(|| {
            AotError::Codegen("Int comparison requires runtime imports".into())
        })?;
        let rt_handle = self.rt_handle_param.ok_or_else(|| {
            AotError::Codegen("Int comparison requires runtime handle".into())
        })?;

        // Get pointers to operands.
        let lhs_ptr = self.get_operand_ptr(builder, lhs)?;
        let rhs_ptr = self.get_operand_ptr(builder, rhs)?;

        // Get Int TyDesc.
        let int_tydesc_id = self.tydesc_emitter.get(&IrType::Int).ok_or_else(|| {
            AotError::Codegen("TyDesc not found for Int".into())
        })?;
        let int_tydesc_gv = self.module.declare_data_in_func(int_tydesc_id, builder.func);
        let int_tydesc_ptr = builder.ins().global_value(PTR_TYPE, int_tydesc_gv);

        // Call runtime function: (rt, a_ref, a_tydesc, b_ref, b_tydesc) -> RtOrdering
        let func_ref = self.module.declare_func_in_func(runtime.int_cmp, builder.func);
        let call = builder.ins().call(func_ref, &[
            rt_handle,
            lhs_ptr,
            int_tydesc_ptr,
            rhs_ptr,
            int_tydesc_ptr,
        ]);
        let ordering = builder.inst_results(call)[0];

        // RtOrdering values: Less=1, Equal=2, Greater=3
        let less = builder.ins().iconst(cl_types::I8, 1);
        let equal = builder.ins().iconst(cl_types::I8, 2);
        let greater = builder.ins().iconst(cl_types::I8, 3);

        // Convert ordering to boolean based on comparison operator.
        let result = match op {
            BinOp::Lt => {
                // ordering == Less
                builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, less)
            }
            BinOp::Le => {
                // ordering == Less || ordering == Equal
                let is_less = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, less);
                let is_equal = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, equal);
                builder.ins().bor(is_less, is_equal)
            }
            BinOp::Gt => {
                // ordering == Greater
                builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, greater)
            }
            BinOp::Ge => {
                // ordering == Greater || ordering == Equal
                let is_greater = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, greater);
                let is_equal = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, equal);
                builder.ins().bor(is_greater, is_equal)
            }
            BinOp::Eq => {
                // ordering == Equal
                builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, ordering, equal)
            }
            BinOp::Ne => {
                // ordering != Equal
                builder.ins().icmp(cl_ir::condcodes::IntCC::NotEqual, ordering, equal)
            }
            _ => {
                return Err(AotError::Unsupported(format!(
                    "Int comparison: unexpected op {:?}",
                    op
                )));
            }
        };

        self.values.insert(dest, result);
        Ok(())
    }

    /// Compile a checked binary operation (produces result + overflow flag).
    pub(super) fn compile_binop_checked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        overflow_dest: ValueId,
        op: BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<(), AotError> {
        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Only supported for fixed-width integers.
        let (is_signed, bits, cl_ty) = match dest_ty {
            IrType::I8 => (true, 8, cl_types::I8),
            IrType::I16 => (true, 16, cl_types::I16),
            IrType::I32 => (true, 32, cl_types::I32),
            IrType::I64 => (true, 64, cl_types::I64),
            IrType::U8 => (false, 8, cl_types::I8),
            IrType::U16 => (false, 16, cl_types::I16),
            IrType::U32 => (false, 32, cl_types::I32),
            IrType::U64 => (false, 64, cl_types::I64),
            _ => {
                return Err(AotError::Unsupported(format!(
                    "checked binop not supported for type: {:?}",
                    dest_ty
                )));
            }
        };

        let lhs_val = self.get_operand_value(builder, lhs)?;
        let rhs_val = self.get_operand_value(builder, rhs)?;

        let (result, overflow) = match op {
            BinOp::Add => {
                let result = builder.ins().iadd(lhs_val, rhs_val);
                let overflow = if is_signed {
                    // Signed overflow: (lhs ^ result) & (rhs ^ result) has sign bit set.
                    let xor1 = builder.ins().bxor(lhs_val, result);
                    let xor2 = builder.ins().bxor(rhs_val, result);
                    let and = builder.ins().band(xor1, xor2);
                    // Extract sign bit.
                    let shift = builder.ins().iconst(cl_ty, (bits - 1) as i64);
                    let shifted = builder.ins().ushr(and, shift);
                    // Reduce to i8 bool.
                    if cl_ty != cl_types::I8 {
                        builder.ins().ireduce(cl_types::I8, shifted)
                    } else {
                        shifted
                    }
                } else {
                    // Unsigned overflow: result < lhs.
                    builder.ins().icmp(cl_ir::condcodes::IntCC::UnsignedLessThan, result, lhs_val)
                };
                (result, overflow)
            }
            BinOp::Sub => {
                let result = builder.ins().isub(lhs_val, rhs_val);
                let overflow = if is_signed {
                    // Signed overflow: (lhs ^ rhs) & (lhs ^ result) has sign bit set.
                    let xor1 = builder.ins().bxor(lhs_val, rhs_val);
                    let xor2 = builder.ins().bxor(lhs_val, result);
                    let and = builder.ins().band(xor1, xor2);
                    let shift = builder.ins().iconst(cl_ty, (bits - 1) as i64);
                    let shifted = builder.ins().ushr(and, shift);
                    if cl_ty != cl_types::I8 {
                        builder.ins().ireduce(cl_types::I8, shifted)
                    } else {
                        shifted
                    }
                } else {
                    // Unsigned underflow: lhs < rhs.
                    builder.ins().icmp(cl_ir::condcodes::IntCC::UnsignedLessThan, lhs_val, rhs_val)
                };
                (result, overflow)
            }
            BinOp::Mul => {
                let result = builder.ins().imul(lhs_val, rhs_val);
                // Overflow check: if lhs != 0, check result / lhs == rhs.
                let zero = builder.ins().iconst(cl_ty, 0);
                let lhs_is_zero = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, lhs_val, zero);

                // Divide result by lhs (if lhs != 0).
                let quotient = if is_signed {
                    builder.ins().sdiv(result, lhs_val)
                } else {
                    builder.ins().udiv(result, lhs_val)
                };

                // Check if quotient != rhs (overflow occurred).
                let not_equal = builder.ins().icmp(cl_ir::condcodes::IntCC::NotEqual, quotient, rhs_val);

                // overflow = lhs_is_zero ? false : not_equal
                let false_val = builder.ins().iconst(cl_types::I8, 0);
                let overflow = builder.ins().select(lhs_is_zero, false_val, not_equal);

                (result, overflow)
            }
            BinOp::Div => {
                // Division overflow cases:
                // - Divide by zero
                // - Signed: MIN / -1 overflows
                let zero = builder.ins().iconst(cl_ty, 0);
                let div_by_zero = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, rhs_val, zero);

                let overflow = if is_signed {
                    // Check for MIN / -1.
                    let min_val = match bits {
                        8 => i8::MIN as i64,
                        16 => i16::MIN as i64,
                        32 => i32::MIN as i64,
                        64 => i64::MIN,
                        _ => unreachable!(),
                    };
                    let minus_one = builder.ins().iconst(cl_ty, -1i64);
                    let min_const = builder.ins().iconst(cl_ty, min_val);
                    let is_min = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, lhs_val, min_const);
                    let is_minus_one = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, rhs_val, minus_one);
                    let signed_overflow = builder.ins().band(is_min, is_minus_one);
                    builder.ins().bor(div_by_zero, signed_overflow)
                } else {
                    div_by_zero
                };

                // Perform the division, selecting safe divisor to avoid trap.
                let one = builder.ins().iconst(cl_ty, 1);
                let safe_rhs = builder.ins().select(overflow, one, rhs_val);
                let result = if is_signed {
                    builder.ins().sdiv(lhs_val, safe_rhs)
                } else {
                    builder.ins().udiv(lhs_val, safe_rhs)
                };

                // Select zero result on overflow.
                let result = builder.ins().select(overflow, zero, result);

                (result, overflow)
            }
            _ => {
                return Err(AotError::Unsupported(format!(
                    "checked binop only supports Add/Sub/Mul/Div, got {:?}",
                    op
                )));
            }
        };

        self.values.insert(dest, result);
        self.values.insert(overflow_dest, overflow);
        Ok(())
    }

    /// Compile a checked unary operation (produces result + overflow flag).
    pub(super) fn compile_unaryop_checked(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        overflow_dest: ValueId,
        op: UnaryOp,
        operand: &Operand,
    ) -> Result<(), AotError> {
        // Only negation can overflow for signed integers.
        if op != UnaryOp::Neg {
            return Err(AotError::Unsupported(format!(
                "checked unaryop only supports Neg, got {:?}",
                op
            )));
        }

        let dest_ty = &self.func.value_types[dest.0 as usize];

        // Only supported for signed integers.
        let (min_val, cl_ty) = match dest_ty {
            IrType::I8 => (i8::MIN as i64, cl_types::I8),
            IrType::I16 => (i16::MIN as i64, cl_types::I16),
            IrType::I32 => (i32::MIN as i64, cl_types::I32),
            IrType::I64 => (i64::MIN, cl_types::I64),
            _ => {
                return Err(AotError::Unsupported(format!(
                    "checked negation only supported for signed integers, got {:?}",
                    dest_ty
                )));
            }
        };

        let val = self.get_operand_value(builder, operand)?;

        // Negation overflows only for MIN value.
        let min_const = builder.ins().iconst(cl_ty, min_val);
        let overflow = builder.ins().icmp(cl_ir::condcodes::IntCC::Equal, val, min_const);

        // Perform negation.
        let result = builder.ins().ineg(val);

        self.values.insert(dest, result);
        self.values.insert(overflow_dest, overflow);
        Ok(())
    }
}
