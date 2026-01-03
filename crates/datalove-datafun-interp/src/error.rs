//! Interpreter errors.

use datalove_datafun_ir::{ValueId, SlotId, ParamId, BlockId, FuncId, IrModuleId};

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Type information missing for value.
    MissingType(ValueId),
    /// Type information missing for slot.
    MissingSlotType(SlotId),
    /// Parameter not found.
    MissingParam(ParamId),
    /// Value not initialized.
    UninitializedValue(ValueId),
    /// Slot not initialized.
    UninitializedSlot(SlotId),
    /// Parameter not initialized.
    UninitializedParam(ParamId),
    /// Block not found.
    BlockNotFound(BlockId),
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
    /// Phi node missing predecessor.
    PhiMissingPredecessor {
        dest: ValueId,
        pred: BlockId,
        available: Vec<BlockId>,
    },
}
