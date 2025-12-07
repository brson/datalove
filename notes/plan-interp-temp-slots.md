# Plan: Interpreter Temporary Slots

Use pre-computed temporary slots for expression intermediates instead of heap allocation.

## Status: Phase 6 Complete

All phases complete. Frame-based function evaluation now uses pre-allocated slots for all temporaries - no heap allocation for expression intermediates.

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

### Phase 6: Eliminate All Heap-Allocated Temporaries
- Added `write_datalit_to_dest()` - direct writes for scalars (bool, int, f32)
- Added `write_string_to_dest()` - uses `dtlv_rti_string_create_local` at dest
- Added `write_tuple_to_dest()` - writes elements directly to field offsets
- Added `write_struct_to_dest()` - writes fields directly to field offsets
- Added `write_list_to_dest()` - builds list at dest via runtime API
- Added `write_f32_result()`, `write_bool_result()` helpers
- Updated f32 arithmetic (add, sub, mul, div) to use `write_f32_result`
- Updated comparisons to use `write_bool_result` with dest parameter
- Rewrote Tuple expression case to write elements directly to tuple field offsets
- Removed `allocate_tuple_from_values` call from frame-based evaluation

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
