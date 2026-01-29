//! Compile-time function evaluation (CTFE) for const bindings.
//!
//! The const evaluator executes expressions at compile time to produce
//! [`ConstValue`]s that can be inlined at use sites.
//!
//! # Architecture
//!
//! Const evaluation reuses the existing interpreter with gas limits:
//!
//! 1. Lower the const initializer expression to a temporary IR unit
//! 2. Execute via interpreter with step limits
//! 3. Extract result as ConstValue
//! 4. Store for substitution at use sites
//!
//! # Limitations (MVP)
//!
//! - Function calls in const expressions are not yet supported
//! - Only primitive types are fully supported for extraction
//! - Gas limit is fixed (will be configurable later)

use datalove_datafun_ir::{ConstValue, IrType};

/// Default step limit for const evaluation.
pub const DEFAULT_GAS_LIMIT: usize = 1_000_000;

/// Errors that can occur during const evaluation.
#[derive(Debug, Clone)]
pub enum ConstEvalError {
    /// The expression did not terminate within the step limit.
    StepLimitExceeded { limit: usize },
    /// The expression returned an error via the `!` operator.
    EarlyReturn(String),
    /// Type extraction not yet supported.
    UnsupportedType(String),
    /// Function calls not yet supported in const expressions.
    FunctionCallNotSupported,
    /// Internal lowering error.
    LowerError(String),
    /// Internal interpreter error.
    InterpError(String),
}

impl std::fmt::Display for ConstEvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstEvalError::StepLimitExceeded { limit } => {
                write!(f, "const evaluation did not terminate within {} steps", limit)
            }
            ConstEvalError::EarlyReturn(msg) => {
                write!(f, "const evaluation returned an error: {}", msg)
            }
            ConstEvalError::UnsupportedType(ty) => {
                write!(f, "const evaluation: unsupported type {}", ty)
            }
            ConstEvalError::FunctionCallNotSupported => {
                write!(f, "function calls in const expressions not yet supported")
            }
            ConstEvalError::LowerError(msg) => {
                write!(f, "const evaluation lowering error: {}", msg)
            }
            ConstEvalError::InterpError(msg) => {
                write!(f, "const evaluation interpreter error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ConstEvalError {}

/// Extract a ConstValue from an interpreter value.
///
/// This reads the raw bytes from the interpreter's memory and converts
/// them to a ConstValue representation.
///
/// # Safety
///
/// The value pointer must be valid and the type must match.
pub unsafe fn extract_const_value(
    ptr: *const u8,
    ir_type: &IrType,
) -> Result<ConstValue, ConstEvalError> {
    // SAFETY: Caller guarantees ptr is valid and type matches.
    unsafe {
        match ir_type {
            IrType::Unit => Ok(ConstValue::Unit),
            IrType::Bool => {
                let val = *(ptr as *const bool);
                Ok(ConstValue::Bool(val))
            }
            IrType::U8 => {
                let val = *(ptr as *const u8);
                Ok(ConstValue::U8(val))
            }
            IrType::U16 => {
                let val = *(ptr as *const u16);
                Ok(ConstValue::U16(val))
            }
            IrType::U32 => {
                let val = *(ptr as *const u32);
                Ok(ConstValue::U32(val))
            }
            IrType::U64 => {
                let val = *(ptr as *const u64);
                Ok(ConstValue::U64(val))
            }
            IrType::I8 => {
                let val = *(ptr as *const i8);
                Ok(ConstValue::I8(val))
            }
            IrType::I16 => {
                let val = *(ptr as *const i16);
                Ok(ConstValue::I16(val))
            }
            IrType::I32 => {
                let val = *(ptr as *const i32);
                Ok(ConstValue::I32(val))
            }
            IrType::I64 => {
                let val = *(ptr as *const i64);
                Ok(ConstValue::I64(val))
            }
            IrType::Index => {
                let val = *(ptr as *const datalove_rtdt::IndexRepr);
                Ok(ConstValue::Index(val))
            }
            IrType::Offset => {
                let val = *(ptr as *const datalove_rtdt::OffsetRepr);
                Ok(ConstValue::Offset(val))
            }
            IrType::F32 => {
                let val = *(ptr as *const f32);
                Ok(ConstValue::F32(val))
            }
            IrType::F64 => {
                let val = *(ptr as *const f64);
                Ok(ConstValue::F64(val))
            }
            // Aggregate and collection types not yet supported for extraction.
            // TODO: Add support for Tuple, Struct, Option, Result, List, etc.
            _ => Err(ConstEvalError::UnsupportedType(format!("{:?}", ir_type))),
        }
    }
}
