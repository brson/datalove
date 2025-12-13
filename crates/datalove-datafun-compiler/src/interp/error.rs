//! Interpreter error types.

use super::Value;

/// Interpreter errors.
#[derive(Debug)]
pub enum InterpError {
    // Analysis-time errors.
    TypeErrors,
    /// Typecheck found errors.
    TypecheckErrors(Vec<crate::tycheck::TypeError>),
    AnalysisErrors(Vec<String>),

    // Runtime errors.
    VariableNotFound(String),
    UseAfterMove(String),
    FunctionNotFound(String),
    ModuleNotFound(String),
    InvalidExpression(String),
    RuntimeError(String),

    // Control flow.
    ReturnOutsideFunction,
    IfOutsideFunction,
    /// Internal: propagates return value up the call stack.
    FunctionReturn(Value),
    /// Internal: try operator (? or !) triggered early return from CFG.
    EarlyReturn,

    // Checked arithmetic errors.
    Overflow,
    DivisionByZero,

    /// Optional arithmetic overflow - triggers early return with None.
    OptionNone,
    /// Result error - triggers early return with Err.
    /// Carries the error value (tydesc + ptr) to be wrapped in Result::Err.
    ResultErr {
        tydesc: *const datalove_rt::rtdt::TyDesc,
        ptr: *mut u8,
    },

    /// Script completed without setting `output` variable.
    NoOutputVariable,
}
