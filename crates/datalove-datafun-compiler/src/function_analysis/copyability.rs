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
            let inner_type = opt.inner_type(db);
            is_datalit_copy(db, inner_type.ty(db))
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
                is_datalit_copy(db, field_ty.ty(db))
            })
        }
        Type::NamedTuple(tuple) => {
            tuple.fields(db).iter().all(|field_ty| {
                is_datalit_copy(db, field_ty.ty(db))
            })
        }
        Type::AnonStruct(struct_ty) => {
            struct_ty.fields(db).iter().all(|field| {
                let field_type = field.ty(db);
                is_datalit_copy(db, field_type.ty(db))
            })
        }
        Type::NamedStruct(struct_ty) => {
            struct_ty.fields(db).iter().all(|field| {
                let field_type = field.ty(db);
                is_datalit_copy(db, field_type.ty(db))
            })
        }
        Type::AnonEnum(enum_ty) => {
            enum_ty.variants(db).iter().all(|variant| {
                // Variant is copy if it has no payload OR payload is copy.
                variant.payload(db).map_or(true, |payload_ty| {
                    is_datalit_copy(db, payload_ty.ty(db))
                })
            })
        }
        Type::NamedEnum(enum_ty) => {
            enum_ty.variants(db).iter().all(|variant| {
                variant.payload(db).map_or(true, |payload_ty| {
                    is_datalit_copy(db, payload_ty.ty(db))
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
    if let Some(name) = local_name {
        // Find the let statement with this name recursively.
        if let Some(ty) = find_local_type_in_stmts(db, func.body(db), name, expr_types) {
            return ty;
        }
    }
    // Fallback to placeholder if local not found or no type.
    create_placeholder_type(db)
}

/// Recursively search for a let binding in statements (including nested blocks).
fn find_local_type_in_stmts<'db>(
    db: &'db dyn crate::Db,
    stmts: &[crate::ast::Statement<'db>],
    name: bct::text::InternedText<'db>,
    expr_types: &[Option<crate::tycheck::TypeAndHeap<'db>>],
) -> Option<crate::tycheck::TypeAndHeap<'db>> {
    use salsa::plumbing::AsId;
    use crate::ast::Statement;

    for stmt in stmts {
        match stmt {
            Statement::Let(let_stmt) => {
                if let_stmt.name(db) == name {
                    // First, check if there's an explicit type hint.
                    if let Some(type_hint) = let_stmt.type_hint(db) {
                        return Some(convert_type_hint_to_type(db, type_hint));
                    }

                    // No type hint - get the type from the RHS expression.
                    let value_expr = let_stmt.value(db);
                    let expr_id = value_expr.as_id();
                    let index = expr_id.index() as usize;

                    if let Some(Some(ty)) = expr_types.get(index) {
                        return Some(*ty);
                    }
                }
            }
            Statement::If(if_stmt) => {
                // Search in then-body.
                if let Some(ty) = find_local_type_in_stmts(db, if_stmt.then_body(db), name, expr_types) {
                    return Some(ty);
                }
                // Search in else-body if present.
                if let Some(else_body) = if_stmt.else_body(db) {
                    if let Some(ty) = find_local_type_in_stmts(db, else_body, name, expr_types) {
                        return Some(ty);
                    }
                }
            }
            Statement::Loop(loop_stmt) => {
                // Search in loop body.
                if let Some(ty) = find_local_type_in_stmts(db, loop_stmt.body(db), name, expr_types) {
                    return Some(ty);
                }
            }
            _ => {}
        }
    }
    None
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
    use super::*;
    use bct::input::Source;
    use bct::text::InternedText;
    use crate::ast::{Statement, StmtFun};
    use crate::function_analysis::slot_allocation::allocate_slots;

    /// Salsa tracked function to test if a variable is copy type.
    ///
    /// This must be tracked to properly create salsa tracked structs.
    #[salsa::tracked]
    fn test_var_is_copy<'db>(
        db: &'db dyn crate::Db,
        source_code: &'db str,
        var_name: &'db str,
    ) -> bool {
        let source = Source::new(db, S(source_code));
        let script = crate::parser::parse_for_diagnostics(db, source);
        let tycheck_result = crate::tycheck::type_check(db, source, script);
        let statements = script.statements(db);

        // Find the function.
        let func = statements.iter().find_map(|stmt| {
            if let Statement::Fun(fun) = stmt {
                Some(*fun)
            } else {
                None
            }
        }).expect("No function found in source code");

        // Find the slot and get its type.
        let slot_alloc = allocate_slots(db, func);
        let slots = slot_alloc.slots(db);
        let name = InternedText::new(db, var_name);

        for slot in slots {
            if slot.name(db) == Some(name) {
                let ty = get_slot_type(db, slot, tycheck_result, func);
                return is_copy_type(db, ty);
            }
        }
        panic!("Variable {} not found", var_name);
    }

    // Group A: Primitive Copy Types

    #[test]
    fn test_primitives_are_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): bool
    let a = true
    let b: u8 = @5
    let c: i8 = @-5
    let d: u16 = @300
    let e: i16 = @-300
    let f = @42u32
    let g = @-42i32
    let h = @1000000u64
    let i = @-1000000i64
    let j = @3.14f32
    ret a
end fun
        "#;

        // All primitives should be copy.
        assert!(test_var_is_copy(db, source, "a"), "bool should be copy");
        assert!(test_var_is_copy(db, source, "b"), "u8 should be copy");
        assert!(test_var_is_copy(db, source, "c"), "i8 should be copy");
        assert!(test_var_is_copy(db, source, "d"), "u16 should be copy");
        assert!(test_var_is_copy(db, source, "e"), "i16 should be copy");
        assert!(test_var_is_copy(db, source, "f"), "u32 should be copy");
        assert!(test_var_is_copy(db, source, "g"), "i32 should be copy");
        assert!(test_var_is_copy(db, source, "h"), "u64 should be copy");
        assert!(test_var_is_copy(db, source, "i"), "i64 should be copy");
        assert!(test_var_is_copy(db, source, "j"), "f32 should be copy");
    }

    // Group B: Heap-Allocated Linear Types

    #[test]
    fn test_heap_types_not_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): String
    let a: String = "hello"
    let b: [Int] = [1, 2, 3]
    ret a
end fun
        "#;

        // String and List are heap-allocated, should not be copy.
        assert!(!test_var_is_copy(db, source, "a"), "String local should not be copy");
        assert!(!test_var_is_copy(db, source, "b"), "List local should not be copy");
    }

    #[test]
    fn test_list_of_copy_type_still_not_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): [u32]
    let a: [u32] = [1u32, 2u32, 3u32]
    ret a
end fun
        "#;

        // List<u32> is still heap-allocated even though u32 is copy.
        assert!(!test_var_is_copy(db, source, "a"),
                "List<copy> should not be copy (container semantics)");
    }

    // Group C: Option Types

    #[test]
    fn test_option_copy_when_inner_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): ?u32
    let a: ?u32 = some(42u32)
    ret a
end fun
        "#;

        // Option<u32> should be copy.
        assert!(test_var_is_copy(db, source, "a"),
                "Option<copy> should be copy");
    }

    #[test]
    fn test_option_not_copy_when_inner_not_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): ?String
    let a: ?String = some("hello")
    ret a
end fun
        "#;

        // Option<String> should not be copy.
        assert!(!test_var_is_copy(db, source, "a"),
                "Option<linear> should not be copy");
    }

    // Group D: Compound Types - Tuples

    #[test]
    fn test_tuple_all_copy_is_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): (u32, bool, f32)
    let a: (u32, bool, f32) = (42u32, true, @3.14)
    ret a
end fun
        "#;

        // Tuple with all copy fields should be copy.
        assert!(test_var_is_copy(db, source, "a"),
                "Tuple<copy, copy, copy> should be copy");
    }

    #[test]
    fn test_tuple_with_linear_field_not_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): (u32, String)
    let a: (u32, String) = (42u32, "hello")
    ret a
end fun
        "#;

        // Tuple with any linear field should not be copy.
        assert!(!test_var_is_copy(db, source, "a"),
                "Tuple with linear field should not be copy");
    }

    #[test]
    fn test_nested_tuples_all_copy() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): ((u32, bool), (i32, f32))
    let a: ((u32, bool), (i32, f32)) = ((42u32, true), (@-10, @2.5))
    ret a
end fun
        "#;

        // Nested tuples with all copy fields should be copy.
        assert!(test_var_is_copy(db, source, "a"),
                "Nested tuple<copy...> should be copy");
    }

    #[test]
    fn test_nested_tuples_with_linear() {
        let ref db = crate::Database::default();

        let source = r#"
fun test(): ((u32, String), bool)
    let a: ((u32, String), bool) = ((42u32, "hello"), true)
    ret a
end fun
        "#;

        // Nested tuple with linear field deep inside should not be copy.
        assert!(!test_var_is_copy(db, source, "a"),
                "Nested tuple with linear field should not be copy");
    }

}
