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

        // Int (bigint) operations require runtime calls.
        // We require both operands to also be Int (widening from fixed-width not yet supported).
        if matches!(dest_ty, IrType::Int) {
            let lhs_ty = self.get_operand_type(lhs)?;
            let rhs_ty = self.get_operand_type(rhs)?;

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

        let is_signed = matches!(dest_ty, IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64);
        let is_float = matches!(dest_ty, IrType::F32);

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
}
