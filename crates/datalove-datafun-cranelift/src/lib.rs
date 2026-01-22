//! Shared Cranelift codegen infrastructure for datalove.
//!
//! This crate provides the core code generation facilities used by both
//! AOT and JIT compilation backends:
//!
//! - [`codegen`]: IR to Cranelift translation.
//! - [`types`]: Type mapping from IR to Cranelift.
//! - [`layout`]: Stack frame layout computation.
//! - [`runtime`]: Runtime function imports.
//! - [`tydesc_emit`]: Type descriptor emission.
//! - [`index_types`]: Index-sized value types.

/// IR to Cranelift translation.
pub mod codegen;
/// Cranelift types for index-sized values.
pub mod index_types;
/// Stack frame layout computation.
pub mod layout;
/// Runtime function imports.
pub mod runtime;
/// Type descriptor emission as static data.
pub mod tydesc_emit;
/// Type mapping from IR to Cranelift.
pub mod types;

/// Errors during Cranelift compilation.
#[derive(Debug)]
pub enum CraneliftError {
    /// Cranelift codegen error.
    Codegen(String),
    /// Module error.
    Module(String),
    /// Unsupported feature.
    Unsupported(String),
}

impl std::fmt::Display for CraneliftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CraneliftError::Codegen(msg) => write!(f, "codegen error: {}", msg),
            CraneliftError::Module(msg) => write!(f, "module error: {}", msg),
            CraneliftError::Unsupported(msg) => write!(f, "unsupported: {}", msg),
        }
    }
}

impl std::error::Error for CraneliftError {}
