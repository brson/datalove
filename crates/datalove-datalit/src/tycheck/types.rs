//! Type definitions and utilities for datalit typechecking.
//!
//! Organization:
//! 1. Type representation - core types
//! 2. Type error - error type
//! 3. Heap utilities - heap functions
//! 4. Type predicates - type queries
//! 5. Type equivalence - comparing types
//! 6. Type hint conversion - AST to types
//! 7. Type to string - types to strings for errors
//! 8. Unit and empty collection helpers - type constructors
//! 9. Element compatibility - collection element checking
//! 10. Integer range checking - literal validation
//! 11. Diagnostic helpers - error message helpers

use rmx::prelude::*;
use bct::text::InternedText;
use crate::ast::{Heap, TypeHint, TypeHintAndHeap};

// ============================================================================
// Type Representation
// ============================================================================

/// Type representation (synthesized types, mirrors TypeHint but without parse errors).
#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
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
    F32,
    F64,
    Int,
    String,
    AnonTuple(TypeAnonTuple<'db>),
    AnonStruct(TypeAnonStruct<'db>),
    AnonEnum(TypeAnonEnum<'db>),
    List(TypeList<'db>),
    Map(TypeMap<'db>),
    Set(TypeSet<'db>),
    Option(TypeOption<'db>),
    Result(TypeResult<'db>),
    Tensor(TypeTensor<'db>),
    Data,
    Error,
}

#[salsa::tracked]
pub struct TypeAndHeap<'db> {
    pub heap: Heap,
    #[returns(ref)]
    pub ty: Type<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeAnonTuple<'db> {
    pub fields: Vec<TypeAndHeap<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeAnonStruct<'db> {
    pub fields: Vec<TypeNamedField<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeNamedField<'db> {
    pub name: InternedText<'db>,
    pub ty: TypeAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeAnonEnum<'db> {
    pub variants: Vec<TypeEnumVariant<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeEnumVariant<'db> {
    pub name: InternedText<'db>,
    pub payload: Option<TypeAndHeap<'db>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeList<'db> {
    pub element_type: TypeAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeMap<'db> {
    pub key_type: TypeAndHeap<'db>,
    pub value_type: TypeAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeSet<'db> {
    pub element_type: TypeAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeOption<'db> {
    pub inner_type: TypeAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeResult<'db> {
    pub inner_type: TypeAndHeap<'db>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct TypeTensor<'db> {
    pub element_type: TypeAndHeap<'db>,
    pub rank: u32,
}

// ============================================================================
// Type Error
// ============================================================================

/// Type error representation.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum TypeError {
    TypeMismatch { expected: String, actual: String },
    HeapMismatch { expected_heap: String, actual_heap: String },
    CannotSynthesize,
    MissingField(String),
    ExtraField(String),
    FieldOrderMismatch,
    IntOutOfRange,
    VariantNotFound(String),
    ArityMismatch { expected: usize, actual: usize },
}

// ============================================================================
// Heap Utilities
// ============================================================================

/// Check if two heaps are compatible.
///
/// Omitted heap is generic and compatible with any heap.
pub fn heaps_compatible(h1: Heap, h2: Heap) -> bool {
    match (h1, h2) {
        (Heap::Local, Heap::Local) => true,
        (Heap::Global, Heap::Global) => true,
        (Heap::Omitted, _) => true,
        (_, Heap::Omitted) => true,
        _ => false,
    }
}

/// Convert a heap to a string for error messages.
pub fn heap_to_string(heap: Heap) -> String {
    match heap {
        Heap::Local => "@".to_string(),
        Heap::Global => "#".to_string(),
        Heap::Omitted => "".to_string(),
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
        Type::U64 | Type::I64
    )
}

/// Check if a type is an unsigned integer type.
pub fn is_unsigned_int_type(ty: &Type<'_>) -> bool {
    matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64)
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
        (Type::F32, Type::F32) => true,
        (Type::F64, Type::F64) => true,
        (Type::Int, Type::Int) => true,
        (Type::String, Type::String) => true,
        (Type::Data, Type::Data) => true,
        (Type::Error, Type::Error) => true,

        (Type::AnonTuple(t1), Type::AnonTuple(t2)) => {
            let f1 = t1.fields.clone();
            let f2 = t2.fields.clone();
            f1.len() == f2.len()
                && f1
                    .iter()
                    .zip(f2.iter())
                    .all(|(a, b)| types_and_heaps_equivalent(db, a, b))
        }

        (Type::AnonStruct(s1), Type::AnonStruct(s2)) => {
            let f1 = s1.fields.clone();
            let f2 = s2.fields.clone();
            f1.len() == f2.len()
                && f1.iter().zip(f2.iter()).all(|(a, b)| {
                    a.name == b.name && types_and_heaps_equivalent(db, &a.ty, &b.ty)
                })
        }

        (Type::AnonEnum(e1), Type::AnonEnum(e2)) => {
            let v1 = e1.variants.clone();
            let v2 = e2.variants.clone();
            v1.len() == v2.len()
                && v1.iter().all(|var1| {
                    v2.iter().any(|var2| {
                        var1.name == var2.name
                            && match (var1.payload, var2.payload) {
                                (Some(p1), Some(p2)) => types_and_heaps_equivalent(db, &p1, &p2),
                                (None, None) => true,
                                _ => false,
                            }
                    })
                })
        }

        (Type::List(l1), Type::List(l2)) => {
            types_and_heaps_equivalent(db, &l1.element_type, &l2.element_type)
        }

        (Type::Map(m1), Type::Map(m2)) => {
            types_and_heaps_equivalent(db, &m1.key_type, &m2.key_type)
                && types_and_heaps_equivalent(db, &m1.value_type, &m2.value_type)
        }

        (Type::Set(s1), Type::Set(s2)) => {
            types_and_heaps_equivalent(db, &s1.element_type, &s2.element_type)
        }

        (Type::Option(o1), Type::Option(o2)) => {
            types_and_heaps_equivalent(db, &o1.inner_type, &o2.inner_type)
        }

        (Type::Result(r1), Type::Result(r2)) => {
            types_and_heaps_equivalent(db, &r1.inner_type, &r2.inner_type)
        }

        (Type::Tensor(t1), Type::Tensor(t2)) => {
            t1.rank == t2.rank
                && types_and_heaps_equivalent(db, &t1.element_type, &t2.element_type)
        }

        _ => false,
    }
}

/// Check if two TypeAndHeap values are equivalent.
pub fn types_and_heaps_equivalent<'db>(
    db: &'db dyn crate::Db,
    t1: &TypeAndHeap<'db>,
    t2: &TypeAndHeap<'db>,
) -> bool {
    heaps_compatible(t1.heap(db), t2.heap(db)) && types_equivalent(db, t1.ty(db), t2.ty(db))
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
        _ => false,
    }
}

// ============================================================================
// Type Hint Conversion
// ============================================================================

/// Convert a type hint to a type.
pub fn convert_type_hint<'db>(
    db: &'db dyn crate::Db,
    type_hint_and_heap: TypeHintAndHeap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let heap = type_hint_and_heap.heap(db);
    let type_hint = type_hint_and_heap.type_hint(db);

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
                .map(|f| convert_type_hint(db, *f))
                .collect();
            Type::AnonTuple(TypeAnonTuple { fields: fields? })
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, _> = s
                .fields
                .iter()
                .map(|f| {
                    let name = f.name;
                    let ty = convert_type_hint(db, f.type_hint)?;
                    Ok(TypeNamedField { name, ty })
                })
                .collect();
            Type::AnonStruct(TypeAnonStruct { fields: fields? })
        }

        TypeHint::AnonEnum(e) => {
            let variants: Result<Vec<_>, _> = e
                .variants
                .iter()
                .map(|v| {
                    let name = v.name;
                    let payload = v
                        .payload
                        .map(|p| convert_type_hint(db, p))
                        .transpose()?;
                    Ok(TypeEnumVariant { name, payload })
                })
                .collect();
            Type::AnonEnum(TypeAnonEnum { variants: variants? })
        }

        TypeHint::List(l) => {
            let element_type = convert_type_hint(db, l.element_type)?;
            Type::List(TypeList { element_type })
        }

        TypeHint::Map(m) => {
            let key_type = convert_type_hint(db, m.key_type)?;
            let value_type = convert_type_hint(db, m.value_type)?;
            Type::Map(TypeMap { key_type, value_type })
        }

        TypeHint::Set(s) => {
            let element_type = convert_type_hint(db, s.element_type)?;
            Type::Set(TypeSet { element_type })
        }

        TypeHint::Option(o) => {
            let inner_type = convert_type_hint(db, o.inner_type)?;
            Type::Option(TypeOption { inner_type })
        }

        TypeHint::Result(r) => {
            let inner_type = convert_type_hint(db, r.inner_type)?;
            Type::Result(TypeResult { inner_type })
        }

        TypeHint::Tensor(t) => {
            let element_type = convert_type_hint(db, t.element_type)?;
            Type::Tensor(TypeTensor { element_type, rank: t.rank })
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),
    };

    Ok(TypeAndHeap::new(db, heap, ty))
}

// ============================================================================
// Type to String
// ============================================================================

/// Convert a type to a string for error messages.
pub fn type_to_string<'db>(db: &'db dyn crate::Db, ty: &Type<'db>) -> String {
    match ty {
        Type::Bool => "bool".to_string(),
        Type::U8 => "u8".to_string(),
        Type::I8 => "i8".to_string(),
        Type::U16 => "u16".to_string(),
        Type::I16 => "i16".to_string(),
        Type::U32 => "u32".to_string(),
        Type::I32 => "i32".to_string(),
        Type::U64 => "u64".to_string(),
        Type::I64 => "i64".to_string(),
        Type::F32 => "f32".to_string(),
        Type::F64 => "f64".to_string(),
        Type::Int => "int".to_string(),
        Type::String => "string".to_string(),
        Type::Data => "data".to_string(),
        Type::Error => "error".to_string(),
        Type::AnonTuple(t) => {
            let fields: Vec<_> = t.fields.clone()
                .iter()
                .map(|f| {
                    let heap = heap_to_string(f.heap(db));
                    let ty_str = type_to_string(db, f.ty(db));
                    format!("{}{}", heap, ty_str)
                })
                .collect();
            format!("({})", fields.join(", "))
        }
        Type::AnonStruct(s) => {
            let fields: Vec<_> = s.fields.clone()
                .iter()
                .map(|f| {
                    let name = f.name.as_str(db);
                    let heap = heap_to_string(f.ty.heap(db));
                    let ty_str = type_to_string(db, f.ty.ty(db));
                    format!("{}: {}{}", name, heap, ty_str)
                })
                .collect();
            format!("{{{}}}", fields.join(", "))
        }
        Type::AnonEnum(_) => {
            format!("@enum{{...}}")
        }
        Type::List(l) => {
            let elem = l.element_type;
            let heap = heap_to_string(elem.heap(db));
            let ty_str = type_to_string(db, elem.ty(db));
            format!("[{}{}]", heap, ty_str)
        }
        Type::Map(m) => {
            let key = m.key_type;
            let value = m.value_type;
            format!("@map<{}, {}>",
                format!("{}{}", heap_to_string(key.heap(db)), type_to_string(db, key.ty(db))),
                format!("{}{}", heap_to_string(value.heap(db)), type_to_string(db, value.ty(db))))
        }
        Type::Set(s) => {
            let elem = s.element_type;
            let heap = heap_to_string(elem.heap(db));
            let ty_str = type_to_string(db, elem.ty(db));
            format!("@set<{}{}>", heap, ty_str)
        }
        Type::Option(o) => {
            let inner = o.inner_type;
            let heap = heap_to_string(inner.heap(db));
            let ty_str = type_to_string(db, inner.ty(db));
            format!("@?{}{}", heap, ty_str)
        }
        Type::Result(r) => {
            let inner = r.inner_type;
            let heap = heap_to_string(inner.heap(db));
            let ty_str = type_to_string(db, inner.ty(db));
            format!("@!{}{}", heap, ty_str)
        }
        Type::Tensor(t) => {
            let elem = t.element_type;
            let heap = heap_to_string(elem.heap(db));
            let ty_str = type_to_string(db, elem.ty(db));
            format!("@tensor<{}{}, {}>", heap, ty_str, t.rank)
        }
    }
}

// ============================================================================
// Unit and Empty Collection Helpers
// ============================================================================

/// Create the unit type `()` (empty anonymous tuple) with the given heap.
pub fn unit_type<'db>(db: &'db dyn crate::Db, heap: Heap) -> TypeAndHeap<'db> {
    TypeAndHeap::new(db, heap, Type::AnonTuple(TypeAnonTuple { fields: vec![] }))
}

/// Create an empty list type `List<()>` with the given heap.
pub fn empty_list_type<'db>(db: &'db dyn crate::Db, heap: Heap) -> TypeAndHeap<'db> {
    let elem_ty = unit_type(db, heap);
    TypeAndHeap::new(db, heap, Type::List(TypeList { element_type: elem_ty }))
}

/// Create an empty set type `Set<()>` with the given heap.
pub fn empty_set_type<'db>(db: &'db dyn crate::Db, heap: Heap) -> TypeAndHeap<'db> {
    let elem_ty = unit_type(db, heap);
    TypeAndHeap::new(db, heap, Type::Set(TypeSet { element_type: elem_ty }))
}

/// Create an empty map type `Map<(), ()>` with the given heap.
pub fn empty_map_type<'db>(db: &'db dyn crate::Db, heap: Heap) -> TypeAndHeap<'db> {
    let elem_ty = unit_type(db, heap);
    TypeAndHeap::new(db, heap, Type::Map(TypeMap {
        key_type: elem_ty,
        value_type: elem_ty,
    }))
}

/// Create an empty tensor type `Tensor<(), rank>` with the given heap.
pub fn empty_tensor_type<'db>(db: &'db dyn crate::Db, heap: Heap, rank: u32) -> TypeAndHeap<'db> {
    let elem_ty = unit_type(db, heap);
    TypeAndHeap::new(db, heap, Type::Tensor(TypeTensor {
        element_type: elem_ty,
        rank,
    }))
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
// Element Compatibility
// ============================================================================

/// Check that an element type is compatible with the expected element type.
///
/// Used for homogeneous collections (list, set, tensor) to verify all elements
/// have the same type and compatible heaps.
pub fn check_element_compatible<'db>(
    db: &'db dyn crate::Db,
    expected: TypeAndHeap<'db>,
    actual: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    if !types_equivalent(db, expected.ty(db), actual.ty(db)) {
        return Err(TypeError::TypeMismatch {
            expected: type_to_string(db, expected.ty(db)),
            actual: type_to_string(db, actual.ty(db)),
        });
    }

    if !heaps_compatible(expected.heap(db), actual.heap(db)) {
        return Err(TypeError::HeapMismatch {
            expected_heap: heap_to_string(expected.heap(db)),
            actual_heap: heap_to_string(actual.heap(db)),
        });
    }

    Ok(())
}

/// Check that a key-value pair is compatible with expected key and value types.
pub fn check_map_entry_compatible<'db>(
    db: &'db dyn crate::Db,
    expected_key: TypeAndHeap<'db>,
    expected_value: TypeAndHeap<'db>,
    actual_key: TypeAndHeap<'db>,
    actual_value: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    check_element_compatible(db, expected_key, actual_key)?;
    check_element_compatible(db, expected_value, actual_value)?;
    Ok(())
}

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
pub fn check_int_fits_type(value_str: &str, ty: &Type<'_>) -> Result<(), TypeError> {
    match ty {
        Type::U8 => value_str.parse::<u8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I8 => value_str.parse::<i8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::U16 => value_str.parse::<u16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I16 => value_str.parse::<i16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::U32 => value_str.parse::<u32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I32 => value_str.parse::<i32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::U64 => value_str.parse::<u64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::I64 => value_str.parse::<i64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange),
        Type::Int => Ok(()),
        _ => Ok(()),
    }
}

/// Check if an integer value fits within the innermost integer type of a possibly wrapped type.
pub fn check_int_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &Type<'db>,
    db: &'db dyn crate::Db,
) -> Result<(), TypeError> {
    match ty {
        Type::Option(opt) => check_int_fits_wrapped_type(value_str, opt.inner_type.ty(db), db),
        Type::Result(res) => check_int_fits_wrapped_type(value_str, res.inner_type.ty(db), db),
        _ => check_int_fits_type(value_str, ty),
    }
}

/// Check if a hex value fits within a given type.
pub fn check_hex_fits_type(value_str: &str, ty: &Type<'_>) -> Result<(), TypeError> {
    let is_negative = value_str.starts_with('-');
    let hex_part = value_str
        .trim_start_matches('-')
        .trim_start_matches("0x")
        .trim_start_matches("0X");

    match ty {
        Type::U8 if !is_negative => {
            u8::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        Type::I8 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 128 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 127 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        Type::U16 if !is_negative => {
            u16::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        Type::I16 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 32768 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 32767 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        Type::U32 if !is_negative => {
            u32::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        Type::I32 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 2147483648 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 2147483647 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        Type::U64 if !is_negative => {
            u64::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        Type::I64 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 9223372036854775808 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 9223372036854775807 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        Type::Int => Ok(()),
        Type::F32 if !is_negative => {
            u32::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        Type::F64 if !is_negative => {
            u64::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        _ if is_negative => Err(TypeError::IntOutOfRange),
        _ => Ok(()),
    }
}

/// Check if a hex value fits within the innermost integer type of a possibly wrapped type.
pub fn check_hex_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &Type<'db>,
    db: &'db dyn crate::Db,
) -> Result<(), TypeError> {
    match ty {
        Type::Option(opt) => check_hex_fits_wrapped_type(value_str, opt.inner_type.ty(db), db),
        Type::Result(res) => check_hex_fits_wrapped_type(value_str, res.inner_type.ty(db), db),
        _ => check_hex_fits_type(value_str, ty),
    }
}

// ============================================================================
// Diagnostic Helpers
// ============================================================================

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
        Type::I8 => ("T006", "i8 can represent hex values from -0x80 to 0x7F"),
        Type::I16 => ("T008", "i16 can represent hex values from -0x8000 to 0x7FFF"),
        Type::I32 => ("T010", "i32 can represent hex values from -0x80000000 to 0x7FFFFFFF"),
        Type::I64 => ("T012", "i64 can represent hex values from -0x8000000000000000 to 0x7FFFFFFFFFFFFFFF"),
        Type::Int => ("T000", "int is arbitrary precision"),
        Type::F32 => ("T013", "f32 bit patterns must be 32-bit hex values (0x00000000 to 0xFFFFFFFF)"),
        Type::F64 => ("T014", "f64 bit patterns must be 64-bit hex values (0x0000000000000000 to 0xFFFFFFFFFFFFFFFF)"),
        _ => ("T000", ""),
    }
}
