//! Compile-time constant handling for datafun.
//!
//! This crate provides:
//! - Const evaluation: executing pre-lowered IR to produce constant values
//! - Const inlining: replacing const binding expressions with evaluated values in IR

pub mod eval;
pub mod inline;

// Re-export commonly used items.
pub use eval::{
    PreparedConst, ScriptFunctionConstsResult,
    evaluate_consts_prepared, evaluate_function_consts_prepared, evaluate_prepared_const,
};
pub use inline::{inline_function_consts, inline_module_functions, inline_script_consts};
