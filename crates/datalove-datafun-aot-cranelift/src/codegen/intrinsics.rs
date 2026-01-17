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
            RemU32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().urem(a, b)
            }

            // Type conversions.
            // For u32 <-> i32, these are no-ops at the IR level (same bit representation).
            U32ToI32 => {
                self.get_operand_value(builder, &args[0])?
            }
            I32ToU32 => {
                self.get_operand_value(builder, &args[0])?
            }
            NegWrappingI32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ineg(a)
            }

            // Platform queries.
            IsBigEndian => {
                // Check the target endianness at compile time.
                let is_big = self.isa.endianness() == cl_ir::Endianness::Big;
                let bool_ty = cl_ir::types::I8;
                builder.ins().iconst(bool_ty, if is_big { 1 } else { 0 })
            }

            // Signed i32 operations.
            SshrI32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                // Signed shift right (arithmetic shift).
                builder.ins().sshr(a, b)
            }
            SremI32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                // Signed remainder.
                builder.ins().srem(a, b)
            }

            // F32 classification intrinsics.
            IsNanF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                // NaN is the only value where x != x.
                builder.ins().fcmp(cl_ir::condcodes::FloatCC::Unordered, a, a)
            }
            IsInfiniteF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let abs_val = builder.ins().fabs(a);
                let inf = builder.ins().f32const(f32::INFINITY);
                builder.ins().fcmp(cl_ir::condcodes::FloatCC::Equal, abs_val, inf)
            }

            // F32 bit conversion.
            F32ToBits => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::I32, cl_ir::MemFlags::new(), a)
            }
            BitsToF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::F32, cl_ir::MemFlags::new(), a)
            }

            // F32 math intrinsics.
            AbsF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().fabs(a)
            }
            SqrtF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().sqrt(a)
            }
            FloorF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().floor(a)
            }
            CeilF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ceil(a)
            }
            RoundF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                // Cranelift nearest rounds to nearest even.
                builder.ins().nearest(a)
            }
            TruncF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().trunc(a)
            }
            CopysignF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().fcopysign(a, b)
            }
            MinF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().fmin(a, b)
            }
            MaxF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().fmax(a, b)
            }

            // F64 classification intrinsics.
            IsNanF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                // NaN is the only value where x != x.
                builder.ins().fcmp(cl_ir::condcodes::FloatCC::Unordered, a, a)
            }
            IsInfiniteF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let abs_val = builder.ins().fabs(a);
                let inf = builder.ins().f64const(f64::INFINITY);
                builder.ins().fcmp(cl_ir::condcodes::FloatCC::Equal, abs_val, inf)
            }

            // F64 bit conversion.
            F64ToBits => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::I64, cl_ir::MemFlags::new(), a)
            }
            BitsToF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::F64, cl_ir::MemFlags::new(), a)
            }

            // F64 math intrinsics.
            AbsF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().fabs(a)
            }
            SqrtF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().sqrt(a)
            }
            FloorF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().floor(a)
            }
            CeilF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ceil(a)
            }
            RoundF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                // Cranelift nearest rounds to nearest even.
                builder.ins().nearest(a)
            }
            TruncF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().trunc(a)
            }
            CopysignF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().fcopysign(a, b)
            }
            MinF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().fmin(a, b)
            }
            MaxF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().fmax(a, b)
            }

            // U64 bitwise operations.
            BitandU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().band(a, b)
            }
            BitxorU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bxor(a, b)
            }

            // U64/I64 type conversion (no-op at IR level).
            U64ToI64 => {
                self.get_operand_value(builder, &args[0])?
            }
            I64ToU64 => {
                self.get_operand_value(builder, &args[0])?
            }
        };

        // Store result.
        self.values.insert(dest, result);
        Ok(())
    }
}
