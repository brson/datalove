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
//! 8. Coercion - checking a type against an expected one
//! 9. Integer range checking - literal validation
//! 10. Diagnostic helpers - error message helpers

use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::TypeHint;
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
/// It clones a value to its own type, widens an integer to one that holds
/// every value of it, widens `f32` to `f64`, and makes an atom or term a value
/// of an enum that lists it. Nothing else: no narrowing, and no enum adapts to
/// another.
pub fn can_adapt_to<'db>(db: &'db dyn crate::Db, from: &Type<'db>, to: &Type<'db>) -> bool {
    if types_equivalent(db, from, to) || can_widen_to(from, to) {
        return true;
    }
    match (from, to) {
        // An unsigned integer widens to any wider signed one.
        (Type::U8, Type::I16 | Type::I32 | Type::I64 | Type::Int) => true,
        (Type::U16, Type::I32 | Type::I64 | Type::Int) => true,
        (Type::U32, Type::I64 | Type::Int) => true,
        // Every f32 is exactly an f64. This is not in `can_widen_to`, which
        // datalit also uses for implicit coercion, and floats only widen by `@`.
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
            let fields: Result<Vec<_>, _> = t
                .fields
                .iter()
                .map(|f| convert_type_hint(db, f))
                .collect();
            Type::AnonTuple(TypeAnonTuple { fields: fields? })
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, _> = s
                .fields
                .iter()
                .map(|f| {
                    let name = f.name;
                    let ty = convert_type_hint(db, &f.type_hint)?;
                    Ok(TypeNamedField { name, ty: Box::new(ty) })
                })
                .collect();
            Type::AnonStruct(TypeAnonStruct { fields: fields? })
        }


        TypeHint::List(l) => {
            let element_type = convert_type_hint(db, &l.element_type)?;
            Type::List(TypeList { element_type: Box::new(element_type) })
        }

        TypeHint::Map(m) => {
            let key_type = convert_type_hint(db, &m.key_type)?;
            let value_type = convert_type_hint(db, &m.value_type)?;
            Type::Map(TypeMap { key_type: Box::new(key_type), value_type: Box::new(value_type) })
        }

        TypeHint::Set(s) => {
            let element_type = convert_type_hint(db, &s.element_type)?;
            Type::Set(TypeSet { element_type: Box::new(element_type) })
        }

        TypeHint::Option(o) => {
            let inner_type = convert_type_hint(db, &o.inner_type)?;
            Type::Option(TypeOption { inner_type: Box::new(inner_type) })
        }

        TypeHint::Result(r) => {
            let inner_type = convert_type_hint(db, &r.inner_type)?;
            Type::Result(TypeResult { inner_type: Box::new(inner_type) })
        }

        TypeHint::Tensor(t) => {
            let element_type = convert_type_hint(db, &t.element_type)?;
            Type::Tensor(TypeTensor { element_type: Box::new(element_type), rank: t.rank })
        }

        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, _> = t
                .columns
                .iter()
                .map(|c| {
                    let name = c.name;
                    let ty = convert_type_hint(db, &c.type_hint)?;
                    Ok(TypeNamedField { name, ty: Box::new(ty) })
                })
                .collect();
            Type::Table(TypeTable { columns: columns? })
        }

        TypeHint::Atom(a) => {
            Type::Atom(TypeAtom { name: a.name })
        }

        TypeHint::Term(t) => {
            let payload = convert_type_hint(db, &t.payload)?;
            Type::Term(TypeTerm { name: t.name, payload: Box::new(payload) })
        }

        TypeHint::Enum(e) => {
            let variants: Result<Vec<_>, _> = e.variants.iter()
                .map(|v| {
                    let payload = match &v.payload {
                        Some(p) => Some(Box::new(convert_type_hint(db, p)?)),
                        None => None,
                    };
                    Ok(TypeEnumVariant { name: v.name, payload })
                })
                .collect();
            let mut variants = variants?;
            variants.sort_by(|a, b| a.name.as_str(db).cmp(b.name.as_str(db)));
            Type::Enum(TypeEnum { variants })
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),
        TypeHint::Alias(_) => {
            // Type aliases are resolved by datafun typechecker, not datalit.
            return Err(TypeError::CannotSynthesize);
        }
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
        let boxed = |ty: &Type<'db>| Box::new(ty.syntax(db));
        let fields = |fields: &[TypeNamedField<'db>]| fields.iter()
            .map(|field| (Cow::Borrowed(field.name.as_str(db)), field.ty.syntax(db)))
            .collect();
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
            Type::AnonTuple(t) => TypeSyntax::Tuple(t.fields.iter().map(|f| f.syntax(db)).collect()),
            Type::AnonStruct(s) => TypeSyntax::Struct(fields(&s.fields)),
            Type::Table(t) => TypeSyntax::Table(fields(&t.columns)),
            Type::List(l) => TypeSyntax::List(boxed(&l.element_type)),
            Type::Set(s) => TypeSyntax::Set(boxed(&s.element_type)),
            Type::Map(m) => TypeSyntax::Map(boxed(&m.key_type), boxed(&m.value_type)),
            Type::Option(o) => TypeSyntax::Option(boxed(&o.inner_type)),
            Type::Result(r) => TypeSyntax::Result(boxed(&r.inner_type)),
            Type::Tensor(t) => TypeSyntax::Tensor(boxed(&t.element_type), t.rank),
            Type::Atom(a) => TypeSyntax::Atom(Cow::Borrowed(a.name.as_str(db))),
            Type::Term(t) => TypeSyntax::Term(Cow::Borrowed(t.name.as_str(db)), boxed(&t.payload)),
            Type::Enum(e) => TypeSyntax::Enum(e.variants.iter()
                .map(|v| (Cow::Borrowed(v.name.as_str(db)), v.payload.as_ref().map(|p| p.syntax(db))))
                .collect()),
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
// Coercion
// ============================================================================

/// Check if a type can be coerced to an expected type.
///
/// Coercion rules:
/// - Exact type match (via types_equivalent)
/// - Numeric widening (via can_widen_to)
/// - Any type coerces to Data
pub fn check_type_coercion<'db>(
    db: &'db dyn crate::Db,
    actual: &Type<'db>,
    expected: &Type<'db>,
) -> Result<(), TypeError> {
    if types_equivalent(db, actual, expected) {
        return Ok(());
    }

    if can_widen_to(actual, expected) {
        return Ok(());
    }

    if let Type::Data = expected {
        return Ok(());
    }

    check_coercion_arity_or_mismatch(db, actual, expected)
}

/// Helper for check_type_coercion that distinguishes arity mismatches from type mismatches.
fn check_coercion_arity_or_mismatch<'db>(
    db: &'db dyn crate::Db,
    actual: &Type<'db>,
    expected: &Type<'db>,
) -> Result<(), TypeError> {
    if let (Type::AnonStruct(actual_struct), Type::AnonStruct(expected_struct)) = (actual, expected) {
        let actual_count = actual_struct.fields.len();
        let expected_count = expected_struct.fields.len();
        if actual_count != expected_count {
            return Err(TypeError::ArityMismatch {
                expected: expected_count,
                actual: actual_count,
            });
        }
    }

    if let (Type::AnonTuple(actual_tuple), Type::AnonTuple(expected_tuple)) = (actual, expected) {
        let actual_count = actual_tuple.fields.len();
        let expected_count = expected_tuple.fields.len();
        if actual_count != expected_count {
            return Err(TypeError::ArityMismatch {
                expected: expected_count,
                actual: actual_count,
            });
        }
    }

    Err(TypeError::TypeMismatch {
        expected: type_to_string(db, expected),
        actual: type_to_string(db, actual),
    })
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
