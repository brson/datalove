//! Cranelift types for index-sized values (usize/isize).
//!
//! Centralizes the conditional compilation for 32-bit vs 64-bit index types.

use cranelift_codegen::ir::types as cl_types;

/// Cranelift type for IndexRepr/OffsetRepr values.
#[cfg(not(feature = "index-64"))]
pub const INDEX_TYPE: cranelift_codegen::ir::Type = cl_types::I32;
#[cfg(feature = "index-64")]
pub const INDEX_TYPE: cranelift_codegen::ir::Type = cl_types::I64;

/// Bit width of index types.
#[cfg(not(feature = "index-64"))]
pub const INDEX_BITS: u8 = 32;
#[cfg(feature = "index-64")]
pub const INDEX_BITS: u8 = 64;
