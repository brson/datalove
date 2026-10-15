//! SSA-based intermediate representation for datafun functions.
//!
//! This IR uses SSA form for immutable values (expression temps, let bindings)
//! and explicit slots for mutable bindings (var). Designed for:
//! - Simple slot-based interpretation
//! - Direct lowering to LLVM/Cranelift (SSA values -> registers, slots -> stack)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod dense;
pub mod display;
pub mod frame_layout;
pub mod inline;
pub mod layout;
pub mod params;
pub mod registry;
pub mod simplify;

pub use display::expand_ir_strings;
pub use params::{
    map_operands_in_instruction, map_operands_in_terminator,
    replace_params_in_instruction, replace_params_in_terminator,
};
pub use registry::{FunctionRegistry, ModuleFunctionRegistry, UnitFunctionRegistry, ModuleCodeUnits};

/// SSA value - defined exactly once, immutable.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct ValueId(pub u32);

/// Mutable slot - for var bindings, can be reassigned.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SlotId(pub u32);

/// Function parameter - reference to caller's data.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct ParamId(pub u32);

/// Block identifier.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// Globally unique function identifier within a compilation context.
///
/// Ordered, like its two neighbours here and unlike a `salsa::Id`: this is a
/// position assigned from source order, so sorting by it is deterministic and
/// means something.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[derive(salsa::SalsaValue)]
pub struct FuncId(pub u32);

/// Module identifier for IR.
///
/// Serializable numeric ID, unlike salsa's ModuleId.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[derive(salsa::SalsaValue)]
pub struct IrModuleId(pub u32);


// ============================================================================
// Unified Code Unit Types
// ============================================================================

/// Identifier for a code unit.
///
/// Replaces FuncId for unified addressing. The numeric value is local to
/// the containing scope (module or script execution session).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
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
    /// Table with named columns, in the order they were written.
    Table(Vec<(String, Box<IrType>)>),
    /// A cursor walking a collection for a `for` loop, laid out as the
    /// runtime's cursor of that kind. Plain data owning nothing, so copy;
    /// it lives in a slot for the length of the loop.
    Cursor(CursorKind),
    /// A type parameter of the function the type is written in, by its
    /// position in the declaration.
    ///
    /// A type that mentions one is *dynamic*: its layout is not known until a
    /// call binds the parameter, so a value of it is held in a frame as a fat
    /// pointer -- the value's address and its descriptor -- and the value
    /// itself lives where the pointer says. See `layout::layout_of` and
    /// `botdocs/plan-fat-generics.md`.
    Param(u32),
}

/// Which runtime cursor walks a collection, and so how a cursor is laid out.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum CursorKind {
    /// A list or a rank-1 tensor: `rtdt::StrideCursor`.
    Stride,
    /// The rows of a tensor of rank above 1: `rtdt::RowCursor`.
    Row,
    /// A set or a map: `rtdt::TreeCursor`.
    Tree,
}

impl CursorKind {
    /// The cursor that walks a collection of this type, or `None` for a type
    /// that is not walked.
    pub fn of(coll: &IrType) -> Option<CursorKind> {
        match coll {
            IrType::List(_) | IrType::Tensor(_, 1) => Some(CursorKind::Stride),
            IrType::Tensor(_, _) => Some(CursorKind::Row),
            IrType::Set(_) | IrType::Map(_, _) => Some(CursorKind::Tree),
            _ => None,
        }
    }

    /// What a walk over a collection of this type visits: an element, a
    /// map's key, or a tensor's row of one rank less.
    pub fn elem_type(coll: &IrType) -> IrType {
        match coll {
            IrType::List(elem) | IrType::Set(elem) | IrType::Tensor(elem, 1) => (**elem).clone(),
            IrType::Tensor(elem, rank) => IrType::Tensor(elem.clone(), rank - 1),
            IrType::Map(key, _) => (**key).clone(),
            other => panic!("a walk over a {other}"),
        }
    }
}

impl IrType {
    /// Whether a type parameter appears anywhere in this type, which is what
    /// makes its layout unknown until a call.
    pub fn is_dynamic(&self) -> bool {
        match self {
            IrType::Param(_) => true,
            IrType::Unit | IrType::Bool | IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64
            | IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 | IrType::Index | IrType::Offset
            | IrType::Int | IrType::F32 | IrType::F64 | IrType::String | IrType::Data
            | IrType::Error | IrType::Atom(_) | IrType::Cursor(_) => false,
            IrType::Tuple(fields) => fields.iter().any(IrType::is_dynamic),
            IrType::Struct(fields) => fields.iter().any(|(_, t)| t.is_dynamic()),
            IrType::Enum(variants) => variants.iter().any(|(_, p)| p.as_ref().is_some_and(IrType::is_dynamic)),
            IrType::Table(columns) => columns.iter().any(|(_, t)| t.is_dynamic()),
            IrType::Term(_, inner) | IrType::List(inner) | IrType::Set(inner) | IrType::Option(inner)
            | IrType::Result(inner) | IrType::Tensor(inner, _) | IrType::Ref(inner) => inner.is_dynamic(),
            IrType::Map(k, v) => k.is_dynamic() || v.is_dynamic(),
        }
    }

    /// This type with each type parameter `i` replaced by `args[i]`.
    ///
    /// How a call site's binding is read: a callee's type written over its
    /// own parameters, substituted with what the caller bound them to, is
    /// that type written over the caller's.
    pub fn substitute(&self, args: &[IrType]) -> IrType {
        let sub = |t: &IrType| t.substitute(args);
        let boxed = |t: &IrType| Box::new(t.substitute(args));
        match self {
            IrType::Param(i) => match args.get(*i as usize) {
                Some(arg) => arg.clone(),
                None => panic!("type parameter {i} substituted from {} arguments", args.len()),
            },
            IrType::Unit | IrType::Bool | IrType::U8 | IrType::U16 | IrType::U32 | IrType::U64
            | IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64 | IrType::Index | IrType::Offset
            | IrType::Int | IrType::F32 | IrType::F64 | IrType::String | IrType::Data
            | IrType::Error | IrType::Atom(_) | IrType::Cursor(_) => self.clone(),
            IrType::Tuple(fields) => IrType::Tuple(fields.iter().map(sub).collect()),
            IrType::Struct(fields) => IrType::Struct(fields.iter().map(|(n, t)| (n.clone(), sub(t))).collect()),
            IrType::Enum(variants) => IrType::Enum(
                variants.iter().map(|(n, p)| (n.clone(), p.as_ref().map(sub))).collect(),
            ),
            IrType::Table(columns) => IrType::Table(columns.iter().map(|(n, t)| (n.clone(), boxed(t))).collect()),
            IrType::Term(name, inner) => IrType::Term(name.clone(), boxed(inner)),
            IrType::List(inner) => IrType::List(boxed(inner)),
            IrType::Set(inner) => IrType::Set(boxed(inner)),
            IrType::Option(inner) => IrType::Option(boxed(inner)),
            IrType::Result(inner) => IrType::Result(boxed(inner)),
            IrType::Tensor(inner, rank) => IrType::Tensor(boxed(inner), *rank),
            IrType::Ref(inner) => IrType::Ref(boxed(inner)),
            IrType::Map(k, v) => IrType::Map(boxed(k), boxed(v)),
        }
    }

    /// Every dynamic type within this one, itself included, parts before
    /// what holds them.
    ///
    /// A `Ref` is left out: a reference to a dynamic type is described by what
    /// it points at, which is in.
    pub fn dynamic_parts(&self, out: &mut Vec<IrType>) {
        if !self.is_dynamic() {
            return;
        }
        match self {
            IrType::Tuple(fields) => fields.iter().for_each(|t| t.dynamic_parts(out)),
            IrType::Struct(fields) => fields.iter().for_each(|(_, t)| t.dynamic_parts(out)),
            IrType::Enum(variants) => variants.iter().filter_map(|(_, p)| p.as_ref()).for_each(|t| t.dynamic_parts(out)),
            IrType::Table(columns) => columns.iter().for_each(|(_, t)| t.dynamic_parts(out)),
            IrType::Term(_, inner) | IrType::List(inner) | IrType::Set(inner) | IrType::Option(inner)
            | IrType::Result(inner) | IrType::Tensor(inner, _) => inner.dynamic_parts(out),
            IrType::Map(k, v) => {
                k.dynamic_parts(out);
                v.dynamic_parts(out);
            }
            IrType::Ref(inner) => {
                inner.dynamic_parts(out);
                return;
            }
            _ => {}
        }
        if !out.contains(self) {
            out.push(self.clone());
        }
    }

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
            IrType::Term(name, payload) => write!(f, "term {} {}", name, payload),
            IrType::List(elem) => write!(f, "list<{}>", elem),
            IrType::Set(elem) => write!(f, "set<{}>", elem),
            IrType::Map(k, v) => write!(f, "map<{}, {}>", k, v),
            IrType::Option(inner) => write!(f, "option<{}>", inner),
            IrType::Result(ok) => write!(f, "result<{}>", ok),
            IrType::Tensor(elem, rank) => write!(f, "[|{}, {}|]", elem, rank),
            IrType::Ref(inner) => write!(f, "ref<{}>", inner),
            IrType::Cursor(kind) => write!(f, "cursor<{:?}>", kind),
            IrType::Param(i) => write!(f, "T{}", i),
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
        Self::from_type_hint_inner(db, ty, &[])
    }

    /// Convert from AST type hint to IR type, keeping the given type
    /// parameters as `IrType::Param` by their position.
    ///
    /// How a generic sees its own types: `(T, u32)`
    /// is `Tuple([Param(0), U32])`, and its layout is the descriptor's to say.
    pub fn from_type_hint_generic<'db>(
        db: &'db dyn salsa::Database,
        ty: &datalove_datalit::ast::TypeHint<'db>,
        type_params: &[bct::text::InternedText<'db>],
    ) -> Self {
        Self::from_type_hint_inner(db, ty, type_params)
    }

    /// Convert from AST TypeHint to IR type.
    fn from_type_hint_inner<'db>(
        db: &'db dyn salsa::Database,
        ty: &datalove_datalit::ast::TypeHint<'db>,
        type_params: &[bct::text::InternedText<'db>],
    ) -> Self {
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
                    .map(|f| Self::from_type_hint_inner(db, f, type_params))
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
                    .map(|f| (f.name.text(db).to_string(), Self::from_type_hint_inner(db, &f.type_hint, type_params)))
                    .collect();
                IrType::Struct(fields)
            }

            TypeHint::List(list) => {
                let elem = Self::from_type_hint_inner(db, &list.element_type, type_params);
                IrType::List(Box::new(elem))
            }
            TypeHint::Set(set) => {
                let elem = Self::from_type_hint_inner(db, &set.element_type, type_params);
                IrType::Set(Box::new(elem))
            }
            TypeHint::Map(map) => {
                let key = Self::from_type_hint_inner(db, &map.key_type, type_params);
                let val = Self::from_type_hint_inner(db, &map.value_type, type_params);
                IrType::Map(Box::new(key), Box::new(val))
            }
            TypeHint::Option(opt) => {
                let inner = Self::from_type_hint_inner(db, &opt.inner_type, type_params);
                IrType::Option(Box::new(inner))
            }
            TypeHint::Result(res) => {
                let ok = Self::from_type_hint_inner(db, &res.inner_type, type_params);
                IrType::Result(Box::new(ok))
            }
            TypeHint::Tensor(t) => {
                let elem = Self::from_type_hint_inner(db, &t.element_type, type_params);
                IrType::Tensor(Box::new(elem), t.rank)
            }
            TypeHint::Table(table) => {
                // In the order the columns were written, which is the order
                // every other layer keeps them in: datalit compares two table
                // types column by column, the typechecker makes a literal's
                // header match its type, and the literal's cells are lowered
                // left to right. Sorting here for a canonical layout agreed
                // with none of them, and a table whose columns were not
                // already alphabetical was built with each cell at another
                // column's offset.
                let columns: Vec<_> = table.columns
                    .iter()
                    .map(|c| {
                        let name = c.name.text(db).to_string();
                        let ty = Self::from_type_hint_inner(db, &c.type_hint, type_params);
                        (name, Box::new(ty))
                    })
                    .collect();
                IrType::Table(columns)
            }
            TypeHint::Atom(a) => {
                let name = a.name.text(db).to_string();
                IrType::Atom(name)
            }
            TypeHint::Term(t) => {
                let name = t.name.text(db).to_string();
                let payload = Self::from_type_hint_inner(db, &t.payload, type_params);
                IrType::Term(name, Box::new(payload))
            }
            TypeHint::Enum(e) => {
                let mut variants: Vec<_> = e.variants.iter()
                    .map(|v| {
                        let name = v.name.text(db).to_string();
                        let payload = v.payload.as_ref().map(|p| Self::from_type_hint_inner(db, p, type_params));
                        (name, payload)
                    })
                    .collect();
                variants.sort_by(|a, b| a.0.cmp(&b.0));
                IrType::Enum(variants)
            }
            TypeHint::ParseError(_) => {
                IrType::Error
            }
            TypeHint::Alias(alias) => {
                if let Some(i) = type_params.iter().position(|p| *p == alias.name) {
                    IrType::Param(i as u32)
                } else {
                    // Type aliases should be resolved before IR generation.
                    IrType::Error
                }
            }
        }
    }

    /// Convert from datalit typechecker type to IR type, outside any generic.
    pub fn from_datalit<'db>(db: &'db dyn salsa::Database, ty: &datalove_datalit::tycheck::Type<'db>) -> Self {
        Self::from_datalit_generic(db, ty, &[])
    }

    /// Convert from datalit typechecker type to IR type, keeping the given
    /// type parameters as `IrType::Param` by their position; see
    /// `from_type_hint_generic`.
    pub fn from_datalit_generic<'db>(
        db: &'db dyn salsa::Database,
        ty: &datalove_datalit::tycheck::Type<'db>,
        type_params: &[bct::text::InternedText<'db>],
    ) -> Self {
        Self::from_datalit_inner(db, ty, type_params)
    }

    fn from_datalit_inner<'db>(
        db: &'db dyn salsa::Database,
        ty: &datalove_datalit::tycheck::Type<'db>,
        keep: &[bct::text::InternedText<'db>],
    ) -> Self {
        use datalove_datalit::tycheck::Type as DlType;

        match ty {
            DlType::Var(name) => match keep.iter().position(|p| p == name) {
                Some(i) => IrType::Param(i as u32),
                None => panic!("type variable {} is not a parameter of the function it is in",
                    name.as_str(db)),
            },
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
                    .map(|f| Self::from_datalit_inner(db, f, keep))
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
                    .map(|f| (f.name.text(db).to_string(), Self::from_datalit_inner(db, &*f.ty, keep)))
                    .collect();
                IrType::Struct(fields)
            }

            DlType::List(list) => {
                let elem = Self::from_datalit_inner(db, &*list.element_type, keep);
                IrType::List(Box::new(elem))
            }
            DlType::Set(set) => {
                let elem = Self::from_datalit_inner(db, &*set.element_type, keep);
                IrType::Set(Box::new(elem))
            }
            DlType::Map(map) => {
                let key = Self::from_datalit_inner(db, &*map.key_type, keep);
                let val = Self::from_datalit_inner(db, &*map.value_type, keep);
                IrType::Map(Box::new(key), Box::new(val))
            }
            DlType::Option(opt) => {
                let inner = Self::from_datalit_inner(db, &*opt.inner_type, keep);
                IrType::Option(Box::new(inner))
            }
            DlType::Result(res) => {
                let ok = Self::from_datalit_inner(db, &*res.inner_type, keep);
                IrType::Result(Box::new(ok))
            }
            DlType::Tensor(t) => {
                let elem = Self::from_datalit_inner(db, &*t.element_type, keep);
                IrType::Tensor(Box::new(elem), t.rank)
            }
            DlType::Table(table) => {
                // Written order, as above.
                let columns: Vec<_> = table.columns
                    .iter()
                    .map(|c| {
                        let name = c.name.text(db).to_string();
                        let ty = Self::from_datalit_inner(db, &*c.ty, keep);
                        (name, Box::new(ty))
                    })
                    .collect();
                IrType::Table(columns)
            }
            DlType::Atom(a) => {
                let name = a.name.text(db).to_string();
                IrType::Atom(name)
            }
            DlType::Term(t) => {
                let name = t.name.text(db).to_string();
                let payload = Self::from_datalit_inner(db, &t.payload, keep);
                IrType::Term(name, Box::new(payload))
            }
            DlType::Enum(e) => {
                let mut variants: Vec<_> = e.variants.iter()
                    .map(|v| {
                        let name = v.name.text(db).to_string();
                        let payload = v.payload.as_ref().map(|p| Self::from_datalit_inner(db, p, keep));
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
            // A cursor points into a collection and owns nothing.
            IrType::Cursor(_) => true,

            // A type parameter is always linear, whatever it is bound to.
            IrType::Param(_) => false,
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

/// A 32-bit float as a constant.
///
/// See [`ConstF64`] for why it is wrapped.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConstF32(pub f32);

/// A 64-bit float as a constant.
///
/// Compared and hashed by its bits, which is the relation sets and maps keep
/// their keys in -- IEEE 754-2008 `totalOrder`, which gives NaN a place and
/// tells the two zeros apart -- and not the one the `==` operator has. The
/// runtime draws the same distinction, between `FloatEqPolicy::Bitwise` and
/// `FloatEqPolicy::Ieee`, and uses the bitwise one where a value has to have a
/// single representation.
///
/// Asking a constant whether it is the same constant is that question, not the
/// operator's: a `ConstValue` is what decides whether two call sites name one
/// instantiation, and what salsa compares to decide whether lowering can be
/// reused. Under the operator's relation a NaN constant is not equal to itself,
/// which makes `Eq` a lie and stops any of that working.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConstF64(pub f64);

macro_rules! const_float_by_bits {
    ($ty:ident, $bits:ty) => {
        impl PartialEq for $ty {
            fn eq(&self, other: &Self) -> bool {
                self.0.to_bits() == other.0.to_bits()
            }
        }

        impl Eq for $ty {}

        impl std::hash::Hash for $ty {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.0.to_bits().hash(state);
            }
        }

        impl std::fmt::Display for $ty {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

const_float_by_bits!(ConstF32, u32);
const_float_by_bits!(ConstF64, u64);

/// Constant value that can be loaded or computed at compile time.
///
/// Supports all Datafun types for compile-time function evaluation (CTFE).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    F32(ConstF32),
    /// 64-bit float.
    F64(ConstF64),
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
    /// Dynamic data wrapper, with the type of what it holds.
    ///
    /// The type is kept because a `data` carries its own descriptor at run
    /// time and the value alone cannot always say what that should be: an
    /// empty map says `%{() = ()}`, which is not a type the program has and
    /// which a backend has no descriptor for. The same reason a tensor keeps
    /// its shape.
    Data { payload_type: Box<IrType>, value: Box<ConstValue> },
    /// Error value, with the type of what it boxes. See `Data`.
    Error { payload_type: Box<IrType>, value: Box<ConstValue> },

    // Collections.
    /// List with elements.
    List(Vec<ConstValue>),
    /// Set with elements, in no particular order and possibly repeated: a
    /// backend builds one by inserting each, as a literal is built. One read
    /// back by compile-time evaluation is sorted; one read from a data file is
    /// in the order the file wrote it.
    Set(Vec<ConstValue>),
    /// Map with key-value pairs, which like a set's elements need not be
    /// sorted or each key given once; a later entry for a key replaces an
    /// earlier one.
    Map(Vec<(ConstValue, ConstValue)>),
    /// A tensor: its shape, and its elements in row-major order.
    ///
    /// The shape is kept because the elements do not say it -- nine numbers
    /// are a 3 by 3 or a 9 by 1 depending only on this -- and because the rank
    /// belongs to the type while the lengths belong to the value.
    Tensor { shape: Vec<u32>, elements: Vec<ConstValue> },
    /// Table with column names and row data.
    Table { columns: Vec<String>, rows: Vec<Vec<ConstValue>> },
}

/// A shared const value that hashes by its shape rather than its contents.
///
/// For a const that is part of a salsa key, or a field of a tracked struct:
/// salsa hashes those on every compile, and a const can be a dataset, so a
/// module's data was hashed whole each time it was lowered -- and once more
/// for every function naming it, since a `StaticRef` holds the value and a
/// lowered function is part of a tracked struct. Hashing the shape
/// -- the variant, a collection's length, a scalar's value -- is constant
/// time and still gives equal values equal hashes. Equality is still whole,
/// and `Arc`'s starts by asking whether the two are one value, which a data
/// file's const is from one compile to the next.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SharedConst(pub std::sync::Arc<ConstValue>);

impl std::hash::Hash for SharedConst {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let value: &ConstValue = &self.0;
        std::mem::discriminant(value).hash(state);
        match value {
            ConstValue::List(items) | ConstValue::Set(items) | ConstValue::Tuple(items) => items.len().hash(state),
            ConstValue::Map(entries) => entries.len().hash(state),
            ConstValue::Struct(fields) => fields.len().hash(state),
            ConstValue::Tensor { shape, .. } => shape.hash(state),
            ConstValue::Table { columns, rows } => (columns, rows.len()).hash(state),
            ConstValue::String(s) => s.len().hash(state),
            ConstValue::Enum { variant, .. } => variant.hash(state),
            ConstValue::OptionSome(_) | ConstValue::ResultOk(_) | ConstValue::ResultErr(_)
            | ConstValue::Data { .. } | ConstValue::Error { .. } => {}
            scalar => scalar.hash(state),
        }
    }
}

impl std::ops::Deref for SharedConst {
    type Target = std::sync::Arc<ConstValue>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The type a constant value is of.
///
/// A `ConstValue` says its own shape, so the type can be read off it. Where a
/// value cannot say the whole of a type this answers with the part it can: an
/// enum gives the one variant it holds, and a `none` gives `?()`. That is
/// enough for the two things this is for -- making a descriptor for a `data`,
/// and sizing a buffer to build one in -- and is not enough to check anything
/// against.
///
/// Here rather than in a backend because three of them wanted it and had a
/// copy each, character for character.
pub fn ir_type_of_const_value(value: &ConstValue) -> IrType {
    match value {
        ConstValue::Unit => IrType::Unit,
        ConstValue::Bool(_) => IrType::Bool,
        ConstValue::U8(_) => IrType::U8,
        ConstValue::U16(_) => IrType::U16,
        ConstValue::U32(_) => IrType::U32,
        ConstValue::U64(_) => IrType::U64,
        ConstValue::I8(_) => IrType::I8,
        ConstValue::I16(_) => IrType::I16,
        ConstValue::I32(_) => IrType::I32,
        ConstValue::I64(_) => IrType::I64,
        ConstValue::Index(_) => IrType::Index,
        ConstValue::Offset(_) => IrType::Offset,
        ConstValue::Int { .. } => IrType::Int,
        ConstValue::F32(_) => IrType::F32,
        ConstValue::F64(_) => IrType::F64,
        ConstValue::String(_) => IrType::String,
        ConstValue::Tuple(fields) => {
            IrType::Tuple(fields.iter().map(ir_type_of_const_value).collect())
        }
        ConstValue::Struct(fields) => {
            IrType::Struct(
                fields.iter()
                    .map(|(name, v)| (name.clone(), ir_type_of_const_value(v)))
                    .collect()
            )
        }
        ConstValue::Enum { variant, payload } => {
            // For enum, we can only infer a single-variant enum type.
            let payload_type = payload.as_ref().map(|p| ir_type_of_const_value(p));
            IrType::Enum(vec![(variant.clone(), payload_type)])
        }
        ConstValue::OptionNone => {
            // Cannot fully infer the inner type for None; default to Unit.
            IrType::Option(Box::new(IrType::Unit))
        }
        ConstValue::OptionSome(inner) => {
            IrType::Option(Box::new(ir_type_of_const_value(inner)))
        }
        ConstValue::ResultOk(inner) => {
            IrType::Result(Box::new(ir_type_of_const_value(inner)))
        }
        ConstValue::ResultErr(_) => {
            // Result::Err - cannot infer Ok type from Err; default to Unit.
            IrType::Result(Box::new(IrType::Unit))
        }
        ConstValue::Data { .. } => IrType::Data,
        ConstValue::Error { .. } => IrType::Error,
        ConstValue::List(elements) => {
            let elem_type = elements.first()
                .map(ir_type_of_const_value)
                .unwrap_or(IrType::Unit);
            IrType::List(Box::new(elem_type))
        }
        ConstValue::Set(elements) => {
            let elem_type = elements.first()
                .map(ir_type_of_const_value)
                .unwrap_or(IrType::Unit);
            IrType::Set(Box::new(elem_type))
        }
        ConstValue::Map(entries) => {
            let (key_type, value_type) = entries.first()
                .map(|(k, v)| (ir_type_of_const_value(k), ir_type_of_const_value(v)))
                .unwrap_or((IrType::Unit, IrType::Unit));
            IrType::Map(Box::new(key_type), Box::new(value_type))
        }
        ConstValue::Tensor { shape, elements } => IrType::Tensor(
            Box::new(
                elements.first()
                    .map(ir_type_of_const_value)
                    .unwrap_or(IrType::Unit),
            ),
            shape.len() as u32,
        ),
        ConstValue::Table { columns, rows } => {
            // Infer column types from first row if available.
            let col_types: Vec<(String, Box<IrType>)> = if let Some(first_row) = rows.first() {
                columns.iter()
                    .zip(first_row.iter())
                    .map(|(name, value)| (name.clone(), Box::new(ir_type_of_const_value(value))))
                    .collect()
            } else {
                columns.iter().map(|name| (name.clone(), Box::new(IrType::Unit))).collect()
            };
            IrType::Table(col_types)
        }
    }
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

    /// A reference to a const of a non-copy type, built once and kept for as
    /// long as the code that names it.
    ///
    /// `dest` has type `Ref(T)` for the const's type `T`, and is read through
    /// like any other reference. Nothing owns what it points at, so nothing
    /// drops it. Each backend builds the value once, the first time it is
    /// needed or before the program starts, and equal values may share one.
    /// The value is held in an `Arc` so that copying the instruction, as
    /// inlining and specialization do, keeps one identity a backend can key on.
    ///
    /// **Ownership:** Produces `dest`, a borrow.
    StaticRef { dest: ValueId, value: SharedConst },

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

    /// Update a place with a binary operator: `place = place op rhs`.
    ///
    /// What `set p op= v` lowers to, for an operator that cannot fail. `place`
    /// is a slot, a parameter, an earlier unit's slot, or a reference to the
    /// place, as for `RefStore`. Its type is `f32`, `f64` or `int`, never
    /// dynamic: a type parameter's arithmetic is a store of a `BinOp`'s result.
    ///
    /// The place's value is replaced where it is. For an `int` that can mean
    /// reusing its buffer, and `rhs` may be the place itself.
    ///
    /// **Ownership:** Borrows `rhs`. The place owns its new value as it did
    /// the old one, which the instruction drops.
    OpAssign {
        place: Operand,
        op: BinOp,
        rhs: Operand,
    },

    /// Update a place with a checked binary operator, if it succeeds.
    ///
    /// As `OpAssign`, for an operator that can fail: `overflow` is true if it
    /// did, and the place is left as it was. Fixed-width integers, and `int`
    /// for division by zero.
    ///
    /// **Ownership:** As `OpAssign`. Produces `overflow`.
    OpAssignChecked {
        overflow: ValueId,
        place: Operand,
        op: BinOp,
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
        dest: ValueId,
        func: CodeRef,
        args: Vec<Operand>,
        /// What this call site bound each of the callee's type parameters to,
        /// in the callee's declaration order and written over this function's
        /// own: concrete, or mentioning this function's `Param`s where it
        /// passed them along. The callee is handed a descriptor for each, a
        /// static one or this function's table entry for it. Empty for a
        /// non-generic callee.
        #[serde(default)]
        bindings: Vec<IrType>,
    },

    /// Call to a function with const parameters.
    ///
    /// This is emitted during lowering when the callee has const parameters.
    /// Contains metadata for the specialization pass to rewrite the call.
    ///
    /// **Behavior without specialization:** Acts exactly like `Call` - the interpreter
    /// and AOT codegen treat this as a normal call to the original function.
    ///
    /// **Behavior with specialization:** The specialization pass points this at
    /// the copy built for its const arguments' values, as a `Call` without the
    /// const arguments. Those are passed by reference, so there is nothing to
    /// drop when they go.
    ///
    /// **Ownership:** Same as `Call`.
    ComptimeCall {
        dest: ValueId,
        func: CodeRef,
        /// Original arguments including const parameter args.
        args: Vec<Operand>,
        /// Indices of const parameters (to be removed during specialization).
        comptime_param_indices: Vec<usize>,
        /// As on [`Instruction::Call`]. A callee's const parameters and its
        /// type parameters are independent, so a copy built for an
        /// instantiation is still generic and still takes these.
        #[serde(default)]
        bindings: Vec<IrType>,
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
    /// **Ownership:** Borrows `src`, produces `dest`. Only a field of a copy
    /// type is read this way -- reading one that owns something would move it
    /// out of an aggregate that still holds it (F070) -- so `dest` is a copy and
    /// `src` keeps everything it had. Inside a generic, where the destination is
    /// a `data`, the runtime clones the field into it instead.
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
    //
    // **Stores do not destroy.** Every store -- to a slot, a parameter, a field,
    // or through a reference -- writes the new value over the place and nothing
    // else. Where the place may hold a value that owns something, lowering
    // emits the drop in front of the store: `Drop` or `DropTracked` for a slot
    // or a parameter, `DropViaRef` for a store through a reference, `DropField`
    // for a field. For a copy type there is nothing to destroy and no drop.
    // Whether one is needed is decided once, here, rather than by each engine.

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
    /// Mut params are precise - caller always provides initialized memory, which
    /// a `Drop` of the parameter destroys first if it owns anything.
    ///
    /// **Ownership:** Consumes `value`, writes to caller's param location.
    ParamStore { param: ParamId, value: Operand },

    /// Store value to Out parameter (tracked).
    ///
    /// Out params start uninitialized; a `DropTracked` of the parameter in front
    /// destroys what an earlier write left, if anything.
    ///
    /// **Ownership:** Consumes `value`, writes to caller's param location.
    /// **Tracking:** Writes LIVE after store.
    ParamStoreTracked { param: ParamId, value: Operand },

    /// Store value to a field within mutable parameter (Mut params only).
    ///
    /// **Ownership:** Consumes `value`, writes to field within param.
    ParamSetField {
        param: ParamId,
        field_path: Vec<u32>,
        value: Operand,
    },

    /// Store value to a field within Out parameter (tracked).
    ///
    /// **Ownership:** Consumes `value`, writes to field within param.
    /// **Tracking:** Writes LIVE after store.
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

    /// Drop a field within a place: a slot, a parameter, or what a reference
    /// points at, named as `SetField`, `ParamSetField` and `RefSetField` name it.
    ///
    /// Emitted in front of a store to the field when its type owns something.
    ///
    /// **Ownership:** Destroys the field; does not consume `base`.
    DropField { base: Operand, field_path: Vec<u32> },

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
    // List Indexing
    // ========================================================================

    /// Get a list element by index.
    ///
    /// Copies the element at `index` from `list` to `dest`.
    /// Sets `is_valid` to true if index was in bounds.
    /// If out of bounds, `dest` is undefined and must not be used.
    ///
    /// **Ownership:** Borrows `list` and `index`, produces `dest` (only valid if is_valid).
    ListGet {
        dest: ValueId,
        is_valid: ValueId,
        list: Operand,
        index: Operand,
    },

    /// Check if a list index is in bounds.
    ///
    /// **Ownership:** Borrows `list` and `index`, produces `is_valid`.
    ListBoundsCheck {
        is_valid: ValueId,
        list: Operand,
        index: Operand,
    },

    /// Replace a list element, if the index is in bounds.
    ///
    /// In bounds, destroys the old element, stores `value` and sets `is_valid`
    /// to true. Past the end, destroys `value`, leaves the list as it was and
    /// sets `is_valid` to false. One operation, as the runtime's `list_set` is:
    /// no state between the check and the write is visible.
    ///
    /// **Ownership:** Borrows `list` mutably, borrows `index`, consumes `value`
    /// whatever the outcome, produces `is_valid`.
    ListSet {
        is_valid: ValueId,
        list: Operand,
        index: Operand,
        value: Operand,
    },

    /// Get a reference (pointer) to an element in a list.
    ///
    /// Computes element address without copying. Used for nested set targets
    /// where we need to modify a list element's fields in place.
    ///
    /// **Ownership:** Borrows `list`, produces `dest` (Ref type, always Copy).
    /// Bounds must be checked before use.
    ListElementRef {
        dest: ValueId,
        list: Operand,
        index: Operand,
    },

    // ========================================================================
    // Map Indexing
    // ========================================================================

    /// Get a value from a map by key (returns cloned value + validity flag).
    ///
    /// **Ownership:** Borrows `map` and `key`, produces `dest` and `is_valid`.
    MapGet {
        dest: ValueId,
        is_valid: ValueId,
        map: Operand,
        key: Operand,
    },

    /// Check if a map contains a given key.
    ///
    /// **Ownership:** Borrows `map` and `key`, produces `is_valid`.
    MapContainsKey {
        is_valid: ValueId,
        map: Operand,
        key: Operand,
    },

    /// Replace the value for a key, if the map holds the key.
    ///
    /// As `ListSet`: if the key is there, destroys its old value, stores
    /// `value` and sets `is_valid`; if not, destroys `value`, leaves the map as
    /// it was and clears `is_valid`.
    ///
    /// **Ownership:** Borrows `map` mutably, borrows `key`, consumes `value`
    /// whatever the outcome, produces `is_valid`.
    MapSetValue {
        is_valid: ValueId,
        map: Operand,
        key: Operand,
        value: Operand,
    },

    /// Get a reference (pointer) to a value in a map by key.
    ///
    /// Used for ref/mut params and chained field access. Key must exist
    /// (caller checks with MapContainsKey).
    ///
    /// **Ownership:** Borrows `map`, borrows `key`, produces `dest` (Ref type).
    MapValueRef {
        dest: ValueId,
        map: Operand,
        key: Operand,
    },

    /// Upsert a key-value pair into a map.
    ///
    /// If the key exists: destroys old value, stores new value, destroys
    /// provided key. If the key is absent: inserts both key and value.
    /// Always succeeds — no early return, no branching.
    ///
    /// **Ownership:** Borrows `map` mutably, consumes `key`, consumes `value`.
    MapUpsert {
        map: Operand,
        key: Operand,
        value: Operand,
    },

    // ========================================================================
    // Tensor Indexing
    // ========================================================================

    /// Get a tensor element or sub-tensor by axis-0 index.
    ///
    /// Rank 1: clones element to dest. Rank > 1: calls hyperplane_clone
    /// to produce owned sub-tensor. Sets `is_valid` to true if index is
    /// in bounds; if out of bounds, `dest` is undefined.
    ///
    /// **Ownership:** Borrows `tensor` and `index`, produces `dest` (only valid if is_valid).
    TensorGet {
        dest: ValueId,
        is_valid: ValueId,
        tensor: Operand,
        index: Operand,
    },

    /// Check if a tensor axis-0 index is in bounds.
    ///
    /// **Ownership:** Borrows `tensor` and `index`, produces `is_valid`.
    TensorBoundsCheck {
        is_valid: ValueId,
        tensor: Operand,
        index: Operand,
    },

    /// Replace a tensor element at an axis-0 index (rank 1 only), if the index
    /// is in bounds.
    ///
    /// As `ListSet`: in bounds, destroys the old element, stores `value` and
    /// sets `is_valid`; past the end, destroys `value` and clears `is_valid`.
    ///
    /// **Ownership:** Borrows `tensor` mutably, borrows `index`, consumes
    /// `value` whatever the outcome, produces `is_valid`.
    TensorSet {
        is_valid: ValueId,
        tensor: Operand,
        index: Operand,
        value: Operand,
    },

    /// Get a reference to a tensor element or sub-tensor view at axis-0 index.
    ///
    /// Rank 1: dest is pointer to element. Rank > 1: constructs a view
    /// Tensor (capacity=0) on the stack and returns pointer to it.
    /// Bounds must be checked before use.
    ///
    /// **Ownership:** Borrows `tensor`, produces `dest` (Ref type, always Copy).
    TensorIndexRef {
        dest: ValueId,
        tensor: Operand,
        index: Operand,
    },

    // ========================================================================
    // Cursors
    // ========================================================================

    /// Start a walk over a collection: fill `cursor` at its first element, or
    /// done if it has none.
    ///
    /// The cursor's kind is the slot's type, `IrType::Cursor`, and agrees
    /// with `coll`'s. The collection is frozen until the walk ends.
    ///
    /// **Ownership:** Borrows `coll` for the walk; writes the cursor slot.
    CursorBegin { cursor: SlotId, coll: Operand },

    /// Whether a walk is over.
    ///
    /// **Ownership:** Reads the cursor slot, produces `dest` (Bool).
    CursorDone { dest: ValueId, cursor: SlotId },

    /// A reference to the current element of a walk: a set's element, a
    /// map's key, a tensor's row. The walk is not done.
    ///
    /// **Ownership:** Reads the cursor slot, produces `dest` (Ref type).
    CursorElem { dest: ValueId, cursor: SlotId },

    /// The position of the current element of a walk over a list or a
    /// tensor, counting from zero. The walk is not done.
    ///
    /// **Ownership:** Reads the cursor slot, produces `dest` (Index).
    CursorIndex { dest: ValueId, cursor: SlotId },

    /// A reference to the current value of a walk over a map. The walk is not
    /// done.
    ///
    /// **Ownership:** Reads the cursor slot, produces `dest` (Ref type).
    CursorValue { dest: ValueId, cursor: SlotId },

    /// Move a walk to the next element, or to done after the last. The walk
    /// is not done. `coll` is the collection begun on. Advancing reads only the
    /// cursor, but naming `coll` keeps the walk's borrow of it in view until
    /// the loop's last advance.
    ///
    /// **Ownership:** Borrows `coll`; writes the cursor slot.
    CursorAdvance { cursor: SlotId, coll: Operand },

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

impl Instruction {
    /// Where this instruction calls, and what it bound the callee's type
    /// parameters to, for the two instructions that call a named function.
    ///
    /// The callee of a `Call` or a `ComptimeCall`.
    ///
    /// Read through here rather than matched separately, so that no pass
    /// asking which instructions are calls misses a kind.
    pub fn call_target(&self) -> Option<&CodeRef> {
        match self {
            Instruction::Call { func, .. } | Instruction::ComptimeCall { func, .. } => Some(func),
            _ => None,
        }
    }
}

impl Instruction {
    /// Visit every value and slot an instruction names outside its operands:
    /// what it defines, a slot it loads from or stores to, the reference a
    /// `DropViaRef` drops through. With `map_operands_in_instruction` for the
    /// operands, that is every id in it, for renumbering them.
    ///
    /// Exhaustive, so a new instruction has to say.
    pub fn for_each_id_mut(&mut self, value: &mut impl FnMut(&mut ValueId), slot: &mut impl FnMut(&mut SlotId)) {
        let mut slot_dest = |dest: &mut SlotDest| match dest {
            SlotDest::Local(s) => slot(s),
            SlotDest::External { .. } => {}
        };
        match self {
            Instruction::Const { dest, .. } | Instruction::StaticRef { dest, .. }
            | Instruction::Copy { dest, .. } | Instruction::Move { dest, .. }
            | Instruction::BinOp { dest, .. } | Instruction::UnaryOp { dest, .. }
            | Instruction::Widen { dest, .. } | Instruction::WidenFixed { dest, .. }
            | Instruction::Clone { dest, .. } | Instruction::Call { dest, .. }
            | Instruction::ComptimeCall { dest, .. } | Instruction::Pack { dest, .. }
            | Instruction::GetField { dest, .. } | Instruction::GetFieldRef { dest, .. }
            | Instruction::WrapSome { dest, .. } | Instruction::WrapNone { dest }
            | Instruction::WrapOk { dest, .. } | Instruction::WrapErr { dest, .. }
            | Instruction::EnumVariant { dest, .. } | Instruction::EnumDiscriminant { dest, .. }
            | Instruction::EnumPayload { dest, .. } | Instruction::ErrorFrom { dest, .. }
            | Instruction::DataFrom { dest, .. } | Instruction::ListNew { dest, .. }
            | Instruction::SetNew { dest, .. } | Instruction::MapNew { dest, .. }
            | Instruction::TensorNew { dest, .. } | Instruction::TableNew { dest, .. }
            | Instruction::ListElementRef { dest, .. } | Instruction::MapValueRef { dest, .. }
            | Instruction::TensorIndexRef { dest, .. } | Instruction::Intrinsic { dest, .. } => value(dest),
            Instruction::BinOpChecked { dest, overflow, .. }
            | Instruction::UnaryOpChecked { dest, overflow, .. } => {
                value(dest);
                value(overflow);
            }
            Instruction::OpAssignChecked { overflow, .. } => value(overflow),
            Instruction::Unpack { dests, .. } => dests.iter_mut().for_each(value),
            Instruction::UnwrapOption { dest, is_some, .. } => {
                value(dest);
                value(is_some);
            }
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, .. } => {
                value(ok_dest);
                value(err_dest);
                value(is_ok);
            }
            Instruction::ListGet { dest, is_valid, .. } | Instruction::MapGet { dest, is_valid, .. }
            | Instruction::TensorGet { dest, is_valid, .. } => {
                value(dest);
                value(is_valid);
            }
            Instruction::ListBoundsCheck { is_valid, .. } | Instruction::ListSet { is_valid, .. }
            | Instruction::MapContainsKey { is_valid, .. } | Instruction::MapSetValue { is_valid, .. }
            | Instruction::TensorBoundsCheck { is_valid, .. } | Instruction::TensorSet { is_valid, .. } => {
                value(is_valid)
            }
            Instruction::SlotLoadCopy { dest, slot: s } | Instruction::SlotLoadMove { dest, slot: s }
            | Instruction::SlotLoadMoveTracked { dest, slot: s } => {
                value(dest);
                slot(s);
            }
            Instruction::SlotStoreCopy { dest, .. } | Instruction::SlotStoreCopyTracked { dest, .. }
            | Instruction::SlotStoreMove { dest, .. } | Instruction::SlotStoreMoveTracked { dest, .. } => {
                slot_dest(dest)
            }
            Instruction::CursorBegin { cursor, .. } | Instruction::CursorAdvance { cursor, .. } => slot(cursor),
            Instruction::CursorDone { dest, cursor } | Instruction::CursorElem { dest, cursor }
            | Instruction::CursorIndex { dest, cursor } | Instruction::CursorValue { dest, cursor } => {
                value(dest);
                slot(cursor);
            }
            Instruction::SetField { slot: dest, .. } | Instruction::SetFieldTracked { slot: dest, .. } => {
                slot_dest(dest)
            }
            Instruction::DropViaRef { ref_value } => value(ref_value),
            Instruction::OpAssign { .. } | Instruction::ParamStore { .. }
            | Instruction::ParamStoreTracked { .. } | Instruction::ParamSetField { .. }
            | Instruction::ParamSetFieldTracked { .. } | Instruction::RefStore { .. }
            | Instruction::RefSetField { .. } | Instruction::RefStoreTracked { .. }
            | Instruction::RefSetFieldTracked { .. } | Instruction::Drop { .. }
            | Instruction::DropTracked { .. } | Instruction::DropField { .. }
            | Instruction::UnitEndDrop { .. } | Instruction::UnitEndDropTracked { .. }
            | Instruction::MapUpsert { .. } | Instruction::DebugLog { .. } | Instruction::Nop => {}
        }
    }

    /// Every value an instruction defines.
    ///
    /// Exhaustive, so a new instruction has to say. Several define a second or
    /// third value, usually the flag a branch tests, and missing one is
    /// missing a definition: dead code elimination once dropped an
    /// `UnwrapOption` nobody read the payload of, and a `ListGet` nobody read
    /// the element of, leaving their branches testing values nothing defined.
    pub fn dests(&self) -> Vec<ValueId> {
        match self {
            // Constants and copies
            Instruction::Const { dest, .. } => vec![*dest],
            Instruction::StaticRef { dest, .. } => vec![*dest],
            Instruction::Copy { dest, .. } => vec![*dest],
            Instruction::Move { dest, .. } => vec![*dest],

            // Arithmetic operations
            Instruction::BinOp { dest, .. } => vec![*dest],
            Instruction::UnaryOp { dest, .. } => vec![*dest],
            Instruction::BinOpChecked { dest, overflow, .. } => vec![*dest, *overflow],
            Instruction::UnaryOpChecked { dest, overflow, .. } => vec![*dest, *overflow],
            Instruction::Widen { dest, .. } => vec![*dest],
            Instruction::WidenFixed { dest, .. } => vec![*dest],
            Instruction::Clone { dest, .. } => vec![*dest],

            // Function calls
            Instruction::Call { dest, .. } => vec![*dest],
            Instruction::ComptimeCall { dest, .. } => vec![*dest],

            // Aggregate construction
            Instruction::Pack { dest, .. } => vec![*dest],
            Instruction::Unpack { dests, .. } => dests.clone(),
            Instruction::GetField { dest, .. } => vec![*dest],
            Instruction::GetFieldRef { dest, .. } => vec![*dest],

            // Option/Result construction
            Instruction::WrapSome { dest, .. } => vec![*dest],
            Instruction::WrapNone { dest, .. } => vec![*dest],
            Instruction::WrapOk { dest, .. } => vec![*dest],
            Instruction::WrapErr { dest, .. } => vec![*dest],

            // Enum construction
            Instruction::EnumVariant { dest, .. } => vec![*dest],
            Instruction::EnumDiscriminant { dest, .. } => vec![*dest],
            Instruction::EnumPayload { dest, .. } => vec![*dest],

            // Option/Result unwrapping (primary dest)
            Instruction::UnwrapOption { dest, is_some, .. } => vec![*dest, *is_some],
            Instruction::UnwrapResult { ok_dest, err_dest, is_ok, .. } => vec![*ok_dest, *err_dest, *is_ok],

            // Boxing operations
            Instruction::ErrorFrom { dest, .. } => vec![*dest],
            Instruction::DataFrom { dest, .. } => vec![*dest],

            // Collection construction
            Instruction::ListNew { dest, .. } => vec![*dest],
            Instruction::SetNew { dest, .. } => vec![*dest],
            Instruction::MapNew { dest, .. } => vec![*dest],
            Instruction::TensorNew { dest, .. } => vec![*dest],
            Instruction::TableNew { dest, .. } => vec![*dest],

            // Slot load operations
            Instruction::SlotLoadCopy { dest, .. } => vec![*dest],
            Instruction::SlotLoadMove { dest, .. } => vec![*dest],
            Instruction::SlotLoadMoveTracked { dest, .. } => vec![*dest],

            // Intrinsics
            Instruction::Intrinsic { dest, .. } => vec![*dest],

            // Slot store operations (no dest)
            Instruction::SlotStoreCopy { .. } => vec![],
            Instruction::SlotStoreCopyTracked { .. } => vec![],
            Instruction::SlotStoreMove { .. } => vec![],
            Instruction::SlotStoreMoveTracked { .. } => vec![],
            Instruction::SetField { .. } => vec![],
            Instruction::SetFieldTracked { .. } => vec![],
            Instruction::ParamStore { .. } => vec![],
            Instruction::ParamStoreTracked { .. } => vec![],
            Instruction::ParamSetField { .. } => vec![],
            Instruction::ParamSetFieldTracked { .. } => vec![],
            Instruction::RefStore { .. } => vec![],
            Instruction::RefStoreTracked { .. } => vec![],
            Instruction::OpAssign { .. } => vec![],
            Instruction::OpAssignChecked { overflow, .. } => vec![*overflow],
            Instruction::RefSetField { .. } => vec![],
            Instruction::RefSetFieldTracked { .. } => vec![],

            // Drop operations (no dest)
            Instruction::Drop { .. } => vec![],
            Instruction::DropTracked { .. } => vec![],
            Instruction::DropViaRef { .. } => vec![],
            Instruction::DropField { .. } => vec![],
            Instruction::UnitEndDrop { .. } => vec![],
            Instruction::UnitEndDropTracked { .. } => vec![],

            // List indexing
            Instruction::ListGet { dest, is_valid, .. } => vec![*dest, *is_valid],
            Instruction::ListBoundsCheck { is_valid, .. } => vec![*is_valid],
            Instruction::ListSet { is_valid, .. } => vec![*is_valid],
            Instruction::ListElementRef { dest, .. } => vec![*dest],

            // Map indexing
            Instruction::MapGet { dest, is_valid, .. } => vec![*dest, *is_valid],
            Instruction::MapContainsKey { is_valid, .. } => vec![*is_valid],
            Instruction::MapSetValue { is_valid, .. } => vec![*is_valid],
            Instruction::MapValueRef { dest, .. } => vec![*dest],
            Instruction::MapUpsert { .. } => vec![],

            // Tensor indexing
            Instruction::TensorGet { dest, is_valid, .. } => vec![*dest, *is_valid],
            Instruction::TensorBoundsCheck { is_valid, .. } => vec![*is_valid],
            Instruction::TensorSet { is_valid, .. } => vec![*is_valid],
            Instruction::TensorIndexRef { dest, .. } => vec![*dest],

            // Cursors
            Instruction::CursorBegin { .. } => vec![],
            Instruction::CursorDone { dest, .. } => vec![*dest],
            Instruction::CursorElem { dest, .. } => vec![*dest],
            Instruction::CursorIndex { dest, .. } => vec![*dest],
            Instruction::CursorValue { dest, .. } => vec![*dest],
            Instruction::CursorAdvance { .. } => vec![],

            // Misc
            Instruction::DebugLog { .. } => vec![],
            Instruction::Nop => vec![],
        }
    }

    /// The value an instruction primarily defines: its only one, or the first
    /// of several, the payload before the flags that say whether it is there.
    /// None for `Unpack`, whose values are alike, and for an instruction that
    /// defines nothing.
    ///
    /// For const inlining, which replaces the instruction that defines a
    /// constant with the constant and gives each flag the value it must have.
    pub fn dest(&self) -> Option<ValueId> {
        match self {
            Instruction::Unpack { .. } => None,
            _ => self.dests().first().copied(),
        }
    }

    /// Visit every operand the instruction names: the ones it reads, and the
    /// ones it writes through (a reference store's destination).
    ///
    /// A destination it defines (`dest`, a `SlotDest`, a parameter it stores
    /// to) is not an operand and is not visited. `DropViaRef`'s reference, a
    /// value the instruction reads, is visited as `Operand::Value`.
    pub fn for_each_operand(&self, mut f: impl FnMut(&Operand)) {
        use Instruction as I;
        match self {
            I::Const { .. } | I::StaticRef { .. } | I::WrapNone { .. } | I::SlotLoadCopy { .. }
            | I::SlotLoadMove { .. } | I::SlotLoadMoveTracked { .. } | I::Nop => {}
            I::Copy { src, .. } | I::Move { src, .. } | I::Widen { src, .. }
            | I::WidenFixed { src, .. } | I::Clone { src, .. } | I::Unpack { src, .. }
            | I::GetField { src, .. } | I::GetFieldRef { src, .. }
            | I::EnumDiscriminant { src, .. } | I::EnumPayload { src, .. }
            | I::UnwrapOption { src, .. } | I::UnwrapResult { src, .. } => f(src),
            I::WrapSome { inner, .. } | I::WrapOk { inner, .. } | I::WrapErr { inner, .. }
            | I::ErrorFrom { inner, .. } | I::DataFrom { inner, .. } => f(inner),
            I::UnaryOp { operand, .. } | I::UnaryOpChecked { operand, .. }
            | I::Drop { operand } | I::DropTracked { operand } | I::UnitEndDrop { operand }
            | I::UnitEndDropTracked { operand } | I::DebugLog { operand } => f(operand),
            I::BinOp { lhs, rhs, .. } | I::BinOpChecked { lhs, rhs, .. } => {
                f(lhs);
                f(rhs);
            }
            I::OpAssign { place, rhs, .. } | I::OpAssignChecked { place, rhs, .. } => {
                f(place);
                f(rhs);
            }
            I::Call { args, .. } | I::ComptimeCall { args, .. } | I::Intrinsic { args, .. } => {
                args.iter().for_each(f)
            }
            I::Pack { fields, .. } => fields.iter().for_each(f),
            I::EnumVariant { payload, .. } => payload.iter().for_each(f),
            I::ListNew { elements, .. } | I::SetNew { elements, .. } | I::TensorNew { elements, .. } => {
                elements.iter().for_each(f)
            }
            I::MapNew { entries, .. } => {
                for (k, v) in entries {
                    f(k);
                    f(v);
                }
            }
            I::TableNew { rows, .. } => rows.iter().for_each(f),
            I::SlotStoreCopy { value, .. } | I::SlotStoreCopyTracked { value, .. }
            | I::SlotStoreMove { value, .. } | I::SlotStoreMoveTracked { value, .. }
            | I::SetField { value, .. } | I::SetFieldTracked { value, .. }
            | I::ParamStore { value, .. } | I::ParamStoreTracked { value, .. }
            | I::ParamSetField { value, .. } | I::ParamSetFieldTracked { value, .. } => f(value),
            I::RefStore { dest, value } | I::RefSetField { dest, value, .. }
            | I::RefStoreTracked { dest, value } | I::RefSetFieldTracked { dest, value, .. } => {
                f(dest);
                f(value);
            }
            I::DropViaRef { ref_value } => f(&Operand::Value(*ref_value)),
            I::DropField { base, .. } => f(base),
            I::ListGet { list, index, .. } | I::ListBoundsCheck { list, index, .. }
            | I::ListElementRef { list, index, .. } => {
                f(list);
                f(index);
            }
            I::ListSet { list, index, value, .. } => {
                f(list);
                f(index);
                f(value);
            }
            I::MapGet { map, key, .. } | I::MapContainsKey { map, key, .. }
            | I::MapValueRef { map, key, .. } => {
                f(map);
                f(key);
            }
            I::MapSetValue { map, key, value, .. } | I::MapUpsert { map, key, value } => {
                f(map);
                f(key);
                f(value);
            }
            I::TensorGet { tensor, index, .. } | I::TensorBoundsCheck { tensor, index, .. }
            | I::TensorIndexRef { tensor, index, .. } => {
                f(tensor);
                f(index);
            }
            I::TensorSet { tensor, index, value, .. } => {
                f(tensor);
                f(index);
                f(value);
            }
            I::CursorBegin { cursor, coll } | I::CursorAdvance { cursor, coll } => {
                f(&Operand::Slot(*cursor));
                f(coll);
            }
            I::CursorDone { cursor, .. } | I::CursorElem { cursor, .. } | I::CursorIndex { cursor, .. }
            | I::CursorValue { cursor, .. } => {
                f(&Operand::Slot(*cursor))
            }
        }
    }
}

impl Terminator {
    /// The blocks control can go on to: none for a return or the end of a
    /// unit, and a block once for each edge to it.
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Terminator::Goto { target, .. } => vec![*target],
            Terminator::Branch { then_block, else_block, .. } => vec![*then_block, *else_block],
            Terminator::Switch { cases, default, .. } => {
                cases.iter().map(|(_, b)| *b).chain(std::iter::once(*default)).collect()
            }
            Terminator::Return { .. } | Terminator::UnitEnd { .. } | Terminator::UnitEarlyReturn { .. } => vec![],
        }
    }

    /// Visit every operand the terminator reads: a condition, a
    /// discriminant, a returned value, and the arguments of every edge.
    pub fn for_each_operand(&self, mut f: impl FnMut(&Operand)) {
        match self {
            Terminator::Goto { args, .. } => args.iter().for_each(f),
            Terminator::Branch { cond, then_args, else_args, .. } => {
                f(cond);
                then_args.iter().for_each(&mut f);
                else_args.iter().for_each(f);
            }
            Terminator::Switch { discriminant, .. } => f(discriminant),
            Terminator::Return { value } | Terminator::UnitEnd { result: value } => value.iter().for_each(f),
            Terminator::UnitEarlyReturn { value } => f(value),
        }
    }
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

/// The runtime's code for an operator applied to a value whose type only a
/// descriptor says.
///
/// `None` for an operator no bound admits. One function, read by every backend,
/// so that what they pass and what the runtime reads cannot drift apart.
pub fn dyn_op_code(op: BinOp) -> Option<datalove_rtdt::DynOp> {
    use datalove_rtdt::DynOp;
    Some(match op {
        BinOp::Add => DynOp::Add,
        BinOp::Sub => DynOp::Sub,
        BinOp::Mul => DynOp::Mul,
        BinOp::Div => DynOp::Div,
        BinOp::Eq => DynOp::Eq,
        BinOp::Ne => DynOp::Ne,
        BinOp::Lt => DynOp::Lt,
        BinOp::Le => DynOp::Le,
        BinOp::Gt => DynOp::Gt,
        BinOp::Ge => DynOp::Ge,
        _ => return None,
    })
}

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
    /// How many type parameters the function was declared with.
    ///
    /// A generic is handed one descriptor per type parameter, after its
    /// ordinary parameters and in this order. The
    /// count comes from the signature, never the body, so a callee's calling
    /// convention does not depend on what its body does.
    #[serde(default)]
    pub type_param_count: u32,
    /// Every dynamic type the body uses, written over this function's type
    /// parameters: the descriptor table.
    ///
    /// A function fills a table of their descriptors on entry -- a bare
    /// `Param(i)` from the descriptor it was handed for it, anything else by
    /// instantiating a template of the type against those -- and any
    /// instruction that needs the descriptor of a dynamic type reads the
    /// entry for it. Built by `descriptor_table`, so that every backend and
    /// every rewrite agrees on the order. Parts come before what holds them.
    #[serde(default)]
    pub descriptor_table: Vec<IrType>,
    /// Whether every call to this function is replaced by its body (`inline`
    /// as the function's first statement); see `inline`. Written only when
    /// set, so that the serialized form of everything else is as it was.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inline: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The descriptor table of a function: every dynamic type it uses.
///
/// Read off the function's parameters, return type, values and slots, and the
/// bindings of the calls it makes -- a callee is handed a descriptor for each
/// of its type parameters, and one bound to a type over this function's is an
/// entry here -- with each type's dynamic parts before it. A projection's
/// result is a value with a type of its own, so the parts the body reaches
/// into are in already.
pub fn descriptor_table(unit: &IrCodeUnit) -> Vec<IrType> {
    let mut table = Vec::new();
    if let CodeUnitContext::Function(ctx) = &unit.context {
        for ty in ctx.param_types.iter().chain(std::iter::once(&ctx.return_type)) {
            ty.dynamic_parts(&mut table);
        }
    }
    for ty in unit.value_types.iter().chain(&unit.slot_types) {
        ty.dynamic_parts(&mut table);
    }
    for block in &unit.blocks {
        for instr in &block.instructions {
            if let Instruction::Call { bindings, .. } | Instruction::ComptimeCall { bindings, .. } = instr {
                for binding in bindings {
                    binding.dynamic_parts(&mut table);
                }
            }
        }
    }
    table
}

/// Whether a unit is a generic, or calls one: either way it takes or hands
/// over a descriptor per type parameter.
pub fn uses_descriptors(unit: &IrCodeUnit) -> bool {
    let generic = unit.function_context().is_some_and(|ctx| ctx.type_param_count > 0);
    generic || unit.blocks.iter().flat_map(|b| &b.instructions).any(|instr| matches!(instr,
        Instruction::Call { bindings, .. } | Instruction::ComptimeCall { bindings, .. }
            if !bindings.is_empty()))
}

/// The first jump in `unit` that passes a block's own parameters back to it
/// in another order, as `(from, to)`.
///
/// Block arguments are a parallel move, and a backend that copies them one at
/// a time reads an argument after it has already written over it if one
/// argument is another position's parameter: in `goto head(b, a)` from `head`
/// with parameters `(a, b)`, copying `b` into `a` loses `a`. The interpreter
/// reads every argument before writing any; cranelift copies an aggregate
/// argument straight into its parameter, which is only right if this never
/// happens, and a dynamic value is always copied that way. Lowering does not
/// emit such a jump, and this is what says so.
pub fn permuted_block_args(unit: &IrCodeUnit) -> Option<(BlockId, BlockId)> {
    let permutes = |target: BlockId, args: &[Operand]| {
        let Some(params) = unit.blocks.iter().find(|b| b.id == target).map(|b| &b.params) else {
            return false;
        };
        args.iter().enumerate().any(|(j, arg)| match arg {
            Operand::Value(v) | Operand::ValueRef(v) => {
                params.iter().enumerate().any(|(i, p)| p == v && i != j)
            }
            _ => false,
        })
    };
    for block in &unit.blocks {
        match &block.terminator {
            Terminator::Goto { target, args } if permutes(*target, args) => {
                return Some((block.id, *target));
            }
            Terminator::Branch { then_block, then_args, else_block, else_args, .. } => {
                if permutes(*then_block, then_args) {
                    return Some((block.id, *then_block));
                }
                if permutes(*else_block, else_args) {
                    return Some((block.id, *else_block));
                }
            }
            _ => {}
        }
    }
    None
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
    /// The binding this unit's result simply names.
    ///
    /// An expression unit that is a bare name is the prompt asking to see a
    /// binding an earlier unit owns. There is nothing to compute and nothing
    /// to copy: the caller reads that binding where it lives.
    #[serde(default)]
    pub result_name: Option<String>,
    /// Bindings exported to subsequent units.
    #[serde(default)]
    pub exports: Vec<(String, ExportBinding)>,
}

/// Context for a native function implemented in Rust.
///
/// Has a signature and linker symbol but no blocks. Built with
/// [`NativeContext::new`], which is what names the symbol: the compiler says
/// which rider function is meant, and how that is spelled to a linker is the
/// native ABI's business, decided here once for every backend.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeContext {
    /// Parameter modes (In, Out, Ref, Mut).
    pub param_modes: Vec<ParamMode>,
    /// Type for each parameter.
    pub param_types: Vec<IrType>,
    /// Return type.
    pub return_type: IrType,
    /// Linker symbol, e.g. "dlr_std__list_push".
    ///
    /// Kept rather than spelled out per call, because the interpreter looks a
    /// native up by it on every call.
    symbol: String,
    /// The type parameters no argument determines, by position, whose
    /// descriptors the call site hands over after the arguments.
    ///
    /// A native reads a type off a descriptor that came with a value. A type
    /// parameter appearing only in the return type has no such value, so the
    /// call site passes a descriptor for what it bound it to. Every other
    /// native has this empty.
    pub undetermined_type_params: Vec<u32>,
}

impl NativeContext {
    /// The native `function` of the rider required as `rider`.
    pub fn new(
        rider: &str,
        function: &str,
        param_modes: Vec<ParamMode>,
        param_types: Vec<IrType>,
        return_type: IrType,
        undetermined_type_params: Vec<u32>,
    ) -> Self {
        Self {
            param_modes,
            param_types,
            return_type,
            symbol: native_symbol(rider, function),
            undetermined_type_params,
        }
    }

    /// What a call binding `bindings` bound each undetermined type parameter
    /// to, in order: the types whose descriptors it hands over.
    pub fn undetermined_bindings(&self, bindings: &[IrType]) -> Vec<IrType> {
        self.undetermined_type_params.iter().map(|&i| bindings[i as usize].clone()).collect()
    }

    /// The linker symbol the native is called by.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
}

/// The linker symbol of the native `function` in the rider required as `rider`.
///
/// `dlr_{rider}__{function}`; see `botdocs/native-abi.md`. A rider defines
/// its functions under these names, and every backend calls them by them.
pub fn native_symbol(rider: &str, function: &str) -> String {
    format!("dlr_{}__{}", rider, function)
}

/// Context determining how a code unit executes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeUnitContext {
    /// A callable function with parameters.
    Function(FunctionContext),
    /// A script unit with captures and exports.
    Script(ScriptContext),
    /// A native function with no blocks.
    Native(NativeContext),
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
    /// Type for each value.
    pub value_types: Vec<IrType>,
    /// Type for each slot.
    pub slot_types: Vec<IrType>,
    /// Slots requiring runtime tracking.
    #[serde(default)]
    pub tracked_slots: Vec<SlotId>,
    /// The value each const binding in this unit is bound to, by name.
    ///
    /// Const inlining writes the evaluated value in where the binding is
    /// defined, looking it up by name, and promotion then makes a function's
    /// non-copy ones statics; see `datalove_datafun_const`.
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

    /// Create a native function code unit (no blocks, just signature + symbol).
    pub fn native(id: CodeUnitId, name: String, ctx: NativeContext) -> Self {
        Self {
            id,
            name,
            blocks: Vec::new(),
            value_count: 0,
            slot_count: 0,
            value_types: Vec::new(),
            slot_types: Vec::new(),
            tracked_slots: Vec::new(),
            const_values: Vec::new(),
            symbols: SymbolTable::default(),
            context: CodeUnitContext::Native(ctx),
            nested_units: Vec::new(),
        }
    }

    /// Get native context if this is a native function.
    pub fn native_context(&self) -> Option<&NativeContext> {
        match &self.context {
            CodeUnitContext::Native(ctx) => Some(ctx),
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

// ============================================================================
// CTFE Memoization Types
// ============================================================================

/// Identifier for a const statement within a script unit.
///
/// Const statements are numbered in source order as they are collected, so
/// the identifier means the same thing to every phase that reads it and
/// keeps meaning it across edits and across runs.
///
/// This used to be a `salsa::Id`, on the grounds that an opaque id avoided
/// coupling the IR to specific AST types. It coupled it to something worse:
/// a salsa id is an index and a generation, it is only meaningful in the
/// revision that minted it, and it renumbers between salsa releases. The IR
/// is what the backends read, and it should not be able to see any of that.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct ConstStmtId(pub u32);

/// Information about a const binding discovered during Phase 1 collection.
///
/// Collected from AST traversal without evaluating expressions.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct ConstBindingInfo {
    /// ID of the const statement in the AST.
    pub stmt_id: ConstStmtId,
    /// Name of the const binding (for error messages and lookup).
    pub name: String,
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
/// Each value is shared rather than owned, so that everything holding a const
/// -- each function it is in scope for, each reference lowered to a
/// `StaticRef` -- holds the one value. A data file's const is as big as the
/// data, and copying it into every one of those was a copy per function.
///
/// This type is hashable for salsa memoization. When the same expressions
/// evaluate to the same values, Phase 3 can use cached results.
/// Uses sorted vectors instead of HashMaps for deterministic hashing.
#[derive(Clone, Debug, Default, Hash, Eq, PartialEq)]
pub struct ResolvedConsts {
    /// Evaluated values sorted by statement ID.
    values: Vec<(ConstStmtId, std::sync::Arc<ConstValue>)>,
    /// Lookup from name to statement ID, sorted by name.
    name_to_stmt: Vec<(String, ConstStmtId)>,
}

impl ResolvedConsts {
    /// Create an empty ResolvedConsts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a const value.
    pub fn insert(&mut self, stmt_id: ConstStmtId, name: String, value: std::sync::Arc<ConstValue>) {
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
    pub fn get_by_name(&self, name: &str) -> Option<&std::sync::Arc<ConstValue>> {
        self.stmt_for_name(name).and_then(|id| self.get(id))
    }

    /// Look up a const value by statement ID.
    pub fn get(&self, stmt_id: ConstStmtId) -> Option<&std::sync::Arc<ConstValue>> {
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
    pub fn iter(&self) -> impl Iterator<Item = (&str, &std::sync::Arc<ConstValue>)> {
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
    pub fn values_iter(&self) -> impl Iterator<Item = (ConstStmtId, &std::sync::Arc<ConstValue>)> {
        self.values.iter().map(|(id, v)| (*id, v))
    }

    /// Iterate over all (name, stmt_id) pairs.
    pub fn names_iter(&self) -> impl Iterator<Item = (&str, ConstStmtId)> {
        self.name_to_stmt.iter().map(|(n, id)| (n.as_str(), *id))
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

#[cfg(test)]
mod const_value_tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_of(value: &ConstValue) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    fn f64c(v: f64) -> ConstValue {
        ConstValue::F64(ConstF64(v))
    }

    #[test]
    fn a_nan_constant_is_the_constant_it_is() {
        // `Eq` promises this, and the `==` operator's relation does not give
        // it: under that one a NaN is not equal to itself, so a code unit
        // holding one would not equal itself either, and salsa could never
        // reuse what it had lowered.
        let nan = f64c(f64::NAN);
        assert_eq!(nan, nan);
        assert_eq!(hash_of(&nan), hash_of(&f64c(f64::NAN)));
    }

    #[test]
    fn the_two_zeros_are_told_apart() {
        // The relation sets and maps keep their keys in distinguishes them,
        // and so does this. The `==` operator does not.
        assert_ne!(f64c(0.0), f64c(-0.0));
        assert_ne!(hash_of(&f64c(0.0)), hash_of(&f64c(-0.0)));
    }

    #[test]
    fn equal_constants_hash_alike() {
        // What a hash map needs of the two impls, over the values where the
        // operator's relation and this one disagree.
        for (a, b) in [
            (f64c(f64::NAN), f64c(f64::NAN)),
            (f64c(0.0), f64c(0.0)),
            (f64c(-0.0), f64c(-0.0)),
            (f64c(f64::INFINITY), f64c(f64::INFINITY)),
        ] {
            assert_eq!(a, b);
            assert_eq!(hash_of(&a), hash_of(&b));
        }
    }

    #[test]
    fn the_rule_reaches_inside_an_aggregate() {
        // The wrapper is why: every variant holding a `ConstValue` inherits it
        // from the derive rather than from an arm someone has to remember.
        let nested = ConstValue::Tuple(vec![f64c(f64::NAN), ConstValue::I32(1)]);
        assert_eq!(nested, nested.clone());

        let plus = ConstValue::OptionSome(Box::new(f64c(0.0)));
        let minus = ConstValue::OptionSome(Box::new(f64c(-0.0)));
        assert_ne!(plus, minus);
    }

    #[test]
    fn a_float_constant_serializes_as_the_float() {
        // `serde(transparent)` on the wrapper, so that what is written down
        // did not change when the relation did.
        let json = rmx::serde_json::to_string(&f64c(1.5)).unwrap();
        assert_eq!(json, r#"{"F64":1.5}"#);
    }
}

#[cfg(test)]
mod dynamic_type_tests {
    use super::*;
    use crate::layout::{dynamic_layout, layout_of};

    fn t(i: u32) -> IrType {
        IrType::Param(i)
    }

    fn list(t: IrType) -> IrType {
        IrType::List(Box::new(t))
    }

    fn option(t: IrType) -> IrType {
        IrType::Option(Box::new(t))
    }

    fn function(param_types: Vec<IrType>, value_types: Vec<IrType>, blocks: Vec<IrBlock>) -> IrCodeUnit {
        IrCodeUnit {
            id: CodeUnitId(0),
            name: "f".to_string(),
            blocks,
            value_count: value_types.len() as u32,
            slot_count: 0,
            value_types,
            slot_types: Vec::new(),
            tracked_slots: Vec::new(),
            const_values: Vec::new(),
            symbols: SymbolTable::new(),
            context: CodeUnitContext::Function(FunctionContext {
                params: (0..param_types.len() as u32).map(ParamId).collect(),
                param_modes: vec![ParamMode::In; param_types.len()],
                param_types,
                return_type: IrType::Unit,
                tracked_params: Vec::new(),
                type_param_count: 1,
                descriptor_table: Vec::new(),
                inline: false,
            }),
            nested_units: Vec::new(),
        }
    }

    fn block(id: u32, params: Vec<u32>, terminator: Terminator) -> IrBlock {
        IrBlock {
            id: BlockId(id),
            params: params.into_iter().map(ValueId).collect(),
            instructions: Vec::new(),
            terminator,
        }
    }

    #[test]
    fn a_type_is_dynamic_when_a_parameter_is_anywhere_in_it() {
        assert!(t(0).is_dynamic());
        assert!(IrType::Tuple(vec![IrType::U32, list(option(t(1)))]).is_dynamic());
        assert!(IrType::Enum(vec![("a".into(), None), ("b".into(), Some(t(0)))]).is_dynamic());
        assert!(IrType::Ref(Box::new(t(0))).is_dynamic());
        assert!(!IrType::Tuple(vec![IrType::U32, list(IrType::String)]).is_dynamic());
        assert!(!IrType::Enum(vec![("a".into(), None)]).is_dynamic());
    }

    #[test]
    fn substituting_binds_each_parameter() {
        let ty = IrType::Map(Box::new(t(0)), Box::new(IrType::Tuple(vec![t(1), t(0)])));
        let bound = ty.substitute(&[IrType::String, list(t(0))]);
        assert_eq!(bound, IrType::Map(
            Box::new(IrType::String),
            Box::new(IrType::Tuple(vec![list(t(0)), IrType::String])),
        ));
    }

    #[test]
    fn a_dynamic_type_is_laid_out_as_a_pointer() {
        let dynamic = dynamic_layout();
        assert_eq!(dynamic.size, std::mem::size_of::<usize>() as u32);
        for ty in [
            t(0),
            IrType::Tuple(vec![t(0), IrType::U32]),
            option(t(0)),
            list(t(0)),
            IrType::Ref(Box::new(t(0))),
        ] {
            assert_eq!(layout_of(&ty), dynamic, "{ty}");
        }
    }

    #[test]
    #[should_panic(expected = "dynamic")]
    fn an_offset_inside_a_dynamic_type_is_refused() {
        let _ = crate::layout::aggregate_field_offsets(&[t(0), IrType::U32]);
    }

    #[test]
    #[should_panic(expected = "dynamic")]
    fn a_payload_offset_of_a_dynamic_type_is_refused() {
        let _ = crate::layout::option_payload_offset(&t(0));
    }

    #[test]
    fn the_table_holds_every_dynamic_type_parts_first() {
        let pair = IrType::Tuple(vec![t(0), IrType::U32]);
        let unit = function(
            vec![list(pair.clone())],
            vec![IrType::U32, IrType::Ref(Box::new(t(0))), option(pair.clone())],
            vec![block(0, vec![], Terminator::Return { value: None })],
        );
        let table = descriptor_table(&unit);
        assert_eq!(table, vec![t(0), pair.clone(), list(pair.clone()), option(pair)]);
    }

    #[test]
    fn a_swapping_jump_is_found() {
        // Block 1 has parameters (v0, v1) and jumps back to itself with them
        // swapped.
        let swapped = function(vec![], vec![IrType::U32, IrType::U32], vec![
            block(0, vec![], Terminator::Return { value: None }),
            block(1, vec![0, 1], Terminator::Goto {
                target: BlockId(1),
                args: vec![Operand::Value(ValueId(1)), Operand::Value(ValueId(0))],
            }),
        ]);
        assert_eq!(permuted_block_args(&swapped), Some((BlockId(1), BlockId(1))));

        // In order, each goes where it already is.
        let in_order = function(vec![], vec![IrType::U32, IrType::U32], vec![
            block(0, vec![], Terminator::Return { value: None }),
            block(1, vec![0, 1], Terminator::Goto {
                target: BlockId(1),
                args: vec![Operand::Value(ValueId(0)), Operand::Value(ValueId(1))],
            }),
        ]);
        assert_eq!(permuted_block_args(&in_order), None);
    }
}
