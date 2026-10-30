//! Type definitions and utilities for datalit typechecking.
//!
//! Organization:
//! 1. Type representation - core types
//! 2. Type error - error type
//! 3. Type predicates - type queries
//! 4. Type equivalence - comparing types
//! 5. Type hint conversion - AST to types
//! 6. Type to string - types to strings for errors
//! 7. Unit and shape helpers - type constructors
//! 8. Integer range checking - literal validation
//! 9. Diagnostic helpers - error message helpers

use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::{TypeHint, TypeHintAlias};
use crate::type_syntax::TypeSyntax;
use std::borrow::Cow;

// ============================================================================
// Type Representation
// ============================================================================

/// Type representation (synthesized types, mirrors TypeHint but without parse errors).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum Type<'db> {
    Bool,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    Index,
    Offset,
    F32,
    F64,
    Int,
    String,
    AnonTuple(TypeAnonTuple<'db>),
    AnonStruct(TypeAnonStruct<'db>),

    List(TypeList<'db>),
    Map(TypeMap<'db>),
    Set(TypeSet<'db>),
    Option(TypeOption<'db>),
    Result(TypeResult<'db>),
    Tensor(TypeTensor<'db>),
    Table(TypeTable<'db>),
    Data,
    Error,
    Atom(TypeAtom<'db>),
    Term(TypeTerm<'db>),
    Enum(TypeEnum<'db>),
    /// A type parameter of the enclosing generic function.
    Var(InternedText<'db>),
    /// `rec Name T`: `T`, in which [`Type::RecVar`] refers back to the whole.
    ///
    /// Recursive types are iso-recursive: a `rec` type is not the type its
    /// body unfolds to, though the two have the same layout. See
    /// `botdocs/plan-recursive-types.md`.
    Rec(TypeRec<'db>),
    /// A use of an enclosing `rec` type's name, by de Bruijn index: 0 is the
    /// innermost binder. Only a `rec` body holds one; a type taken out of a
    /// body has been unfolded, and is closed.
    RecVar(u32),
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeAnonTuple<'db> {
    pub fields: Vec<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeAnonStruct<'db> {
    pub fields: Vec<TypeNamedField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeNamedField<'db> {
    pub name: InternedText<'db>,
    pub ty: Box<Type<'db>>,
}


#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeRec<'db> {
    pub name: RecName<'db>,
    pub body: Box<Type<'db>>,
}

/// The name a `rec` type was written with, for showing it.
///
/// The name is no part of the type: `rec A [A]` and `rec B [B]` are one
/// type, so every two of these are equal and none hashes to anything. A memo
/// whose only change is the name can then be backdated, and a diagnostic show
/// the old one.
#[derive(Clone, Copy)]
#[derive(salsa::SalsaValue)]
pub struct RecName<'db>(pub InternedText<'db>);

impl PartialEq for RecName<'_> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for RecName<'_> {}

impl std::hash::Hash for RecName<'_> {
    fn hash<H: std::hash::Hasher>(&self, _state: &mut H) {}
}

impl<'db> TypeRec<'db> {
    /// The body with the whole put back where its name was: the type a value
    /// of this one is, one level down.
    pub fn unfold(&self) -> Type<'db> {
        let whole = Type::Rec(self.clone());
        substitute(&self.body, 0, &whole)
    }
}

/// `ty` with the binder `depth` levels out replaced by `whole`, which is
/// closed.
fn substitute<'db>(ty: &Type<'db>, depth: u32, whole: &Type<'db>) -> Type<'db> {
    let sub = |t: &Type<'db>| Box::new(substitute(t, depth, whole));
    let fields = |fs: &[TypeNamedField<'db>]| fs.iter()
        .map(|f| TypeNamedField { name: f.name, ty: sub(&f.ty) })
        .collect();
    match ty {
        Type::RecVar(index) if *index == depth => whole.clone(),
        Type::RecVar(_) => ty.clone(),
        Type::Rec(r) => Type::Rec(TypeRec { name: r.name, body: Box::new(substitute(&r.body, depth + 1, whole)) }),
        Type::AnonTuple(t) => Type::AnonTuple(TypeAnonTuple { fields: t.fields.iter().map(|f| substitute(f, depth, whole)).collect() }),
        Type::AnonStruct(s) => Type::AnonStruct(TypeAnonStruct { fields: fields(&s.fields) }),
        Type::Table(t) => Type::Table(TypeTable { columns: fields(&t.columns) }),
        Type::List(l) => Type::List(TypeList { element_type: sub(&l.element_type) }),
        Type::Set(s) => Type::Set(TypeSet { element_type: sub(&s.element_type) }),
        Type::Map(m) => Type::Map(TypeMap { key_type: sub(&m.key_type), value_type: sub(&m.value_type) }),
        Type::Option(o) => Type::Option(TypeOption { inner_type: sub(&o.inner_type) }),
        Type::Result(r) => Type::Result(TypeResult { inner_type: sub(&r.inner_type) }),
        Type::Tensor(t) => Type::Tensor(TypeTensor { element_type: sub(&t.element_type), rank: t.rank }),
        Type::Term(t) => Type::Term(TypeTerm { name: t.name, payload: sub(&t.payload) }),
        Type::Enum(e) => Type::Enum(TypeEnum {
            variants: e.variants.iter()
                .map(|v| TypeEnumVariant { name: v.name, payload: v.payload.as_deref().map(sub) })
                .collect(),
        }),
        Type::Bool | Type::U8 | Type::I8 | Type::U16 | Type::I16 | Type::U32 | Type::I32
        | Type::U64 | Type::I64 | Type::Index | Type::Offset | Type::F32 | Type::F64
        | Type::Int | Type::String | Type::Data | Type::Error | Type::Atom(_) | Type::Var(_) => ty.clone(),
    }
}

/// Whether a recursive type is anywhere in a type.
pub fn contains_rec(ty: &Type<'_>) -> bool {
    let named = |fs: &[TypeNamedField<'_>]| fs.iter().any(|f| contains_rec(&f.ty));
    match ty {
        Type::Rec(_) | Type::RecVar(_) => true,
        Type::AnonTuple(t) => t.fields.iter().any(contains_rec),
        Type::AnonStruct(s) => named(&s.fields),
        Type::Table(t) => named(&t.columns),
        Type::List(l) => contains_rec(&l.element_type),
        Type::Set(s) => contains_rec(&s.element_type),
        Type::Map(m) => contains_rec(&m.key_type) || contains_rec(&m.value_type),
        Type::Option(o) => contains_rec(&o.inner_type),
        Type::Result(r) => contains_rec(&r.inner_type),
        Type::Tensor(t) => contains_rec(&t.element_type),
        Type::Term(t) => contains_rec(&t.payload),
        Type::Enum(e) => e.variants.iter().any(|v| v.payload.as_deref().is_some_and(contains_rec)),
        Type::Bool | Type::U8 | Type::I8 | Type::U16 | Type::I16 | Type::U32 | Type::I32
        | Type::U64 | Type::I64 | Type::Index | Type::Offset | Type::F32 | Type::F64
        | Type::Int | Type::String | Type::Data | Type::Error | Type::Atom(_) | Type::Var(_) => false,
    }
}

/// A type with any `rec` at its head unfolded, so that its head is what a
/// value of it is made of. Every place that takes a value apart by its type
/// looks through this.
pub fn expose<'t, 'db>(ty: &'t Type<'db>) -> Cow<'t, Type<'db>> {
    match ty {
        Type::Rec(r) => {
            let mut unfolded = r.unfold();
            while let Type::Rec(inner) = unfolded {
                unfolded = inner.unfold();
            }
            Cow::Owned(unfolded)
        }
        _ => Cow::Borrowed(ty),
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeList<'db> {
    pub element_type: Box<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeMap<'db> {
    pub key_type: Box<Type<'db>>,
    pub value_type: Box<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeSet<'db> {
    pub element_type: Box<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeOption<'db> {
    pub inner_type: Box<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeResult<'db> {
    pub inner_type: Box<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeTensor<'db> {
    pub element_type: Box<Type<'db>>,
    pub rank: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeTable<'db> {
    pub columns: Vec<TypeNamedField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeAtom<'db> {
    pub name: InternedText<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeTerm<'db> {
    pub name: InternedText<'db>,
    pub payload: Box<Type<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeEnum<'db> {
    pub variants: Vec<TypeEnumVariant<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct TypeEnumVariant<'db> {
    pub name: InternedText<'db>,
    pub payload: Option<Box<Type<'db>>>,
}

// ============================================================================
// Type Error
// ============================================================================

/// Type error representation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum TypeError {
    TypeMismatch { expected: String, actual: String },
    CannotSynthesize,
    MissingField(String),
    ExtraField(String),
    FieldOrderMismatch,
    IntOutOfRange,
    VariantNotFound(String),
    ArityMismatch { expected: usize, actual: usize },
    /// A recursive type that is not well formed.
    Rec(RecError),
}

/// What is wrong with a recursive type, and the name it is wrong about.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct RecError {
    pub kind: RecErrorKind,
    pub name: String,
    /// The name's position among the names in type position its parse
    /// filed: the binder's, or the use's.
    pub local_index: Option<u32>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum RecErrorKind {
    /// A `rec` whose name its body never uses.
    Unused,
    /// A `rec` name that is already the name of an enclosing `rec`, or of an
    /// alias or type parameter.
    Shadowed,
    /// A use of a `rec` name with no collection between it and its binder,
    /// which would make the type infinitely large.
    Unguarded,
    /// A recursive type in a map's key or a set's element.
    InKey,
    /// A type parameter inside a `rec` body.
    Generic,
}

impl RecErrorKind {
    /// The diagnostic code, a label for the name, and a note saying why.
    pub fn diagnostic(self) -> (&'static str, &'static str, &'static str) {
        match self {
            RecErrorKind::Unused => ("T062", "never uses its name",
                "a `rec` type is one whose body names it; without a use, write the body alone"),
            RecErrorKind::Shadowed => ("T063", "the name is taken",
                "a `rec` takes a name no enclosing `rec`, alias or type parameter has"),
            RecErrorKind::Unguarded => ("T064", "recurs inline",
                "a recursive type recurs inside a list, map, set or table, so that its values have a size"),
            RecErrorKind::InKey => ("T065", "recursive type in a key",
                "a map's key or a set's element cannot be a recursive type"),
            RecErrorKind::Generic => ("T066", "a type parameter",
                "a recursive type cannot hold a type parameter yet"),
        }
    }
}

impl std::fmt::Display for RecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = &self.name;
        match self.kind {
            RecErrorKind::Unused => write!(f, "`rec {name}` never uses `{name}`"),
            RecErrorKind::Shadowed => write!(f, "`{name}` already names a type here"),
            RecErrorKind::Unguarded => write!(f, "`{name}` recurs without a collection between it and its `rec`"),
            RecErrorKind::InKey => write!(f, "the recursive type `{name}` is a map's key or a set's element"),
            RecErrorKind::Generic => write!(f, "the type parameter `{name}` is inside a recursive type"),
        }
    }
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::TypeMismatch { expected, actual } => {
                write!(f, "mismatched types: expected `{expected}`, found `{actual}`")
            }
            TypeError::CannotSynthesize => write!(f, "cannot infer a type here"),
            TypeError::MissingField(name) => write!(f, "missing field `{name}`"),
            TypeError::ExtraField(name) => write!(f, "unexpected field `{name}`"),
            TypeError::FieldOrderMismatch => write!(f, "the fields are not in the order the type has them"),
            TypeError::IntOutOfRange => write!(f, "integer literal out of range for its type"),
            TypeError::VariantNotFound(name) => write!(f, "no variant `{name}`"),
            TypeError::ArityMismatch { expected, actual } => write!(f, "expected {expected} elements, found {actual}"),
            TypeError::Rec(error) => write!(f, "{error}"),
        }
    }
}

// ============================================================================
// Type Predicates
// ============================================================================

/// Check if a type is numeric (any integer or float type).
pub fn is_numeric_type(ty: &Type<'_>) -> bool {
    matches!(
        ty,
        Type::U8 | Type::I8 |
        Type::U16 | Type::I16 |
        Type::U32 | Type::I32 |
        Type::U64 | Type::I64 |
        Type::Index | Type::Offset |
        Type::F32 | Type::F64 |
        Type::Int
    )
}

/// Check if a type is a floating-point type.
pub fn is_float_type(ty: &Type<'_>) -> bool {
    matches!(ty, Type::F32 | Type::F64)
}

/// Check if a type is the arbitrary-precision integer type.
pub fn is_bigint_type(ty: &Type<'_>) -> bool {
    matches!(ty, Type::Int)
}

/// Check if a type is a fixed-size integer type.
pub fn is_fixed_int_type(ty: &Type<'_>) -> bool {
    matches!(
        ty,
        Type::U8 | Type::I8 |
        Type::U16 | Type::I16 |
        Type::U32 | Type::I32 |
        Type::U64 | Type::I64 |
        Type::Index | Type::Offset
    )
}

/// Check if a type is an unsigned integer type.
pub fn is_unsigned_int_type(ty: &Type<'_>) -> bool {
    matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::Index)
}

/// Check if a type is boolean.
pub fn is_bool_type(ty: &Type<'_>) -> bool {
    matches!(ty, Type::Bool)
}

// ============================================================================
// Type Equivalence
// ============================================================================

/// Check if two types are equivalent.
pub fn types_equivalent<'db>(db: &'db dyn crate::Db, t1: &Type<'db>, t2: &Type<'db>) -> bool {
    match (t1, t2) {
        (Type::Bool, Type::Bool) => true,
        (Type::U8, Type::U8) => true,
        (Type::I8, Type::I8) => true,
        (Type::U16, Type::U16) => true,
        (Type::I16, Type::I16) => true,
        (Type::U32, Type::U32) => true,
        (Type::I32, Type::I32) => true,
        (Type::U64, Type::U64) => true,
        (Type::I64, Type::I64) => true,
        (Type::Index, Type::Index) => true,
        (Type::Offset, Type::Offset) => true,
        (Type::F32, Type::F32) => true,
        (Type::F64, Type::F64) => true,
        (Type::Int, Type::Int) => true,
        (Type::String, Type::String) => true,
        (Type::Data, Type::Data) => true,

        // Two type parameters are the same type only when they are the same
        // parameter. Nothing else is equivalent to one.
        (Type::Var(a), Type::Var(b)) => a == b,
        (Type::Error, Type::Error) => true,

        // The names are not compared: a de Bruijn index says which binder a
        // use is of, so two bodies that agree are the same type.
        (Type::Rec(r1), Type::Rec(r2)) => types_equivalent(db, &r1.body, &r2.body),
        (Type::RecVar(a), Type::RecVar(b)) => a == b,

        (Type::AnonTuple(t1), Type::AnonTuple(t2)) => {
            let f1 = t1.fields.C();
            let f2 = t2.fields.C();
            f1.len() == f2.len()
                && f1
                    .iter()
                    .zip(f2.iter())
                    .all(|(a, b)| types_equivalent(db, a, b))
        }

        (Type::AnonStruct(s1), Type::AnonStruct(s2)) => {
            let f1 = s1.fields.C();
            let f2 = s2.fields.C();
            f1.len() == f2.len()
                && f1.iter().zip(f2.iter()).all(|(a, b)| {
                    a.name == b.name && types_equivalent(db, &a.ty, &b.ty)
                })
        }


        (Type::List(l1), Type::List(l2)) => {
            types_equivalent(db, &l1.element_type, &l2.element_type)
        }

        (Type::Map(m1), Type::Map(m2)) => {
            types_equivalent(db, &m1.key_type, &m2.key_type)
                && types_equivalent(db, &m1.value_type, &m2.value_type)
        }

        (Type::Set(s1), Type::Set(s2)) => {
            types_equivalent(db, &s1.element_type, &s2.element_type)
        }

        (Type::Option(o1), Type::Option(o2)) => {
            types_equivalent(db, &o1.inner_type, &o2.inner_type)
        }

        (Type::Result(r1), Type::Result(r2)) => {
            types_equivalent(db, &r1.inner_type, &r2.inner_type)
        }

        (Type::Tensor(t1), Type::Tensor(t2)) => {
            t1.rank == t2.rank
                && types_equivalent(db, &t1.element_type, &t2.element_type)
        }

        (Type::Table(t1), Type::Table(t2)) => {
            let c1 = t1.columns.C();
            let c2 = t2.columns.C();
            c1.len() == c2.len()
                && c1.iter().zip(c2.iter()).all(|(a, b)| {
                    a.name == b.name && types_equivalent(db, &a.ty, &b.ty)
                })
        }

        (Type::Atom(a1), Type::Atom(a2)) => a1.name == a2.name,

        (Type::Term(t1), Type::Term(t2)) => {
            t1.name == t2.name && types_equivalent(db, &t1.payload, &t2.payload)
        }

        (Type::Enum(e1), Type::Enum(e2)) => {
            e1.variants.len() == e2.variants.len()
                && e1.variants.iter().zip(e2.variants.iter()).all(|(a, b)| {
                    a.name == b.name
                        && match (&a.payload, &b.payload) {
                            (None, None) => true,
                            (Some(p1), Some(p2)) => types_equivalent(db, p1, p2),
                            _ => false,
                        }
                })
        }

        _ => false,
    }
}

/// Whether a value of `actual` may stand where `expected` is wanted: the two
/// are equivalent, or are once a recursive type at the head of one is
/// unfolded.
///
/// A recursive type and its unfolding mean the same thing and have one
/// layout, and are told apart only so that equivalence stays syntactic. Where
/// a value meets the type expected of it, they meet at the head: a `rec T [?T]`
/// is a `[?(rec T [?T])]`, and the other way round. Inside a type they stay
/// apart, so `[rec T [?T]]` is not `[[?(rec T [?T])]]`.
pub fn types_equivalent_at_head<'db>(db: &'db dyn crate::Db, actual: &Type<'db>, expected: &Type<'db>) -> bool {
    types_equivalent(db, actual, expected)
        || (matches!(actual, Type::Rec(_)) && types_equivalent(db, &expose(actual), expected))
        || (matches!(expected, Type::Rec(_)) && types_equivalent(db, actual, &expose(expected)))
}

/// Check if a type can widen to another type.
///
/// Supports numeric widening chains:
/// - u8 -> u16 -> u32 -> u64 -> int
/// - i8 -> i16 -> i32 -> i64 -> int
pub fn can_widen_to<'db>(from: &Type<'db>, to: &Type<'db>) -> bool {
    match (from, to) {
        (Type::U8, Type::U16 | Type::U32 | Type::U64 | Type::Int) => true,
        (Type::U16, Type::U32 | Type::U64 | Type::Int) => true,
        (Type::U32, Type::U64 | Type::Int) => true,
        (Type::U64, Type::Int) => true,
        (Type::I8, Type::I16 | Type::I32 | Type::I64 | Type::Int) => true,
        (Type::I16, Type::I32 | Type::I64 | Type::Int) => true,
        (Type::I32, Type::I64 | Type::Int) => true,
        (Type::I64, Type::Int) => true,
        (Type::Index, Type::Int) => true,
        (Type::Offset, Type::Int) => true,
        _ => false,
    }
}

/// Whether `@` converts a value of one type to another.
///
/// It clones a value to its own type, or to one equivalent at the head (see
/// `types_equivalent_at_head`), widens an integer to one that holds every value
/// of it, widens `f32` to `f64`, and makes an atom or term a value of an enum
/// that lists it. Nothing else: no narrowing, and no enum adapts to
/// another.
pub fn can_adapt_to<'db>(db: &'db dyn crate::Db, from: &Type<'db>, to: &Type<'db>) -> bool {
    if types_equivalent_at_head(db, from, to) || can_widen_to(from, to) {
        return true;
    }
    match (from, to) {
        // An unsigned integer widens to any wider signed one.
        (Type::U8, Type::I16 | Type::I32 | Type::I64 | Type::Int) => true,
        (Type::U16, Type::I32 | Type::I64 | Type::Int) => true,
        (Type::U32, Type::I64 | Type::Int) => true,
        // Every f32 is exactly an f64.
        (Type::F32, Type::F64) => true,
        (Type::Atom(a), Type::Enum(e)) => {
            e.variants.iter().any(|v| v.name == a.name && v.payload.is_none())
        }
        (Type::Term(t), Type::Enum(e)) => e.variants.iter().any(|v| {
            v.name == t.name
                && v.payload.as_ref().is_some_and(|p| types_equivalent(db, &t.payload, p))
        }),
        _ => false,
    }
}

// ============================================================================
// Type Hint Conversion
// ============================================================================

/// Convert a type hint to a type.
pub fn convert_type_hint<'db>(
    db: &'db dyn crate::Db,
    type_hint: &TypeHint<'db>,
) -> Result<Type<'db>, TypeError> {
    // Datalit has no aliases: a name in a type is a `rec`'s, or nothing.
    convert_type_hint_with(db, type_hint, &NoAliases)
}

/// What the bare names in a hint that no `rec` binds resolve to.
pub trait TypeNames<'db, E> {
    /// The type an alias or type parameter names, or why it names none.
    fn resolve(&self, alias: &TypeHintAlias<'db>) -> Result<Type<'db>, E>;
    /// Whether a name is taken by an alias or a type parameter, which a
    /// `rec` may not take as well.
    fn is_named(&self, name: InternedText<'db>) -> bool;
}

struct NoAliases;

impl<'db> TypeNames<'db, TypeError> for NoAliases {
    fn resolve(&self, _alias: &TypeHintAlias<'db>) -> Result<Type<'db>, TypeError> {
        Err(TypeError::CannotSynthesize)
    }
    fn is_named(&self, _name: InternedText<'db>) -> bool {
        false
    }
}

/// Convert a type hint to a type, resolving the names no `rec` binds with
/// `names`.
pub fn convert_type_hint_with<'db, E: From<TypeError>>(
    db: &'db dyn crate::Db,
    type_hint: &TypeHint<'db>,
    names: &dyn TypeNames<'db, E>,
) -> Result<Type<'db>, E> {
    convert_in(db, type_hint, &mut RecScope::default(), names)
}

/// The `rec` binders around a part of a hint being converted, and what is
/// between them and it.
#[derive(Default)]
struct RecScope<'db> {
    /// Innermost last.
    binders: Vec<RecBinder<'db>>,
    /// How many collections the part is inside.
    collections: u32,
    /// Whether the part is in a map's key or a set's element.
    in_key: bool,
}

struct RecBinder<'db> {
    name: InternedText<'db>,
    local_index: Option<u32>,
    /// `RecScope::collections` at the binder.
    collections: u32,
    used: bool,
}

fn rec_error(db: &dyn crate::Db, kind: RecErrorKind, name: InternedText<'_>, local_index: Option<u32>) -> TypeError {
    TypeError::Rec(RecError { kind, name: name.as_str(db).to_string(), local_index })
}

impl<'db> RecScope<'db> {
    /// Enter the body of `rec name`.
    fn bind(&mut self, db: &'db dyn crate::Db, rec: &crate::ast::TypeHintRec<'db>, taken: bool) -> Result<(), TypeError> {
        let error = |kind| rec_error(db, kind, rec.name, rec.local_index);
        if self.in_key {
            return Err(error(RecErrorKind::InKey));
        }
        if taken || self.binders.iter().any(|b| b.name == rec.name) {
            return Err(error(RecErrorKind::Shadowed));
        }
        self.binders.push(RecBinder {
            name: rec.name,
            local_index: rec.local_index,
            collections: self.collections,
            used: false,
        });
        Ok(())
    }

    /// Leave the body of the innermost `rec`.
    fn unbind(&mut self, db: &'db dyn crate::Db) -> Result<(), TypeError> {
        let binder = self.binders.pop().expect("unbind follows bind");
        if !binder.used {
            return Err(rec_error(db, RecErrorKind::Unused, binder.name, binder.local_index));
        }
        Ok(())
    }

    /// The use of a `rec` name, if the alias is one in scope.
    fn lookup(&mut self, db: &'db dyn crate::Db, alias: &TypeHintAlias<'db>) -> Option<Result<Type<'db>, TypeError>> {
        let depth = self.binders.iter().rev().position(|b| b.name == alias.name)?;
        let collections = self.collections;
        let in_key = self.in_key;
        let binder = self.binders.iter_mut().rev().nth(depth).expect("found above");
        binder.used = true;
        let error = |kind| rec_error(db, kind, alias.name, alias.local_index);
        Some(if in_key {
            Err(error(RecErrorKind::InKey))
        } else if collections <= binder.collections {
            Err(error(RecErrorKind::Unguarded))
        } else {
            Ok(Type::RecVar(depth as u32))
        })
    }

    /// Check what an alias or type parameter names against where it is used.
    fn check_named(&self, db: &'db dyn crate::Db, alias: &TypeHintAlias<'db>, ty: &Type<'db>) -> Result<(), TypeError> {
        let error = |kind| rec_error(db, kind, alias.name, alias.local_index);
        if self.in_key && contains_rec(ty) {
            return Err(error(RecErrorKind::InKey));
        }
        if !self.binders.is_empty() && contains_var(ty) {
            return Err(error(RecErrorKind::Generic));
        }
        Ok(())
    }

    /// Run `f` inside a collection, and inside a key or element that is
    /// ordered if `key`.
    fn collection<T>(&mut self, key: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let was_in_key = self.in_key;
        self.collections += 1;
        self.in_key |= key;
        let result = f(self);
        self.collections -= 1;
        self.in_key = was_in_key;
        result
    }
}

/// Whether a type parameter is anywhere in a type.
fn contains_var(ty: &Type<'_>) -> bool {
    let named = |fs: &[TypeNamedField<'_>]| fs.iter().any(|f| contains_var(&f.ty));
    match ty {
        Type::Var(_) => true,
        Type::Rec(r) => contains_var(&r.body),
        Type::AnonTuple(t) => t.fields.iter().any(contains_var),
        Type::AnonStruct(s) => named(&s.fields),
        Type::Table(t) => named(&t.columns),
        Type::List(l) => contains_var(&l.element_type),
        Type::Set(s) => contains_var(&s.element_type),
        Type::Map(m) => contains_var(&m.key_type) || contains_var(&m.value_type),
        Type::Option(o) => contains_var(&o.inner_type),
        Type::Result(r) => contains_var(&r.inner_type),
        Type::Tensor(t) => contains_var(&t.element_type),
        Type::Term(t) => contains_var(&t.payload),
        Type::Enum(e) => e.variants.iter().any(|v| v.payload.as_deref().is_some_and(contains_var)),
        Type::Bool | Type::U8 | Type::I8 | Type::U16 | Type::I16 | Type::U32 | Type::I32
        | Type::U64 | Type::I64 | Type::Index | Type::Offset | Type::F32 | Type::F64
        | Type::Int | Type::String | Type::Data | Type::Error | Type::Atom(_) | Type::RecVar(_) => false,
    }
}

fn convert_in<'db, E: From<TypeError>>(
    db: &'db dyn crate::Db,
    type_hint: &TypeHint<'db>,
    scope: &mut RecScope<'db>,
    names: &dyn TypeNames<'db, E>,
) -> Result<Type<'db>, E> {
    let ty = match type_hint {
        TypeHint::Bool => Type::Bool,
        TypeHint::U8 => Type::U8,
        TypeHint::I8 => Type::I8,
        TypeHint::U16 => Type::U16,
        TypeHint::I16 => Type::I16,
        TypeHint::U32 => Type::U32,
        TypeHint::I32 => Type::I32,
        TypeHint::U64 => Type::U64,
        TypeHint::I64 => Type::I64,
        TypeHint::Index => Type::Index,
        TypeHint::Offset => Type::Offset,
        TypeHint::F32 => Type::F32,
        TypeHint::F64 => Type::F64,
        TypeHint::Int => Type::Int,
        TypeHint::String => Type::String,
        TypeHint::Data => Type::Data,
        TypeHint::Error => Type::Error,

        TypeHint::AnonTuple(t) => {
            let fields: Result<Vec<_>, E> = t
                .fields
                .iter()
                .map(|f| convert_in(db, f, scope, names))
                .collect();
            Type::AnonTuple(TypeAnonTuple { fields: fields? })
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, E> = s
                .fields
                .iter()
                .map(|f| {
                    let name = f.name;
                    let ty = convert_in(db, &f.type_hint, scope, names)?;
                    Ok(TypeNamedField { name, ty: Box::new(ty) })
                })
                .collect();
            Type::AnonStruct(TypeAnonStruct { fields: fields? })
        }


        TypeHint::List(l) => {
            let element_type = scope.collection(false, |scope| convert_in(db, &l.element_type, scope, names))?;
            Type::List(TypeList { element_type: Box::new(element_type) })
        }

        TypeHint::Map(m) => {
            let key_type = scope.collection(true, |scope| convert_in(db, &m.key_type, scope, names))?;
            let value_type = scope.collection(false, |scope| convert_in(db, &m.value_type, scope, names))?;
            Type::Map(TypeMap { key_type: Box::new(key_type), value_type: Box::new(value_type) })
        }

        TypeHint::Set(s) => {
            let element_type = scope.collection(true, |scope| convert_in(db, &s.element_type, scope, names))?;
            Type::Set(TypeSet { element_type: Box::new(element_type) })
        }

        TypeHint::Option(o) => {
            let inner_type = convert_in(db, &o.inner_type, scope, names)?;
            Type::Option(TypeOption { inner_type: Box::new(inner_type) })
        }

        TypeHint::Result(r) => {
            let inner_type = convert_in(db, &r.inner_type, scope, names)?;
            Type::Result(TypeResult { inner_type: Box::new(inner_type) })
        }

        TypeHint::Tensor(t) => {
            let element_type = scope.collection(false, |scope| convert_in(db, &t.element_type, scope, names))?;
            Type::Tensor(TypeTensor { element_type: Box::new(element_type), rank: t.rank })
        }

        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, E> = t
                .columns
                .iter()
                .map(|c| {
                    let name = c.name;
                    let ty = scope.collection(false, |scope| convert_in(db, &c.type_hint, scope, names))?;
                    Ok(TypeNamedField { name, ty: Box::new(ty) })
                })
                .collect();
            Type::Table(TypeTable { columns: columns? })
        }

        TypeHint::Atom(a) => {
            Type::Atom(TypeAtom { name: a.name })
        }

        TypeHint::Term(t) => {
            let payload = convert_in(db, &t.payload, scope, names)?;
            Type::Term(TypeTerm { name: t.name, payload: Box::new(payload) })
        }

        TypeHint::Enum(e) => {
            let variants: Result<Vec<_>, E> = e.variants.iter()
                .map(|v| {
                    let payload = match &v.payload {
                        Some(p) => Some(Box::new(convert_in(db, p, scope, names)?)),
                        None => None,
                    };
                    Ok(TypeEnumVariant { name: v.name, payload })
                })
                .collect();
            let mut variants = variants?;
            variants.sort_by(|a, b| a.name.as_str(db).cmp(b.name.as_str(db)));
            Type::Enum(TypeEnum { variants })
        }

        TypeHint::Rec(r) => {
            scope.bind(db, r, names.is_named(r.name))?;
            let body = convert_in(db, &r.body, scope, names);
            // What is wrong in the body is the thing to say, and a name it
            // never reached is a consequence of it.
            let unbound = scope.unbind(db);
            let body = body?;
            unbound?;
            Type::Rec(TypeRec { name: RecName(r.name), body: Box::new(body) })
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize.into()),
        TypeHint::Alias(alias) => match scope.lookup(db, alias) {
            Some(use_) => use_?,
            None => {
                let ty = names.resolve(alias)?;
                scope.check_named(db, alias, &ty)?;
                ty
            }
        },
    };

    Ok(ty)
}

// ============================================================================
// Type to String
// ============================================================================

/// A literal that leaves its type open, with nothing around it to decide it.
///
/// Datalit and datafun report the same literals the same way, from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenLiteral {
    None,
    Er,
    Enum,
    List,
    Set,
    Map,
    Tensor,
    Table,
}

impl OpenLiteral {
    /// What could not be decided about it.
    pub fn message(self) -> &'static str {
        match self {
            OpenLiteral::None => "cannot infer what `none` is an option of",
            OpenLiteral::Er => "cannot infer what `er` is a result of",
            OpenLiteral::Enum => "cannot infer which enum this literal is of",
            OpenLiteral::List => "cannot infer what an empty list is a list of",
            OpenLiteral::Set => "cannot infer what an empty set is a set of",
            OpenLiteral::Map => "cannot infer the key and value types of an empty map",
            OpenLiteral::Tensor => "cannot infer what an empty tensor is a tensor of",
            OpenLiteral::Table => "cannot infer the column types of a table with no rows",
        }
    }
}

/// A type as datalove writes it, for error messages and anything else a
/// reader sees.
pub fn type_to_string<'db>(db: &'db dyn crate::Db, ty: &Type<'db>) -> String {
    ty.syntax(db).to_string()
}

impl<'db> Type<'db> {
    /// This type, shaped as datalove writes it; see [`crate::type_syntax`].
    pub fn syntax(&self, db: &'db dyn crate::Db) -> TypeSyntax<'db> {
        self.syntax_under(db, &mut Vec::new())
    }

    /// The syntax of a type under the `rec` binders named in `binders`,
    /// innermost last.
    fn syntax_under(&self, db: &'db dyn crate::Db, binders: &mut Vec<&'db str>) -> TypeSyntax<'db> {
        fn boxed<'db>(ty: &Type<'db>, db: &'db dyn crate::Db, binders: &mut Vec<&'db str>) -> Box<TypeSyntax<'db>> {
            Box::new(ty.syntax_under(db, binders))
        }
        fn fields<'db>(
            fields: &[TypeNamedField<'db>],
            db: &'db dyn crate::Db,
            binders: &mut Vec<&'db str>,
        ) -> Vec<(Cow<'db, str>, TypeSyntax<'db>)> {
            fields.iter()
                .map(|field| (Cow::Borrowed(field.name.as_str(db)), field.ty.syntax_under(db, binders)))
                .collect()
        }
        match self {
            Type::Bool => TypeSyntax::word("bool"),
            Type::U8 => TypeSyntax::word("u8"),
            Type::I8 => TypeSyntax::word("i8"),
            Type::U16 => TypeSyntax::word("u16"),
            Type::I16 => TypeSyntax::word("i16"),
            Type::U32 => TypeSyntax::word("u32"),
            Type::I32 => TypeSyntax::word("i32"),
            Type::U64 => TypeSyntax::word("u64"),
            Type::I64 => TypeSyntax::word("i64"),
            Type::Index => TypeSyntax::word("index"),
            Type::Offset => TypeSyntax::word("offset"),
            Type::F32 => TypeSyntax::word("f32"),
            Type::F64 => TypeSyntax::word("f64"),
            Type::Int => TypeSyntax::word("int"),
            Type::String => TypeSyntax::word("string"),
            Type::Data => TypeSyntax::word("data"),
            Type::Error => TypeSyntax::word("error"),
            Type::Var(name) => TypeSyntax::word(name.as_str(db)),
            Type::AnonTuple(t) => TypeSyntax::Tuple(t.fields.iter().map(|f| f.syntax_under(db, binders)).collect()),
            Type::AnonStruct(s) => TypeSyntax::Struct(fields(&s.fields, db, binders)),
            Type::Table(t) => TypeSyntax::Table(fields(&t.columns, db, binders)),
            Type::List(l) => TypeSyntax::List(boxed(&l.element_type, db, binders)),
            Type::Set(s) => TypeSyntax::Set(boxed(&s.element_type, db, binders)),
            Type::Map(m) => {
                let key = boxed(&m.key_type, db, binders);
                TypeSyntax::Map(key, boxed(&m.value_type, db, binders))
            }
            Type::Option(o) => TypeSyntax::Option(boxed(&o.inner_type, db, binders)),
            Type::Result(r) => TypeSyntax::Result(boxed(&r.inner_type, db, binders)),
            Type::Tensor(t) => TypeSyntax::Tensor(boxed(&t.element_type, db, binders), t.rank),
            Type::Atom(a) => TypeSyntax::Atom(Cow::Borrowed(a.name.as_str(db))),
            Type::Term(t) => TypeSyntax::Term(Cow::Borrowed(t.name.as_str(db)), boxed(&t.payload, db, binders)),
            Type::Enum(e) => TypeSyntax::Enum(e.variants.iter()
                .map(|v| (Cow::Borrowed(v.name.as_str(db)), v.payload.as_ref().map(|p| p.syntax_under(db, binders))))
                .collect()),
            Type::Rec(r) => {
                let name = r.name.0.as_str(db);
                binders.push(name);
                let body = boxed(&r.body, db, binders);
                binders.pop();
                TypeSyntax::Rec(Cow::Borrowed(name), body)
            }
            Type::RecVar(index) => {
                let name = binders.iter().rev().nth(*index as usize)
                    .expect("a rec name is used under its binder");
                TypeSyntax::word(*name)
            }
        }
    }
}

// ============================================================================
// Unit and Shape Helpers
// ============================================================================

/// Create the unit type `()` (empty anonymous tuple).
pub fn unit_type<'db>() -> Type<'db> {
    Type::AnonTuple(TypeAnonTuple { fields: vec![] })
}

/// Check that tensor element count matches the shape product.
pub fn check_tensor_element_count(shape: &[u32], actual: usize) -> Result<(), TypeError> {
    let expected = shape.iter().map(|&d| d as usize).product::<usize>();
    if actual != expected {
        return Err(TypeError::ArityMismatch { expected, actual });
    }
    Ok(())
}

// ============================================================================
// Integer Range Checking
// ============================================================================

/// Check if an integer value fits within a given type.
///
/// Only valid for integer types (fixed-size or bigint).
pub fn check_int_fits_type(value_str: &str, ty: &Type<'_>) -> Result<(), TypeError> {
    // Underscores group digits for a reader; the value is what is left.
    let value_str = crate::parser_util::strip_separators(value_str);
    let value_str = value_str.as_ref();
    match ty {
        Type::U8 => value_str.parse::<u8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I8 => value_str.parse::<i8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::U16 => value_str.parse::<u16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I16 => value_str.parse::<i16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::U32 => value_str.parse::<u32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I32 => value_str.parse::<i32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::U64 => value_str.parse::<u64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I64 => value_str.parse::<i64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::Index => value_str.parse::<datalove_rtdt::IndexRepr>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::Offset => value_str.parse::<datalove_rtdt::OffsetRepr>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::Int => Ok(()),
        // An integer written with a hint that is not an integer type. `: bool
        // / 0` is a thing a person can write, and saying so is the answer; it
        // used to bring the compiler down here instead.
        _ => Err(TypeError::TypeMismatch {
            expected: "an integer type".to_string(),
            actual: "a hint that is not one".to_string(),
        }),
    }
}

/// Check if a hex value fits within a given type.
pub fn check_hex_fits_type(value_str: &str, ty: &Type<'_>) -> Result<(), TypeError> {
    let value_str = crate::parser_util::strip_separators(value_str);
    let hex_part = value_str
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    let fits = match ty {
        Type::U8 => u8::from_str_radix(hex_part, 16).is_ok(),
        Type::U16 => u16::from_str_radix(hex_part, 16).is_ok(),
        Type::U32 | Type::F32 => u32::from_str_radix(hex_part, 16).is_ok(),
        Type::U64 | Type::F64 => u64::from_str_radix(hex_part, 16).is_ok(),
        Type::Index => datalove_rtdt::IndexRepr::from_str_radix(hex_part, 16).is_ok(),
        Type::Int => true,
        _ => panic!("check_hex_fits_type called with unsupported type"),
    };
    if fits { Ok(()) } else { Err(TypeError::IntOutOfRange) }
}

// ============================================================================
// Diagnostic Helpers
// ============================================================================

/// Description of index range based on index-64 feature.
#[cfg(not(feature = "index-64"))]
const INDEX_RANGE: &str = "index can represent values from 0 to 4,294,967,295";
#[cfg(feature = "index-64")]
const INDEX_RANGE: &str = "index can represent values from 0 to 18,446,744,073,709,551,615";

/// Description of offset range based on index-64 feature.
#[cfg(not(feature = "index-64"))]
const OFFSET_RANGE: &str = "offset can represent values from -2,147,483,648 to 2,147,483,647";
#[cfg(feature = "index-64")]
const OFFSET_RANGE: &str = "offset can represent values from -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807";

/// Hex range description for index based on index-64 feature.
#[cfg(not(feature = "index-64"))]
const INDEX_HEX_RANGE: &str = "index can represent hex values from 0x00000000 to 0xFFFFFFFF";
#[cfg(feature = "index-64")]
const INDEX_HEX_RANGE: &str = "index can represent hex values from 0x0000000000000000 to 0xFFFFFFFFFFFFFFFF";

/// Get the diagnostic code and range note for an integer type.
pub fn int_type_range_info(ty: &Type<'_>) -> (&'static str, &'static str) {
    match ty {
        Type::U8 => ("T005", "u8 can represent values from 0 to 255"),
        Type::I8 => ("T006", "i8 can represent values from -128 to 127"),
        Type::U16 => ("T007", "u16 can represent values from 0 to 65,535"),
        Type::I16 => ("T008", "i16 can represent values from -32,768 to 32,767"),
        Type::U32 => ("T009", "u32 can represent values from 0 to 4,294,967,295"),
        Type::I32 => ("T010", "i32 can represent values from -2,147,483,648 to 2,147,483,647"),
        Type::U64 => ("T011", "u64 can represent values from 0 to 18,446,744,073,709,551,615"),
        Type::I64 => ("T012", "i64 can represent values from -9,223,372,036,854,775,808 to 9,223,372,036,854,775,807"),
        Type::Index => ("T015", INDEX_RANGE),
        Type::Offset => ("T061", OFFSET_RANGE),
        Type::Int => ("T000", "int is arbitrary precision"),
        _ => ("T000", ""),
    }
}

/// Get the diagnostic code and range note for a hex literal target type.
pub fn hex_type_range_info(ty: &Type<'_>) -> (&'static str, &'static str) {
    match ty {
        Type::U8 => ("T005", "u8 can represent hex values from 0x00 to 0xFF"),
        Type::U16 => ("T007", "u16 can represent hex values from 0x0000 to 0xFFFF"),
        Type::U32 => ("T009", "u32 can represent hex values from 0x00000000 to 0xFFFFFFFF"),
        Type::U64 => ("T011", "u64 can represent hex values from 0x0000000000000000 to 0xFFFFFFFFFFFFFFFF"),
        Type::Index => ("T015", INDEX_HEX_RANGE),
        Type::Int => ("T000", "int is arbitrary precision"),
        Type::F32 => ("T013", "f32 bit patterns must be 32-bit hex values (0x00000000 to 0xFFFFFFFF)"),
        Type::F64 => ("T014", "f64 bit patterns must be 64-bit hex values (0x0000000000000000 to 0xFFFFFFFFFFFFFFFF)"),
        _ => ("T000", ""),
    }
}
