//! Intrinsic function execution for the interpreter.

use datalove_datafun_intrinsics::IntrinsicId;
use datalove_datafun_ir::Operand;
use crate::{Frame, FrameStore, Destination, InterpError, IrInterpreter};

impl IrInterpreter {
    /// Execute an intrinsic function and write result to destination.
    pub(crate) fn execute_intrinsic(
        &self,
        intrinsic: IntrinsicId,
        args: &[Operand],
        dest: Destination,
        frame: &Frame,
        frames: &FrameStore,
    ) -> Result<(), InterpError> {
        use IntrinsicId::*;

        match intrinsic {
            // Bitwise operations on u32.
            BitnotU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_u32(!a, dest);
            }
            BitandU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a & b, dest);
            }
            BitorU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a | b, dest);
            }
            BitxorU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a ^ b, dest);
            }

            // Shift operations on u32.
            ShlU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a.wrapping_shl(b), dest);
            }
            ShrU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a.wrapping_shr(b), dest);
            }

            // Bit counting operations on u32.
            PopcountU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_u32(a.count_ones(), dest);
            }
            ClzU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_u32(a.leading_zeros(), dest);
            }
            CtzU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_u32(a.trailing_zeros(), dest);
            }

            // Byte/bit manipulation on u32.
            SwapBytesU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_u32(a.swap_bytes(), dest);
            }
            ReverseBitsU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_u32(a.reverse_bits(), dest);
            }

            // Wrapping arithmetic on u32.
            AddWrappingU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a.wrapping_add(b), dest);
            }
            SubWrappingU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a.wrapping_sub(b), dest);
            }
            MulWrappingU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a.wrapping_mul(b), dest);
            }
            RemU32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_u32(a % b, dest);
            }

            // Type conversions.
            U32ToI32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_i32(a as i32, dest);
            }
            I32ToU32 => {
                let a = self.read_i32(&args[0], frame, frames)?;
                self.write_u32(a as u32, dest);
            }
            NegWrappingI32 => {
                let a = self.read_i32(&args[0], frame, frames)?;
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
                let a = self.read_i32(&args[0], frame, frames)?;
                let b = self.read_u32(&args[1], frame, frames)?;
                self.write_i32(a.wrapping_shr(b), dest);
            }
            SremI32 => {
                let a = self.read_i32(&args[0], frame, frames)?;
                let b = self.read_i32(&args[1], frame, frames)?;
                self.write_i32(a % b, dest);
            }

            // F32 classification intrinsics.
            IsNanF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_bool(a.is_nan(), dest);
            }
            IsInfiniteF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_bool(a.is_infinite(), dest);
            }

            // F32 bit conversion.
            F32ToBits => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_u32(a.to_bits(), dest);
            }
            BitsToF32 => {
                let a = self.read_u32(&args[0], frame, frames)?;
                self.write_f32(f32::from_bits(a), dest);
            }

            // F32 math intrinsics.
            AbsF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_f32(a.abs(), dest);
            }
            SqrtF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_f32(a.sqrt(), dest);
            }
            FloorF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_f32(a.floor(), dest);
            }
            CeilF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_f32(a.ceil(), dest);
            }
            RoundF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                // Round to nearest even to match Cranelift's `nearest`.
                self.write_f32(round_ties_even(a), dest);
            }
            TruncF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                self.write_f32(a.trunc(), dest);
            }
            CopysignF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                let b = self.read_f32(&args[1], frame, frames)?;
                self.write_f32(a.copysign(b), dest);
            }
            MinF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                let b = self.read_f32(&args[1], frame, frames)?;
                self.write_f32(a.min(b), dest);
            }
            MaxF32 => {
                let a = self.read_f32(&args[0], frame, frames)?;
                let b = self.read_f32(&args[1], frame, frames)?;
                self.write_f32(a.max(b), dest);
            }
        }
        Ok(())
    }

    /// Read a u32 value from an operand.
    fn read_u32(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> Result<u32, InterpError> {
        let val = self.read_operand(op, frame, frames)?;
        Ok(unsafe { *(val.ptr as *const u32) })
    }

    /// Read an i32 value from an operand.
    fn read_i32(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> Result<i32, InterpError> {
        let val = self.read_operand(op, frame, frames)?;
        Ok(unsafe { *(val.ptr as *const i32) })
    }

    /// Write a u32 value to destination.
    fn write_u32(&self, value: u32, dest: Destination) {
        unsafe { *(dest.ptr as *mut u32) = value; }
    }

    /// Write an i32 value to destination.
    fn write_i32(&self, value: i32, dest: Destination) {
        unsafe { *(dest.ptr as *mut i32) = value; }
    }

    /// Write a bool value to destination.
    fn write_bool(&self, value: bool, dest: Destination) {
        unsafe { *(dest.ptr as *mut u8) = value as u8; }
    }

    /// Read an f32 value from an operand.
    fn read_f32(&self, op: &Operand, frame: &Frame, frames: &FrameStore) -> Result<f32, InterpError> {
        let val = self.read_operand(op, frame, frames)?;
        Ok(unsafe { *(val.ptr as *const f32) })
    }

    /// Write an f32 value to destination.
    fn write_f32(&self, value: f32, dest: Destination) {
        unsafe { *(dest.ptr as *mut f32) = value; }
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
