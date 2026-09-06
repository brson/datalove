//! Cranelift codegen for intrinsic functions.
//!
//! Intrinsics compile directly to Cranelift IR instructions without function call overhead.

use cranelift_frontend::FunctionBuilder;
use cranelift_codegen::ir::{self as cl_ir};
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::InstBuilder;
use cranelift_module::Module;

use datalove_datafun_intrinsics::IntrinsicId;
use datalove_datafun_ir::{ValueId, Operand};

use crate::CraneliftError;
use crate::codegen::FunctionCompiler;

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Compile an intrinsic call to Cranelift IR.
    pub(crate) fn compile_intrinsic(
        &mut self,
        builder: &mut FunctionBuilder,
        dest: ValueId,
        intrinsic: IntrinsicId,
        args: &[Operand],
    ) -> Result<(), CraneliftError> {
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
                builder.ins().bitcast(cl_ir::types::I32, cl_ir::MemFlagsData::new(), a)
            }
            BitsToF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::F32, cl_ir::MemFlagsData::new(), a)
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
            F32ToF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().fpromote(cl_ir::types::F64, a)
            }
            F64ToF32 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().fdemote(cl_ir::types::F32, a)
            }
            F64ToBits => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::I64, cl_ir::MemFlagsData::new(), a)
            }
            BitsToF64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitcast(cl_ir::types::F64, cl_ir::MemFlagsData::new(), a)
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

            // U8 bitwise operations.
            BitnotU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bnot(a)
            }
            BitandU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().band(a, b)
            }
            BitorU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bor(a, b)
            }
            BitxorU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bxor(a, b)
            }

            // U8 shift operations.
            ShlU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ishl(a, b)
            }
            ShrU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ushr(a, b)
            }

            // U8 bit counting operations.
            PopcountU8 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().popcnt(a);
                builder.ins().uextend(cl_types::I32, result)
            }
            ClzU8 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().clz(a);
                builder.ins().uextend(cl_types::I32, result)
            }
            CtzU8 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().ctz(a);
                builder.ins().uextend(cl_types::I32, result)
            }

            // U8 bit manipulation.
            ReverseBitsU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitrev(a)
            }

            // U8 wrapping arithmetic.
            AddWrappingU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().iadd(a, b)
            }
            SubWrappingU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().isub(a, b)
            }
            MulWrappingU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().imul(a, b)
            }
            RemU8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().urem(a, b)
            }

            // U8/I8 type conversion (no-op at IR level).
            U8ToI8 => {
                self.get_operand_value(builder, &args[0])?
            }
            I8ToU8 => {
                self.get_operand_value(builder, &args[0])?
            }

            // I8 operations.
            NegWrappingI8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ineg(a)
            }
            SshrI8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().sshr(a, b)
            }
            SremI8 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().srem(a, b)
            }

            // U16 bitwise operations.
            BitnotU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bnot(a)
            }
            BitandU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().band(a, b)
            }
            BitorU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bor(a, b)
            }
            BitxorU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bxor(a, b)
            }

            // U16 shift operations.
            ShlU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ishl(a, b)
            }
            ShrU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ushr(a, b)
            }

            // U16 bit counting operations.
            PopcountU16 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().popcnt(a);
                builder.ins().uextend(cl_types::I32, result)
            }
            ClzU16 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().clz(a);
                builder.ins().uextend(cl_types::I32, result)
            }
            CtzU16 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().ctz(a);
                builder.ins().uextend(cl_types::I32, result)
            }

            // U16 byte/bit manipulation.
            SwapBytesU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bswap(a)
            }
            ReverseBitsU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitrev(a)
            }

            // U16 wrapping arithmetic.
            AddWrappingU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().iadd(a, b)
            }
            SubWrappingU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().isub(a, b)
            }
            MulWrappingU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().imul(a, b)
            }
            RemU16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().urem(a, b)
            }

            // U16/I16 type conversion (no-op at IR level).
            U16ToI16 => {
                self.get_operand_value(builder, &args[0])?
            }
            I16ToU16 => {
                self.get_operand_value(builder, &args[0])?
            }

            // I16 operations.
            NegWrappingI16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ineg(a)
            }
            SshrI16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().sshr(a, b)
            }
            SremI16 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().srem(a, b)
            }

            // U64 additional bitwise operations.
            BitnotU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bnot(a)
            }
            BitorU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bor(a, b)
            }

            // U64 shift operations.
            ShlU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ishl(a, b)
            }
            ShrU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ushr(a, b)
            }

            // U64 bit counting operations.
            PopcountU64 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().popcnt(a);
                builder.ins().ireduce(cl_types::I32, result)
            }
            ClzU64 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().clz(a);
                builder.ins().ireduce(cl_types::I32, result)
            }
            CtzU64 => {
                // A bit count is a u32 whatever width it counted.
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().ctz(a);
                builder.ins().ireduce(cl_types::I32, result)
            }

            // U64 byte/bit manipulation.
            SwapBytesU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bswap(a)
            }
            ReverseBitsU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitrev(a)
            }

            // U64 wrapping arithmetic.
            AddWrappingU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().iadd(a, b)
            }
            SubWrappingU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().isub(a, b)
            }
            MulWrappingU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().imul(a, b)
            }
            RemU64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().urem(a, b)
            }

            // I64 operations.
            NegWrappingI64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ineg(a)
            }
            SshrI64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().sshr(a, b)
            }
            SremI64 => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().srem(a, b)
            }

            // Index bitwise operations.
            BitnotIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bnot(a)
            }
            BitandIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().band(a, b)
            }
            BitorIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bor(a, b)
            }
            BitxorIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().bxor(a, b)
            }

            // Index shift operations.
            ShlIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ishl(a, b)
            }
            ShrIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().ushr(a, b)
            }

            // Index bit counting operations.
            // These return u32 (I32) regardless of usize width.
            #[cfg(not(feature = "index-64"))]
            PopcountIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().popcnt(a)
            }
            #[cfg(feature = "index-64")]
            PopcountIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().popcnt(a);
                builder.ins().ireduce(cl_types::I32, result)
            }
            #[cfg(not(feature = "index-64"))]
            ClzIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().clz(a)
            }
            #[cfg(feature = "index-64")]
            ClzIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().clz(a);
                builder.ins().ireduce(cl_types::I32, result)
            }
            #[cfg(not(feature = "index-64"))]
            CtzIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ctz(a)
            }
            #[cfg(feature = "index-64")]
            CtzIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let result = builder.ins().ctz(a);
                builder.ins().ireduce(cl_types::I32, result)
            }

            // Index byte/bit manipulation.
            SwapBytesIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bswap(a)
            }
            ReverseBitsIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().bitrev(a)
            }

            // Index wrapping arithmetic.
            AddWrappingIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().iadd(a, b)
            }
            SubWrappingIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().isub(a, b)
            }
            MulWrappingIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().imul(a, b)
            }
            RemIndex => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().urem(a, b)
            }

            // Usize/Isize type conversion (no-op at IR level).
            IndexToOffset => {
                self.get_operand_value(builder, &args[0])?
            }
            OffsetToIndex => {
                self.get_operand_value(builder, &args[0])?
            }

            // Offset operations.
            NegWrappingOffset => {
                let a = self.get_operand_value(builder, &args[0])?;
                builder.ins().ineg(a)
            }
            SshrOffset => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().sshr(a, b)
            }
            SremOffset => {
                let a = self.get_operand_value(builder, &args[0])?;
                let b = self.get_operand_value(builder, &args[1])?;
                builder.ins().srem(a, b)
            }
        };

        // Store result.
        self.values.insert(dest, result);
        Ok(())
    }
}
