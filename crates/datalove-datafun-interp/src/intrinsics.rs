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
}
