//! Lower AST to SSA IR.
//!
//! This module transforms typechecked AST into flat SSA IR suitable for
//! interpretation and codegen.
//!
//! ## Module Structure
//!
//! - `context`: Lowering context types (`LowerCtx`, `ScriptLowerContext`)
//! - `scope`: Scope tracking for drop insertion
//! - `literal`: Literal parsing utilities
//! - `expr`: Expression lowering
//! - `stmt`: Statement and control flow lowering
//! - `func`: Function lowering
//! - `script`: Script unit lowering

mod context;
mod scope;
mod literal;
mod expr;
mod stmt;
mod func;
mod script;

// Re-export public types and functions.
pub use context::{LowerCtx, ScriptLowerContext, ScriptUnitKind};
pub use func::lower_function_for_module;
pub use script::{
    lower_script_unit, lower_script_fragment_raw, lower_script_expr,
    analyze_script_functions, ScriptFunctionAnalyses,
};

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
        }
    }
}

impl std::error::Error for LowerError {}
