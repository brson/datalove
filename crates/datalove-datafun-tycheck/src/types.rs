//! Type utilities for datafun typechecking.
//!
//! Organization (parallel to datalit/tycheck/types.rs):
//! 1. Re-exports - datalit types
//! 2. Heap utilities - heap functions
//! 3. Type predicates - type queries
//! 4. Type equivalence - comparing types
//! 5. Type hint conversion - AST to types
//! 6. Type to string - types to strings for errors
//! 7. Unit type helper - type constructor
//! 8. Element compatibility - delegated to datalit
//! 9. Integer range checking - delegated to datalit
//! 10. Heap unwrapping - unwrap wrapper types
//! 11. Expression heap extraction - get heap from expressions

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use datalit::ast::TypeHint;
use crate::{Type, TypeError, TypeAndHeap};

// ============================================================================
// Re-exports
// ============================================================================

pub use datalit::tycheck::{
    TypeAnonTuple,
    TypeAnonStruct,
    TypeNamedField,
    TypeAnonEnum,
    TypeEnumVariant,
    TypeList,
    TypeMap,
    TypeSet,
    TypeOption,
    TypeResult,
    TypeTensor,
    TypeTable,
};

// ============================================================================
// Type Predicates
// ============================================================================

/// Check if a type is numeric.
pub fn is_numeric_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_numeric_type(dt),
        _ => false,
    }
}

/// Check if a type is a floating-point type.
pub fn is_float_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_float_type(dt),
        _ => false,
    }
}

/// Check if a type is the arbitrary-precision integer type.
pub fn is_bigint_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_bigint_type(dt),
        _ => false,
    }
}

/// Check if a type is a fixed-size integer type.
pub fn is_fixed_int_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_fixed_int_type(dt),
        _ => false,
    }
}

/// Check if a type is an unsigned integer type.
pub fn is_unsigned_int_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_unsigned_int_type(dt),
        _ => false,
    }
}

/// Check if a type is a signed fixed-size integer type (i8, i16, i32, i64).
pub fn is_signed_fixed_int_type(ty: &Type<'_>) -> bool {
    is_fixed_int_type(ty) && !is_unsigned_int_type(ty)
}

/// Check if a type is boolean.
pub fn is_bool_type(ty: &Type<'_>) -> bool {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::is_bool_type(dt),
        _ => false,
    }
}

// ============================================================================
// Type Equivalence
// ============================================================================

/// Check if two types are equivalent.
pub fn types_equivalent<'db>(db: &'db dyn crate::Db, t1: &Type<'db>, t2: &Type<'db>) -> bool {
    match (t1, t2) {
        (Type::Datalit(dt1), Type::Datalit(dt2)) => datalit::tycheck::types_equivalent(db, dt1, dt2),
        (Type::Function(f1), Type::Function(f2)) => {
            f1.param_types(db).len() == f2.param_types(db).len()
                && f1.param_types(db).iter().zip(f2.param_types(db).iter())
                    .all(|(a, b)| types_equivalent(db, a.ty(db), b.ty(db)))
                && types_equivalent(db, f1.return_type(db).ty(db), f2.return_type(db).ty(db))
        }
        _ => false,
    }
}

// ============================================================================
// Primitive Type Names
// ============================================================================

/// Check if a name is a primitive type name that cannot be shadowed.
pub fn is_primitive_name(name: &str) -> bool {
    matches!(name,
        "bool" | "u8" | "i8" | "u16" | "i16" | "u32" | "i32" | "u64" | "i64" |
        "usize" | "isize" | "f32" | "f64" | "int" | "string" | "data" | "error" |
        "tuple" | "enum" | "map" | "set" | "tensor"
    )
}

// ============================================================================
// Type Hint Conversion
// ============================================================================

/// Convert a type hint to a type.
pub fn convert_type_hint<'db>(
    db: &'db dyn crate::Db,
    type_hint: TypeHint<'db>,
) -> Result<crate::TypeAndHeap<'db>, TypeError> {
    convert_type_hint_inner(db, type_hint)
}

fn convert_type_hint_inner<'db>(
    db: &'db dyn crate::Db,
    type_hint: TypeHint<'db>,
) -> Result<crate::TypeAndHeap<'db>, TypeError> {
    let heap = datalove_datalit::ast_serde::Heap::Omitted;
    let ty = match type_hint {
        TypeHint::Bool => Type::Datalit(datalit::tycheck::Type::Bool),
        TypeHint::U8 => Type::Datalit(datalit::tycheck::Type::U8),
        TypeHint::I8 => Type::Datalit(datalit::tycheck::Type::I8),
        TypeHint::U16 => Type::Datalit(datalit::tycheck::Type::U16),
        TypeHint::I16 => Type::Datalit(datalit::tycheck::Type::I16),
        TypeHint::U32 => Type::Datalit(datalit::tycheck::Type::U32),
        TypeHint::I32 => Type::Datalit(datalit::tycheck::Type::I32),
        TypeHint::U64 => Type::Datalit(datalit::tycheck::Type::U64),
        TypeHint::I64 => Type::Datalit(datalit::tycheck::Type::I64),
        TypeHint::Usize => Type::Datalit(datalit::tycheck::Type::Usize),
        TypeHint::Isize => Type::Datalit(datalit::tycheck::Type::Isize),
        TypeHint::F32 => Type::Datalit(datalit::tycheck::Type::F32),
        TypeHint::F64 => Type::Datalit(datalit::tycheck::Type::F64),
        TypeHint::Int => Type::Datalit(datalit::tycheck::Type::Int),
        TypeHint::String => Type::Datalit(datalit::tycheck::Type::String),
        TypeHint::Data => Type::Datalit(datalit::tycheck::Type::Data),
        TypeHint::Error => Type::Datalit(datalit::tycheck::Type::Error),

        TypeHint::AnonTuple(t) => {
            let fields: Result<Vec<_>, TypeError> = t.fields.iter()
                .map(|f| {
                    let f_ty = convert_type_hint_inner(db, f.clone())?;
                    match f_ty.ty(db) {
                        Type::Datalit(dt) => Ok(dt.clone()),
                        Type::Function(_) => Err(TypeError::CannotSynthesize),
                    }
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonTuple(
                datalit::tycheck::TypeAnonTuple { fields: fields? }
            ))
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, TypeError> = s.fields.iter()
                .map(|f| {
                    let name = f.name;
                    let f_ty = convert_type_hint_inner(db, (*f.type_hint).clone())?;
                    let dt = match f_ty.ty(db) {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => return Err(TypeError::CannotSynthesize),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonStruct(
                datalit::tycheck::TypeAnonStruct { fields: fields? }
            ))
        }

        TypeHint::AnonEnum(e) => {
            let variants: Result<Vec<_>, TypeError> = e.variants.iter()
                .map(|v| {
                    let name = v.name;
                    let payload = v.payload.as_ref().map(|p| {
                        let p_ty = convert_type_hint_inner(db, (**p).clone())?;
                        match p_ty.ty(db) {
                            Type::Datalit(dt) => Ok(Box::new(dt.clone())),
                            Type::Function(_) => Err(TypeError::CannotSynthesize),
                        }
                    }).transpose()?;
                    Ok(datalit::tycheck::TypeEnumVariant { name, payload })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonEnum(
                datalit::tycheck::TypeAnonEnum { variants: variants? }
            ))
        }

        TypeHint::List(l) => {
            let elem_ty = convert_type_hint_inner(db, (*l.element_type).clone())?;
            let dt = match elem_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::List(
                datalit::tycheck::TypeList {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Map(m) => {
            let key_ty = convert_type_hint_inner(db, (*m.key_type).clone())?;
            let key_dt = match key_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            let value_ty = convert_type_hint_inner(db, (*m.value_type).clone())?;
            let value_dt = match value_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Map(
                datalit::tycheck::TypeMap {
                    key_type: Box::new(key_dt),
                    value_type: Box::new(value_dt),
                }
            ))
        }

        TypeHint::Set(s) => {
            let elem_ty = convert_type_hint_inner(db, (*s.element_type).clone())?;
            let dt = match elem_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Set(
                datalit::tycheck::TypeSet {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Option(o) => {
            let inner_ty = convert_type_hint_inner(db, (*o.inner_type).clone())?;
            let dt = match inner_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Result(r) => {
            let inner_ty = convert_type_hint_inner(db, (*r.inner_type).clone())?;
            let dt = match inner_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Tensor(t) => {
            let elem_ty = convert_type_hint_inner(db, (*t.element_type).clone())?;
            let dt = match elem_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Tensor(
                datalit::tycheck::TypeTensor {
                    element_type: Box::new(dt),
                    rank: t.rank,
                }
            ))
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),
        TypeHint::Alias(name) => {
            // Type alias cannot be resolved without alias map.
            return Err(TypeError::UnresolvedTypeAlias(name.as_str(db).to_string()));
        }
        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, TypeError> = t.columns.iter()
                .map(|c| {
                    let name = c.name;
                    let c_ty = convert_type_hint_inner(db, (*c.type_hint).clone())?;
                    let dt = match c_ty.ty(db) {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => return Err(TypeError::CannotSynthesize),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::Table(
                datalit::tycheck::TypeTable { columns: columns? }
            ))
        }
    };

    Ok(TypeAndHeap::new(db, heap, ty))
}

use std::collections::HashMap;
use bct::text::InternedText;

/// Convert a type hint to a type, resolving type aliases.
pub fn convert_type_hint_with_aliases<'db>(
    db: &'db dyn crate::Db,
    type_hint: TypeHint<'db>,
    aliases: &HashMap<InternedText<'db>, TypeAndHeap<'db>>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    convert_type_hint_with_aliases_inner(db, type_hint, aliases)
}

fn convert_type_hint_with_aliases_inner<'db>(
    db: &'db dyn crate::Db,
    type_hint: TypeHint<'db>,
    aliases: &HashMap<InternedText<'db>, TypeAndHeap<'db>>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let heap = datalove_datalit::ast_serde::Heap::Omitted;
    let ty = match type_hint {
        TypeHint::Bool => Type::Datalit(datalit::tycheck::Type::Bool),
        TypeHint::U8 => Type::Datalit(datalit::tycheck::Type::U8),
        TypeHint::I8 => Type::Datalit(datalit::tycheck::Type::I8),
        TypeHint::U16 => Type::Datalit(datalit::tycheck::Type::U16),
        TypeHint::I16 => Type::Datalit(datalit::tycheck::Type::I16),
        TypeHint::U32 => Type::Datalit(datalit::tycheck::Type::U32),
        TypeHint::I32 => Type::Datalit(datalit::tycheck::Type::I32),
        TypeHint::U64 => Type::Datalit(datalit::tycheck::Type::U64),
        TypeHint::I64 => Type::Datalit(datalit::tycheck::Type::I64),
        TypeHint::Usize => Type::Datalit(datalit::tycheck::Type::Usize),
        TypeHint::Isize => Type::Datalit(datalit::tycheck::Type::Isize),
        TypeHint::F32 => Type::Datalit(datalit::tycheck::Type::F32),
        TypeHint::F64 => Type::Datalit(datalit::tycheck::Type::F64),
        TypeHint::Int => Type::Datalit(datalit::tycheck::Type::Int),
        TypeHint::String => Type::Datalit(datalit::tycheck::Type::String),
        TypeHint::Data => Type::Datalit(datalit::tycheck::Type::Data),
        TypeHint::Error => Type::Datalit(datalit::tycheck::Type::Error),

        TypeHint::AnonTuple(t) => {
            let fields: Result<Vec<_>, TypeError> = t.fields.iter()
                .map(|f| {
                    let f_ty = convert_type_hint_with_aliases_inner(db, f.clone(), aliases)?;
                    match f_ty.ty(db) {
                        Type::Datalit(dt) => Ok(dt.clone()),
                        Type::Function(_) => Err(TypeError::CannotSynthesize),
                    }
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonTuple(
                datalit::tycheck::TypeAnonTuple { fields: fields? }
            ))
        }

        TypeHint::AnonStruct(s) => {
            let fields: Result<Vec<_>, TypeError> = s.fields.iter()
                .map(|f| {
                    let name = f.name;
                    let f_ty = convert_type_hint_with_aliases_inner(db, (*f.type_hint).clone(), aliases)?;
                    let dt = match f_ty.ty(db) {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => return Err(TypeError::CannotSynthesize),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonStruct(
                datalit::tycheck::TypeAnonStruct { fields: fields? }
            ))
        }

        TypeHint::AnonEnum(e) => {
            let variants: Result<Vec<_>, TypeError> = e.variants.iter()
                .map(|v| {
                    let name = v.name;
                    let payload = v.payload.as_ref().map(|p| {
                        let p_ty = convert_type_hint_with_aliases_inner(db, (**p).clone(), aliases)?;
                        match p_ty.ty(db) {
                            Type::Datalit(dt) => Ok(Box::new(dt.clone())),
                            Type::Function(_) => Err(TypeError::CannotSynthesize),
                        }
                    }).transpose()?;
                    Ok(datalit::tycheck::TypeEnumVariant { name, payload })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonEnum(
                datalit::tycheck::TypeAnonEnum { variants: variants? }
            ))
        }

        TypeHint::List(l) => {
            let elem_ty = convert_type_hint_with_aliases_inner(db, (*l.element_type).clone(), aliases)?;
            let dt = match elem_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::List(
                datalit::tycheck::TypeList {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Map(m) => {
            let key_ty = convert_type_hint_with_aliases_inner(db, (*m.key_type).clone(), aliases)?;
            let key_dt = match key_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            let value_ty = convert_type_hint_with_aliases_inner(db, (*m.value_type).clone(), aliases)?;
            let value_dt = match value_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Map(
                datalit::tycheck::TypeMap {
                    key_type: Box::new(key_dt),
                    value_type: Box::new(value_dt),
                }
            ))
        }

        TypeHint::Set(s) => {
            let elem_ty = convert_type_hint_with_aliases_inner(db, (*s.element_type).clone(), aliases)?;
            let dt = match elem_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Set(
                datalit::tycheck::TypeSet {
                    element_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Option(o) => {
            let inner_ty = convert_type_hint_with_aliases_inner(db, (*o.inner_type).clone(), aliases)?;
            let dt = match inner_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Result(r) => {
            let inner_ty = convert_type_hint_with_aliases_inner(db, (*r.inner_type).clone(), aliases)?;
            let dt = match inner_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult {
                    inner_type: Box::new(dt)
                }
            ))
        }

        TypeHint::Tensor(t) => {
            let elem_ty = convert_type_hint_with_aliases_inner(db, (*t.element_type).clone(), aliases)?;
            let dt = match elem_ty.ty(db) {
                Type::Datalit(dt) => dt.clone(),
                Type::Function(_) => return Err(TypeError::CannotSynthesize),
            };
            Type::Datalit(datalit::tycheck::Type::Tensor(
                datalit::tycheck::TypeTensor {
                    element_type: Box::new(dt),
                    rank: t.rank,
                }
            ))
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),

        TypeHint::Alias(name) => {
            // Look up the alias in the map.
            if let Some(resolved_ty) = aliases.get(&name) {
                return Ok(resolved_ty.clone());
            }
            return Err(TypeError::UnresolvedTypeAlias(name.as_str(db).to_string()));
        }

        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, TypeError> = t.columns.iter()
                .map(|c| {
                    let name = c.name;
                    let c_ty = convert_type_hint_with_aliases_inner(db, (*c.type_hint).clone(), aliases)?;
                    let dt = match c_ty.ty(db) {
                        Type::Datalit(dt) => dt.clone(),
                        Type::Function(_) => return Err(TypeError::CannotSynthesize),
                    };
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: Box::new(dt),
                    })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::Table(
                datalit::tycheck::TypeTable { columns: columns? }
            ))
        }
    };

    Ok(TypeAndHeap::new(db, heap, ty))
}

// ============================================================================
// Type to String
// ============================================================================

/// Convert a type to a string for error messages.
pub fn type_to_string<'db>(db: &'db dyn crate::Db, ty: &Type<'db>) -> String {
    match ty {
        Type::Datalit(dt) => datalit::tycheck::type_to_string(db, dt),
        Type::Function(f) => {
            let params: Vec<_> = f.param_types(db).iter()
                .map(|p| type_to_string(db, p.ty(db)))
                .collect();
            let ret = type_to_string(db, f.return_type(db).ty(db));
            format!("fn({}) -> {}", params.join(", "), ret)
        }
    }
}

// ============================================================================
// Unit Type Helper
// ============================================================================

/// Create the unit type `()`.
pub fn unit_type<'db>(db: &'db dyn crate::Db) -> TypeAndHeap<'db> {
    let datalit_unit = datalit::tycheck::unit_type();
    TypeAndHeap::new(
        db,
        datalove_datalit::ast_serde::Heap::Omitted,
        Type::Datalit(datalit_unit)
    )
}

// ============================================================================
// Element Compatibility
// ============================================================================

/// Check that an element type is compatible with the expected element type.
pub fn check_element_compatible<'db>(
    db: &'db dyn crate::Db,
    expected: &TypeAndHeap<'db>,
    actual: &TypeAndHeap<'db>,
) -> Result<(), TypeError> {
    if !types_equivalent(db, expected.ty(db), actual.ty(db)) {
        return Err(TypeError::TypeMismatch {
            expected: type_to_string(db, expected.ty(db)),
            actual: type_to_string(db, actual.ty(db)),
        });
    }
    Ok(())
}

// ============================================================================
// Integer Range Checking
// ============================================================================

/// Check if an integer value fits within a type (delegated to datalit).
pub fn check_int_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &datalit::tycheck::Type<'db>,
    _db: &'db dyn crate::Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_int_fits_wrapped_type(value_str, ty)
        .map_err(TypeError::from)
}

/// Check if a hex value fits within a type (delegated to datalit).
pub fn check_hex_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &datalit::tycheck::Type<'db>,
    _db: &'db dyn crate::Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_hex_fits_wrapped_type(value_str, ty)
        .map_err(TypeError::from)
}

// ============================================================================
// Heap Unwrapping
// ============================================================================

/// Unwrap Option/Result wrappers to get the innermost type.
pub fn unwrap_wrapper_types<'db>(
    db: &'db dyn crate::Db,
    ty: &TypeAndHeap<'db>,
) -> TypeAndHeap<'db> {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            TypeAndHeap::new(db, datalove_datalit::ast_serde::Heap::Omitted, Type::Datalit((*opt.inner_type).clone()))
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            TypeAndHeap::new(db, datalove_datalit::ast_serde::Heap::Omitted, Type::Datalit((*res.inner_type).clone()))
        }
        _ => ty.clone(),
    }
}

/// Get the heap from the innermost type (unwrapping Option/Result).
/// Always returns Omitted since heaps have been removed.
pub fn unwrap_wrapper_heap<'db>(
    _db: &'db dyn crate::Db,
    _ty: &TypeAndHeap<'db>,
) -> datalove_datalit::ast_serde::Heap {
    datalove_datalit::ast_serde::Heap::Omitted
}

/// Get the heap from a datalit TypeAndHeap (unwrapping Option/Result).
/// Always returns Omitted since heaps have been removed.
pub fn unwrap_wrapper_heap_datalit<'db>(
    _db: &'db dyn crate::Db,
    _ty: &crate::TypeAndHeap<'db>,
) -> datalove_datalit::ast_serde::Heap {
    datalove_datalit::ast_serde::Heap::Omitted
}

// ============================================================================
// Expression Heap Extraction
// ============================================================================

/// Extract the heap from an expression.
/// Always returns Omitted since heaps have been removed from expressions.
pub fn get_expr_heap<'db>(_db: &'db dyn crate::Db, _expr: ExprFun<'db>) -> datalove_datalit::ast_serde::Heap {
    datalove_datalit::ast_serde::Heap::Omitted
}

// ============================================================================
// Type Conversion Helpers
// ============================================================================

/// Convert datafun TypeAndHeap to datalit TypeAndHeap.
pub fn to_datalit_type_and_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> Result<crate::TypeAndHeap<'db>, TypeError> {
    match ty.ty(db) {
        Type::Datalit(dt) => Ok(crate::TypeAndHeap::new(db, ty.heap(db), Type::Datalit(dt.clone()))),
        Type::Function(_) => Err(TypeError::CannotSynthesize),
    }
}
