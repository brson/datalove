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
pub mod interp;
pub mod drop_analysis;

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
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FuncRef {
    /// Function defined locally (in the current unit or module).
    Local(FuncId),
    /// Function from a previous script unit.
    External { unit: u32, func: FuncId },
    /// Function from a module, referenced by name.
    /// The actual function is stored in ScriptEnvironment::module_functions.
    Module { name: String },
}

/// Reference to a type (used in Pack instructions).
///
/// Lightweight type tag without inner type details.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TypeRef {
    Bool,
    U8, U16, U32, U64,
    I8, I16, I32, I64,
    Int,
    Tuple(u32),
    AnonStruct(u32),
    Option,
    Result,
    List,
    Set,
    Map,
}

/// Full type information for IR values.
///
/// Self-contained type representation for frame layout computation
/// and runtime TyDesc creation. Unlike salsa-interned types, this
/// is serializable and database-independent.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum IrType {
    /// Unit type (empty tuple).
    Unit,
    /// Boolean.
    Bool,
    /// Unsigned integers.
    U8, U16, U32, U64,
    /// Signed integers.
    I8, I16, I32, I64,
    /// Arbitrary-precision integer.
    Int,
    /// 32-bit float.
    F32,
    /// UTF-8 string.
    String,
    /// Dynamic data value.
    Data,
    /// Error value.
    Error,
    /// Anonymous tuple with field types.
    Tuple(Vec<IrType>),
    /// Anonymous struct with named fields (sorted by name).
    Struct(Vec<(String, IrType)>),
    /// List with element type.
    List(Box<IrType>),
    /// Set with element type.
    Set(Box<IrType>),
    /// Map with key and value types.
    Map(Box<IrType>, Box<IrType>),
    /// Option with inner type.
    Option(Box<IrType>),
    /// Result with ok type.
    Result(Box<IrType>),
}

impl IrType {
    /// Convert from typechecker type to IR type.
    pub fn from_tycheck<'db>(db: &'db dyn crate::Db, ty: &crate::tycheck::TypeAndHeap<'db>) -> Self {
        use crate::tycheck::Type as TyType;

        match ty.ty(db) {
            TyType::Datalit(dl_ty) => Self::from_datalit(db, dl_ty),
            TyType::Function(_) => {
                todo!("function types in IR")
            }
        }
    }

    /// Convert from AST type hint to IR type.
    pub fn from_type_hint<'db>(db: &'db dyn crate::Db, ty: &datalove_datalit::ast::TypeHintAndHeap<'db>) -> Self {
        Self::from_type_hint_inner(db, &ty.type_hint(db))
    }

    /// Convert from AST TypeHint to IR type.
    fn from_type_hint_inner<'db>(db: &'db dyn crate::Db, ty: &datalove_datalit::ast::TypeHint<'db>) -> Self {
        use datalove_datalit::ast::TypeHint;

        match ty {
            TypeHint::Bool => IrType::Bool,
            TypeHint::U8 => IrType::U8,
            TypeHint::I8 => IrType::I8,
            TypeHint::U16 => IrType::U16,
            TypeHint::I16 => IrType::I16,
            TypeHint::U32 => IrType::U32,
            TypeHint::I32 => IrType::I32,
            TypeHint::U64 => IrType::U64,
            TypeHint::I64 => IrType::I64,
            TypeHint::F32 => IrType::F32,
            TypeHint::Int => IrType::Int,
            TypeHint::String => IrType::String,
            TypeHint::Data => IrType::Data,
            TypeHint::Error => IrType::Error,
            TypeHint::AnonTuple(tuple) => {
                let fields: Vec<_> = tuple.fields(db)
                    .iter()
                    .map(|f| Self::from_type_hint(db, f))
                    .collect();
                if fields.is_empty() {
                    IrType::Unit
                } else {
                    IrType::Tuple(fields)
                }
            }
            TypeHint::AnonStruct(struct_) => {
                let fields: Vec<_> = struct_.fields(db)
                    .iter()
                    .map(|f| (f.name(db).text(db).to_string(), Self::from_type_hint(db, &f.type_hint(db))))
                    .collect();
                IrType::Struct(fields)
            }
            TypeHint::AnonEnum(_) => {
                todo!("anonymous enum types in IR")
            }
            TypeHint::List(list) => {
                let elem = Self::from_type_hint(db, &list.element_type(db));
                IrType::List(Box::new(elem))
            }
            TypeHint::Set(set) => {
                let elem = Self::from_type_hint(db, &set.element_type(db));
                IrType::Set(Box::new(elem))
            }
            TypeHint::Map(map) => {
                let key = Self::from_type_hint(db, &map.key_type(db));
                let val = Self::from_type_hint(db, &map.value_type(db));
                IrType::Map(Box::new(key), Box::new(val))
            }
            TypeHint::Option(opt) => {
                let inner = Self::from_type_hint(db, &opt.inner_type(db));
                IrType::Option(Box::new(inner))
            }
            TypeHint::Result(res) => {
                let ok = Self::from_type_hint(db, &res.inner_type(db));
                IrType::Result(Box::new(ok))
            }
            TypeHint::Tensor(_) => {
                todo!("tensor types in IR")
            }
            TypeHint::ParseError(_) => {
                IrType::Error
            }
        }
    }

    /// Convert from datalit TypeAndHeap to IR type.
    fn from_datalit_tyandheap<'db>(db: &'db dyn crate::Db, ty: &datalove_datalit::tycheck::TypeAndHeap<'db>) -> Self {
        Self::from_datalit(db, ty.ty(db))
    }

    /// Convert from datalit typechecker type to IR type.
    pub fn from_datalit<'db>(db: &'db dyn crate::Db, ty: &datalove_datalit::tycheck::Type<'db>) -> Self {
        use datalove_datalit::tycheck::Type as DlType;

        match ty {
            DlType::Bool => IrType::Bool,
            DlType::U8 => IrType::U8,
            DlType::I8 => IrType::I8,
            DlType::U16 => IrType::U16,
            DlType::I16 => IrType::I16,
            DlType::U32 => IrType::U32,
            DlType::I32 => IrType::I32,
            DlType::U64 => IrType::U64,
            DlType::I64 => IrType::I64,
            DlType::F32 => IrType::F32,
            DlType::Int => IrType::Int,
            DlType::String => IrType::String,
            DlType::Data => IrType::Data,
            DlType::Error => IrType::Error,
            DlType::AnonTuple(tuple) => {
                let fields: Vec<_> = tuple.fields(db)
                    .iter()
                    .map(|f| Self::from_datalit_tyandheap(db, f))
                    .collect();
                if fields.is_empty() {
                    IrType::Unit
                } else {
                    IrType::Tuple(fields)
                }
            }
            DlType::AnonStruct(struct_) => {
                let fields: Vec<_> = struct_.fields(db)
                    .iter()
                    .map(|f| (f.name(db).text(db).to_string(), Self::from_datalit_tyandheap(db, &f.ty(db))))
                    .collect();
                IrType::Struct(fields)
            }
            DlType::AnonEnum(_) => {
                todo!("anonymous enum types in IR")
            }
            DlType::List(list) => {
                let elem = Self::from_datalit_tyandheap(db, &list.element_type(db));
                IrType::List(Box::new(elem))
            }
            DlType::Set(set) => {
                let elem = Self::from_datalit_tyandheap(db, &set.element_type(db));
                IrType::Set(Box::new(elem))
            }
            DlType::Map(map) => {
                let key = Self::from_datalit_tyandheap(db, &map.key_type(db));
                let val = Self::from_datalit_tyandheap(db, &map.value_type(db));
                IrType::Map(Box::new(key), Box::new(val))
            }
            DlType::Option(opt) => {
                let inner = Self::from_datalit_tyandheap(db, &opt.inner_type(db));
                IrType::Option(Box::new(inner))
            }
            DlType::Result(res) => {
                let ok = Self::from_datalit_tyandheap(db, &res.inner_type(db));
                IrType::Result(Box::new(ok))
            }
            DlType::Tensor(_) => {
                todo!("tensor types in IR")
            }
        }
    }

    /// Check if this type is Copy (no heap allocations).
    ///
    /// Copy types can be duplicated with a shallow bitwise copy.
    /// Non-copy types require move semantics.
    pub fn is_copy(&self) -> bool {
        match self {
            // Primitives are always copy.
            IrType::Unit | IrType::Bool => true,
            IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64 => true,
            IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 => true,
            IrType::F32 => true,

            // Heap-allocated types are never copy.
            IrType::Int | IrType::String | IrType::Data | IrType::Error => false,
            IrType::List(_) | IrType::Set(_) | IrType::Map(_, _) => false,

            // Composite types are copy if all fields are copy.
            IrType::Tuple(fields) => fields.iter().all(|f| f.is_copy()),
            IrType::Struct(fields) => fields.iter().all(|(_, f)| f.is_copy()),

            // Option is copy if inner is copy.
            IrType::Option(inner) => inner.is_copy(),
            // Result is never copy (Err variant contains non-copy Error).
            IrType::Result(_) => false,
        }
    }
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
    /// Bigint stored as limbs (little-endian base 2^32) and sign.
    /// Empty limbs = 0.
    Int { limbs: Vec<u32>, negative: bool },
    /// String literal.
    String(String),
    // TODO: Float, etc.
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

    /// Checked unary operation (produces value + overflow flag).
    UnaryOpChecked {
        dest: ValueId,
        overflow: ValueId,
        op: UnaryOp,
        operand: Operand,
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

    /// Unwrap Result, producing (ok_value, err_value, is_ok).
    ///
    /// - ok_dest: receives Ok payload when is_ok=true
    /// - err_dest: receives Error when is_ok=false
    UnwrapResult {
        ok_dest: ValueId,
        err_dest: ValueId,
        is_ok: ValueId,
        src: Operand,
    },

    /// Create Error from any value (consumes inner).
    ErrorFrom { dest: ValueId, inner: Operand },

    /// Create Data from any value (consumes inner).
    DataFrom { dest: ValueId, inner: Operand },

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
    /// Type for each ValueId (indexed by ValueId.0).
    pub value_types: Vec<IrType>,
    /// Type for each SlotId (indexed by SlotId.0).
    pub slot_types: Vec<IrType>,
}

impl IrFunction {
    /// Get the entry block (always block 0).
    pub fn entry_block(&self) -> &IrBlock {
        &self.blocks[0]
    }

    /// Infer the return type from the IR.
    ///
    /// Finds the first Return terminator and gets the type of its value.
    /// Returns `IrType::Unit` if no value is returned.
    pub fn infer_return_type(&self) -> IrType {
        for block in &self.blocks {
            if let Terminator::Return { value: Some(op) } = &block.terminator {
                return match op {
                    Operand::Value(id) => self.value_types[id.0 as usize].clone(),
                    Operand::Slot(id) => self.slot_types[id.0 as usize].clone(),
                    // External operands not expected in function returns.
                    Operand::ExternalValue { .. } | Operand::ExternalSlot { .. } => IrType::Unit,
                };
            }
        }
        IrType::Unit
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
    /// Type for each ValueId (indexed by ValueId.0).
    pub value_types: Vec<IrType>,
    /// Type for each SlotId (indexed by SlotId.0).
    pub slot_types: Vec<IrType>,
    /// Functions defined in this unit.
    pub functions: Vec<IrFunction>,
    /// Symbol table for this unit.
    pub symbols: SymbolTable,
    /// Result value of this unit (for expression units in REPL).
    pub result: Option<ValueId>,
    /// Names exported to later units.
    pub exports: Vec<(String, ExportBinding)>,
}
