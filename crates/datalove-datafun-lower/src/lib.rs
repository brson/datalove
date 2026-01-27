//! Lower AST to SSA IR.
//!
//! Transforms typechecked AST into flat SSA IR suitable for interpretation
//! and AOT codegen.
//!
//! # Two Lowering Modes
//!
//! **Module lowering** (`lower_function_for_module`): Lowers standalone functions
//! defined in `.dlm` module files. Each function is lowered independently with
//! its own `LowerCtx`.
//!
//! **Script lowering** (`lower_script_fragment_raw`, `lower_script_expr`): Lowers
//! REPL-style script units, which are sequences of statements that can define
//! functions, create bindings, and reference values from previous units. Script
//! units support incremental execution where bindings persist across units.
//!
//! # Script Function Nesting
//!
//! When a script unit contains function definitions (`fn foo() { ... }`), those
//! functions are lowered as nested calls within the script lowering process:
//!
//! ```text
//! lower_script_fragment_raw
//!   -> lower_statement_for_script (for each statement)
//!        -> Statement::Fun case:
//!             1. swap_body_state(FrameState::new())  // Save script state
//!             2. lower_function_body(...)            // Lower the function
//!             3. swap_body_state(saved)              // Restore script state
//!             4. Add IrFunction to unit's functions list
//! ```
//!
//! The `FrameState` swap isolates each function's IR (blocks, values, slots)
//! from the parent script's IR, while `LowerCtx` fields like `symbols` and
//! `func_scope` are shared so the function can see script-level definitions.
//!
//! # Key Types
//!
//! - [`LowerCtx`]: Main lowering context, holds both shared state (db, type info)
//!   and the current [`FrameState`]
//! - [`FrameState`]: Per-function IR state (blocks, values, slots, variables).
//!   Swapped when entering/exiting nested functions.
//! - [`ScriptLowerContext`]: Tracks bindings exported from previous script units
//!   for cross-unit references
//!
//! # Submodules
//!
//! - `context`: Context types (`LowerCtx`, `FrameState`, `ScriptLowerContext`)
//! - `const_expr`: Compile-time constant expression evaluation
//! - `func`: Function lowering (`lower_function_for_module`, `lower_function_body`)
//! - `script`: Script unit lowering (`lower_script_fragment_raw`, `lower_script_expr`)
//! - `stmt`: Statement lowering (let, var, set, if, loop, return, etc.)
//! - `expr`: Expression lowering
//! - `literal`: Literal parsing (int, float, string constants)

use std::collections::HashMap;
use datalove_datafun_ast::ast::StmtFun;
use datalove_datafun_sema::FunctionAnalysis;

mod context;
pub(crate) mod literal;
pub mod const_expr;
mod expr;
mod stmt;
mod func;
mod script;
mod ir_ext;

// Re-export public types and functions.
pub use context::{LowerCtx, FrameState, ScriptLowerContext, ScriptUnitKind};
pub use func::lower_function_for_module;
pub use script::{lower_script_fragment_raw, lower_script_expr, lower_script_functions};
pub use const_expr::{evaluate_consts, evaluate_script_function_consts, ScriptFunctionConstsResult};
pub use ir_ext::IrTypeExt;

/// Pre-computed drop analyses for functions in a script unit.
pub type ScriptFunctionAnalyses<'db> = HashMap<StmtFun<'db>, FunctionAnalysis>;

/// Errors that can occur during lowering.
///
/// Most semantic errors are caught by the typechecker before lowering runs.
/// These represent edge cases that lowering cannot handle.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    /// Function not found in scope. Occurs for mutual recursion in script units
    /// where functions are lowered sequentially and a function references another
    /// that hasn't been lowered yet.
    FunctionNotFound(String),
    /// Invalid literal value (e.g., integer out of range for target type).
    InvalidLiteral(String),
    /// Feature not implemented for CTFE or const expressions.
    NotImplemented(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::FunctionNotFound(name) => write!(f, "function not found: {}", name),
            LowerError::InvalidLiteral(lit) => write!(f, "invalid literal: {}", lit),
            LowerError::NotImplemented(msg) => write!(f, "not implemented: {}", msg),
        }
    }
}

impl std::error::Error for LowerError {}
