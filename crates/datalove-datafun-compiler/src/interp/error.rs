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
    /// Try operator (? or !) triggered early return.
    EarlyReturn,

    // Checked arithmetic errors.
    Overflow,
    DivisionByZero,

    /// Optional operator overflow/error - return None.
    OptionNone,
    /// Result operator error - return Err with this payload.
    ResultErr {
        tydesc: *const datalove_rt::rtdt::TyDesc,
        ptr: *mut u8,
    },

    NoOutputVariable,
}
