//! Intrinsic function execution for the interpreter.

use datalove_datafun_intrinsics::IntrinsicId;
use datalove_datafun_ir::Operand;
use crate::{Frame, FrameStore, Destination, IrInterpreter};

impl IrInterpreter {
    /// Execute an intrinsic function and write result to destination.
    pub(crate) fn execute_intrinsic(
        &self,
        intrinsic: IntrinsicId,
        args: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) {
        use IntrinsicId::*;

        match intrinsic {
            // Bitwise operations on u32.
            BitnotU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u32(!a, dest);
            }
            BitandU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a & b, dest);
            }
            BitorU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a | b, dest);
            }
            BitxorU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a ^ b, dest);
            }

            // Shift operations on u32.
            ShlU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a.wrapping_shl(b), dest);
            }
            ShrU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a.wrapping_shr(b), dest);
            }

            // Bit counting operations on u32.
            PopcountU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u32(a.count_ones(), dest);
            }
            ClzU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u32(a.leading_zeros(), dest);
            }
            CtzU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u32(a.trailing_zeros(), dest);
            }

            // Byte/bit manipulation on u32.
            SwapBytesU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u32(a.swap_bytes(), dest);
            }
            ReverseBitsU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u32(a.reverse_bits(), dest);
            }

            // Wrapping arithmetic on u32.
            AddWrappingU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a.wrapping_add(b), dest);
            }
            SubWrappingU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a.wrapping_sub(b), dest);
            }
            MulWrappingU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a.wrapping_mul(b), dest);
            }
            RemU32 => {
                let a = self.read_u32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u32(a % b, dest);
            }

            // Type conversions.
            U32ToI32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_i32(a as i32, dest);
            }
            I32ToU32 => {
                let a = self.read_i32(&args[0], frame, frames);
                self.write_u32(a as u32, dest);
            }
            NegWrappingI32 => {
                let a = self.read_i32(&args[0], frame, frames);
                self.write_i32(a.wrapping_neg(), dest);
            }

            // Platform queries.
            IsBigEndian => {
                #[cfg(target_endian = "big")]
                let is_big = true;
                #[cfg(not(target_endian = "big"))]
                let is_big = false;
                self.write_bool(is_big, dest);
            }

            // Signed i32 operations.
            SshrI32 => {
                let a = self.read_i32(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_i32(a.wrapping_shr(b), dest);
            }
            SremI32 => {
                let a = self.read_i32(&args[0], frame, frames);
                let b = self.read_i32(&args[1], frame, frames);
                self.write_i32(a % b, dest);
            }

            // F32 classification intrinsics.
            IsNanF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_bool(a.is_nan(), dest);
            }
            IsInfiniteF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_bool(a.is_infinite(), dest);
            }

            // F32 bit conversion.
            F32ToBits => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_u32(a.to_bits(), dest);
            }
            BitsToF32 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_f32(f32::from_bits(a), dest);
            }

            // F32 math intrinsics.
            AbsF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_f32(a.abs(), dest);
            }
            SqrtF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_f32(a.sqrt(), dest);
            }
            FloorF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_f32(a.floor(), dest);
            }
            CeilF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_f32(a.ceil(), dest);
            }
            RoundF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                // Round to nearest even to match Cranelift's `nearest`.
                self.write_f32(round_ties_even(a), dest);
            }
            TruncF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_f32(a.trunc(), dest);
            }
            CopysignF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                let b = self.read_f32(&args[1], frame, frames);
                self.write_f32(a.copysign(b), dest);
            }
            MinF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                let b = self.read_f32(&args[1], frame, frames);
                self.write_f32(a.min(b), dest);
            }
            MaxF32 => {
                let a = self.read_f32(&args[0], frame, frames);
                let b = self.read_f32(&args[1], frame, frames);
                self.write_f32(a.max(b), dest);
            }

            // F64 classification intrinsics.
            IsNanF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_bool(a.is_nan(), dest);
            }
            IsInfiniteF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_bool(a.is_infinite(), dest);
            }

            // F64 bit conversion.
            F32ToF64 => {
                let a = self.read_f32(&args[0], frame, frames);
                self.write_f64(a as f64, dest);
            }
            F64ToF32 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_f32(a as f32, dest);
            }
            U64ToF64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_f64(a as f64, dest);
            }
            I64ToF64 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_f64(a as f64, dest);
            }
            U64ToF32 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_f32(a as f32, dest);
            }
            I64ToF32 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_f32(a as f32, dest);
            }

            // Fixed-width narrowing, keeping the low bits.
            U16ToU8 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u8(a as u8, dest);
            }
            U32ToU8 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u8(a as u8, dest);
            }
            U32ToU16 => {
                let a = self.read_u32(&args[0], frame, frames);
                self.write_u16(a as u16, dest);
            }
            U64ToU8 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u8(a as u8, dest);
            }
            U64ToU16 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u16(a as u16, dest);
            }
            U64ToU32 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u32(a as u32, dest);
            }
            I16ToI8 => {
                let a = self.read_i16(&args[0], frame, frames);
                self.write_i8(a as i8, dest);
            }
            I32ToI8 => {
                let a = self.read_i32(&args[0], frame, frames);
                self.write_i8(a as i8, dest);
            }
            I32ToI16 => {
                let a = self.read_i32(&args[0], frame, frames);
                self.write_i16(a as i16, dest);
            }
            I64ToI8 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_i8(a as i8, dest);
            }
            I64ToI16 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_i16(a as i16, dest);
            }
            I64ToI32 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_i32(a as i32, dest);
            }
            F64ToBits => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_u64(a.to_bits(), dest);
            }
            BitsToF64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_f64(f64::from_bits(a), dest);
            }

            // F64 math intrinsics.
            AbsF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_f64(a.abs(), dest);
            }
            SqrtF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_f64(a.sqrt(), dest);
            }
            FloorF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_f64(a.floor(), dest);
            }
            CeilF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_f64(a.ceil(), dest);
            }
            RoundF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                // Round to nearest even to match Cranelift's `nearest`.
                self.write_f64(round_ties_even_f64(a), dest);
            }
            TruncF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                self.write_f64(a.trunc(), dest);
            }
            CopysignF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                let b = self.read_f64(&args[1], frame, frames);
                self.write_f64(a.copysign(b), dest);
            }
            MinF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                let b = self.read_f64(&args[1], frame, frames);
                self.write_f64(a.min(b), dest);
            }
            MaxF64 => {
                let a = self.read_f64(&args[0], frame, frames);
                let b = self.read_f64(&args[1], frame, frames);
                self.write_f64(a.max(b), dest);
            }

            // U64 bitwise operations.
            BitandU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a & b, dest);
            }
            BitxorU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a ^ b, dest);
            }

            // U64/I64 type conversion.
            U64ToI64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_i64(a as i64, dest);
            }
            I64ToU64 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_u64(a as u64, dest);
            }

            // U8 bitwise operations.
            BitnotU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                self.write_u8(!a, dest);
            }
            BitandU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a & b, dest);
            }
            BitorU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a | b, dest);
            }
            BitxorU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a ^ b, dest);
            }

            // U8 shift operations.
            ShlU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u8(a.wrapping_shl(b), dest);
            }
            ShrU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u8(a.wrapping_shr(b), dest);
            }

            // U8 bit counting operations.
            PopcountU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                self.write_u32(a.count_ones(), dest);
            }
            ClzU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                self.write_u32(a.leading_zeros(), dest);
            }
            CtzU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                self.write_u32(a.trailing_zeros(), dest);
            }

            // U8 bit manipulation.
            ReverseBitsU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                self.write_u8(a.reverse_bits(), dest);
            }

            // U8 wrapping arithmetic.
            AddWrappingU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a.wrapping_add(b), dest);
            }
            SubWrappingU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a.wrapping_sub(b), dest);
            }
            MulWrappingU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a.wrapping_mul(b), dest);
            }
            RemU8 => {
                let a = self.read_u8(&args[0], frame, frames);
                let b = self.read_u8(&args[1], frame, frames);
                self.write_u8(a % b, dest);
            }

            // U8/I8 type conversion.
            U8ToI8 => {
                let a = self.read_u8(&args[0], frame, frames);
                self.write_i8(a as i8, dest);
            }
            I8ToU8 => {
                let a = self.read_i8(&args[0], frame, frames);
                self.write_u8(a as u8, dest);
            }

            // I8 operations.
            NegWrappingI8 => {
                let a = self.read_i8(&args[0], frame, frames);
                self.write_i8(a.wrapping_neg(), dest);
            }
            SshrI8 => {
                let a = self.read_i8(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_i8(a.wrapping_shr(b), dest);
            }
            SremI8 => {
                let a = self.read_i8(&args[0], frame, frames);
                let b = self.read_i8(&args[1], frame, frames);
                self.write_i8(a % b, dest);
            }

            // U16 bitwise operations.
            BitnotU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u16(!a, dest);
            }
            BitandU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a & b, dest);
            }
            BitorU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a | b, dest);
            }
            BitxorU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a ^ b, dest);
            }

            // U16 shift operations.
            ShlU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u16(a.wrapping_shl(b), dest);
            }
            ShrU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u16(a.wrapping_shr(b), dest);
            }

            // U16 bit counting operations.
            PopcountU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u32(a.count_ones(), dest);
            }
            ClzU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u32(a.leading_zeros(), dest);
            }
            CtzU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u32(a.trailing_zeros(), dest);
            }

            // U16 byte/bit manipulation.
            SwapBytesU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u16(a.swap_bytes(), dest);
            }
            ReverseBitsU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_u16(a.reverse_bits(), dest);
            }

            // U16 wrapping arithmetic.
            AddWrappingU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a.wrapping_add(b), dest);
            }
            SubWrappingU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a.wrapping_sub(b), dest);
            }
            MulWrappingU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a.wrapping_mul(b), dest);
            }
            RemU16 => {
                let a = self.read_u16(&args[0], frame, frames);
                let b = self.read_u16(&args[1], frame, frames);
                self.write_u16(a % b, dest);
            }

            // U16/I16 type conversion.
            U16ToI16 => {
                let a = self.read_u16(&args[0], frame, frames);
                self.write_i16(a as i16, dest);
            }
            I16ToU16 => {
                let a = self.read_i16(&args[0], frame, frames);
                self.write_u16(a as u16, dest);
            }

            // I16 operations.
            NegWrappingI16 => {
                let a = self.read_i16(&args[0], frame, frames);
                self.write_i16(a.wrapping_neg(), dest);
            }
            SshrI16 => {
                let a = self.read_i16(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_i16(a.wrapping_shr(b), dest);
            }
            SremI16 => {
                let a = self.read_i16(&args[0], frame, frames);
                let b = self.read_i16(&args[1], frame, frames);
                self.write_i16(a % b, dest);
            }

            // U64 additional bitwise operations.
            BitnotU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u64(!a, dest);
            }
            BitorU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a | b, dest);
            }

            // U64 shift operations.
            ShlU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u64(a.wrapping_shl(b), dest);
            }
            ShrU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_u64(a.wrapping_shr(b), dest);
            }

            // U64 bit counting operations.
            PopcountU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u32(a.count_ones(), dest);
            }
            ClzU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u32(a.leading_zeros(), dest);
            }
            CtzU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u32(a.trailing_zeros(), dest);
            }

            // U64 byte/bit manipulation.
            SwapBytesU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u64(a.swap_bytes(), dest);
            }
            ReverseBitsU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_u64(a.reverse_bits(), dest);
            }

            // U64 wrapping arithmetic.
            AddWrappingU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a.wrapping_add(b), dest);
            }
            SubWrappingU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a.wrapping_sub(b), dest);
            }
            MulWrappingU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a.wrapping_mul(b), dest);
            }
            RemU64 => {
                let a = self.read_u64(&args[0], frame, frames);
                let b = self.read_u64(&args[1], frame, frames);
                self.write_u64(a % b, dest);
            }

            // I64 operations.
            NegWrappingI64 => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_i64(a.wrapping_neg(), dest);
            }
            SshrI64 => {
                let a = self.read_i64(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_i64(a.wrapping_shr(b), dest);
            }
            SremI64 => {
                let a = self.read_i64(&args[0], frame, frames);
                let b = self.read_i64(&args[1], frame, frames);
                self.write_i64(a % b, dest);
            }

            // Index bitwise operations.
            BitnotIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_usize(!a, dest);
            }
            BitandIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a & b, dest);
            }
            BitorIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a | b, dest);
            }
            BitxorIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a ^ b, dest);
            }

            // Index shift operations.
            ShlIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_usize(a.wrapping_shl(b), dest);
            }
            ShrIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_usize(a.wrapping_shr(b), dest);
            }

            // Index bit counting operations.
            PopcountIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_u32(a.count_ones(), dest);
            }
            ClzIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_u32(a.leading_zeros(), dest);
            }
            CtzIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_u32(a.trailing_zeros(), dest);
            }

            // Index byte/bit manipulation.
            SwapBytesIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_usize(a.swap_bytes(), dest);
            }
            ReverseBitsIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_usize(a.reverse_bits(), dest);
            }

            // Index wrapping arithmetic.
            AddWrappingIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a.wrapping_add(b), dest);
            }
            SubWrappingIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a.wrapping_sub(b), dest);
            }
            MulWrappingIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a.wrapping_mul(b), dest);
            }
            RemIndex => {
                let a = self.read_usize(&args[0], frame, frames);
                let b = self.read_usize(&args[1], frame, frames);
                self.write_usize(a % b, dest);
            }

            // How wide an index is.
            IndexBits => {
                self.write_u32(datalove_rtdt::IndexRepr::BITS, dest);
            }

            // Usize/Isize type conversion.
            IndexToOffset => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_isize(a as datalove_rtdt::OffsetRepr, dest);
            }
            OffsetToIndex => {
                let a = self.read_isize(&args[0], frame, frames);
                self.write_usize(a as datalove_rtdt::IndexRepr, dest);
            }
            IndexToU64 => {
                let a = self.read_usize(&args[0], frame, frames);
                self.write_u64(a as u64, dest);
            }
            U64ToIndex => {
                let a = self.read_u64(&args[0], frame, frames);
                self.write_usize(a as datalove_rtdt::IndexRepr, dest);
            }
            OffsetToI64 => {
                let a = self.read_isize(&args[0], frame, frames);
                self.write_i64(a as i64, dest);
            }
            I64ToOffset => {
                let a = self.read_i64(&args[0], frame, frames);
                self.write_isize(a as datalove_rtdt::OffsetRepr, dest);
            }

            // Offset operations.
            NegWrappingOffset => {
                let a = self.read_isize(&args[0], frame, frames);
                self.write_isize(a.wrapping_neg(), dest);
            }
            SshrOffset => {
                let a = self.read_isize(&args[0], frame, frames);
                let b = self.read_u32(&args[1], frame, frames);
                self.write_isize(a.wrapping_shr(b), dest);
            }
            SremOffset => {
                let a = self.read_isize(&args[0], frame, frames);
                let b = self.read_isize(&args[1], frame, frames);
                self.write_isize(a % b, dest);
            }
        }
    }

    /// Read a u32 value from an operand.
    #[inline(always)]
    fn read_u32(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> u32 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const u32) }
    }

    /// Read an i32 value from an operand.
    #[inline(always)]
    fn read_i32(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> i32 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const i32) }
    }

    /// Write a u32 value to destination.
    #[inline(always)]
    fn write_u32(&self, value: u32, dest: Destination) {
        unsafe { *(dest.ptr as *mut u32) = value; }
    }

    /// Write an i32 value to destination.
    #[inline(always)]
    fn write_i32(&self, value: i32, dest: Destination) {
        unsafe { *(dest.ptr as *mut i32) = value; }
    }

    /// Write a bool value to destination.
    #[inline(always)]
    fn write_bool(&self, value: bool, dest: Destination) {
        unsafe { *(dest.ptr as *mut u8) = value as u8; }
    }

    /// Read an f32 value from an operand.
    #[inline(always)]
    fn read_f32(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> f32 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const f32) }
    }

    /// Write an f32 value to destination.
    #[inline(always)]
    fn write_f32(&self, value: f32, dest: Destination) {
        unsafe { *(dest.ptr as *mut f32) = value; }
    }

    /// Read an f64 value from an operand.
    #[inline(always)]
    fn read_f64(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> f64 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const f64) }
    }

    /// Write an f64 value to destination.
    #[inline(always)]
    fn write_f64(&self, value: f64, dest: Destination) {
        unsafe { *(dest.ptr as *mut f64) = value; }
    }

    /// Read a u64 value from an operand.
    #[inline(always)]
    fn read_u64(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> u64 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const u64) }
    }

    /// Write a u64 value to destination.
    #[inline(always)]
    fn write_u64(&self, value: u64, dest: Destination) {
        unsafe { *(dest.ptr as *mut u64) = value; }
    }

    /// Read an i64 value from an operand.
    #[inline(always)]
    fn read_i64(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> i64 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const i64) }
    }

    /// Write an i64 value to destination.
    #[inline(always)]
    fn write_i64(&self, value: i64, dest: Destination) {
        unsafe { *(dest.ptr as *mut i64) = value; }
    }

    /// Read a u8 value from an operand.
    #[inline(always)]
    fn read_u8(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> u8 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const u8) }
    }

    /// Write a u8 value to destination.
    #[inline(always)]
    fn write_u8(&self, value: u8, dest: Destination) {
        unsafe { *(dest.ptr as *mut u8) = value; }
    }

    /// Read an i8 value from an operand.
    #[inline(always)]
    fn read_i8(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> i8 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const i8) }
    }

    /// Write an i8 value to destination.
    #[inline(always)]
    fn write_i8(&self, value: i8, dest: Destination) {
        unsafe { *(dest.ptr as *mut i8) = value; }
    }

    /// Read a u16 value from an operand.
    #[inline(always)]
    fn read_u16(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> u16 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const u16) }
    }

    /// Write a u16 value to destination.
    #[inline(always)]
    fn write_u16(&self, value: u16, dest: Destination) {
        unsafe { *(dest.ptr as *mut u16) = value; }
    }

    /// Read an i16 value from an operand.
    #[inline(always)]
    fn read_i16(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> i16 {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const i16) }
    }

    /// Write an i16 value to destination.
    #[inline(always)]
    fn write_i16(&self, value: i16, dest: Destination) {
        unsafe { *(dest.ptr as *mut i16) = value; }
    }

    /// Read a usize value from an operand.
    #[inline(always)]
    fn read_usize(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> datalove_rtdt::IndexRepr {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const datalove_rtdt::IndexRepr) }
    }

    /// Write a usize value to destination.
    #[inline(always)]
    fn write_usize(&self, value: datalove_rtdt::IndexRepr, dest: Destination) {
        unsafe { *(dest.ptr as *mut datalove_rtdt::IndexRepr) = value; }
    }

    /// Read an isize value from an operand.
    #[inline(always)]
    fn read_isize(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> datalove_rtdt::OffsetRepr {
        let val = self.read_operand(op, frame, frames);
        unsafe { *(val.ptr as *const datalove_rtdt::OffsetRepr) }
    }

    /// Write an isize value to destination.
    #[inline(always)]
    fn write_isize(&self, value: datalove_rtdt::OffsetRepr, dest: Destination) {
        unsafe { *(dest.ptr as *mut datalove_rtdt::OffsetRepr) = value; }
    }
}

/// Round f32 to nearest integer, with ties going to nearest even.
fn round_ties_even(x: f32) -> f32 {
    // Handle special cases.
    if x.is_nan() || x.is_infinite() {
        return x;
    }

    let rounded = x.round();
    let diff = x - rounded;

    // Check if exactly halfway.
    if diff.abs() == 0.5 {
        // Round to nearest even.
        if rounded as i32 % 2 != 0 {
            if x > 0.0 {
                rounded - 1.0
            } else {
                rounded + 1.0
            }
        } else {
            rounded
        }
    } else {
        rounded
    }
}

/// Round f64 to nearest integer, with ties going to nearest even.
fn round_ties_even_f64(x: f64) -> f64 {
    // Handle special cases.
    if x.is_nan() || x.is_infinite() {
        return x;
    }

    let rounded = x.round();
    let diff = x - rounded;

    // Check if exactly halfway.
    if diff.abs() == 0.5 {
        // Round to nearest even.
        if rounded as i64 % 2 != 0 {
            if x > 0.0 {
                rounded - 1.0
            } else {
                rounded + 1.0
            }
        } else {
            rounded
        }
    } else {
        rounded
    }
}
