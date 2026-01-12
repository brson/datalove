//! Type utilities for datafun typechecking.
//!
//! Provides type predicates, conversion functions, and helper utilities.
//! Many utilities delegate to or re-export from datalit's tycheck module.

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;

pub use crate::{Type, TypeAndHeap, TypeFunction, TypeError};

// Re-export heap utilities from datalit.
pub use datalit::tycheck::{heaps_compatible, heap_to_string};

// ============================================================================
// Type Predicates (wrap datalit predicates for datafun's Type enum)
// ============================================================================

/// Check if a type is numeric (any integer or float type).
pub fn is_numeric_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::is_numeric_type(datalit_ty),
        Type::Function(_) => false,
    }
}

/// Check if a type is a floating-point type.
pub fn is_float_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::is_float_type(datalit_ty),
        Type::Function(_) => false,
    }
}

/// Check if a type is the arbitrary-precision integer type.
pub fn is_bigint_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::is_bigint_type(datalit_ty),
        Type::Function(_) => false,
    }
}

/// Check if a type is a fixed-size integer type.
pub fn is_fixed_int_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::is_fixed_int_type(datalit_ty),
        Type::Function(_) => false,
    }
}

/// Check if a type is an unsigned integer type.
pub fn is_unsigned_int_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::is_unsigned_int_type(datalit_ty),
        Type::Function(_) => false,
    }
}

/// Check if a type is boolean.
pub fn is_bool_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::is_bool_type(datalit_ty),
        Type::Function(_) => false,
    }
}

// ============================================================================
// Integer Range Checking (delegate to datalit)
// ============================================================================

/// Check if an integer value fits within a given type.
///
/// Returns Ok(()) if the value fits, Err(IntOutOfRange) if not.
pub fn check_int_fits_type(value_str: &str, ty: &datalit::tycheck::Type<'_>) -> Result<(), TypeError> {
    datalit::tycheck::check_int_fits_type(value_str, ty).map_err(TypeError::from)
}

/// Check if an integer value fits within the innermost integer type of a possibly wrapped type.
///
/// Handles Option<u8>, Result<u8>, etc.
pub fn check_int_fits_wrapped_type(
    value_str: &str,
    ty: &datalit::tycheck::Type<'_>,
    db: &dyn crate::Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_int_fits_wrapped_type(value_str, ty, db).map_err(TypeError::from)
}

/// Check if a hex value fits within a given type.
pub fn check_hex_fits_type(value_str: &str, ty: &datalit::tycheck::Type<'_>) -> Result<(), TypeError> {
    datalit::tycheck::check_hex_fits_type(value_str, ty).map_err(TypeError::from)
}

/// Check if a hex value fits within the innermost integer type of a possibly wrapped type.
pub fn check_hex_fits_wrapped_type(
    value_str: &str,
    ty: &datalit::tycheck::Type<'_>,
    db: &dyn crate::Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_hex_fits_wrapped_type(value_str, ty, db).map_err(TypeError::from)
}

// ============================================================================
// Type Hint Conversion
// ============================================================================

/// Convert a datalit type hint to a datafun type.
pub fn convert_type_hint<'db>(
    db: &'db dyn crate::Db,
    type_hint_and_heap: datalit::ast::TypeHintAndHeap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    // Delegate to datalit's convert_type_hint.
    let datalit_ty = datalit::tycheck::convert_type_hint(db, type_hint_and_heap)
        .map_err(TypeError::from)?;

    let heap = datalit_ty.heap(db);
    let ty = Type::Datalit(datalit_ty.ty(db).clone());

    Ok(TypeAndHeap::new(db, heap, ty))
}

// ============================================================================
// Heap Unwrapping
// ============================================================================

/// Unwrap Option/Result types to get the innermost heap.
///
/// Used when checking heap compatibility for typed literals.
pub fn unwrap_wrapper_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            unwrap_wrapper_heap_datalit(db, opt.inner_type)
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            unwrap_wrapper_heap_datalit(db, res.inner_type)
        }
        _ => ty.heap(db),
    }
}

/// Unwrap Option/Result types from a datalit TypeAndHeap.
pub fn unwrap_wrapper_heap_datalit<'db>(
    db: &'db dyn crate::Db,
    ty: datalit::tycheck::TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        datalit::tycheck::Type::Option(opt) => {
            unwrap_wrapper_heap_datalit(db, opt.inner_type)
        }
        datalit::tycheck::Type::Result(res) => {
            unwrap_wrapper_heap_datalit(db, res.inner_type)
        }
        _ => ty.heap(db),
    }
}

// ============================================================================
// Expression Heap Extraction
// ============================================================================

/// Extract the outer heap from an expression.
///
/// Returns the heap sigil used on the expression itself (e.g. `@` in `@{...}`).
/// Returns `Heap::Omitted` for expressions that don't have an explicit heap.
pub fn get_expr_heap<'db>(db: &'db dyn crate::Db, expr: ExprFun<'db>) -> datalit::ast::Heap {
    use ExprFunKind;
    match expr.expr(db) {
        ExprFunKind::True(lit) | ExprFunKind::False(lit) | ExprFunKind::None(lit) => lit.heap,
        ExprFunKind::Int(e) => e.heap,
        ExprFunKind::Float(e) => e.heap,
        ExprFunKind::Hex(e) => e.heap,
        ExprFunKind::String(e) => e.heap,
        ExprFunKind::List(e) => e.heap,
        ExprFunKind::Set(e) => e.heap,
        ExprFunKind::Map(e) => e.heap,
        ExprFunKind::Tensor(e) => e.heap,
        ExprFunKind::AnonTuple(e) => e.heap,
        ExprFunKind::AnonStruct(e) => e.heap,
        ExprFunKind::AnonEnum(e) => e.heap,
        ExprFunKind::Some(e) => e.heap,
        ExprFunKind::Ok(e) => e.heap,
        ExprFunKind::Er(e) => e.heap,
        ExprFunKind::Data(e) => e.heap,
        ExprFunKind::Error(e) => e.heap,
        // Non-literal expressions don't have an outer heap.
        _ => datalit::ast::Heap::Omitted,
    }
}

// ============================================================================
// Type Equivalence and Conversion
// ============================================================================

/// Check if two types are equivalent.
pub fn types_equivalent<'db>(db: &'db dyn crate::Db, t1: &Type<'db>, t2: &Type<'db>) -> bool {
    match (t1, t2) {
        (Type::Datalit(d1), Type::Datalit(d2)) => {
            datalit::tycheck::types_equivalent(db, d1, d2)
        }
        (Type::Function(f1), Type::Function(f2)) => {
            // Check parameter types.
            let p1 = f1.param_types(db);
            let p2 = f2.param_types(db);
            if p1.len() != p2.len() {
                return false;
            }
            for (param1, param2) in p1.iter().zip(p2.iter()) {
                if !types_equivalent(db, param1.ty(db), param2.ty(db)) {
                    return false;
                }
            }

            // Check return type.
            types_equivalent(db, f1.return_type(db).ty(db), f2.return_type(db).ty(db))
        }
        _ => false,
    }
}

/// Convert a type to a string for error messages.
pub fn type_to_string<'db>(db: &'db dyn crate::Db, ty: &Type<'db>) -> String {
    match ty {
        Type::Datalit(datalit_ty) => datalit::tycheck::type_to_string(db, datalit_ty),
        Type::Function(func) => {
            let params: Vec<_> = func
                .param_types(db)
                .iter()
                .map(|p| type_to_string(db, p.ty(db)))
                .collect();
            let ret = type_to_string(db, func.return_type(db).ty(db));
            format!("({}) -> {}", params.join(", "), ret)
        }
    }
}

/// Convert datafun TypeAndHeap to datalit TypeAndHeap.
pub fn to_datalit_type_and_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> Result<datalit::tycheck::TypeAndHeap<'db>, TypeError> {
    match ty.ty(db) {
        Type::Datalit(datalit_ty) => {
            Ok(datalit::tycheck::TypeAndHeap::new(db, ty.heap(db), datalit_ty.clone()))
        }
        _ => Err(TypeError::CannotSynthesize),
    }
}

/// Check that an element type is compatible with the expected element type.
///
/// Wrapper around datalit's check_element_compatible that handles datafun types.
pub fn check_element_compatible<'db>(
    db: &'db dyn crate::Db,
    expected: TypeAndHeap<'db>,
    actual: TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    let expected_datalit = to_datalit_type_and_heap(db, expected)?;
    let actual_datalit = to_datalit_type_and_heap(db, actual)?;
    datalit::tycheck::check_element_compatible(db, expected_datalit, actual_datalit)
        .map_err(TypeError::from)
}

/// Unwrap Option/Result wrappers to get inner type.
///
/// Used to check collection elements when type hint includes Option/Result.
pub fn unwrap_wrapper_types<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> TypeAndHeap<'db> {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            // Convert datalit TypeAndHeap to datafun TypeAndHeap.
            let inner = opt.inner_type;
            let datafun_inner = TypeAndHeap::new(
                db,
                inner.heap(db),
                Type::Datalit(inner.ty(db).clone()),
            );
            unwrap_wrapper_types(db, datafun_inner)
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            // Convert datalit TypeAndHeap to datafun TypeAndHeap.
            let inner = res.inner_type;
            let datafun_inner = TypeAndHeap::new(
                db,
                inner.heap(db),
                Type::Datalit(inner.ty(db).clone()),
            );
            unwrap_wrapper_types(db, datafun_inner)
        }
        _ => ty,
    }
}

// ============================================================================
// Unit Type
// ============================================================================

/// Create the unit type `()` (empty anonymous tuple).
///
/// Used as the implicit return type for void functions.
/// Memoized to avoid creating tracked structs outside of tracked functions.
#[salsa::tracked]
pub fn unit_type<'db>(db: &'db dyn crate::Db) -> TypeAndHeap<'db> {
    let unit_tuple = datalit::tycheck::TypeAnonTuple { fields: vec![] };
    TypeAndHeap::new(
        db,
        datalit::ast::Heap::Omitted,
        Type::Datalit(datalit::tycheck::Type::AnonTuple(unit_tuple)),
    )
}

/// Check if a type is the unit type `()`.
pub fn is_unit_type<'db>(db: &'db dyn crate::Db, ty: TypeAndHeap<'db>) -> bool {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonTuple(tuple)) => {
            tuple.fields.is_empty()
        }
        _ => false,
    }
}
