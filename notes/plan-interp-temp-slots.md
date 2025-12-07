# Plan: Interpreter Temporary Slots

Use pre-computed temporary slots for expression intermediates instead of heap allocation.

## Status: Phase 5 Complete

Phases 1-5 implemented. All expressions now have mandatory temp slots and use DPS.

## Completed

### Phase 1-4: Core DPS Infrastructure
- Added `expr` field to `SlotInfo` for temp slot lookup
- Added `get_temp_slot_for_expr()` helper to `FrameLayout`
- Added `get_destination_for_expr()` returning `Result<Destination, InterpError>`
- Added `mark_temp_slot_available()` for cleanup tracking
- Updated BinOp, UnaryOp, Tuple, FunctionCall to use temp slots for operands/elements

### Phase 5: Mandatory Temp Slots
- Allocated temp slots for Datalit and Name expressions in `slot_allocation.rs`
- Changed `get_destination_for_expr` from `Option` to `Result` (fail hard)
- Added `eval_datalit_expression_to_dest()` - evaluates datalit, copies to dest
- Added `clone_value_to_dest()` - clones value into pre-allocated dest
- Updated Datalit handling to use dest
- Updated Name handling for copy types to clone to dest
- Removed all `if dest.is_some()` fallback checks
- Fixed type recording in `tycheck.rs` for bidirectional Datalit checking
- Updated `check_value_not_used` to skip Temporary slots
- Updated `check_use_after_move` to skip Temporary slots
- Updated liveness tests to find slots by name (indices shifted)
- Blessed 73 interp tests with updated slot IDs

## Remaining Work

### Not Yet Using DPS (still heap-allocating)

1. **Tuple results** (`interp/mod.rs:2024`)
   ```rust
   // TODO: Support dest for tuple allocation to avoid heap.
   allocate_tuple_from_values(ctx, values)
   ```
   Tuple elements use temp slots, but final tuple goes to heap.

2. **Datalit evaluation** (`interp/mod.rs:1059-1060`)
   ```rust
   let temp_value = eval_datalit_expression(ctx, expr)?;  // heap alloc
   // then copy to dest and free
   ```
   Uses allocate-copy-free pattern instead of direct write to dest.

3. **f32 arithmetic** (`interp/mod.rs:2914,3128,3259,3347`)
   ```rust
   return allocate_f32(ctx, a + b);
   ```
   f32 operations don't support DPS - always heap-allocate.

4. **Compound datalit types** (strings, lists, maps, etc.)
   Go through `eval_datalit_expression` which heap-allocates.

### Future Optimization: Direct Datalit Write

Replace allocate-copy-free with direct writes for simple types:
```rust
fn write_datalit_to_dest(ctx, expr, dest) -> Result<Value, InterpError> {
    match expr.expr(db) {
        Expr::True => { *(dest.ptr as *mut bool) = true; ... }
        Expr::Int(i) => { write_int_to_dest(ctx, i, dest); ... }
        // etc.
    }
}
```

## Original Problem

The interpreter allocates temporary slots during analysis but doesn't use them at runtime. All intermediate expression results heap-allocate via `allocate_*()` functions.

## Original Root Cause

`SlotInfo` (used at runtime) doesn't preserve the `expr` field from `AllocatedSlot` (used during analysis). No way to look up which temp slot belongs to which expression.

## Original Implementation Plan (Historical)

### Phase 1: Schema Changes (DONE)

**File:** `function_analysis/layout.rs`

Add `expr: Option<ExprFun<'db>>` to `SlotInfo`:
```rust
#[salsa::tracked]
pub struct SlotInfo<'db> {
    pub slot_id: SlotId,
    pub name: Option<InternedText<'db>>,
    pub kind: SlotKind,
    pub offset: u32,
    pub ty: crate::tycheck::TypeAndHeap<'db>,
    pub expr: Option<crate::ast::ExprFun<'db>>,  // NEW
}
```

Update `FrameLayout::compute_layout` signature to accept expr.

Add lookup helper:
```rust
pub fn get_temp_slot_for_expr(self, db: &'db dyn crate::Db, expr: ExprFun<'db>) -> Option<SlotInfo<'db>>
```

**File:** `function_analysis/mod.rs`

Update `build_frame_layout` to pass `slot.expr(db)` through.

### Phase 2: Interpreter Helpers (DONE)

**File:** `interp/mod.rs`

Add `get_destination_for_expr()` - returns Destination pointing to temp slot offset.

Add `mark_temp_slot_available()` - marks slot Available after writing.

### Phase 3: Update Expression Evaluation (DONE)

Modify `eval_expression_frame()` for each expression type with temp slots:
- BinOp, FunctionCall, Tuple, UnaryOp, TryOption, TryResult

Pattern:
```rust
ast::ExprFunKind::BinOp(binop_expr) => {
    let lhs_dest = get_destination_for_expr(ctx, binop_expr.lhs(ctx.db));
    let rhs_dest = get_destination_for_expr(ctx, binop_expr.rhs(ctx.db));

    let lhs = eval_expression_frame(ctx, binop_expr.lhs(ctx.db), lhs_dest)?;
    if lhs_dest.is_some() { mark_temp_slot_available(ctx, binop_expr.lhs(ctx.db)); }

    let rhs = eval_expression_frame(ctx, binop_expr.rhs(ctx.db), rhs_dest)?;
    if rhs_dest.is_some() { mark_temp_slot_available(ctx, binop_expr.rhs(ctx.db)); }

    let result_dest = dest.or_else(|| get_destination_for_expr(ctx, expr));
    execute_binop(ctx, binop_expr.op(ctx.db), lhs, rhs, result_dest)
}
```

### Phase 4: Cleanup (DONE)

Existing `cleanup_frame` already handles destroying Available slots. No changes needed.

### Phase 5: Mandatory Temp Slots (DONE)

Currently DPS is "best effort" - gracefully falls back to heap when temp slots missing. Make it mandatory.

**Problem:**
1. `Datalit` and `Name` expressions don't get temp slots allocated
2. `Datalit` and `Name` ignore the `dest` parameter, always heap-allocating
3. `get_destination_for_expr` returns `Option`, allowing graceful fallback

**Analysis changes** (`slot_allocation.rs`):

Allocate temp slots for Datalit and Name:
```rust
ExprFunKind::Datalit(_) => {
    self.alloc_slot(None, SlotKind::Temporary, Some(expr));
}
ExprFunKind::Name(_) => {
    // For copy-type clones; unused for moves.
    self.alloc_slot(None, SlotKind::Temporary, Some(expr));
}
```

**Interpreter changes** (`interp/mod.rs`):

1. Change `get_destination_for_expr` to return `Result<Destination, InterpError>` - fail hard on missing slots

2. Update Datalit to use dest:
```rust
ast::ExprFunKind::Datalit(datalit_expr) => {
    let dest = dest.or_else(|| get_destination_for_expr(ctx, expr))?;
    eval_datalit_expression_to_dest(ctx, datalit_expr, dest)
}
```

3. Update Name to use dest for copy types:
```rust
if is_copy_type {
    clone_value_to_dest(ctx, source_value, dest)?;
    Ok(Value { ptr: dest.ptr, tydesc: dest.tydesc, location: ValueLocation::Borrowed })
} else {
    // Move: return existing pointer, dest unused.
    Ok(existing_value)
}
```

4. Remove all `if dest.is_some()` fallback checks

**New helpers needed:**
- `eval_datalit_expression_to_dest` - write literal to dest
- `clone_value_to_dest` - clone into pre-allocated dest

**Edge cases:**
- Name (move types): Has temp slot but doesn't use it - returns existing pointer
- Top-level return: Uses temp slot, clones to heap before frame cleanup (already handled)

## Files Modified

1. `crates/datalove-datafun/src/function_analysis/layout.rs` - SlotInfo schema + helper ✓
2. `crates/datalove-datafun/src/function_analysis/mod.rs` - build_frame_layout ✓
3. `crates/datalove-datafun/src/function_analysis/slot_allocation.rs` - temp slots for Datalit, Name ✓
4. `crates/datalove-datafun/src/interp/mod.rs` - helpers + eval_expression_frame + mandatory dest ✓
5. `crates/datalove-datafun/src/tycheck.rs` - store expr type for bidirectional Datalit ✓
6. `crates/datalove-datafun/src/function_analysis/validation.rs` - skip Temporary slots in checks ✓
7. `crates/datalove-datafun/src/function_analysis/liveness.rs` - test helper for slot lookup by name ✓
