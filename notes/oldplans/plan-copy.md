# Copy Type Detection Implementation Plan

## Overview

This document details the implementation plan for copy type detection in the datafun function analysis system. Copy type detection is critical for building a leak-free interpreter because it determines which values can be freely copied vs. which require ownership tracking and explicit drops.

## 1. Type Classification

### Copy Types (Bitwise Copyable, No Heap Allocation)

**Primitive scalars** - Always copy:
- `Bool`
- `U8`, `I8`, `U16`, `I16`, `U32`, `I32`, `U64`, `I64`
- `F32`

**Key property**: These types have no heap allocation and can be safely bitwise copied.

### Linear Types (Require Ownership Tracking)

**Heap-allocated types** - Never copy:
- `Int` (bigint - heap allocated)
- `String` (heap-allocated buffer)
- `List`, `Map`, `Set` (collections with heap storage)
- `Tensor` (multi-dimensional array with heap storage)
- `Data`, `Error` (special runtime types)

**Wrapper types** - Never copy (contain potentially linear values):
- `Option<T>` (contains T)
- `Result<T>` (contains T or Error)

**Compound types** - Copy only if all components are copy:
- `Tuple` - copy if all fields are copy
- `Struct` - copy if all fields are copy
- `Enum` - copy if all variant payloads are copy

**Function types** - Never copy:
- `Function` (function references/closures)

**Special types**:
- `Void` - Trivially copyable (zero-sized)

### Classification Principle

**A type is copy if and only if it contains no heap-allocated components.**

This is a structural property that must be checked recursively for compound types.

## 2. Implementation Architecture

### New Module: `crates/datalove-datafun/src/function_analysis/copyability.rs`

```rust
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
        Type::Function(_) => false,  // Functions are not copy
        Type::Void => true,           // Void is trivially copyable
    }
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
            is_copy_type(db, opt.inner_type(db))
        }
        Type::Result(res) => {
            // Result is copy only if inner type is copy.
            // (Error is never copy, so Result<T> is only copy if T is copy AND
            // we never actually store an error variant)
            // Conservative: treat Result as never copy.
            false
        }

        // Compound types - copy only if all fields/variants are copy.
        Type::AnonTuple(tuple) => {
            tuple.fields(db).iter().all(|field_ty| is_copy_type(db, *field_ty))
        }
        Type::NamedTuple(tuple) => {
            tuple.fields(db).iter().all(|field_ty| is_copy_type(db, *field_ty))
        }
        Type::AnonStruct(struct_ty) => {
            struct_ty.fields(db).iter().all(|field| {
                is_copy_type(db, field.ty(db))
            })
        }
        Type::NamedStruct(struct_ty) => {
            struct_ty.fields(db).iter().all(|field| {
                is_copy_type(db, field.ty(db))
            })
        }
        Type::AnonEnum(enum_ty) => {
            enum_ty.variants(db).iter().all(|variant| {
                // Variant is copy if it has no payload OR payload is copy.
                variant.payload(db).map_or(true, |payload_ty| {
                    is_copy_type(db, payload_ty)
                })
            })
        }
        Type::NamedEnum(enum_ty) => {
            enum_ty.variants(db).iter().all(|variant| {
                variant.payload(db).map_or(true, |payload_ty| {
                    is_copy_type(db, payload_ty)
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
) -> crate::tycheck::TypeAndHeap<'db> {
    use crate::function_analysis::SlotKind;
    use salsa::plumbing::AsId;

    match slot.kind(db) {
        SlotKind::Reference => {
            // Parameter - would need to look up from function signature.
            // For now, return placeholder.
            create_placeholder_type(db)
        }
        SlotKind::Local => {
            // Let binding - would need to find the let statement.
            // For now, return placeholder.
            create_placeholder_type(db)
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

    #[test]
    fn test_scalar_types_are_copy() {
        let ref db = crate::Database::default();

        // Test all scalar types are copy.
        use crate::datalit::tycheck::Type;
        use crate::tycheck::TypeAndHeap;
        use crate::datalit::ast::Heap;

        let test_types = vec![
            Type::Bool,
            Type::U8, Type::I8,
            Type::U16, Type::I16,
            Type::U32, Type::I32,
            Type::U64, Type::I64,
            Type::F32,
        ];

        for ty in test_types {
            let ty_and_heap = TypeAndHeap::new(
                db,
                Heap::Local,
                crate::tycheck::Type::Datalit(ty)
            );
            assert!(is_copy_type(db, ty_and_heap), "{:?} should be copy", ty);
        }
    }

    #[test]
    fn test_heap_types_not_copy() {
        let ref db = crate::Database::default();

        use crate::datalit::tycheck::{Type, TypeList};
        use crate::tycheck::TypeAndHeap;
        use crate::datalit::ast::Heap;

        // Int, String are never copy.
        let int_ty = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::Int));
        assert!(!is_copy_type(db, int_ty), "Int should not be copy");

        let string_ty = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::String));
        assert!(!is_copy_type(db, string_ty), "String should not be copy");

        // List is never copy.
        let u32_ty = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::U32));
        let list_ty = TypeList::new(db, u32_ty);
        let list_ty_and_heap = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::List(list_ty)));
        assert!(!is_copy_type(db, list_ty_and_heap), "List should not be copy");
    }

    #[test]
    fn test_tuple_copyability() {
        let ref db = crate::Database::default();

        use crate::datalit::tycheck::{Type, TypeAnonTuple};
        use crate::tycheck::TypeAndHeap;
        use crate::datalit::ast::Heap;

        // (u32, bool) is copy.
        let u32_ty = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::U32));
        let bool_ty = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::Bool));
        let tuple_ty = TypeAnonTuple::new(db, vec![u32_ty, bool_ty]);
        let tuple_ty_and_heap = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::AnonTuple(tuple_ty)));
        assert!(is_copy_type(db, tuple_ty_and_heap), "(u32, bool) should be copy");

        // (u32, String) is NOT copy.
        let string_ty = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::String));
        let mixed_tuple = TypeAnonTuple::new(db, vec![u32_ty, string_ty]);
        let mixed_tuple_and_heap = TypeAndHeap::new(db, Heap::Local, crate::tycheck::Type::Datalit(Type::AnonTuple(mixed_tuple)));
        assert!(!is_copy_type(db, mixed_tuple_and_heap), "(u32, String) should not be copy");
    }

    #[test]
    fn test_void_is_copy() {
        let ref db = crate::Database::default();

        use crate::tycheck::{Type, TypeAndHeap};
        use crate::datalit::ast::Heap;

        let void_ty = TypeAndHeap::new(db, Heap::Local, Type::Void);
        assert!(is_copy_type(db, void_ty), "Void should be copy");
    }

    #[test]
    fn test_function_not_copy() {
        let ref db = crate::Database::default();

        use crate::tycheck::{Type, TypeFunction, TypeAndHeap};
        use crate::datalit::ast::Heap;

        let void_ty = TypeAndHeap::new(db, Heap::Local, Type::Void);
        let func_ty = TypeFunction::new(db, vec![], void_ty);
        let func_ty_and_heap = TypeAndHeap::new(db, Heap::Local, Type::Function(func_ty));
        assert!(!is_copy_type(db, func_ty_and_heap), "Function should not be copy");
    }
}
```

## 3. Integration Points

### A. Move Tracking (`moves.rs`)

**Current behavior**: All moves are classified as `Assignment`, `FunctionCall`, `FunctionReturn`, or `LastUse`.

**New behavior**: Before creating a `MoveOp`, check if the type is copy. If so, use `MoveKind::Copy` instead.

**Changes required**:

1. Add `tycheck_result` parameter to `compute_move_info`:
```rust
pub fn compute_move_info<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    slots: &'db [AllocatedSlot<'db>],
    live_ranges: LiveRanges<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,  // NEW
) -> MoveInfo<'db>
```

2. Pass `tycheck_result` through helper functions:
```rust
fn walk_statements<'db>(
    // ... existing params ...
    tycheck_result: crate::tycheck::TypecheckResult<'db>,  // NEW
)

fn collect_moves_from_expr<'db>(
    // ... existing params ...
    tycheck_result: crate::tycheck::TypecheckResult<'db>,  // NEW
)
```

3. Update move creation logic in `collect_moves_from_expr`:
```rust
ExprFunKind::Name(name) => {
    if let Some(source_slot) = find_slot_by_name(db, slots, name) {
        // Get slot info and type.
        let slot_info = slots.iter()
            .find(|s| s.slot_id(db) == source_slot)
            .expect("slot should exist");
        let slot_type = copyability::get_slot_type(db, slot_info, tycheck_result);

        // Determine actual move kind based on copyability.
        let actual_move_kind = if copyability::is_copy_type(db, slot_type) {
            MoveKind::Copy
        } else {
            move_kind  // Use the passed-in kind
        };

        moves.push(MoveOp::new(db, expr_id, source_slot, actual_move_kind));
    }
}
```

### B. Drop Points (`drops.rs`)

**Current behavior**: Creates drop points for all initialized slots that weren't moved.

**New behavior**: Also skip drop points for copy type slots (they don't need drops).

**Changes required**:

1. Add `tycheck_result` parameter to `compute_drop_points`:
```rust
pub fn compute_drop_points<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    cfg: ControlFlowGraph<'db>,
    slots: &'db [AllocatedSlot<'db>],
    init_analysis: InitializationAnalysis<'db>,
    move_info: MoveInfo<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,  // NEW
) -> DropPoints<'db>
```

2. Check copyability before creating drop points:
```rust
for slot in slots {
    // ... existing checks ...

    // Get slot type and check if it's copy.
    let slot_type = copyability::get_slot_type(db, slot, tycheck_result);
    if copyability::is_copy_type(db, slot_type) {
        continue;  // Copy types never need drops
    }

    // If the slot was moved, no drop is needed.
    if moved_slots.contains(&slot_id) {
        continue;
    }

    // ... create drop point ...
}
```

### C. Validation (`validation.rs`)

**Current behavior**: Already correctly filters `MoveKind::Copy` moves in:
- `check_double_move` (line 233)
- `check_use_after_move` (line 296)

**Required changes**: None! Validation already handles copy moves correctly.

### D. Main Analysis Function (`mod.rs`)

Update `analyze_function` to pass `tycheck_result` through:

```rust
pub fn analyze_function<'db>(
    db: &'db dyn crate::Db,
    func: StmtFun<'db>,
    tycheck_result: crate::tycheck::TypecheckResult<'db>,
) -> FunctionAnalysis<'db> {
    // ... existing phases 1-4 ...

    // Phase 5: Move tracking - now with tycheck_result.
    let move_info = moves::compute_move_info(
        db,
        func,
        slots,
        live_ranges,
        tycheck_result  // NEW
    );

    // Phase 6: Drop points - now with tycheck_result.
    let drop_points = drops::compute_drop_points(
        db,
        func,
        control_flow,
        slots,
        init_analysis,
        move_info,
        tycheck_result  // NEW
    );

    // ... rest unchanged ...
}
```

## 4. Impact on Analysis Passes

| Pass | Impact | Changes Required |
|------|--------|------------------|
| **Slot Allocation** | None | No changes - all types need slots |
| **CFG** | None | No changes - control flow independent of copyability |
| **Initialization** | None | No changes - copy types still need initialization tracking |
| **Liveness** | None | No changes - copy types still have liveness ranges |
| **Move Tracking** | **Major** | Mark moves of copy types as `MoveKind::Copy` |
| **Drop Points** | **Major** | Never create drops for copy type slots |
| **Validation** | Minor | Already handles Copy moves correctly |

**Key insight**: Initialization and liveness analysis are still needed for copy types! Even though they can be copied freely, we still need to track:
- When they're initialized (can't read uninitialized memory)
- When they go out of scope (for frame layout purposes)
- Their live ranges (for register allocation / optimization)

Only move tracking and drop insertion change behavior based on copyability.

## 5. Testing Strategy

### Unit Tests (`copyability.rs::tests`)

```rust
#[test]
fn test_scalar_types_are_copy()
// Verify: Bool, U8-U64, I8-I64, F32 are all copy

#[test]
fn test_heap_types_not_copy()
// Verify: Int, String, List, Map, Set, Tensor, Data, Error are NOT copy

#[test]
fn test_tuple_copyability()
// Verify: (u32, bool) is copy, (u32, String) is NOT copy

#[test]
fn test_struct_copyability()
// Verify: struct { x: u32, y: bool } is copy
// struct { name: String, age: u32 } is NOT copy

#[test]
fn test_enum_copyability()
// Verify: enum { A, B } is copy (no payloads)
// enum { Some(u32), None } is copy
// enum { Some(String), None } is NOT copy

#[test]
fn test_option_copyability()
// Verify: Option<u32> is copy, Option<String> is NOT copy

#[test]
fn test_nested_compound_types()
// Verify: (u32, (bool, u32)) is copy
// (u32, (bool, String)) is NOT copy
```

### Integration Tests (`moves.rs::tests`)

```rust
#[test]
fn test_copy_type_generates_copy_moves() {
    let source = r#"
fun test(x: In u32)
    let y = x
    let z = x  // Both should be Copy moves
end fun
    "#;

    // Verify both moves are MoveKind::Copy
}

#[test]
fn test_linear_type_generates_real_moves() {
    let source = r#"
fun test(x: In String)
    let y = x
end fun
    "#;

    // Verify move is NOT MoveKind::Copy
}

#[test]
fn test_copy_type_multiple_uses_no_error() {
    let source = r#"
fun test(x: In u32): u32
    let y = x
    let z = x  // OK - u32 is copy
    ret y +! z
end fun
    "#;

    // Run full analysis - should have NO double-move errors
}

#[test]
fn test_linear_type_multiple_uses_error() {
    let source = r#"
fun test(x: In String): String
    let y = x
    let z = x  // ERROR - String is not copy
    ret y
end fun
    "#;

    // Run full analysis - SHOULD have double-move error
}
```

### Integration Tests (`drops.rs::tests`)

```rust
#[test]
fn test_copy_type_no_drop_needed() {
    let source = r#"
fun test(): u32
    let x = @42
    let y = @100
    ret x  // y not returned
end fun
    "#;

    // Verify y has NO drop point (u32 is copy)
}

#[test]
fn test_linear_type_needs_drop() {
    let source = r#"
fun test(): String
    let x = "hello"
    let y = "world"
    ret x  // y not returned
end fun
    "#;

    // Verify y HAS a drop point (String is linear)
}

#[test]
fn test_mixed_types_selective_drops() {
    let source = r#"
fun test(): u32
    let a = @42        // copy
    let b = "hello"    // linear
    let c = @100       // copy
    ret a
end fun
    "#;

    // Verify:
    // - a: no drop (returned)
    // - b: HAS drop (linear, not returned)
    // - c: no drop (copy, not returned)
}
```

### Integration Tests (`validation.rs::tests`)

```rust
#[test]
fn test_validation_allows_copy_double_use() {
    let source = r#"
fun test(x: In u32)
    fun helper(a: In u32): u32
        ret a
    end fun
    let y = helper(x)
    let z = helper(x)  // OK for copy types
end fun
    "#;

    // Should have NO validation errors
}

#[test]
fn test_validation_rejects_linear_double_use() {
    let source = r#"
fun test(x: In String)
    fun helper(a: In String): String
        ret a
    end fun
    let y = helper(x)
    let z = helper(x)  // ERROR for linear types
end fun
    "#;

    // Should have DoubleMove error
}
```

## 6. Implementation Phases

### Phase 1: Core Infrastructure (Low Risk)

**Goal**: Create copy detection logic without integrating into analysis pipeline.

**Tasks**:
1. Create `crates/datalove-datafun/src/function_analysis/copyability.rs`
2. Implement `is_copy_type` function with full type traversal
3. Implement `is_datalit_copy` helper
4. Implement `get_slot_type` helper
5. Add comprehensive unit tests (8+ test cases)
6. Add module to `function_analysis/mod.rs` exports

**Success criteria**:
- All unit tests pass
- No integration yet - just detection logic
- Can be reviewed independently

### Phase 2: Move Tracking Integration (Medium Risk)

**Goal**: Integrate copy detection into move tracking.

**Tasks**:
1. Add `tycheck_result` parameter to `compute_move_info`
2. Thread `tycheck_result` through helper functions
3. Update move creation to check copyability
4. Set `MoveKind::Copy` for copy type moves
5. Update `analyze_function` to pass `tycheck_result`
6. Add integration tests for move generation (4+ tests)
7. Verify existing tests still pass

**Success criteria**:
- Copy types generate `MoveKind::Copy` moves
- Linear types generate non-Copy moves
- Validation correctly allows multiple uses of copy types
- All existing move tracking tests still pass

### Phase 3: Drop Points Integration (Medium Risk)

**Goal**: Integrate copy detection into drop point computation.

**Tasks**:
1. Add `tycheck_result` parameter to `compute_drop_points`
2. Check copyability before creating drop points
3. Skip drop creation for copy type slots
4. Update `analyze_function` to pass `tycheck_result`
5. Add integration tests for drop generation (3+ tests)
6. Verify existing tests still pass

**Success criteria**:
- Copy types never generate drop points
- Linear types generate drops when needed
- All existing drop point tests still pass

### Phase 4: Validation & Cleanup (Low Risk)

**Goal**: Ensure validation works correctly with new copy semantics.

**Tasks**:
1. Verify validation tests still pass
2. Add new validation tests for copy vs linear behavior
3. Update any documentation
4. Review all error messages for clarity

**Success criteria**:
- All validation tests pass
- Copy/linear distinction is clear in error messages
- No regressions in existing functionality

## 7. Edge Cases & Design Decisions

### Q1: What about compound types?

**Decision**: Recursive structural check.

- Tuple/Struct/Enum are copy ONLY if all fields/variants are copy
- Must traverse nested structures completely
- Example: `(u32, (bool, String))` is NOT copy because inner tuple contains String

### Q2: Should we cache copyability results?

**Not in initial implementation.**

- Could add `is_copy: bool` to `SlotInfo` in future optimization
- Avoids repeated traversal of compound types
- Premature optimization for now - profile first

### Q3: What about Option and Result?

**Option<T>**: Copy if T is copy.

**Result<T>**: Always treated as NOT copy.
- Even though inner type might be copy, Result contains Error variant
- Error is a runtime type that's not copy
- Conservative: treat Result as linear

### Q4: What about generic/unknown types?

**Not relevant yet** - no generics in current system.

Future: Conservative default would be "not copy" for unknown types.

### Q5: Should temporaries for copy types be optimized away?

**Not in this phase** - keep uniform treatment.

- Analysis framework supports it (just don't allocate temp slots for copy exprs)
- Future optimization opportunity
- For now, copy types still get temporary slots

### Q6: Heap attribute interaction?

**Heap location doesn't affect copyability.**

- Copy types can be `@Local` or `#Global`
- Both `@u32` and `#u32` are copy
- Heap affects allocation, not ownership semantics

### Q7: What if type resolution fails for a slot?

**Defensive: Assume linear if type unknown.**

- Use placeholder type (currently Bool, which is copy)
- Better to be conservative and treat as linear
- Log warning in debug builds

## 8. Risk Analysis & Mitigation

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| Type resolution fails for some slots | Medium | High | Defensive: assume linear if unknown; add logging |
| Recursive types cause infinite loop | Low | High | No recursive types in current system; add cycle detection if added later |
| Compound type check is expensive | Low | Medium | Profile first; cache in SlotInfo if needed |
| Tests don't cover all combinations | Medium | High | Comprehensive test matrix; code review |
| Integration breaks existing tests | Medium | Medium | Phased rollout; run full test suite after each phase |
| Validation logic inconsistency | Low | High | Validation already handles Copy correctly; verify with tests |

## 9. Success Criteria

Implementation is complete when:

✓ All primitive scalars correctly identified as copy
✓ All heap types correctly identified as linear
✓ Compound types correctly inherit copyability from components
✓ Copy types generate `MoveKind::Copy` in move tracking
✓ Copy types never generate drop points
✓ Validation passes for multiple uses of copy types
✓ Validation fails for multiple uses of linear types
✓ All existing function analysis tests continue to pass (26 tests)
✓ New test suite covers edge cases (15+ new tests)
✓ No performance regression (< 5% slowdown on analysis)
✓ Documentation updated to reflect copy semantics

## 10. Future Enhancements

After initial implementation:

1. **Performance optimization**: Cache copyability in SlotInfo
2. **Eliminate copy temporaries**: Don't allocate slots for copy type temporaries
3. **Explicit clone syntax**: Add `.clone()` for linear types
4. **Better error messages**: Distinguish copy vs linear in diagnostics
5. **User-defined copy types**: Allow marking custom types as copy (unsafe)

## 11. References

- `notes/research-linear-types.md` - Background on linear type systems
- `notes/oldplans/plan-function-analysis.md` - Original function analysis plan
- `crates/datalove-datafun/src/function_analysis/moves.rs` - Move tracking implementation
- `crates/datalove-datafun/src/function_analysis/drops.rs` - Drop point implementation
- `crates/datalove-datafun/src/function_analysis/validation.rs` - Validation passes

## Status

**Current**: Plan complete, ready for implementation.

**Next steps**: Begin Phase 1 (Core Infrastructure).
