//! Interpreter errors.

use datalove_datafun_ir::{FuncId, IrModuleId};

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Function not found.
    FunctionNotFound(FuncId),
    /// Arithmetic overflow.
    Overflow,
    /// Division by zero.
    DivisionByZero,
    /// Runtime error.
    RuntimeError(String),
    /// Module function not found.
    ModuleFunctionNotFound { module: IrModuleId, func: FuncId },
}
