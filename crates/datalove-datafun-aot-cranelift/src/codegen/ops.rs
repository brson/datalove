//! Binary and unary operation instruction compilation.

use cranelift_codegen::ir::{self as cl_ir, types as cl_types, InstBuilder};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use datalove_datafun_ir::{BinOp, IrType, Operand, UnaryOp, ValueId};

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

        // Check for runtime types that need special handling.
        if matches!(dest_ty, IrType::Int) {
            return Err(AotError::Unsupported(format!(
                "BinOp with Int (bigint) result type - requires runtime call: {:?}",
                op
            )));
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
        let val = self.get_operand_value(builder, operand)?;
        let dest_ty = &self.func.value_types[dest.0 as usize];
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
}
