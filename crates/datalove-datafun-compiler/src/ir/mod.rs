//! SSA-based intermediate representation for datafun functions.
//!
//! This IR uses SSA form for immutable values (expression temps, let bindings)
//! and explicit slots for mutable bindings (var). Designed for:
//! - Simple slot-based interpretation
//! - Direct lowering to LLVM/Cranelift (SSA values -> registers, slots -> stack)

use rmx::prelude::*;
use serde::{Deserialize, Serialize};

pub mod lower;
pub mod display;

/// SSA value - defined exactly once, immutable.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ValueId(pub u32);

/// Mutable slot - for var bindings, can be reassigned.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SlotId(pub u32);

/// Block identifier.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// Operand - either SSA value or mutable slot.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Operand {
    Value(ValueId),
    Slot(SlotId),
}

/// Constant value that can be loaded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConstValue {
    Unit,
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    // TODO: Int (bigint), Float, String, etc.
}

/// Binary operator.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// Unary operator.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

/// Flat instruction - no nesting, 2-3 operands max.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Instruction {
    /// Load a constant value.
    Const { dest: ValueId, value: ConstValue },

    /// Copy a value (for Copy types).
    Copy { dest: ValueId, src: Operand },

    /// Move a value (transfers ownership).
    Move { dest: ValueId, src: Operand },

    /// Binary operation.
    BinOp {
        dest: ValueId,
        op: BinOp,
        lhs: Operand,
        rhs: Operand,
    },

    /// Unary operation.
    UnaryOp {
        dest: ValueId,
        op: UnaryOp,
        operand: Operand,
    },

    /// Checked binary operation (produces value + overflow flag).
    BinOpChecked {
        dest: ValueId,
        overflow: ValueId,
        op: BinOp,
        lhs: Operand,
        rhs: Operand,
    },

    /// Function call.
    Call {
        dest: ValueId,
        func: String, // TODO: proper FuncId
        args: Vec<Operand>,
    },

    /// Pack fields into a struct/tuple.
    Pack {
        dest: ValueId,
        ty: String, // TODO: proper TypeId
        fields: Vec<Operand>,
    },

    /// Unpack struct/tuple into fields.
    Unpack { dests: Vec<ValueId>, src: Operand },

    /// Access a field of a struct.
    FieldAccess {
        dest: ValueId,
        base: Operand,
        field: String, // TODO: proper FieldId
    },

    /// Access a tuple element by index.
    TupleIndex {
        dest: ValueId,
        base: Operand,
        index: u32,
    },

    /// Wrap value in Some.
    WrapSome { dest: ValueId, inner: Operand },

    /// Wrap value in Ok.
    WrapOk { dest: ValueId, inner: Operand },

    /// Wrap value in Err.
    WrapErr { dest: ValueId, inner: Operand },

    /// Create None.
    WrapNone { dest: ValueId },

    /// Unwrap Option, producing (inner_value, is_some).
    UnwrapOption {
        dest: ValueId,
        is_some: ValueId,
        src: Operand,
    },

    /// Unwrap Result, producing (inner_value, is_ok).
    UnwrapResult {
        dest: ValueId,
        is_ok: ValueId,
        src: Operand,
    },

    /// Create a new list.
    ListNew {
        dest: ValueId,
        elements: Vec<Operand>,
    },

    /// Create a new set.
    SetNew {
        dest: ValueId,
        elements: Vec<Operand>,
    },

    /// Create a new map.
    MapNew {
        dest: ValueId,
        entries: Vec<(Operand, Operand)>,
    },

    /// Store value to mutable slot.
    SlotStore { slot: SlotId, value: Operand },

    /// Load value from mutable slot.
    SlotLoad { dest: ValueId, slot: SlotId },

    /// Phi node - merge values at control flow join.
    Phi {
        dest: ValueId,
        incoming: Vec<(BlockId, Operand)>,
    },

    /// Drop a value (run destructor).
    Drop { operand: Operand },

    /// No operation.
    Nop,
}

/// Block terminator - how control leaves a basic block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Terminator {
    /// Unconditional jump.
    Goto(BlockId),

    /// Conditional branch.
    Branch {
        cond: Operand,
        then_block: BlockId,
        else_block: BlockId,
    },

    /// Return from function.
    Return { value: Option<Operand> },

    /// Early return (from ? or checked operators).
    TryReturn { value: Option<Operand> },
}

/// A basic block - sequence of instructions followed by a terminator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrBlock {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

/// IR for a single function.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrFunction {
    pub name: String,
    pub params: Vec<ValueId>,
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
}

impl IrFunction {
    /// Get the entry block (always block 0).
    pub fn entry_block(&self) -> &IrBlock {
        &self.blocks[0]
    }
}

/// Result of lowering a module to IR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrModule {
    pub functions: Vec<IrFunction>,
}
