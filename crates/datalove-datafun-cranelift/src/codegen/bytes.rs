//! Copying and filling runs of bytes whose length is known when compiling.
//!
//! Most copies the backend makes are of one value's bytes, a size fixed by
//! its type and rarely more than a few words. Calling libc's `memcpy` or
//! `memset` for those costs more than the copy: Towers spent a third of its
//! time in them. Up to `INLINE_LIMIT` bytes these emit the loads and stores
//! themselves, eight bytes at a time and then the tail in smaller pieces.

use cranelift_codegen::ir::{self as cl_ir, InstBuilder, MemFlagsData, types as cl_types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;

use crate::types::PTR_TYPE;
use super::FunctionCompiler;

/// The most bytes copied or filled inline; above it, libc is called.
const INLINE_LIMIT: u64 = 64;

/// The pieces a run of `size` bytes is split into, as offset and width.
fn pieces(size: u64) -> impl Iterator<Item = (i32, cl_ir::Type)> {
    let mut offset = 0;
    std::iter::from_fn(move || {
        let left = size - offset;
        let (width, ty) = match left {
            0 => return None,
            8.. => (8, cl_types::I64),
            4..=7 => (4, cl_types::I32),
            2..=3 => (2, cl_types::I16),
            _ => (1, cl_types::I8),
        };
        let at = offset as i32;
        offset += width;
        Some((at, ty))
    })
}

impl<'a, M: Module> FunctionCompiler<'a, M> {
    /// Copy `size` bytes from `src` to `dst`, which do not overlap unless
    /// they are the same.
    pub(super) fn copy_bytes(
        &self,
        builder: &mut FunctionBuilder,
        dst: cl_ir::Value,
        src: cl_ir::Value,
        size: u64,
    ) {
        if size > INLINE_LIMIT {
            let size = builder.ins().iconst(PTR_TYPE, size as i64);
            builder.call_memcpy(self.isa.frontend_config(), dst, src, size);
            return;
        }
        let flags = MemFlagsData::new();
        for (at, ty) in pieces(size) {
            let v = builder.ins().load(ty, flags, src, at);
            builder.ins().store(flags, v, dst, at);
        }
    }

    /// Set `size` bytes at `dst` to `byte`.
    pub(super) fn fill_bytes(
        &self,
        builder: &mut FunctionBuilder,
        dst: cl_ir::Value,
        byte: u8,
        size: u64,
    ) {
        if size > INLINE_LIMIT {
            let byte = builder.ins().iconst(cl_types::I8, byte as i64);
            let size = builder.ins().iconst(PTR_TYPE, size as i64);
            builder.call_memset(self.isa.frontend_config(), dst, byte, size);
            return;
        }
        let flags = MemFlagsData::new();
        let pattern = byte as u64 * 0x0101_0101_0101_0101;
        for (at, ty) in pieces(size) {
            let bits = pattern >> (64 - ty.bits());
            let v = builder.ins().iconst(ty, bits as i64);
            builder.ins().store(flags, v, dst, at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_cover_the_run() {
        for size in 0..=INLINE_LIMIT {
            let mut next = 0;
            for (at, ty) in pieces(size) {
                assert_eq!(at as u64, next);
                next += ty.bytes() as u64;
            }
            assert_eq!(next, size);
        }
    }
}
