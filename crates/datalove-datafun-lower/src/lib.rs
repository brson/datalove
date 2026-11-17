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
//!             4. Add IrCodeUnit to unit's nested_units list
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

use datalove_datafun_ast::ast::FunKey;
use datalove_datafun_sema::FunctionAnalysis;

mod context;
mod datafile;
pub(crate) mod literal;
pub mod const_expr;
mod expr;
mod stmt;
mod func;
mod script;
mod ir_ext;

// Re-export public types and functions.
pub use context::{DataFiles, FuncIds, LowerCtx, FrameState, ScriptLowerContext};
pub use datafile::data_file_value;
pub use func::lower_function_for_module;
pub use script::{
    lower_script_fragment_raw, lower_script_expr, lower_script_functions,
    consts_above, ScriptFunctionsLowered,
};
pub use const_expr::{
    lower_const_binding, lower_const_expr_to_unit_standalone, try_extract_literal,
};
pub use ir_ext::IrTypeExt;

/// Where each module function lives in the IR, by module and name.
pub type FuncIdTable<'db> = bct::lookup::LookupMap<
    (bct::module_graph::ModuleId<'db>, String),
    (datalove_datafun_ir::IrModuleId, datalove_datafun_ir::FuncId),
>;

/// Pre-computed drop analyses for functions in a script unit.
pub type ScriptFunctionAnalyses<'db> = bct::lookup::LookupMap<FunKey<'db>, FunctionAnalysis<'db>>;

/// Errors that can occur during lowering.
///
/// Most semantic errors are caught by the typechecker before lowering runs.
/// These represent edge cases that lowering cannot handle.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    /// A name the typechecker accepted has no value yet.
    ///
    /// Reached for a module-level const, whose value is only known once it has
    /// been evaluated, which needs the functions it calls to be lowered first.
    /// The caller defers the function and lowers it again after evaluation.
    ///
    /// In a script unit it is also a binding of an earlier unit that
    /// typechecked and did not compile, which the unit cannot lower against.
    BindingNotAvailable {
        name: String,
        /// Where the name is written: the innermost expression being lowered
        /// when it was found missing, or the `set` writing it. Filled in as
        /// the error leaves the expression or statement.
        at: Option<WrittenAt>,
    },
}

/// Where in a unit or module a name is written, as its span table finds it.
///
/// Owned rather than an `ExprKey`, whose function name is interned, so that
/// `LowerError` need not carry the database's lifetime.
#[derive(Debug, Clone, PartialEq)]
pub enum WrittenAt {
    /// An expression, by the parts of its `ExprKey` but the module.
    Expr { fn_name: Option<String>, local_index: u32 },
    /// A `set`, by its `local_index`.
    Set(u32),
}

impl LowerError {
    /// A missing binding, `name`, with where it is written still to come.
    pub fn missing(name: impl Into<String>) -> LowerError {
        LowerError::BindingNotAvailable { name: name.into(), at: None }
    }

    /// This error, written at `at` unless it already says where it is.
    pub fn at(self, at: impl FnOnce() -> WrittenAt) -> LowerError {
        match self {
            LowerError::BindingNotAvailable { name, at: None } =>
                LowerError::BindingNotAvailable { name, at: Some(at()) },
            located => located,
        }
    }
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::BindingNotAvailable { name, .. } => write!(f, "binding not available yet: {}", name),
        }
    }
}

impl std::error::Error for LowerError {}
