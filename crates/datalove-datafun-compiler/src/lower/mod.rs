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
//! **Script lowering** (`lower_script_unit`): Lowers REPL-style script units,
//! which are sequences of statements that can define functions, create bindings,
//! and reference values from previous units. Script units support incremental
//! execution where bindings persist across units.
//!
//! # Script Function Nesting
//!
//! When a script unit contains function definitions (`fn foo() { ... }`), those
//! functions are lowered as nested calls within the script lowering process:
//!
//! ```text
//! lower_script_unit
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
//! - `func`: Function lowering (`lower_function_for_module`, `lower_function_body`)
//! - `script`: Script unit lowering (`lower_script_unit`, `lower_script_fragment_raw`)
//! - `stmt`: Statement lowering (let, var, set, if, loop, return, etc.)
//! - `expr`: Expression lowering
//! - `literal`: Literal parsing (int, float, string constants)

mod context;
mod literal;
mod expr;
mod stmt;
mod func;
mod script;

// Re-export public types and functions.
pub use context::{LowerCtx, FrameState, ScriptLowerContext, ScriptUnitKind};
pub use func::lower_function_for_module;
pub use script::{lower_script_unit, lower_script_fragment_raw, lower_script_expr};

/// Errors that can occur during lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    VariableNotFound(String),
    VariableNotMutable(String),
    FunctionNotFound(String),
    InvalidLiteral(String),
    NotImplemented(String),
    ParseError,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    /// Drop analysis error (use-after-move, move-in-loop, etc.)
    DropAnalysisError(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::VariableNotFound(name) => write!(f, "variable not found: {}", name),
            LowerError::VariableNotMutable(name) => write!(f, "variable not mutable: {}", name),
            LowerError::FunctionNotFound(name) => write!(f, "function not found: {}", name),
            LowerError::InvalidLiteral(lit) => write!(f, "invalid literal: {}", lit),
            LowerError::NotImplemented(what) => write!(f, "not implemented: {}", what),
            LowerError::ParseError => write!(f, "parse error in source"),
            LowerError::BreakOutsideLoop => write!(f, "break outside of loop"),
            LowerError::ContinueOutsideLoop => write!(f, "continue outside of loop"),
            LowerError::DropAnalysisError(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for LowerError {}
