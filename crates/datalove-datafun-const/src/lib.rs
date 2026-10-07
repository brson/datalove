//! Compile-time constant handling for datafun.
//!
//! This crate provides:
//! - Const evaluation: executing pre-lowered IR to produce constant values
//! - Const inlining: replacing const binding expressions with evaluated values in IR
//! - Promotion: turning a function's consts into statics, built once and borrowed
//! - Dead code elimination: removing unused instructions and unreachable blocks

pub mod dce;
pub mod eval;
pub mod inline;
pub mod promote;

// Re-export commonly used items.
pub use dce::{eliminate_dead_blocks_func, eliminate_dead_code_unit, instruction_dest};
pub use eval::{ScriptFunctionConstsResult, evaluate_const_unit};
pub use inline::{inline_function_consts, inline_module_functions, inline_script_consts};
pub use promote::promote_function_consts;
