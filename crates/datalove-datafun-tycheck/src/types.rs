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
use datalit::ast::{TypeHint, TypeHintAndHeap};
use crate::{Type, TypeAndHeap, TypeError};

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
// Heap Utilities
// ============================================================================

pub use datalit::tycheck::heaps_compatible;
pub use datalit::tycheck::heap_to_string;

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
    type_hint_and_heap: TypeHintAndHeap<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let heap = type_hint_and_heap.heap(db);
    let type_hint = type_hint_and_heap.type_hint(db);
    convert_type_hint_inner(db, heap, type_hint)
}

fn convert_type_hint_inner<'db>(
    db: &'db dyn crate::Db,
    heap: datalit::ast::Heap,
    type_hint: TypeHint<'db>,
) -> Result<TypeAndHeap<'db>, TypeError> {
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
                    let f_heap = f.heap(db);
                    let f_hint = f.type_hint(db);
                    let f_ty = convert_type_hint_inner(db, f_heap, f_hint)?;
                    to_datalit_type_and_heap(db, f_ty)
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
                    let f_heap = f.type_hint.heap(db);
                    let f_hint = f.type_hint.type_hint(db);
                    let f_ty = convert_type_hint_inner(db, f_heap, f_hint)?;
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: to_datalit_type_and_heap(db, f_ty)?,
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
                    let payload = v.payload.map(|p| {
                        let p_heap = p.heap(db);
                        let p_hint = p.type_hint(db);
                        let p_ty = convert_type_hint_inner(db, p_heap, p_hint)?;
                        to_datalit_type_and_heap(db, p_ty)
                    }).transpose()?;
                    Ok(datalit::tycheck::TypeEnumVariant { name, payload })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonEnum(
                datalit::tycheck::TypeAnonEnum { variants: variants? }
            ))
        }

        TypeHint::List(l) => {
            let elem_heap = l.element_type.heap(db);
            let elem_hint = l.element_type.type_hint(db);
            let elem_ty = convert_type_hint_inner(db, elem_heap, elem_hint)?;
            Type::Datalit(datalit::tycheck::Type::List(
                datalit::tycheck::TypeList {
                    element_type: to_datalit_type_and_heap(db, elem_ty)?
                }
            ))
        }

        TypeHint::Map(m) => {
            let key_heap = m.key_type.heap(db);
            let key_hint = m.key_type.type_hint(db);
            let key_ty = convert_type_hint_inner(db, key_heap, key_hint)?;
            let value_heap = m.value_type.heap(db);
            let value_hint = m.value_type.type_hint(db);
            let value_ty = convert_type_hint_inner(db, value_heap, value_hint)?;
            Type::Datalit(datalit::tycheck::Type::Map(
                datalit::tycheck::TypeMap {
                    key_type: to_datalit_type_and_heap(db, key_ty)?,
                    value_type: to_datalit_type_and_heap(db, value_ty)?,
                }
            ))
        }

        TypeHint::Set(s) => {
            let elem_heap = s.element_type.heap(db);
            let elem_hint = s.element_type.type_hint(db);
            let elem_ty = convert_type_hint_inner(db, elem_heap, elem_hint)?;
            Type::Datalit(datalit::tycheck::Type::Set(
                datalit::tycheck::TypeSet {
                    element_type: to_datalit_type_and_heap(db, elem_ty)?
                }
            ))
        }

        TypeHint::Option(o) => {
            let inner_heap = o.inner_type.heap(db);
            let inner_hint = o.inner_type.type_hint(db);
            let inner_ty = convert_type_hint_inner(db, inner_heap, inner_hint)?;
            Type::Datalit(datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption {
                    inner_type: to_datalit_type_and_heap(db, inner_ty)?
                }
            ))
        }

        TypeHint::Result(r) => {
            let inner_heap = r.inner_type.heap(db);
            let inner_hint = r.inner_type.type_hint(db);
            let inner_ty = convert_type_hint_inner(db, inner_heap, inner_hint)?;
            Type::Datalit(datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult {
                    inner_type: to_datalit_type_and_heap(db, inner_ty)?
                }
            ))
        }

        TypeHint::Tensor(t) => {
            let elem_heap = t.element_type.heap(db);
            let elem_hint = t.element_type.type_hint(db);
            let elem_ty = convert_type_hint_inner(db, elem_heap, elem_hint)?;
            Type::Datalit(datalit::tycheck::Type::Tensor(
                datalit::tycheck::TypeTensor {
                    element_type: to_datalit_type_and_heap(db, elem_ty)?,
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
                    let c_heap = c.type_hint.heap(db);
                    let c_hint = c.type_hint.type_hint(db);
                    let c_ty = convert_type_hint_inner(db, c_heap, c_hint)?;
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: to_datalit_type_and_heap(db, c_ty)?,
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
    type_hint_and_heap: TypeHintAndHeap<'db>,
    aliases: &HashMap<InternedText<'db>, TypeAndHeap<'db>>,
) -> Result<TypeAndHeap<'db>, TypeError> {
    let heap = type_hint_and_heap.heap(db);
    let type_hint = type_hint_and_heap.type_hint(db);
    convert_type_hint_with_aliases_inner(db, heap, type_hint, aliases)
}

fn convert_type_hint_with_aliases_inner<'db>(
    db: &'db dyn crate::Db,
    heap: datalit::ast::Heap,
    type_hint: TypeHint<'db>,
    aliases: &HashMap<InternedText<'db>, TypeAndHeap<'db>>,
) -> Result<TypeAndHeap<'db>, TypeError> {
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
                    let f_heap = f.heap(db);
                    let f_hint = f.type_hint(db);
                    let f_ty = convert_type_hint_with_aliases_inner(db, f_heap, f_hint, aliases)?;
                    to_datalit_type_and_heap(db, f_ty)
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
                    let f_heap = f.type_hint.heap(db);
                    let f_hint = f.type_hint.type_hint(db);
                    let f_ty = convert_type_hint_with_aliases_inner(db, f_heap, f_hint, aliases)?;
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: to_datalit_type_and_heap(db, f_ty)?,
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
                    let payload = v.payload.map(|p| {
                        let p_heap = p.heap(db);
                        let p_hint = p.type_hint(db);
                        let p_ty = convert_type_hint_with_aliases_inner(db, p_heap, p_hint, aliases)?;
                        to_datalit_type_and_heap(db, p_ty)
                    }).transpose()?;
                    Ok(datalit::tycheck::TypeEnumVariant { name, payload })
                })
                .collect();
            Type::Datalit(datalit::tycheck::Type::AnonEnum(
                datalit::tycheck::TypeAnonEnum { variants: variants? }
            ))
        }

        TypeHint::List(l) => {
            let elem_heap = l.element_type.heap(db);
            let elem_hint = l.element_type.type_hint(db);
            let elem_ty = convert_type_hint_with_aliases_inner(db, elem_heap, elem_hint, aliases)?;
            Type::Datalit(datalit::tycheck::Type::List(
                datalit::tycheck::TypeList {
                    element_type: to_datalit_type_and_heap(db, elem_ty)?
                }
            ))
        }

        TypeHint::Map(m) => {
            let key_heap = m.key_type.heap(db);
            let key_hint = m.key_type.type_hint(db);
            let key_ty = convert_type_hint_with_aliases_inner(db, key_heap, key_hint, aliases)?;
            let value_heap = m.value_type.heap(db);
            let value_hint = m.value_type.type_hint(db);
            let value_ty = convert_type_hint_with_aliases_inner(db, value_heap, value_hint, aliases)?;
            Type::Datalit(datalit::tycheck::Type::Map(
                datalit::tycheck::TypeMap {
                    key_type: to_datalit_type_and_heap(db, key_ty)?,
                    value_type: to_datalit_type_and_heap(db, value_ty)?,
                }
            ))
        }

        TypeHint::Set(s) => {
            let elem_heap = s.element_type.heap(db);
            let elem_hint = s.element_type.type_hint(db);
            let elem_ty = convert_type_hint_with_aliases_inner(db, elem_heap, elem_hint, aliases)?;
            Type::Datalit(datalit::tycheck::Type::Set(
                datalit::tycheck::TypeSet {
                    element_type: to_datalit_type_and_heap(db, elem_ty)?
                }
            ))
        }

        TypeHint::Option(o) => {
            let inner_heap = o.inner_type.heap(db);
            let inner_hint = o.inner_type.type_hint(db);
            let inner_ty = convert_type_hint_with_aliases_inner(db, inner_heap, inner_hint, aliases)?;
            Type::Datalit(datalit::tycheck::Type::Option(
                datalit::tycheck::TypeOption {
                    inner_type: to_datalit_type_and_heap(db, inner_ty)?
                }
            ))
        }

        TypeHint::Result(r) => {
            let inner_heap = r.inner_type.heap(db);
            let inner_hint = r.inner_type.type_hint(db);
            let inner_ty = convert_type_hint_with_aliases_inner(db, inner_heap, inner_hint, aliases)?;
            Type::Datalit(datalit::tycheck::Type::Result(
                datalit::tycheck::TypeResult {
                    inner_type: to_datalit_type_and_heap(db, inner_ty)?
                }
            ))
        }

        TypeHint::Tensor(t) => {
            let elem_heap = t.element_type.heap(db);
            let elem_hint = t.element_type.type_hint(db);
            let elem_ty = convert_type_hint_with_aliases_inner(db, elem_heap, elem_hint, aliases)?;
            Type::Datalit(datalit::tycheck::Type::Tensor(
                datalit::tycheck::TypeTensor {
                    element_type: to_datalit_type_and_heap(db, elem_ty)?,
                    rank: t.rank,
                }
            ))
        }

        TypeHint::ParseError(_) => return Err(TypeError::CannotSynthesize),

        TypeHint::Alias(name) => {
            // Look up the alias in the map.
            if let Some(resolved_ty) = aliases.get(&name) {
                return Ok(*resolved_ty);
            }
            return Err(TypeError::UnresolvedTypeAlias(name.as_str(db).to_string()));
        }

        TypeHint::Table(t) => {
            let columns: Result<Vec<_>, TypeError> = t.columns.iter()
                .map(|c| {
                    let name = c.name;
                    let c_heap = c.type_hint.heap(db);
                    let c_hint = c.type_hint.type_hint(db);
                    let c_ty = convert_type_hint_with_aliases_inner(db, c_heap, c_hint, aliases)?;
                    Ok(datalit::tycheck::TypeNamedField {
                        name,
                        ty: to_datalit_type_and_heap(db, c_ty)?,
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
    let datalit_unit = datalit::tycheck::unit_type(db, datalit::ast::Heap::Omitted);
    TypeAndHeap::new(
        db,
        datalit::ast::Heap::Omitted,
        Type::Datalit(datalit_unit.ty(db).clone())
    )
}

// ============================================================================
// Element Compatibility
// ============================================================================

/// Check that an element type is compatible with the expected element type.
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

// ============================================================================
// Integer Range Checking
// ============================================================================

/// Check if an integer value fits within a type (delegated to datalit).
pub fn check_int_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &datalit::tycheck::Type<'db>,
    db: &'db dyn crate::Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_int_fits_wrapped_type(value_str, ty, db)
        .map_err(TypeError::from)
}

/// Check if a hex value fits within a type (delegated to datalit).
pub fn check_hex_fits_wrapped_type<'db>(
    value_str: &str,
    ty: &datalit::tycheck::Type<'db>,
    db: &'db dyn crate::Db,
) -> Result<(), TypeError> {
    datalit::tycheck::check_hex_fits_wrapped_type(value_str, ty, db)
        .map_err(TypeError::from)
}

// ============================================================================
// Heap Unwrapping
// ============================================================================

/// Unwrap Option/Result wrappers to get the innermost type.
pub fn unwrap_wrapper_types<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> TypeAndHeap<'db> {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => {
            let inner = opt.inner_type;
            TypeAndHeap::new(db, inner.heap(db), Type::Datalit(inner.ty(db).clone()))
        }
        Type::Datalit(datalit::tycheck::Type::Result(res)) => {
            let inner = res.inner_type;
            TypeAndHeap::new(db, inner.heap(db), Type::Datalit(inner.ty(db).clone()))
        }
        _ => ty,
    }
}

/// Get the heap from the innermost type (unwrapping Option/Result).
pub fn unwrap_wrapper_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::Option(opt)) => opt.inner_type.heap(db),
        Type::Datalit(datalit::tycheck::Type::Result(res)) => res.inner_type.heap(db),
        _ => ty.heap(db),
    }
}

/// Get the heap from a datalit TypeAndHeap (unwrapping Option/Result).
pub fn unwrap_wrapper_heap_datalit<'db>(
    db: &'db dyn crate::Db,
    ty: datalit::tycheck::TypeAndHeap<'db>,
) -> datalit::ast::Heap {
    match ty.ty(db) {
        datalit::tycheck::Type::Option(opt) => opt.inner_type.heap(db),
        datalit::tycheck::Type::Result(res) => res.inner_type.heap(db),
        _ => ty.heap(db),
    }
}

// ============================================================================
// Expression Heap Extraction
// ============================================================================

/// Extract the heap from an expression.
pub fn get_expr_heap<'db>(db: &'db dyn crate::Db, expr: ExprFun<'db>) -> datalit::ast::Heap {
    match expr.expr(db) {
        ExprFunKind::True(lit) => lit.heap,
        ExprFunKind::False(lit) => lit.heap,
        ExprFunKind::None(lit) => lit.heap,
        ExprFunKind::Int(int_expr) => int_expr.heap,
        ExprFunKind::Float(float_expr) => float_expr.heap,
        ExprFunKind::Hex(hex_expr) => hex_expr.heap,
        ExprFunKind::String(str_expr) => str_expr.heap,
        ExprFunKind::List(list_expr) => list_expr.heap,
        ExprFunKind::Set(set_expr) => set_expr.heap,
        ExprFunKind::Map(map_expr) => map_expr.heap,
        ExprFunKind::Tensor(tensor_expr) => tensor_expr.heap,
        ExprFunKind::AnonTuple(tuple_expr) => tuple_expr.heap,
        ExprFunKind::AnonStruct(struct_expr) => struct_expr.heap,
        ExprFunKind::AnonEnum(enum_expr) => enum_expr.heap,
        ExprFunKind::Some(some_expr) => some_expr.heap,
        ExprFunKind::Ok(ok_expr) => ok_expr.heap,
        ExprFunKind::Er(er_expr) => er_expr.heap,
        ExprFunKind::Data(data_expr) => data_expr.heap,
        ExprFunKind::Error(err_expr) => err_expr.heap,
        ExprFunKind::Table(table_expr) => table_expr.heap,
        // Non-literal expressions use Omitted heap.
        ExprFunKind::Name(_)
        | ExprFunKind::BinOp(_)
        | ExprFunKind::UnaryOp(_)
        | ExprFunKind::FunctionCall(_)
        | ExprFunKind::IntrinsicCall(_)
        | ExprFunKind::Tuple(_)
        | ExprFunKind::TryOption(_)
        | ExprFunKind::TryResult(_)
        | ExprFunKind::FieldProj(_)
        | ExprFunKind::ParseError(_) => datalit::ast::Heap::Omitted,
    }
}

// ============================================================================
// Type Conversion Helpers
// ============================================================================

/// Convert datafun TypeAndHeap to datalit TypeAndHeap.
pub fn to_datalit_type_and_heap<'db>(
    db: &'db dyn crate::Db,
    ty: TypeAndHeap<'db>,
) -> Result<datalit::tycheck::TypeAndHeap<'db>, TypeError> {
    match ty.ty(db) {
        Type::Datalit(dt) => Ok(datalit::tycheck::TypeAndHeap::new(db, ty.heap(db), dt.clone())),
        Type::Function(_) => Err(TypeError::CannotSynthesize),
    }
}
