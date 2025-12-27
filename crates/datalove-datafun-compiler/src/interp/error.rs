//! Interpreter error types.

use super::Value;

/// Interpreter errors.
#[derive(Debug)]
pub enum InterpError {
    // Analysis-time errors.
    TypeErrors,
    TypecheckErrors(Vec<crate::tycheck::TypeError>),
    AnalysisErrors(Vec<String>),

    // Runtime errors.
    VariableNotFound(String),
    UseAfterMove(String),
    FunctionNotFound(String),
    ModuleNotFound(String),
    InvalidExpression(String),
    RuntimeError(String),

    // Control flow signals.
    ReturnOutsideFunction,
    IfOutsideFunction,
    /// Propagates return value up the call stack.
    FunctionReturn(Value),

    NoOutputVariable,
}
