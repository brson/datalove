//! SSA-based intermediate representation for datafun functions.
//!
//! This IR uses SSA form for immutable values (expression temps, let bindings)
//! and explicit slots for mutable bindings (var). Designed for:
//! - Simple slot-based interpretation
//! - Direct lowering to LLVM/Cranelift (SSA values -> registers, slots -> stack)

use rmx::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Globally unique function identifier within a compilation context.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

/// Reference to a function.
///
/// Functions can be defined in the current unit or imported from previous units.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FuncRef {
    /// Function defined locally (in the current unit or module).
    Local(FuncId),
    /// Function from a previous script unit.
    External { unit: u32, func: FuncId },
}

/// Reference to a type.
///
/// All compound types in datalove are structural (anonymous).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TypeRef {
    /// Boolean type.
    Bool,
    /// Unsigned integers.
    U8, U16, U32, U64,
    /// Signed integers.
    I8, I16, I32, I64,
    /// Arbitrary-precision integer.
    Int,
    /// Tuple with N elements (0 = unit type).
    Tuple(u32),
    /// Anonymous struct with N fields.
    AnonStruct(u32),
    /// Option type.
    Option,
    /// Result type.
    Result,
    /// List type.
    List,
    /// Set type.
    Set,
    /// Map type.
    Map,
}

/// Metadata about a function definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FuncDef {
    pub id: FuncId,
    pub name: String,
    pub param_count: usize,
}

/// Symbol table for IR resolution.
///
/// Maps function IDs to their definitions and provides name lookup.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SymbolTable {
    /// All function definitions, indexed by FuncId.
    pub functions: Vec<FuncDef>,
    /// Name to FuncId mapping for the current scope (lowering-time only).
    #[serde(skip)]
    name_to_func: HashMap<String, FuncId>,
    /// Next FuncId to allocate.
    next_func_id: u32,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Define a new function and return its ID.
    pub fn define_func(&mut self, name: String, param_count: usize) -> FuncId {
        let id = FuncId(self.next_func_id);
        self.next_func_id += 1;
        self.functions.push(FuncDef { id, name: name.clone(), param_count });
        self.name_to_func.insert(name, id);
        id
    }

    /// Look up a function by name.
    pub fn lookup_func(&self, name: &str) -> Option<FuncId> {
        self.name_to_func.get(name).copied()
    }

    /// Get function definition by ID.
    pub fn get_func(&self, id: FuncId) -> Option<&FuncDef> {
        self.functions.get(id.0 as usize)
    }

    /// Import a function from an external source into the current scope.
    pub fn import_func(&mut self, name: String, id: FuncId) {
        self.name_to_func.insert(name, id);
    }
}

/// Operand - either SSA value or mutable slot, local or from a previous script unit.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Operand {
    /// Local SSA value.
    Value(ValueId),
    /// Local mutable slot.
    Slot(SlotId),
    /// SSA value from a previous script unit.
    ExternalValue { unit: u32, value: ValueId },
    /// Mutable slot from a previous script unit.
    ExternalSlot { unit: u32, slot: SlotId },
}

/// Destination for slot store operations.
///
/// Distinguishes between local slots (in current unit) and external slots
/// (in a previous script unit).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SlotDest {
    /// Local slot in current unit.
    Local(SlotId),
    /// Slot in a previous script unit.
    External { unit: u32, slot: SlotId },
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
        func: FuncRef,
        args: Vec<Operand>,
    },

    /// Pack fields into a struct/tuple.
    Pack {
        dest: ValueId,
        ty: TypeRef,
        fields: Vec<Operand>,
    },

    /// Unpack struct/tuple into fields.
    Unpack { dests: Vec<ValueId>, src: Operand },

    /// Access a field of a struct by index.
    FieldAccess {
        dest: ValueId,
        base: Operand,
        field_index: u32,
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
    SlotStore { dest: SlotDest, value: Operand },

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

    /// Early return from function (from ? or checked operators).
    TryReturn { value: Option<Operand> },

    /// End of script unit (normal completion).
    UnitEnd { result: Option<Operand> },

    /// Early return from script unit (from ? or checked operators).
    UnitEarlyReturn { value: Operand },
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
    pub id: FuncId,
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
    pub symbols: SymbolTable,
}

/// What a script unit exports to subsequent units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ExportBinding {
    /// An SSA value (from let binding).
    Value(ValueId),
    /// A mutable slot (from var binding).
    Slot(SlotId),
    /// A function defined in this unit.
    Function(FuncId),
}

/// IR for a script unit.
///
/// Script units are executed sequentially and can reference values from
/// previous units via ExternalValue/ExternalSlot operands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrScriptUnit {
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    /// Functions defined in this unit.
    pub functions: Vec<IrFunction>,
    /// Symbol table for this unit.
    pub symbols: SymbolTable,
    /// Result value of this unit (for expression units in REPL).
    pub result: Option<ValueId>,
    /// Names exported to later units.
    pub exports: Vec<(String, ExportBinding)>,
}
