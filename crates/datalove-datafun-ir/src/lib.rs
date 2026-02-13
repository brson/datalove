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

pub use display::expand_ir_strings;
pub use registry::{FunctionRegistry, ModuleFunctionRegistry, UnitFunctionRegistry};

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

/// Call site identifier, unique within a function.
///
/// Used for stable tracking of call sites across IR transformations like inlining.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CallSiteId(pub u32);

/// Module identifier for IR.
///
/// Serializable numeric ID, unlike salsa's ModuleId.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct IrModuleId(pub u32);


// ============================================================================
// Unified Code Unit Types
// ============================================================================

/// Identifier for a code unit.
///
/// Replaces FuncId for unified addressing. The numeric value is local to
/// the containing scope (module or script execution session).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CodeUnitId(pub u32);

/// Reference to a code unit.
///
/// Unified reference type for functions, script units, and future code unit kinds.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum CodeRef {
    /// Unit in the current compilation scope.
    Local(CodeUnitId),
    /// Unit from a previous script execution.
    External { unit: u32, id: CodeUnitId },
    /// Unit from a compiled module.
    Module { module: IrModuleId, id: CodeUnitId },
}


/// Reference to a type (used in Pack instructions).
///
/// Lightweight type tag without inner type details.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum TypeRef {
    Bool,
    U8, U16, U32, U64,
    I8, I16, I32, I64,
    Index, Offset,
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
    Index, Offset,
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
    /// Named atom (unit-like tag with no payload).
    Atom(String),
    /// Named term (tag with a payload).
    Term(String, Box<IrType>),
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

impl IrType {
    /// Returns enum variants for Enum types only.
    pub fn enum_variants(&self) -> Option<&Vec<(String, Option<IrType>)>> {
        match self {
            IrType::Enum(v) => Some(v),
            _ => None,
        }
    }
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
            IrType::Index => write!(f, "index"),
            IrType::Offset => write!(f, "offset"),
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
            IrType::Atom(name) => write!(f, "atom {}", name),
            IrType::Term(name, payload) => write!(f, "term {}({})", name, payload),
            IrType::List(elem) => write!(f, "list<{}>", elem),
            IrType::Set(elem) => write!(f, "set<{}>", elem),
            IrType::Map(k, v) => write!(f, "map<{}, {}>", k, v),
            IrType::Option(inner) => write!(f, "option<{}>", inner),
            IrType::Result(ok) => write!(f, "result<{}>", ok),
            IrType::Tensor(elem, rank) => write!(f, "[|{}, {}|]", elem, rank),
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
    pub fn from_type_hint<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::ast::TypeHint<'db>) -> Self {
        Self::from_type_hint_inner(db, ty)
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
            TypeHint::Index => IrType::Index,
            TypeHint::Offset => IrType::Offset,
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
            TypeHint::Atom(a) => {
                let name = a.name.text(db).to_string();
                IrType::Atom(name)
            }
            TypeHint::Term(t) => {
                let name = t.name.text(db).to_string();
                let payload = Self::from_type_hint(db, &t.payload);
                IrType::Term(name, Box::new(payload))
            }
            TypeHint::Enum(e) => {
                let mut variants: Vec<_> = e.variants.iter()
                    .map(|v| {
                        let name = v.name.text(db).to_string();
                        let payload = v.payload.as_ref().map(|p| Self::from_type_hint(db, p));
                        (name, payload)
                    })
                    .collect();
                variants.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Enum(variants)
            }
            TypeHint::ParseError(_) => {
                IrType::Error
            }
            TypeHint::Alias(_) => {
                // Type aliases should be resolved before IR generation.
                IrType::Error
            }
        }
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
            DlType::Index => IrType::Index,
            DlType::Offset => IrType::Offset,
            DlType::F32 => IrType::F32,
            DlType::F64 => IrType::F64,
            DlType::Int => IrType::Int,
            DlType::String => IrType::String,
            DlType::Data => IrType::Data,
            DlType::Error => IrType::Error,
            DlType::AnonTuple(tuple) => {
                let fields: Vec<_> = tuple.fields
                    .iter()
                    .map(|f| Self::from_datalit(db, f))
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
                    .map(|f| (f.name.text(db).to_string(), Self::from_datalit(db, &*f.ty)))
                    .collect();
                IrType::Struct(fields)
            }

            DlType::List(list) => {
                let elem = Self::from_datalit(db, &*list.element_type);
                IrType::List(Box::new(elem))
            }
            DlType::Set(set) => {
                let elem = Self::from_datalit(db, &*set.element_type);
                IrType::Set(Box::new(elem))
            }
            DlType::Map(map) => {
                let key = Self::from_datalit(db, &*map.key_type);
                let val = Self::from_datalit(db, &*map.value_type);
                IrType::Map(Box::new(key), Box::new(val))
            }
            DlType::Option(opt) => {
                let inner = Self::from_datalit(db, &*opt.inner_type);
                IrType::Option(Box::new(inner))
            }
            DlType::Result(res) => {
                let ok = Self::from_datalit(db, &*res.inner_type);
                IrType::Result(Box::new(ok))
            }
            DlType::Tensor(t) => {
                let elem = Self::from_datalit(db, &*t.element_type);
                IrType::Tensor(Box::new(elem), t.rank)
            }
            DlType::Table(table) => {
                let mut columns: Vec<_> = table.columns
                    .iter()
                    .map(|c| {
                        let name = c.name.text(db).to_string();
                        let ty = Self::from_datalit(db, &*c.ty);
                        (name, Box::new(ty))
                    })
                    .collect();
                // Sort columns by name for consistent layout.
                columns.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Table(columns)
            }
            DlType::Atom(a) => {
                let name = a.name.text(db).to_string();
                IrType::Atom(name)
            }
            DlType::Term(t) => {
                let name = t.name.text(db).to_string();
                let payload = Self::from_datalit(db, &t.payload);
                IrType::Term(name, Box::new(payload))
            }
            DlType::Enum(e) => {
                let mut variants: Vec<_> = e.variants.iter()
                    .map(|v| {
                        let name = v.name.text(db).to_string();
                        let payload = v.payload.as_ref().map(|p| Self::from_datalit(db, p));
                        (name, payload)
                    })
                    .collect();
                variants.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Enum(variants)
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
            IrType::Index | IrType::Offset => true,
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
            // Atom has no payload, always copy.
            IrType::Atom(_) => true,
            // Term is copy if payload is copy.
            IrType::Term(_, payload) => payload.is_copy(),

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
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct FuncDef {
    pub id: FuncId,
    pub name: String,
    pub param_count: usize,
}

/// Symbol table for IR resolution.
///
/// Maps function IDs to their definitions and provides name lookup.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SymbolTable {
    /// All function definitions, indexed by FuncId.
    pub functions: Vec<FuncDef>,
    /// Name to FuncId mapping for the current scope (lowering-time only).
    #[serde(skip)]
    name_to_func: HashMap<String, FuncId>,
    /// Next FuncId to allocate.
    next_func_id: u32,
}

// Manual Eq/PartialEq/Hash for SymbolTable that ignores the transient name_to_func field.
impl PartialEq for SymbolTable {
    fn eq(&self, other: &Self) -> bool {
        self.functions == other.functions && self.next_func_id == other.next_func_id
    }
}

impl Eq for SymbolTable {}

impl std::hash::Hash for SymbolTable {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.functions.hash(state);
        self.next_func_id.hash(state);
    }
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

    /// Define a function with a specific ID (for reconstructing modules).
    pub fn define_func_with_id(&mut self, id: FuncId, name: String, param_count: usize) {
        self.functions.push(FuncDef { id, name: name.clone(), param_count });
        self.name_to_func.insert(name, id);
        // Update next_func_id to be at least one past this ID.
        if id.0 >= self.next_func_id {
            self.next_func_id = id.0 + 1;
        }
    }
}

/// Operand - SSA value, mutable slot, or function parameter.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Operand {
    /// Local SSA value.
    Value(ValueId),
    /// Local SSA value that holds a reference (auto-dereference on use).
    ///
    /// Produced by GetFieldRef. The value stores a pointer; reading this
    /// operand dereferences it to access the pointed-to data.
    ValueRef(ValueId),
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
/// Distinguishes between local slots (in current unit) and external slots
/// (in a previous script unit).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SlotDest {
    /// Local slot in current unit.
    Local(SlotId),
    /// Slot in a previous script unit.
    External { unit: u32, slot: SlotId },
}

/// Constant value that can be loaded or computed at compile time.
///
/// Supports all Datafun types for compile-time function evaluation (CTFE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConstValue {
    // Primitives.
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
    Index(datalove_rtdt::IndexRepr),
    Offset(datalove_rtdt::OffsetRepr),
    /// Bigint stored as limbs (little-endian base 2^32) and sign.
    /// Empty limbs = 0.
    Int { limbs: Vec<u32>, negative: bool },
    /// 32-bit float.
    F32(f32),
    /// 64-bit float.
    F64(f64),
    /// String literal.
    String(String),

    // Aggregates.
    /// Anonymous tuple.
    Tuple(Vec<ConstValue>),
    /// Anonymous struct with named fields (sorted by name).
    Struct(Vec<(String, ConstValue)>),
    /// Anonymous enum variant with optional payload.
    Enum { variant: String, payload: Option<Box<ConstValue>> },

    // Wrappers.
    /// Option::Some variant.
    OptionSome(Box<ConstValue>),
    /// Option::None variant.
    OptionNone,
    /// Result::Ok variant.
    ResultOk(Box<ConstValue>),
    /// Result::Err variant (contains error value).
    ResultErr(Box<ConstValue>),
    /// Dynamic data wrapper.
    Data(Box<ConstValue>),
    /// Error value (boxes any value).
    Error(Box<ConstValue>),

    // Collections.
    /// List with elements.
    List(Vec<ConstValue>),
    /// Set with elements (sorted for determinism).
    Set(Vec<ConstValue>),
    /// Map with key-value pairs (sorted by key for determinism).
    Map(Vec<(ConstValue, ConstValue)>),
    /// Table with column names and row data.
    Table { columns: Vec<String>, rows: Vec<Vec<ConstValue>> },
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
            ConstValue::Index(v) => v.hash(state),
            ConstValue::Offset(v) => v.hash(state),
            ConstValue::Int { limbs, negative } => {
                limbs.hash(state);
                negative.hash(state);
            }
            ConstValue::F32(v) => v.to_bits().hash(state),
            ConstValue::F64(v) => v.to_bits().hash(state),
            ConstValue::String(v) => v.hash(state),
            ConstValue::Tuple(elems) => elems.hash(state),
            ConstValue::Struct(fields) => fields.hash(state),
            ConstValue::Enum { variant, payload } => {
                variant.hash(state);
                payload.hash(state);
            }
            ConstValue::OptionSome(v) => v.hash(state),
            ConstValue::OptionNone => {}
            ConstValue::ResultOk(v) => v.hash(state),
            ConstValue::ResultErr(v) => v.hash(state),
            ConstValue::Data(v) => v.hash(state),
            ConstValue::Error(v) => v.hash(state),
            ConstValue::List(elems) => elems.hash(state),
            ConstValue::Set(elems) => elems.hash(state),
            ConstValue::Map(entries) => entries.hash(state),
            ConstValue::Table { columns, rows } => {
                columns.hash(state);
                rows.hash(state);
            }
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
///
/// # Tracking Semantics
///
/// Instructions come in precise and tracked variants:
///
/// - **Precise** variants assume ownership analysis has proven the operand state.
///   The backend does not read or write tracking bytes for these instructions.
///
/// - **Tracked** variants update tracking state at runtime. Use these when:
///   - A value may have been moved (source tracking)
///   - A destination needs its tracking byte set to LIVE (dest tracking)
///
/// Instructions that produce values (like `Const`, `Call`, `Pack`, etc.) have tracked
/// variants that mark their destination as LIVE. The tracked variant should be used
/// when the destination is in the function's `tracked_values` set.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Instruction {
    // ========================================================================
    // Constants
    // ========================================================================

    /// Load a constant value.
    ///
    /// **Ownership:** Produces `dest`.
    Const { dest: ValueId, value: ConstValue },

    // ========================================================================
    // Value Movement
    // ========================================================================

    /// Copy a value (for Copy types only).
    ///
    /// **Ownership:** Borrows `src`, produces `dest`.
    /// **Tracking:** None - Copy types don't need tracking.
    Copy { dest: ValueId, src: Operand },

    /// Move a value.
    ///
    /// Transfers ownership from `src` to `dest` via shallow copy.
    ///
    /// **Ownership:** Consumes `src`, produces `dest`.
    Move { dest: ValueId, src: Operand },

    // ========================================================================
    // Arithmetic Operations
    // ========================================================================

    /// Binary operation (produces Copy result).
    ///
    /// **Ownership:** Borrows `lhs` and `rhs`, produces `dest`.
    /// **Tracking:** None - results are Copy types (bool, int, float).
    BinOp {
        dest: ValueId,
        op: BinOp,
        lhs: Operand,
        rhs: Operand,
    },

    /// Unary operation (produces Copy result).
    ///
    /// **Ownership:** Borrows `operand`, produces `dest`.
    /// **Tracking:** None - results are Copy types.
    UnaryOp {
        dest: ValueId,
        op: UnaryOp,
        operand: Operand,
    },

    /// Checked binary operation (produces Copy result + overflow flag).
    ///
    /// **Ownership:** Borrows `lhs` and `rhs`, produces `dest` and `overflow`.
    /// **Tracking:** None - results are Copy types.
    BinOpChecked {
        dest: ValueId,
        overflow: ValueId,
        op: BinOp,
        lhs: Operand,
        rhs: Operand,
    },

    /// Checked unary operation (produces Copy result + overflow flag).
    ///
    /// **Ownership:** Borrows `operand`, produces `dest` and `overflow`.
    /// **Tracking:** None - results are Copy types.
    UnaryOpChecked {
        dest: ValueId,
        overflow: ValueId,
        op: UnaryOp,
        operand: Operand,
    },

    /// Widen a fixed-width integer to Int (bigint).
    ///
    /// **Ownership:** Borrows `src`, produces `dest` (Int is non-Copy).
    Widen { dest: ValueId, src: Operand },

    /// Widen a fixed-width integer to a larger fixed-width integer.
    ///
    /// Performs zero-extension for unsigned types and sign-extension for signed types.
    /// Valid widening paths:
    /// - u8 -> u16 -> u32 -> u64
    /// - i8 -> i16 -> i32 -> i64
    /// - Cross-sign: u8 -> i16, u16 -> i32, u32 -> i64
    ///
    /// **Ownership:** Borrows `src`, produces `dest` (both are Copy types).
    WidenFixed { dest: ValueId, src: Operand },

    /// Clone a linear value (deep copy for the @ operator).
    ///
    /// Creates a deep copy of a linear value so the original remains valid.
    /// Used by the postfix `@` operator for clone/coerce operations.
    ///
    /// **Ownership:** Borrows `src`, produces `dest` (new owned value).
    Clone { dest: ValueId, src: Operand },

    // ========================================================================
    // Function Calls
    // ========================================================================

    /// Function call.
    ///
    /// **Ownership:** Args consumed per param mode (In consumes, Ref/Mut/Out borrow).
    Call {
        site_id: CallSiteId,
        dest: ValueId,
        func: CodeRef,
        args: Vec<Operand>,
    },

    /// Call to a function with const parameters.
    ///
    /// This is emitted during lowering when the callee has const parameters.
    /// Contains metadata for the specialization pass to rewrite the call.
    ///
    /// **Behavior without specialization:** Acts exactly like `Call` - the interpreter
    /// and AOT codegen treat this as a normal call to the original function.
    ///
    /// **Behavior with specialization:** The specialization pass transforms this to
    /// a `Const` instruction (for the discriminant) followed by a `Call` with the
    /// const args removed and discriminant added as first arg.
    ///
    /// **Ownership:** Same as `Call`.
    ComptimeCall {
        dest: ValueId,
        func: CodeRef,
        /// Original arguments including const parameter args.
        args: Vec<Operand>,
        /// Pre-computed discriminant for this instantiation (from typecheck).
        discriminant: u32,
        /// Indices of const parameters (to be removed during specialization).
        comptime_param_indices: Vec<usize>,
    },

    // ========================================================================
    // Aggregate Construction
    // ========================================================================

    /// Pack fields into a struct/tuple.
    ///
    /// **Ownership:** Consumes all `fields`, produces `dest`.
    Pack {
        dest: ValueId,
        ty: TypeRef,
        fields: Vec<Operand>,
    },

    /// Unpack struct/tuple into fields.
    ///
    /// **Ownership:** Consumes `src`, produces all `dests`.
    Unpack { dests: Vec<ValueId>, src: Operand },

    /// Get a single field from a struct/tuple.
    ///
    /// **Ownership:** Consumes `src`, produces `dest` (field moved out).
    GetField {
        dest: ValueId,
        src: Operand,
        field_index: u32,
    },

    /// Get a reference (pointer) to a field within an aggregate.
    ///
    /// Unlike GetField which moves the field value, this returns a pointer
    /// to the field. Used when passing field projections to ref/mut/out params.
    ///
    /// **Ownership:** Borrows `src`, produces `dest` (Ref type, always Copy).
    /// **Tracking:** None - Ref is a Copy type (just a pointer).
    GetFieldRef {
        dest: ValueId,
        src: Operand,
        field_index: u32,
    },

    // ========================================================================
    // Option Construction
    // ========================================================================

    /// Wrap value in Some.
    ///
    /// **Ownership:** Consumes `inner`, produces `dest`.
    WrapSome { dest: ValueId, inner: Operand },

    /// Create None.
    ///
    /// **Ownership:** Produces `dest`.
    WrapNone { dest: ValueId },

    // ========================================================================
    // Result Construction
    // ========================================================================

    /// Wrap value in Ok.
    ///
    /// **Ownership:** Consumes `inner`, produces `dest`.
    WrapOk { dest: ValueId, inner: Operand },

    /// Wrap value in Err.
    ///
    /// **Ownership:** Consumes `inner`, produces `dest`.
    WrapErr { dest: ValueId, inner: Operand },

    // ========================================================================
    // Enum Construction
    // ========================================================================

    /// Create an enum variant.
    ///
    /// The variant_index is the index into the sorted variants of the enum type.
    ///
    /// **Ownership:** Consumes `payload` if present, produces `dest`.
    EnumVariant {
        dest: ValueId,
        variant_index: u32,
        payload: Option<Operand>,
    },

    /// Read discriminant (u32 tag) from enum value.
    ///
    /// **Ownership:** Borrows `src` (does not consume).
    EnumDiscriminant {
        dest: ValueId,
        src: Operand,
    },

    /// Move payload out of enum into dest.
    ///
    /// **Ownership:** Consumes `src`, produces `dest`.
    EnumPayload {
        dest: ValueId,
        src: Operand,
        variant_index: u32,
    },

    // ========================================================================
    // Option/Result Unwrapping
    // ========================================================================

    /// Unwrap Option.
    ///
    /// Produces (inner_value, is_some). The `dest` is valid only when is_some=true.
    ///
    /// **Ownership:** Consumes `src`, conditionally produces `dest`.
    UnwrapOption {
        dest: ValueId,
        is_some: ValueId,
        src: Operand,
    },

    /// Unwrap Result.
    ///
    /// Produces (ok_value, err_value, is_ok). Only one of ok_dest/err_dest is valid
    /// based on is_ok.
    ///
    /// **Ownership:** Consumes `src`, conditionally produces `ok_dest` or `err_dest`.
    UnwrapResult {
        ok_dest: ValueId,
        err_dest: ValueId,
        is_ok: ValueId,
        src: Operand,
    },

    // ========================================================================
    // Boxing Operations
    // ========================================================================

    /// Create Error from any value.
    ///
    /// **Ownership:** Consumes `inner`, produces `dest`.
    ErrorFrom { dest: ValueId, inner: Operand },

    /// Create Data from any value.
    ///
    /// **Ownership:** Consumes `inner`, produces `dest`.
    DataFrom { dest: ValueId, inner: Operand },

    // ========================================================================
    // Collection Construction
    // ========================================================================

    /// Create a new list.
    ///
    /// **Ownership:** Consumes all `elements`, produces `dest`.
    ListNew {
        dest: ValueId,
        elements: Vec<Operand>,
    },

    /// Create a new set.
    ///
    /// **Ownership:** Consumes all `elements`, produces `dest`.
    SetNew {
        dest: ValueId,
        elements: Vec<Operand>,
    },

    /// Create a new map.
    ///
    /// **Ownership:** Consumes all keys and values, produces `dest`.
    MapNew {
        dest: ValueId,
        entries: Vec<(Operand, Operand)>,
    },

    /// Create a new tensor.
    ///
    /// **Ownership:** Consumes all `elements`, produces `dest`.
    TensorNew {
        dest: ValueId,
        shape: Vec<u32>,
        elements: Vec<Operand>,
    },

    /// Create a new table from row tuples.
    ///
    /// **Ownership:** Consumes all `rows`, produces `dest`.
    TableNew {
        dest: ValueId,
        rows: Vec<Operand>,
    },

    // ========================================================================
    // Slot Operations
    // ========================================================================

    /// Store value to mutable slot with copy semantics (precise slot).
    ///
    /// **Ownership:** Borrows `value`, writes to `dest` slot.
    SlotStoreCopy { dest: SlotDest, value: Operand },

    /// Store value to mutable slot with copy semantics (tracked slot).
    ///
    /// **Ownership:** Borrows `value`, writes to `dest` slot.
    /// **Tracking:** Writes LIVE to slot tracking byte.
    SlotStoreCopyTracked { dest: SlotDest, value: Operand },

    /// Store value to mutable slot with move semantics (precise slot).
    ///
    /// **Ownership:** Consumes `value`, writes to `dest` slot.
    SlotStoreMove { dest: SlotDest, value: Operand },

    /// Store value to mutable slot with move semantics (tracked slot).
    ///
    /// **Ownership:** Consumes `value`, writes to `dest` slot.
    /// **Tracking:** Writes LIVE to slot tracking byte.
    SlotStoreMoveTracked { dest: SlotDest, value: Operand },

    /// Store value to a field within a mutable slot (precise slot).
    ///
    /// field_path is the chain of field indices from the slot root
    /// to the target field: `set a.x.0.y = v` becomes field_path [x_idx, 0, y_idx].
    ///
    /// **Ownership:** Consumes `value`, writes to field within slot.
    SetField {
        slot: SlotDest,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value to a field within a mutable slot (tracked slot).
    ///
    /// **Ownership:** Consumes `value`, writes to field within slot.
    /// **Tracking:** Writes LIVE to slot tracking byte.
    SetFieldTracked {
        slot: SlotDest,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value to mutable parameter (Mut params only).
    ///
    /// Mut params are precise - caller always provides initialized memory.
    /// Always destroys the old value before storing.
    ///
    /// **Ownership:** Consumes `value`, writes to caller's param location.
    ParamStore { param: ParamId, value: Operand },

    /// Store value to Out parameter (tracked).
    ///
    /// Out params start uninitialized. Checks tracking byte before destroying:
    /// first write doesn't destroy, subsequent writes destroy old value.
    ///
    /// **Ownership:** Consumes `value`, writes to caller's param location.
    /// **Tracking:** Reads param tracking byte; writes LIVE after store.
    ParamStoreTracked { param: ParamId, value: Operand },

    /// Store value to a field within mutable parameter (Mut params only).
    ///
    /// Mut params are precise - always destroys the old field value.
    ///
    /// **Ownership:** Consumes `value`, writes to field within param.
    ParamSetField {
        param: ParamId,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value to a field within Out parameter (tracked).
    ///
    /// Out params start uninitialized. Checks tracking byte before destroying.
    ///
    /// **Ownership:** Consumes `value`, writes to field within param.
    /// **Tracking:** Reads param tracking byte; writes LIVE after store.
    ParamSetFieldTracked {
        param: ParamId,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value through a reference operand.
    ///
    /// Generalizes ParamStore to work with any reference-like operand:
    /// - Slot(s): store to slot s
    /// - Param(p): store to mut param p (same as ParamStore)
    /// - ValueRef(v): store through a reference value
    ///
    /// Used after inlining to store to the caller's location directly.
    ///
    /// **Ownership:** Consumes `value`, writes to referenced location.
    RefStore { dest: Operand, value: Operand },

    /// Store value to a field through a reference operand.
    ///
    /// Generalizes ParamSetField to work with any reference-like operand.
    ///
    /// **Ownership:** Consumes `value`, writes to field within referenced location.
    RefSetField {
        dest: Operand,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value through a reference operand with tracking.
    ///
    /// Like RefStore but also updates the tracking byte to LIVE.
    /// Used for Out params which start uninitialized.
    ///
    /// **Ownership:** Consumes `value`, writes to referenced location.
    /// **Tracking:** Writes LIVE to tracking byte.
    RefStoreTracked { dest: Operand, value: Operand },

    /// Store value to a field through a reference operand with tracking.
    ///
    /// Like RefSetField but also updates the tracking byte to LIVE.
    ///
    /// **Ownership:** Consumes `value`, writes to field within referenced location.
    /// **Tracking:** Writes LIVE to tracking byte.
    RefSetFieldTracked {
        dest: Operand,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Load value from mutable slot with copy semantics.
    ///
    /// **Ownership:** Borrows `slot`, produces `dest`.
    SlotLoadCopy { dest: ValueId, slot: SlotId },

    /// Load value from mutable slot with move semantics (precise slot).
    ///
    /// **Ownership:** Consumes slot contents, produces `dest`.
    SlotLoadMove { dest: ValueId, slot: SlotId },

    /// Load value from mutable slot with move semantics (tracked slot).
    ///
    /// **Ownership:** Consumes slot contents, produces `dest`.
    /// **Tracking:** Writes MOVED to slot tracking byte.
    SlotLoadMoveTracked { dest: ValueId, slot: SlotId },

    // ========================================================================
    // Drop Operations
    // ========================================================================

    /// Drop a value unconditionally.
    ///
    /// **Ownership:** Consumes `operand`.
    Drop { operand: Operand },

    /// Drop a slot if initialized (for tracked slots).
    ///
    /// Checks slot tracking byte before dropping. Skips if UNINIT or MOVED.
    ///
    /// **Ownership:** Conditionally consumes `operand` (must be a Slot).
    /// **Tracking:** Reads slot tracking byte; writes MOVED after drop.
    DropTracked { operand: Operand },

    /// Drop through a reference value.
    ///
    /// Takes a value containing a reference (from GetFieldRef) and destroys
    /// what the reference points to. Used to destroy field values before
    /// passing a reference as an out param.
    ///
    /// **Ownership:** Does not consume `ref_value`, destroys referent.
    DropViaRef { ref_value: ValueId },

    /// Drop a script-level value binding at unit end.
    ///
    /// **Backend semantics:**
    /// - Interpreter: no-op (binding persists for REPL)
    /// - AOT: unconditional drop
    UnitEndDrop { operand: Operand },

    /// Drop a script-level slot binding at unit end (for tracked slots).
    ///
    /// **Backend semantics:**
    /// - Interpreter: no-op (binding persists for REPL)
    /// - AOT: conditional drop (checks slot tracking byte)
    UnitEndDropTracked { operand: Operand },

    // ========================================================================
    // Miscellaneous
    // ========================================================================

    /// Debug log a value (borrows, does not consume).
    ///
    /// **Ownership:** Borrows `operand`.
    DebugLog { operand: Operand },

    /// Execute an intrinsic function.
    ///
    /// Intrinsics compile directly to machine instructions without function call overhead.
    ///
    /// **Ownership:** Args borrowed or consumed per intrinsic definition.
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

    /// Multi-way branch on an integer discriminant.
    ///
    /// Maps directly to jump tables in backends. No block args.
    Switch {
        discriminant: Operand,
        cases: Vec<(u32, BlockId)>,
        default: BlockId,
    },

    /// Return from a function.
    Return { value: Option<Operand> },

    /// End of a script unit.
    UnitEnd { result: Option<Operand> },

    /// Early exit from a script unit (from `ret`, !, or checked operators).
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

/// Result of lowering a module to IR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrModule {
    pub functions: Vec<IrCodeUnit>,
    pub symbols: SymbolTable,
}

/// What a script unit exports to subsequent units.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ExportBinding {
    /// An SSA value (from let binding).
    Value(ValueId),
    /// A mutable slot (from var binding).
    Slot(SlotId),
    /// A function defined in this unit.
    Function(CodeUnitId),
}

// ============================================================================
// Unified Code Unit
// ============================================================================

/// Context for function execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionContext {
    /// Parameter IDs (references to caller's data).
    pub params: Vec<ParamId>,
    /// Parameter modes (In, Out, Ref, Mut).
    pub param_modes: Vec<ParamMode>,
    /// Type for each parameter.
    pub param_types: Vec<IrType>,
    /// Return type.
    pub return_type: IrType,
    /// Out params that need runtime tracking.
    #[serde(default)]
    pub tracked_params: Vec<ParamId>,
}

/// Context for script unit execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptContext {
    /// Values that persist for REPL cleanup.
    #[serde(default)]
    pub unit_end_values: Vec<ValueId>,
    /// Slots that persist for REPL cleanup.
    #[serde(default)]
    pub unit_end_slots: Vec<SlotId>,
    /// Result value for expression units.
    pub result: Option<ValueId>,
    /// Bindings exported to subsequent units.
    #[serde(default)]
    pub exports: Vec<(String, ExportBinding)>,
}

/// Context determining how a code unit executes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeUnitContext {
    /// A callable function with parameters.
    Function(FunctionContext),
    /// A script unit with captures and exports.
    Script(ScriptContext),
}

/// Unified IR representation for executable code.
///
/// Represents both functions and script units. The `context` field
/// determines execution semantics (parameter passing vs captures,
/// return vs unit-end, etc.).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IrCodeUnit {
    /// Unique identifier within containing scope.
    pub id: CodeUnitId,
    /// Name for debugging/symbol resolution.
    pub name: String,

    // ========== Body ==========
    /// Basic blocks.
    pub blocks: Vec<IrBlock>,
    /// Number of SSA values.
    pub value_count: u32,
    /// Number of mutable slots.
    pub slot_count: u32,
    /// Number of call sites.
    #[serde(default)]
    pub call_site_count: u32,
    /// Type for each value.
    pub value_types: Vec<IrType>,
    /// Type for each slot.
    pub slot_types: Vec<IrType>,
    /// Slots requiring runtime tracking.
    #[serde(default)]
    pub tracked_slots: Vec<SlotId>,
    /// Const bindings for inlining.
    #[serde(default)]
    pub const_values: Vec<(String, ValueId)>,
    /// Symbol table for nested unit resolution.
    pub symbols: SymbolTable,

    // ========== Context ==========
    /// Determines execution semantics.
    pub context: CodeUnitContext,

    // ========== Nested Units ==========
    /// Code units defined inside this unit.
    #[serde(default)]
    pub nested_units: Vec<IrCodeUnit>,
}

impl IrCodeUnit {
    /// Get the entry block (always block 0).
    pub fn entry_block(&self) -> &IrBlock {
        &self.blocks[0]
    }

    /// Check if this is a function.
    pub fn is_function(&self) -> bool {
        matches!(&self.context, CodeUnitContext::Function(_))
    }

    /// Check if this is a script unit.
    pub fn is_script(&self) -> bool {
        matches!(&self.context, CodeUnitContext::Script(_))
    }

    /// Get function context if this is a function.
    pub fn function_context(&self) -> Option<&FunctionContext> {
        match &self.context {
            CodeUnitContext::Function(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get mutable function context if this is a function.
    pub fn function_context_mut(&mut self) -> Option<&mut FunctionContext> {
        match &mut self.context {
            CodeUnitContext::Function(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get script context if this is a script unit.
    pub fn script_context(&self) -> Option<&ScriptContext> {
        match &self.context {
            CodeUnitContext::Script(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get mutable script context if this is a script unit.
    pub fn script_context_mut(&mut self) -> Option<&mut ScriptContext> {
        match &mut self.context {
            CodeUnitContext::Script(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get return type (for functions).
    pub fn return_type(&self) -> Option<&IrType> {
        self.function_context().map(|c| &c.return_type)
    }

    /// Get parameter count (0 for scripts).
    pub fn param_count(&self) -> usize {
        self.function_context().map(|c| c.params.len()).unwrap_or(0)
    }

    /// Serialize to RON format.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        let config = ron::ser::PrettyConfig::new()
            .struct_names(true)
            .enumerate_arrays(false);
        ron::ser::to_string_pretty(self, config)
    }

    /// Deserialize from RON format.
    pub fn from_ron(s: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(s)
    }
}


// ============================================================================
// Compile-Time Function Evaluation (CTFE)
// ============================================================================

/// Error during compile-time const expression evaluation.
#[derive(Debug, Clone)]
pub enum CtfeError {
    /// Interpreter execution error.
    InterpError(String),
    /// Type extraction not supported.
    UnsupportedType(String),
    /// Early return via `!` or `?` operator.
    EarlyReturn(String),
}

impl std::fmt::Display for CtfeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CtfeError::InterpError(msg) => write!(f, "interpreter error: {}", msg),
            CtfeError::UnsupportedType(ty) => write!(f, "unsupported type for extraction: {}", ty),
            CtfeError::EarlyReturn(msg) => write!(f, "early return: {}", msg),
        }
    }
}

impl std::error::Error for CtfeError {}

/// Trait for compile-time evaluation of const expressions.
///
/// Implemented by the interpreter to execute IR at compile time.
/// The compiler generates an `IrCodeUnit` for the const expression
/// and calls this trait to evaluate it.
pub trait CtfeEvaluator {
    /// Execute a code unit and extract the result as a ConstValue.
    ///
    /// The unit should be a simple expression unit (single block, Exit terminator).
    fn evaluate(&mut self, unit: &IrCodeUnit, result_type: &IrType) -> Result<ConstValue, CtfeError>;

    /// Set the module function registry for cross-module CTFE calls.
    ///
    /// This allows const expressions to call functions from other modules.
    /// The registry is built from already-lowered module functions.
    fn set_module_registry(&mut self, registry: std::sync::Arc<ModuleFunctionRegistry>);
}

/// Placeholder CTFE evaluator that always returns an error.
///
/// Used when no interpreter is available (e.g., in minimal compilation contexts).
pub struct NoopCtfeEvaluator;

impl CtfeEvaluator for NoopCtfeEvaluator {
    fn evaluate(&mut self, _unit: &IrCodeUnit, _result_type: &IrType) -> Result<ConstValue, CtfeError> {
        Err(CtfeError::InterpError("CTFE evaluator not configured".to_string()))
    }

    fn set_module_registry(&mut self, _registry: std::sync::Arc<ModuleFunctionRegistry>) {
        // No-op: this evaluator doesn't support module function calls.
    }
}

// ============================================================================
// CTFE Memoization Types
// ============================================================================

/// Identifier for a const statement in the AST.
///
/// Uses salsa's opaque ID type to reference const statements across
/// compilation phases without coupling to specific AST types.
pub type ConstStmtId = salsa::Id;

/// Identifier for a const expression in the AST.
pub type ConstExprId = salsa::Id;

/// Global identifier for a const binding across script units.
///
/// In script contexts, const bindings from earlier units can be referenced
/// by later units. This ID uniquely identifies a const across all units.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct GlobalConstId {
    /// Index of the script unit containing this const.
    pub unit: u32,
    /// ID of the const statement within that unit.
    pub stmt: ConstStmtId,
}

/// Information about a const binding discovered during Phase 1 collection.
///
/// Collected from AST traversal without evaluating expressions.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct ConstBindingInfo {
    /// ID of the const statement in the AST.
    pub stmt_id: ConstStmtId,
    /// Name of the const binding (for error messages and lookup).
    pub name: String,
    /// ID of the expression to evaluate.
    pub expr_id: ConstExprId,
    /// Expected type of the const value.
    pub ir_type: IrType,
    /// Other const bindings this one depends on.
    pub depends_on: Vec<ConstStmtId>,
}

/// Output of Phase 1: const bindings in dependency order.
///
/// This type is hashable for salsa memoization.
#[derive(Clone, Debug, Default, Hash, Eq, PartialEq)]
pub struct ConstBindingGraph {
    /// Const bindings in topological order (dependencies before dependents).
    pub bindings: Vec<ConstBindingInfo>,
    /// Lookup from name to statement ID, sorted by name for determinism.
    name_to_stmt: Vec<(String, ConstStmtId)>,
}

impl ConstBindingGraph {
    /// Create a new graph with bindings in topological order.
    pub fn new(bindings: Vec<ConstBindingInfo>) -> Self {
        let mut name_to_stmt: Vec<_> = bindings.iter()
            .map(|b| (b.name.clone(), b.stmt_id))
            .collect();
        name_to_stmt.sort_by(|a, b| a.0.cmp(&b.0));
        Self { bindings, name_to_stmt }
    }

    /// Look up statement ID by name.
    pub fn stmt_for_name(&self, name: &str) -> Option<ConstStmtId> {
        self.name_to_stmt.iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| *id)
    }

    /// Check if the graph is empty (no const bindings).
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

/// Output of Phase 2: evaluated const values.
///
/// This type is hashable for salsa memoization. When the same expressions
/// evaluate to the same values, Phase 3 can use cached results.
/// Uses sorted vectors instead of HashMaps for deterministic hashing.
#[derive(Clone, Debug, Default, Hash, Eq, PartialEq)]
pub struct ResolvedConsts {
    /// Evaluated values sorted by statement ID.
    values: Vec<(ConstStmtId, ConstValue)>,
    /// Lookup from name to statement ID, sorted by name.
    name_to_stmt: Vec<(String, ConstStmtId)>,
}

impl ResolvedConsts {
    /// Create an empty ResolvedConsts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a const value.
    pub fn insert(&mut self, stmt_id: ConstStmtId, name: String, value: ConstValue) {
        // Insert into values, maintaining sort order.
        match self.values.binary_search_by_key(&stmt_id, |(id, _)| *id) {
            Ok(idx) => self.values[idx].1 = value,
            Err(idx) => self.values.insert(idx, (stmt_id, value)),
        }
        // Insert into name lookup, maintaining sort order.
        match self.name_to_stmt.binary_search_by(|(n, _)| n.as_str().cmp(&name)) {
            Ok(idx) => self.name_to_stmt[idx].1 = stmt_id,
            Err(idx) => self.name_to_stmt.insert(idx, (name, stmt_id)),
        }
    }

    /// Look up a const value by name.
    pub fn get_by_name(&self, name: &str) -> Option<&ConstValue> {
        self.stmt_for_name(name).and_then(|id| self.get(id))
    }

    /// Look up a const value by statement ID.
    pub fn get(&self, stmt_id: ConstStmtId) -> Option<&ConstValue> {
        self.values.binary_search_by_key(&stmt_id, |(id, _)| *id)
            .ok()
            .map(|idx| &self.values[idx].1)
    }

    /// Look up statement ID by name.
    pub fn stmt_for_name(&self, name: &str) -> Option<ConstStmtId> {
        self.name_to_stmt.binary_search_by(|(n, _)| n.as_str().cmp(name))
            .ok()
            .map(|idx| self.name_to_stmt[idx].1)
    }

    /// Get the name for a statement ID.
    pub fn name_for_stmt(&self, stmt_id: ConstStmtId) -> Option<&str> {
        self.name_to_stmt.iter()
            .find(|(_, id)| *id == stmt_id)
            .map(|(name, _)| name.as_str())
    }

    /// Iterate over all (name, value) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &ConstValue)> {
        self.name_to_stmt.iter()
            .filter_map(|(name, stmt_id)| {
                self.get(*stmt_id).map(|v| (name.as_str(), v))
            })
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Iterate over all (stmt_id, value) pairs.
    pub fn values_iter(&self) -> impl Iterator<Item = (ConstStmtId, &ConstValue)> {
        self.values.iter().map(|(id, v)| (*id, v))
    }

    /// Iterate over all (name, stmt_id) pairs.
    pub fn names_iter(&self) -> impl Iterator<Item = (&str, ConstStmtId)> {
        self.name_to_stmt.iter().map(|(n, id)| (n.as_str(), *id))
    }
}

/// Accumulated const values across script units.
///
/// Used in script contexts to track consts from all previous units.
#[derive(Clone, Debug, Default)]
pub struct AccumulatedConsts {
    /// Values keyed by global ID.
    pub values: std::collections::HashMap<GlobalConstId, ConstValue>,
    /// Lookup from name to global ID (most recent definition wins).
    pub name_index: std::collections::HashMap<String, GlobalConstId>,
}

impl AccumulatedConsts {
    /// Add const values from a unit.
    pub fn add_unit(&mut self, unit_index: u32, resolved: &ResolvedConsts) {
        for (stmt_id, value) in resolved.values_iter() {
            let global_id = GlobalConstId { unit: unit_index, stmt: stmt_id };
            self.values.insert(global_id, value.clone());
        }
        for (name, stmt_id) in resolved.names_iter() {
            let global_id = GlobalConstId { unit: unit_index, stmt: stmt_id };
            self.name_index.insert(name.to_string(), global_id);
        }
    }

    /// Look up a const value by name.
    pub fn get_by_name(&self, name: &str) -> Option<&ConstValue> {
        self.name_index.get(name).and_then(|id| self.values.get(id))
    }
}

/// Error during const evaluation in Phase 2.
///
/// Distinct from `CtfeError` which is for interpreter-level errors.
/// These are higher-level errors that can occur during the evaluation phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstEvalError {
    /// Gas limit exceeded - expression may not terminate.
    GasExpired { binding_name: String },
    /// Dependency on a const that failed to evaluate.
    DependencyFailed { binding_name: String, dependency: String },
    /// Lowering the const expression to IR failed.
    LoweringFailed { binding_name: String, message: String },
    /// Early return in const expression (e.g., `?` on none, `!` on error).
    EarlyReturn { binding_name: String, message: String },
    /// Unsupported type for const extraction.
    UnsupportedType { binding_name: String, type_name: String },
}

impl std::fmt::Display for ConstEvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstEvalError::GasExpired { binding_name } => {
                write!(f, "gas limit exceeded evaluating const '{}'", binding_name)
            }
            ConstEvalError::DependencyFailed { binding_name, dependency } => {
                write!(f, "const '{}' depends on failed const '{}'", binding_name, dependency)
            }
            ConstEvalError::LoweringFailed { binding_name, message } => {
                write!(f, "failed to lower const '{}': {}", binding_name, message)
            }
            ConstEvalError::EarlyReturn { binding_name, message } => {
                write!(f, "const '{}' early-returned: {}", binding_name, message)
            }
            ConstEvalError::UnsupportedType { binding_name, type_name } => {
                write!(f, "const '{}' has unsupported type for CTFE: {}", binding_name, type_name)
            }
        }
    }
}

impl std::error::Error for ConstEvalError {}
