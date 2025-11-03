//! Copy type detection for linear type system.
//!
//! Determines which types have copy semantics (can be bitwise copied)
//! vs. linear semantics (require ownership tracking and explicit drops).

use rmx::prelude::*;

/// Check if a datafun type has copy semantics.
///
/// Returns true if the type can be safely bitwise copied without
/// requiring ownership tracking or drop operations.
pub fn is_copy_type<'db>(
    db: &'db dyn crate::Db,
    ty: crate::tycheck::TypeAndHeap<'db>
) -> bool {
    use crate::tycheck::Type;
    match ty.ty(db) {
        Type::Datalit(datalit_ty) => is_datalit_copy(db, datalit_ty),
        Type::Function(_) => false,  // Functions are not copy.
        Type::Void => true,           // Void is trivially copyable.
    }
}

/// Convert a datalit TypeAndHeap to a datafun TypeAndHeap.
fn convert_datalit_type<'db>(
    db: &'db dyn crate::Db,
    datalit_ty: crate::datalit::tycheck::TypeAndHeap<'db>,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::tycheck::{Type, TypeAndHeap};

    TypeAndHeap::new(
        db,
        datalit_ty.heap(db),
        Type::Datalit(datalit_ty.ty(db).clone())
    )
}

/// Check if a datalit type has copy semantics.
fn is_datalit_copy<'db>(
    db: &'db dyn crate::Db,
    ty: &crate::datalit::tycheck::Type<'db>
) -> bool {
    use crate::datalit::tycheck::Type;

    match ty {
        // Scalar primitives - always copy.
        Type::Bool => true,
        Type::U8 | Type::I8 => true,
        Type::U16 | Type::I16 => true,
        Type::U32 | Type::I32 => true,
        Type::U64 | Type::I64 => true,
        Type::F32 => true,

        // Heap-allocated types - never copy.
        Type::Int => false,        // bigint (heap)
        Type::String => false,     // heap buffer
        Type::List(_) => false,    // heap collection
        Type::Map(_) => false,     // heap collection
        Type::Set(_) => false,     // heap collection
        Type::Tensor(_) => false,  // heap array
        Type::Data => false,       // runtime type
        Type::Error => false,      // runtime type

        // Wrapper types - depend on inner type.
        Type::Option(opt) => {
            // Option is copy only if inner type is copy.
            let inner = convert_datalit_type(db, opt.inner_type(db));
            is_copy_type(db, inner)
        }
        Type::Result(_res) => {
            // Result is copy only if inner type is copy.
            // (Error is never copy, so Result<T> is only copy if T is copy AND
            // we never actually store an error variant)
            // Conservative: treat Result as never copy.
            false
        }

        // Compound types - copy only if all fields/variants are copy.
        Type::AnonTuple(tuple) => {
            tuple.fields(db).iter().all(|field_ty| {
                let converted = convert_datalit_type(db, *field_ty);
                is_copy_type(db, converted)
            })
        }
        Type::NamedTuple(tuple) => {
            tuple.fields(db).iter().all(|field_ty| {
                let converted = convert_datalit_type(db, *field_ty);
                is_copy_type(db, converted)
            })
        }
        Type::AnonStruct(struct_ty) => {
            struct_ty.fields(db).iter().all(|field| {
                let converted = convert_datalit_type(db, field.ty(db));
                is_copy_type(db, converted)
            })
        }
        Type::NamedStruct(struct_ty) => {
            struct_ty.fields(db).iter().all(|field| {
                let converted = convert_datalit_type(db, field.ty(db));
                is_copy_type(db, converted)
            })
        }
        Type::AnonEnum(enum_ty) => {
            enum_ty.variants(db).iter().all(|variant| {
                // Variant is copy if it has no payload OR payload is copy.
                variant.payload(db).map_or(true, |payload_ty| {
                    let converted = convert_datalit_type(db, payload_ty);
                    is_copy_type(db, converted)
                })
            })
        }
        Type::NamedEnum(enum_ty) => {
            enum_ty.variants(db).iter().all(|variant| {
                variant.payload(db).map_or(true, |payload_ty| {
                    let converted = convert_datalit_type(db, payload_ty);
                    is_copy_type(db, converted)
                })
            })
        }
    }
}

/// Get the type for a slot from the type checker result.
///
/// Helper function to bridge slot allocation and type checking.
pub fn get_slot_type<'db>(
    db: &'db dyn crate::Db,
    slot: &crate::function_analysis::slot_allocation::AllocatedSlot<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
    func: crate::ast::StmtFun<'db>,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::function_analysis::SlotKind;
    use salsa::plumbing::AsId;

    match slot.kind(db) {
        SlotKind::Reference => {
            // Parameter - get type from tycheck result.
            get_param_type(db, func, slot.name(db), tycheck_result)
        }
        SlotKind::Local => {
            // Let binding - get type from RHS expression.
            let expr_types = tycheck_result.expr_types(db);
            get_local_type(db, func, slot.name(db), expr_types)
        }
        SlotKind::Temporary => {
            // Temporary - get type from the creating expression.
            if let Some(expr) = slot.expr(db) {
                let expr_types = tycheck_result.expr_types(db);
                let expr_id = expr.as_id();
                let index = expr_id.index() as usize;

                expr_types.get(index)
                    .and_then(|opt| *opt)
                    .unwrap_or_else(|| create_placeholder_type(db))
            } else {
                create_placeholder_type(db)
            }
        }
    }
}

/// Get type for a parameter slot.
fn get_param_type<'db>(
    db: &'db dyn crate::Db,
    func: crate::ast::StmtFun<'db>,
    param_name: Option<bct::text::InternedText<'db>>,
    _tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::datalit::ast::TypeHint;

    if let Some(name) = param_name {
        for param in func.params(db) {
            if param.name(db) == name {
                let type_hint = param.type_hint(db);

                // Convert type hint to type.
                return convert_type_hint_to_type(db, type_hint);
            }
        }
    }
    // Fallback to placeholder if param not found.
    create_placeholder_type(db)
}

/// Get type for a local (let binding) slot.
fn get_local_type<'db>(
    db: &'db dyn crate::Db,
    func: crate::ast::StmtFun<'db>,
    local_name: Option<bct::text::InternedText<'db>>,
    expr_types: &[Option<crate::tycheck::TypeAndHeap<'db>>],
) -> crate::tycheck::TypeAndHeap<'db> {
    use salsa::plumbing::AsId;
    use crate::ast::Statement;

    if let Some(name) = local_name {
        // Find the let statement with this name.
        for stmt in func.body(db) {
            if let Statement::Let(let_stmt) = stmt {
                if let_stmt.name(db) == name {
                    // Get the RHS expression and its type.
                    let value_expr = let_stmt.value(db);
                    let expr_id = value_expr.as_id();
                    let index = expr_id.index() as usize;

                    if let Some(Some(ty)) = expr_types.get(index) {
                        return *ty;
                    }
                }
            }
        }
    }
    // Fallback to placeholder if local not found or no type.
    create_placeholder_type(db)
}

/// Convert a type hint to a TypeAndHeap.
fn convert_type_hint_to_type<'db>(
    db: &'db dyn crate::Db,
    type_hint: crate::datalit::ast::TypeHintAndHeap<'db>,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::tycheck::{Type, TypeAndHeap};
    use crate::datalit;

    // Directly convert the type hint to a type.
    let datalit_type = match type_hint.type_hint(db) {
        datalit::ast::TypeHint::Bool => datalit::tycheck::Type::Bool,
        datalit::ast::TypeHint::U8 => datalit::tycheck::Type::U8,
        datalit::ast::TypeHint::I8 => datalit::tycheck::Type::I8,
        datalit::ast::TypeHint::U16 => datalit::tycheck::Type::U16,
        datalit::ast::TypeHint::I16 => datalit::tycheck::Type::I16,
        datalit::ast::TypeHint::U32 => datalit::tycheck::Type::U32,
        datalit::ast::TypeHint::I32 => datalit::tycheck::Type::I32,
        datalit::ast::TypeHint::U64 => datalit::tycheck::Type::U64,
        datalit::ast::TypeHint::I64 => datalit::tycheck::Type::I64,
        datalit::ast::TypeHint::F32 => datalit::tycheck::Type::F32,
        datalit::ast::TypeHint::Int => datalit::tycheck::Type::Int,
        datalit::ast::TypeHint::String => datalit::tycheck::Type::String,
        datalit::ast::TypeHint::Data => datalit::tycheck::Type::Data,
        datalit::ast::TypeHint::Error => datalit::tycheck::Type::Error,
        // ParseError - treat as heap type (non-copy) conservatively.
        datalit::ast::TypeHint::ParseError(_) => {
            // Conservative: treat parse errors as linear (heap) types.
            // This ensures we don't accidentally treat unknown types as copy.
            return TypeAndHeap::new(db, datalit::ast::Heap::Global, Type::Datalit(datalit::tycheck::Type::String));
        }
        // For complex types, fall back to convert_type_hint.
        _ => {
            let datalit_ty = datalit::tycheck::convert_type_hint(db, type_hint)
                .unwrap_or_else(|_| {
                    // Fallback to String (heap type) for safety.
                    datalit::tycheck::TypeAndHeap::new(db, datalit::ast::Heap::Global, datalit::tycheck::Type::String)
                });
            return TypeAndHeap::new(db, datalit_ty.heap(db), Type::Datalit(datalit_ty.ty(db).clone()));
        }
    };

    TypeAndHeap::new(db, type_hint.heap(db), Type::Datalit(datalit_type))
}

/// Create a placeholder type (bool on local heap) for slots without type info.
fn create_placeholder_type<'db>(
    db: &'db dyn crate::Db,
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::tycheck::{Type, TypeAndHeap};
    use crate::datalit;

    TypeAndHeap::new(
        db,
        datalit::ast::Heap::Local,
        Type::Datalit(datalit::tycheck::Type::Bool)
    )
}

#[cfg(test)]
mod tests {
    // Unit tests will be added in Phase 2 when integrated with move tracking.
    // The move tracking integration tests will demonstrate that copyability
    // detection works correctly.
}
