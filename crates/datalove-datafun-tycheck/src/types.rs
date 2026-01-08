//! Type utilities for datafun typechecking.
//!
//! Provides type predicates, conversion functions, and helper utilities.

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;

pub use crate::{Type, TypeAndHeap, TypeFunction, TypeError};

/// Check if a type is numeric.
pub fn is_numeric_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(
                datalit_ty,
                datalit::tycheck::Type::U8 |
                datalit::tycheck::Type::I8 |
                datalit::tycheck::Type::U16 |
                datalit::tycheck::Type::I16 |
                datalit::tycheck::Type::U32 |
                datalit::tycheck::Type::I32 |
                datalit::tycheck::Type::U64 |
                datalit::tycheck::Type::I64 |
                datalit::tycheck::Type::F32 |
                datalit::tycheck::Type::Int
            )
        }
        Type::Function(_) => false,
    }
}

pub fn is_float_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(datalit_ty, datalit::tycheck::Type::F32)
        }
        _ => false,
    }
}

pub fn is_bigint_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(datalit_ty, datalit::tycheck::Type::Int)
        }
        _ => false,
    }
}

pub fn is_fixed_int_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(
                datalit_ty,
                datalit::tycheck::Type::U8 |
                datalit::tycheck::Type::I8 |
                datalit::tycheck::Type::U16 |
                datalit::tycheck::Type::I16 |
                datalit::tycheck::Type::U32 |
                datalit::tycheck::Type::I32 |
                datalit::tycheck::Type::U64 |
                datalit::tycheck::Type::I64
            )
        }
        _ => false,
    }
}

pub fn is_unsigned_int_type<'db>(ty: &Type<'db>) -> bool {
    match ty {
        Type::Datalit(datalit_ty) => {
            matches!(
                datalit_ty,
                datalit::tycheck::Type::U8 |
                datalit::tycheck::Type::U16 |
                datalit::tycheck::Type::U32 |
                datalit::tycheck::Type::U64
            )
        }
        _ => false,
    }
}

/// Check if a type is boolean.
pub fn is_bool_type<'db>(ty: &Type<'db>) -> bool {
    matches!(ty, Type::Datalit(datalit::tycheck::Type::Bool))
}

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

/// Check if an integer value fits within a given type.
/// Returns Ok(()) if the value fits, Err(IntOutOfRange) if not.
pub fn check_int_fits_type(value_str: &str, ty: &datalit::tycheck::Type<'_>) -> Result<(), TypeError> {
    match ty {
        datalit::tycheck::Type::U8 => {
            value_str.parse::<u8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I8 => {
            value_str.parse::<i8>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::U16 => {
            value_str.parse::<u16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I16 => {
            value_str.parse::<i16>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::U32 => {
            value_str.parse::<u32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I32 => {
            value_str.parse::<i32>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::U64 => {
            value_str.parse::<u64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I64 => {
            value_str.parse::<i64>().map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::Int => {
            // Int is arbitrary precision, always fits.
            Ok(())
        }
        _ => Ok(()), // Non-integer types don't need range checking.
    }
}

/// Check if an integer value fits within the innermost integer type of a possibly wrapped type.
/// Handles Option<u8>, Result<u8>, etc.
pub fn check_int_fits_wrapped_type(value_str: &str, ty: &datalit::tycheck::Type<'_>, db: &dyn crate::Db) -> Result<(), TypeError> {
    match ty {
        datalit::tycheck::Type::Option(opt) => {
            check_int_fits_wrapped_type(value_str, opt.inner_type(db).ty(db), db)
        }
        datalit::tycheck::Type::Result(res) => {
            check_int_fits_wrapped_type(value_str, res.inner_type(db).ty(db), db)
        }
        _ => check_int_fits_type(value_str, ty),
    }
}

/// Check if a hex value fits within a given type.
pub fn check_hex_fits_type(value_str: &str, ty: &datalit::tycheck::Type<'_>) -> Result<(), TypeError> {
    let is_negative = value_str.starts_with('-');
    let hex_part = value_str
        .trim_start_matches('-')
        .trim_start_matches("0x")
        .trim_start_matches("0X");

    match ty {
        datalit::tycheck::Type::U8 if !is_negative => {
            u8::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I8 => {
            // For signed types, parse as unsigned first then check range.
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 128 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 127 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::U16 if !is_negative => {
            u16::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I16 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 32768 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 32767 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::U32 if !is_negative => {
            u32::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I32 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 2147483648 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 2147483647 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::U64 if !is_negative => {
            u64::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        datalit::tycheck::Type::I64 => {
            let value = u64::from_str_radix(hex_part, 16).map_err(|_| TypeError::IntOutOfRange)?;
            if is_negative {
                if value <= 9223372036854775808 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            } else {
                if value <= 9223372036854775807 { Ok(()) } else { Err(TypeError::IntOutOfRange) }
            }
        }
        datalit::tycheck::Type::Int => Ok(()), // Arbitrary precision.
        datalit::tycheck::Type::F32 if !is_negative => {
            // Hex must fit in 32 bits for f32 bit pattern.
            u32::from_str_radix(hex_part, 16).map(|_| ()).map_err(|_| TypeError::IntOutOfRange)
        }
        _ if is_negative => Err(TypeError::IntOutOfRange), // Unsigned type with negative value.
        _ => Ok(()), // Non-integer types.
    }
}

/// Check if a hex value fits within the innermost integer type of a possibly wrapped type.
pub fn check_hex_fits_wrapped_type(value_str: &str, ty: &datalit::tycheck::Type<'_>, db: &dyn crate::Db) -> Result<(), TypeError> {
    match ty {
        datalit::tycheck::Type::Option(opt) => {
            check_hex_fits_wrapped_type(value_str, opt.inner_type(db).ty(db), db)
        }
        datalit::tycheck::Type::Result(res) => {
            check_hex_fits_wrapped_type(value_str, res.inner_type(db).ty(db), db)
        }
        _ => check_hex_fits_type(value_str, ty),
    }
}

/// Unwrap Option/Result types to get the innermost heap.
/// Used when checking heap compatibility for typed literals.
pub fn unwrap_wrapper_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            unwrap_wrapper_heap_datalit(db, opt.inner_type(db))
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            unwrap_wrapper_heap_datalit(db, res.inner_type(db))
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
            unwrap_wrapper_heap_datalit(db, opt.inner_type(db))
        }
        datalit::tycheck::Type::Result(res) => {
            unwrap_wrapper_heap_datalit(db, res.inner_type(db))
        }
        _ => ty.heap(db),
    }
}

/// Check if two heaps are compatible.
/// Omitted heap is generic and compatible with any heap.
pub fn heaps_compatible(h1: datalit::ast::Heap, h2: datalit::ast::Heap) -> bool {
    use datalit::ast::Heap;
    match (h1, h2) {
        (Heap::Local, Heap::Local) => true,
        (Heap::Global, Heap::Global) => true,
        // Omitted is compatible with any heap (generic).
        (Heap::Omitted, _) => true,
        (_, Heap::Omitted) => true,
        _ => false,
    }
}

/// Convert a heap to a string for error messages.
pub fn heap_to_string(heap: datalit::ast::Heap) -> String {
    use datalit::ast::Heap;
    match heap {
        Heap::Local => "@".to_string(),
        Heap::Global => "#".to_string(),
        Heap::Omitted => "".to_string(),
    }
}

/// Extract the outer heap from an expression.
///
/// Returns the heap sigil used on the expression itself (e.g. `@` in `@{...}`).
/// Returns `Heap::Omitted` for expressions that don't have an explicit heap.
pub fn get_expr_heap<'db>(db: &'db dyn crate::Db, expr: ExprFun<'db>) -> datalit::ast::Heap {
    use ExprFunKind;
    match expr.expr(db) {
        ExprFunKind::True(lit) | ExprFunKind::False(lit) | ExprFunKind::None(lit) => lit.heap(db),
        ExprFunKind::Int(e) => e.heap(db),
        ExprFunKind::Float(e) => e.heap(db),
        ExprFunKind::Hex(e) => e.heap(db),
        ExprFunKind::String(e) => e.heap(db),
        ExprFunKind::List(e) => e.heap(db),
        ExprFunKind::Set(e) => e.heap(db),
        ExprFunKind::Map(e) => e.heap(db),
        ExprFunKind::Tensor(e) => e.heap(db),
        ExprFunKind::AnonTuple(e) => e.heap(db),
        ExprFunKind::AnonStruct(e) => e.heap(db),
        ExprFunKind::AnonEnum(e) => e.heap(db),
        ExprFunKind::Some(e) => e.heap(db),
        ExprFunKind::Ok(e) => e.heap(db),
        ExprFunKind::Er(e) => e.heap(db),
        ExprFunKind::Data(e) => e.heap(db),
        ExprFunKind::Error(e) => e.heap(db),
        // Non-literal expressions don't have an outer heap.
        _ => datalit::ast::Heap::Omitted,
    }
}

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

/// Unwrap Option/Result wrappers to get inner type.
/// Used to check collection elements when type hint includes Option/Result.
pub fn unwrap_wrapper_types<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> TypeAndHeap<'db> {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            // Convert datalit TypeAndHeap to datafun TypeAndHeap.
            let inner = opt.inner_type(db);
            let datafun_inner = TypeAndHeap::new(
                db,
                inner.heap(db),
                Type::Datalit(inner.ty(db).clone()),
            );
            unwrap_wrapper_types(db, datafun_inner)
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            // Convert datalit TypeAndHeap to datafun TypeAndHeap.
            let inner = res.inner_type(db);
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

/// Create the unit type `()` (empty anonymous tuple).
///
/// Used as the implicit return type for void functions.
/// Memoized to avoid creating tracked structs outside of tracked functions.
#[salsa::tracked]
pub fn unit_type<'db>(db: &'db dyn crate::Db) -> TypeAndHeap<'db> {
    let unit_tuple = datalit::tycheck::TypeAnonTuple::new(db, vec![]);
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
            tuple.fields(db).is_empty()
        }
        _ => false,
    }
}
