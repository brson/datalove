//! SSA-based intermediate representation for datafun functions.
//!
//! This IR uses SSA form for immutable values (expression temps, let bindings)
//! and explicit slots for mutable bindings (var). Designed for:
//! - Simple slot-based interpretation
//! - Direct lowering to LLVM/Cranelift (SSA values -> registers, slots -> stack)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod display;
pub mod registry;

pub use registry::FunctionRegistry;

/// SSA value - defined exactly once, immutable.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ValueId(pub u32);

/// Mutable slot - for var bindings, can be reassigned.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SlotId(pub u32);

/// Function parameter - reference to caller's data.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ParamId(pub u32);

/// Block identifier.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// Globally unique function identifier within a compilation context.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct FuncId(pub u32);

/// Module identifier for IR.
///
/// Serializable numeric ID, unlike salsa's ModuleId.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct IrModuleId(pub u32);

/// Reference to a function.
///
/// Functions can be defined in the current unit or imported from previous units.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FuncRef {
    /// Function defined locally (in the current unit or module).
    Local(FuncId),
    /// Function from a previous script unit.
    External { unit: u32, func: FuncId },
    /// Function from a module.
    Module { module: IrModuleId, func: FuncId },
}

/// Reference to a type (used in Pack instructions).
///
/// Lightweight type tag without inner type details.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TypeRef {
    Bool,
    U8, U16, U32, U64,
    I8, I16, I32, I64,
    Usize, Isize,
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
    /// Collection index types (size depends on index-64 feature).
    Usize, Isize,
    /// Arbitrary-precision integer.
    Int,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
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
    /// Anonymous enum with variants (name and optional payload type, sorted by name).
    Enum(Vec<(String, Option<IrType>)>),
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
    /// Tensor with element type and rank.
    Tensor(Box<IrType>, u32),
    /// Reference to a value (pointer to data of inner type).
    ///
    /// Used for field refs passed to ref/mut/out params.
    /// Layout is pointer-sized (8 bytes), stores a pointer to the inner type.
    Ref(Box<IrType>),
    /// Table with named columns (sorted by name).
    Table(Vec<(String, Box<IrType>)>),
}

impl std::fmt::Display for IrType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrType::Unit => write!(f, "unit"),
            IrType::Bool => write!(f, "bool"),
            IrType::U8 => write!(f, "u8"),
            IrType::U16 => write!(f, "u16"),
            IrType::U32 => write!(f, "u32"),
            IrType::U64 => write!(f, "u64"),
            IrType::I8 => write!(f, "i8"),
            IrType::I16 => write!(f, "i16"),
            IrType::I32 => write!(f, "i32"),
            IrType::I64 => write!(f, "i64"),
            IrType::Usize => write!(f, "usize"),
            IrType::Isize => write!(f, "isize"),
            IrType::Int => write!(f, "int"),
            IrType::F32 => write!(f, "f32"),
            IrType::F64 => write!(f, "f64"),
            IrType::String => write!(f, "string"),
            IrType::Data => write!(f, "data"),
            IrType::Error => write!(f, "error"),
            IrType::Tuple(fields) => {
                write!(f, "(")?;
                for (i, ty) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", ty)?;
                }
                write!(f, ")")
            }
            IrType::Struct(fields) => {
                write!(f, "{{")?;
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", name, ty)?;
                }
                write!(f, "}}")
            }
            IrType::Enum(variants) => {
                write!(f, "enum{{")?;
                for (i, (name, payload)) in variants.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    if let Some(ty) = payload {
                        write!(f, "{}({})", name, ty)?;
                    } else {
                        write!(f, "{}", name)?;
                    }
                }
                write!(f, "}}")
            }
            IrType::List(elem) => write!(f, "list<{}>", elem),
            IrType::Set(elem) => write!(f, "set<{}>", elem),
            IrType::Map(k, v) => write!(f, "map<{}, {}>", k, v),
            IrType::Option(inner) => write!(f, "option<{}>", inner),
            IrType::Result(ok) => write!(f, "result<{}>", ok),
            IrType::Tensor(elem, rank) => write!(f, "tensor<{}, {}>", elem, rank),
            IrType::Ref(inner) => write!(f, "ref<{}>", inner),
            IrType::Table(cols) => {
                write!(f, "{{| ")?;
                for (i, (name, ty)) in cols.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}: {}", name, ty)?;
                }
                write!(f, " |}}")
            }
        }
    }
}

impl IrType {
    /// Convert from AST type hint to IR type.
    pub fn from_type_hint<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::ast::TypeHintAndHeap<'db>) -> Self {
        Self::from_type_hint_inner(db, &ty.type_hint(db))
    }

    /// Convert from AST TypeHint to IR type.
    fn from_type_hint_inner<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::ast::TypeHint<'db>) -> Self {
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
            TypeHint::Usize => IrType::Usize,
            TypeHint::Isize => IrType::Isize,
            TypeHint::F32 => IrType::F32,
            TypeHint::F64 => IrType::F64,
            TypeHint::Int => IrType::Int,
            TypeHint::String => IrType::String,
            TypeHint::Data => IrType::Data,
            TypeHint::Error => IrType::Error,
            TypeHint::AnonTuple(tuple) => {
                let fields: Vec<_> = tuple.fields
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
                let fields: Vec<_> = struct_.fields
                    .iter()
                    .map(|f| (f.name.text(db).to_string(), Self::from_type_hint(db, &f.type_hint)))
                    .collect();
                IrType::Struct(fields)
            }
            TypeHint::AnonEnum(enum_) => {
                let mut variants: Vec<_> = enum_.variants
                    .iter()
                    .map(|v| {
                        let name = v.name.text(db).to_string();
                        let payload = v.payload.map(|p| Self::from_type_hint(db, &p));
                        (name, payload)
                    })
                    .collect();
                // Sort variants by name for consistent layout.
                variants.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Enum(variants)
            }
            TypeHint::List(list) => {
                let elem = Self::from_type_hint(db, &list.element_type);
                IrType::List(Box::new(elem))
            }
            TypeHint::Set(set) => {
                let elem = Self::from_type_hint(db, &set.element_type);
                IrType::Set(Box::new(elem))
            }
            TypeHint::Map(map) => {
                let key = Self::from_type_hint(db, &map.key_type);
                let val = Self::from_type_hint(db, &map.value_type);
                IrType::Map(Box::new(key), Box::new(val))
            }
            TypeHint::Option(opt) => {
                let inner = Self::from_type_hint(db, &opt.inner_type);
                IrType::Option(Box::new(inner))
            }
            TypeHint::Result(res) => {
                let ok = Self::from_type_hint(db, &res.inner_type);
                IrType::Result(Box::new(ok))
            }
            TypeHint::Tensor(t) => {
                let elem = Self::from_type_hint(db, &t.element_type);
                IrType::Tensor(Box::new(elem), t.rank)
            }
            TypeHint::Table(table) => {
                let mut columns: Vec<_> = table.columns
                    .iter()
                    .map(|c| {
                        let name = c.name.text(db).to_string();
                        let ty = Self::from_type_hint(db, &c.type_hint);
                        (name, Box::new(ty))
                    })
                    .collect();
                // Sort columns by name for consistent layout.
                columns.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Table(columns)
            }
            TypeHint::ParseError(_) => {
                IrType::Error
            }
        }
    }

    /// Convert from datalit TypeAndHeap to IR type.
    fn from_datalit_tyandheap<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::tycheck::TypeAndHeap<'db>) -> Self {
        Self::from_datalit(db, ty.ty(db))
    }

    /// Convert from datalit typechecker type to IR type.
    pub fn from_datalit<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::tycheck::Type<'db>) -> Self {
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
            DlType::Usize => IrType::Usize,
            DlType::Isize => IrType::Isize,
            DlType::F32 => IrType::F32,
            DlType::F64 => IrType::F64,
            DlType::Int => IrType::Int,
            DlType::String => IrType::String,
            DlType::Data => IrType::Data,
            DlType::Error => IrType::Error,
            DlType::AnonTuple(tuple) => {
                let fields: Vec<_> = tuple.fields
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
                let fields: Vec<_> = struct_.fields
                    .iter()
                    .map(|f| (f.name.text(db).to_string(), Self::from_datalit_tyandheap(db, &f.ty)))
                    .collect();
                IrType::Struct(fields)
            }
            DlType::AnonEnum(enum_) => {
                let mut variants: Vec<_> = enum_.variants
                    .iter()
                    .map(|v| {
                        let name = v.name.text(db).to_string();
                        let payload = v.payload.clone().map(|p| Self::from_datalit_tyandheap(db, &p));
                        (name, payload)
                    })
                    .collect();
                // Sort variants by name for consistent layout.
                variants.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Enum(variants)
            }
            DlType::List(list) => {
                let elem = Self::from_datalit_tyandheap(db, &list.element_type);
                IrType::List(Box::new(elem))
            }
            DlType::Set(set) => {
                let elem = Self::from_datalit_tyandheap(db, &set.element_type);
                IrType::Set(Box::new(elem))
            }
            DlType::Map(map) => {
                let key = Self::from_datalit_tyandheap(db, &map.key_type);
                let val = Self::from_datalit_tyandheap(db, &map.value_type);
                IrType::Map(Box::new(key), Box::new(val))
            }
            DlType::Option(opt) => {
                let inner = Self::from_datalit_tyandheap(db, &opt.inner_type);
                IrType::Option(Box::new(inner))
            }
            DlType::Result(res) => {
                let ok = Self::from_datalit_tyandheap(db, &res.inner_type);
                IrType::Result(Box::new(ok))
            }
            DlType::Tensor(t) => {
                let elem = Self::from_datalit_tyandheap(db, &t.element_type);
                IrType::Tensor(Box::new(elem), t.rank)
            }
            DlType::Table(table) => {
                let mut columns: Vec<_> = table.columns
                    .iter()
                    .map(|c| {
                        let name = c.name.text(db).to_string();
                        let ty = Self::from_datalit_tyandheap(db, &c.ty);
                        (name, Box::new(ty))
                    })
                    .collect();
                // Sort columns by name for consistent layout.
                columns.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Table(columns)
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
            IrType::Usize | IrType::Isize => true,
            IrType::F32 | IrType::F64 => true,

            // Heap-allocated types are never copy.
            IrType::Int | IrType::String | IrType::Data | IrType::Error => false,
            IrType::List(_) | IrType::Set(_) | IrType::Map(_, _) | IrType::Tensor(_, _) | IrType::Table(_) => false,

            // Composite types are copy if all fields are copy.
            IrType::Tuple(fields) => fields.iter().all(|f| f.is_copy()),
            IrType::Struct(fields) => fields.iter().all(|(_, f)| f.is_copy()),
            // Enum is copy if all variant payloads are copy.
            IrType::Enum(variants) => variants.iter().all(|(_, payload)| {
                payload.as_ref().map(|p| p.is_copy()).unwrap_or(true)
            }),

            // Option is copy if inner is copy.
            IrType::Option(inner) => inner.is_copy(),
            // Result is never copy (Err variant contains non-copy Error).
            IrType::Result(_) => false,

            // Ref is always copy (it's just a pointer, doesn't own the data).
            IrType::Ref(_) => true,
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

/// Operand - SSA value, mutable slot, or function parameter.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Operand {
    /// Local SSA value.
    Value(ValueId),
    /// Local mutable slot.
    Slot(SlotId),
    /// Function parameter (reference to caller's data).
    Param(ParamId),
    /// SSA value from a previous script unit.
    ExternalValue { unit: u32, value: ValueId },
    /// Mutable slot from a previous script unit.
    ExternalSlot { unit: u32, slot: SlotId },
}

/// Destination for slot store operations.
///
/// Distinguishes between local slots (in current unit), external slots
/// (in a previous script unit), and mutable parameters.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SlotDest {
    /// Local slot in current unit.
    Local(SlotId),
    /// Slot in a previous script unit.
    External { unit: u32, slot: SlotId },
    /// Mutable parameter (writes through to caller's data).
    Param(ParamId),
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
    Usize(datalove_rtdt::UsizeRepr),
    Isize(datalove_rtdt::IsizeRepr),
    /// Bigint stored as limbs (little-endian base 2^32) and sign.
    /// Empty limbs = 0.
    Int { limbs: Vec<u32>, negative: bool },
    /// 32-bit float.
    F32(f32),
    /// 64-bit float.
    F64(f64),
    /// String literal.
    String(String),
}

impl std::hash::Hash for ConstValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            ConstValue::Unit => {}
            ConstValue::Bool(v) => v.hash(state),
            ConstValue::U8(v) => v.hash(state),
            ConstValue::U16(v) => v.hash(state),
            ConstValue::U32(v) => v.hash(state),
            ConstValue::U64(v) => v.hash(state),
            ConstValue::I8(v) => v.hash(state),
            ConstValue::I16(v) => v.hash(state),
            ConstValue::I32(v) => v.hash(state),
            ConstValue::I64(v) => v.hash(state),
            ConstValue::Usize(v) => v.hash(state),
            ConstValue::Isize(v) => v.hash(state),
            ConstValue::Int { limbs, negative } => {
                limbs.hash(state);
                negative.hash(state);
            }
            ConstValue::F32(v) => v.to_bits().hash(state),
            ConstValue::F64(v) => v.to_bits().hash(state),
            ConstValue::String(v) => v.hash(state),
        }
    }
}

impl Eq for ConstValue {}
// Note: PartialEq is derived and uses float comparison semantics.

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
    /// Boolean logic AND.
    LogicAnd,
    /// Boolean logic OR.
    LogicOr,
    /// Boolean logic XOR.
    LogicXor,
}

/// Unary operator.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    /// Boolean logic NOT.
    LogicNot,
}

/// Parameter passing mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ParamMode {
    /// By-value (default): ownership transfers to callee.
    In,
    /// By-out-ptr: caller allocates, callee initializes.
    Out,
    /// By-ref: immutable borrow, caller retains ownership.
    Ref,
    /// By-mut-ref: mutable borrow, caller retains ownership.
    Mut,
}

/// Flat instruction - no nesting, 2-3 operands max.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    /// Widen a fixed-width integer to Int (bigint).
    Widen { dest: ValueId, src: Operand },

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

    /// Get a single field from a struct/tuple.
    GetField {
        dest: ValueId,
        src: Operand,
        field_index: u32,
    },

    /// Get a reference (pointer) to a field within an aggregate.
    ///
    /// Unlike GetField which copies the field value, this returns a pointer
    /// to the field. Used when passing field projections to ref/mut/out params.
    /// The dest is an IrType::Ref wrapping the field type.
    GetFieldRef {
        dest: ValueId,
        src: Operand,
        field_index: u32,
    },

    /// Wrap value in Some.
    WrapSome { dest: ValueId, inner: Operand },

    /// Wrap value in Ok.
    WrapOk { dest: ValueId, inner: Operand },

    /// Wrap value in Err.
    WrapErr { dest: ValueId, inner: Operand },

    /// Create None.
    WrapNone { dest: ValueId },

    /// Create an enum variant.
    ///
    /// The variant_index is the index into the sorted variants of the enum type.
    /// Payload is provided for variants that have data.
    EnumVariant {
        dest: ValueId,
        variant_index: u32,
        payload: Option<Operand>,
    },

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

    /// Create a new tensor.
    TensorNew {
        dest: ValueId,
        shape: Vec<u32>,
        elements: Vec<Operand>,
    },

    /// Create a new table from row tuples.
    TableNew {
        dest: ValueId,
        rows: Vec<Operand>,
    },

    /// Store value to mutable slot.
    SlotStore { dest: SlotDest, value: Operand },

    /// Store value to a field within a mutable slot.
    ///
    /// field_path is the chain of field indices from the slot root
    /// to the target field: `set a.x.0.y = v` becomes field_path [x_idx, 0, y_idx].
    SetField {
        slot: SlotDest,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value to mutable parameter (writes through to caller's data).
    ParamStore { param: ParamId, value: Operand },

    /// Load value from mutable slot.
    SlotLoad { dest: ValueId, slot: SlotId },

    /// Drop a value (run destructor).
    Drop { operand: Operand },

    /// Debug log a value (borrows, does not consume).
    DebugLog { operand: Operand },

    /// Execute an intrinsic function.
    ///
    /// Intrinsics compile directly to machine instructions without function call overhead.
    Intrinsic {
        dest: ValueId,
        intrinsic: datalove_datafun_intrinsics::IntrinsicId,
        args: Vec<Operand>,
    },

    /// No operation.
    Nop,
}

/// Block terminator - how control leaves a basic block.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Terminator {
    /// Unconditional jump with block arguments.
    ///
    /// Args are MOVED into the target block's parameters (ownership transfer).
    /// Used for loop continue with new carry values.
    Goto {
        target: BlockId,
        args: Vec<Operand>,
    },

    /// Conditional branch with block arguments.
    ///
    /// Args for the taken branch are MOVED into the target block's parameters.
    /// Used for loop exit (break with bring values) and while condition checks.
    Branch {
        cond: Operand,
        then_block: BlockId,
        then_args: Vec<Operand>,
        else_block: BlockId,
        else_args: Vec<Operand>,
    },

    /// Return from function.
    Return { value: Option<Operand> },

    /// End of script unit (normal completion).
    UnitEnd { result: Option<Operand> },

    /// Early return from script unit (from `ret`, !, or checked operators).
    UnitEarlyReturn { value: Operand },
}

/// A basic block - sequence of instructions followed by a terminator.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IrBlock {
    pub id: BlockId,
    /// Block parameters (SSA values defined at block entry).
    ///
    /// Used for loop carry/bring values instead of Phi nodes. Each parameter
    /// has a fixed frame location. When control transfers via Goto or Branch,
    /// the terminator's args are MOVED into these parameter locations.
    ///
    /// Semantics:
    /// - Parameters define fresh values at each block entry
    /// - Incoming args are consumed (ownership transferred)
    /// - Interpreter: `pass_block_args()` copies data and marks source dropped
    /// - AOT scalars: pure SSA (Cranelift block param is the value)
    /// - AOT aggregates: memcpy from source pointer to local frame location
    pub params: Vec<ValueId>,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

/// IR for a single function.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IrFunction {
    pub id: FuncId,
    pub name: String,
    /// Parameter IDs (references to caller's data).
    pub params: Vec<ParamId>,
    /// Parameter modes (In, Out, Ref, Mut) for each param.
    pub param_modes: Vec<ParamMode>,
    /// Type for each ParamId (indexed by ParamId.0).
    pub param_types: Vec<IrType>,
    /// Return type of the function.
    pub return_type: IrType,
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

// ============================================================================
// RON Serialization Helpers
// ============================================================================

impl IrScriptUnit {
    /// Serialize to RON format for storage or transmission.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        let config = ron::ser::PrettyConfig::new()
            .struct_names(true)
            .enumerate_arrays(false);
        ron::ser::to_string_pretty(self, config)
    }

    /// Deserialize from RON format.
    ///
    /// Note: The `SymbolTable.name_to_func` field is not restored as it is only
    /// needed during lowering. Execution uses `FuncRef` IDs directly.
    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(s)
    }
}
