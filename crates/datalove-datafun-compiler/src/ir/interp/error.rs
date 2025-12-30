//! Interpreter errors.

use super::super::{ValueId, SlotId, BlockId, FuncId};

/// Interpreter error.
#[derive(Debug)]
pub enum InterpError {
    /// Type information missing for value.
    MissingType(ValueId),
    /// Type information missing for slot.
    MissingSlotType(SlotId),
    /// Value not initialized.
    UninitializedValue(ValueId),
    /// Slot not initialized.
    UninitializedSlot(SlotId),
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
    ModuleFunctionNotFound(String),
    /// Phi node missing predecessor.
    PhiMissingPredecessor {
        dest: ValueId,
        pred: BlockId,
        available: Vec<BlockId>,
    },
}
