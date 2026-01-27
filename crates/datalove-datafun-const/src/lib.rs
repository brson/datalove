//! Compile-time constant handling for datafun.
//!
//! This crate provides const inlining: replacing const binding expressions
//! with their evaluated constant values in IR.

pub mod inline;

// Re-export commonly used items.
pub use inline::{inline_function_consts, inline_module_functions, inline_script_consts};
