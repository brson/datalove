//! Cranelift codegen for intrinsic functions.
//!
//! Intrinsics compile directly to Cranelift IR instructions without function call overhead.

use cranelift_frontend::FunctionBuilder;
use cranelift_codegen::ir as cl_ir;
use cranelift_codegen::ir::InstBuilder;
use cranelift_module::Module;

use datalove_datafun_intrinsics::IntrinsicId;
use datalove_datafun_ir::{ValueId, Operand};

use crate::AotError;
use crate::codegen::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile an intrinsic call to Cranelift IR.
    pub(crate) fn compile_intrinsic(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        intrinsic: IntrinsicId,
        args: &[Operand],
    ) -> Result<(), AotError> {
        use IntrinsicId::*;

        let result = match intrinsic {
            // Bitwise operations on u32.
            BitnotU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bnot(a)
            }
            BitandU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().band(a, b)
            }
            BitorU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bor(a, b)
            }
            BitxorU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bxor(a, b)
            }

            // Shift operations on u32.
            ShlU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ishl(a, b)
            }
            ShrU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                // Use ushr for unsigned shift right.
                builder.ins().ushr(a, b)
            }

            // Bit counting operations on u32.
            PopcountU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().popcnt(a)
            }
            ClzU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().clz(a)
            }
            CtzU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ctz(a)
            }

            // Byte/bit manipulation on u32.
            SwapBytesU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bswap(a)
            }
            ReverseBitsU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitrev(a)
            }

            // Wrapping arithmetic on u32.
            // These use the same instructions as regular arithmetic since Cranelift
            // arithmetic naturally wraps.
            AddWrappingU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().iadd(a, b)
            }
            SubWrappingU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().isub(a, b)
            }
            MulWrappingU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().imul(a, b)
            }

            // Type conversions.
            // For u32 <-> i32, these are no-ops at the IR level (same bit representation).
            U32ToI32 => {
                self.get_operand_value(builder, &args[0])?
            }
            I32ToU32 => {
                self.get_operand_value(builder, &args[0])?
            }

            // Platform queries.
            IsBigEndian => {
                // Check the target endianness at compile time.
                let is_big = self.isa.endianness() == cl_ir::Endianness::Big;
                let bool_ty = cl_ir::types::I8;
                builder.ins().iconst(bool_ty, if is_big { 1 } else { 0 })
            }
        };

        // Store result.
        self.values.insert(dest, result);
        Ok(())
    }
}
