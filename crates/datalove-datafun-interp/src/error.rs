//! Interpreter errors.

use datalove_datafun_ir::{ValueId, SlotId, ParamId, FuncId, IrModuleId};

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Value not initialized (may occur during Drop).
    UninitializedValue(ValueId),
    /// Slot not initialized (may occur during Drop).
    UninitializedSlot(SlotId),
    /// Parameter not initialized (may occur during Drop).
    UninitializedParam(ParamId),
    /// Function not found.
    FunctionNotFound(FuncId),
    /// Arithmetic overflow.
    Overflow,
    /// Division by zero.
    DivisionByZero,
    /// Runtime error.
    RuntimeError(String),
    /// Type mismatch.
    TypeMismatch(String),
    /// External unit not found.
    ExternalUnitNotFound(u32),
    /// Module function not found.
    ModuleFunctionNotFound { module: IrModuleId, func: FuncId },
}
