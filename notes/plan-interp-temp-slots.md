# Plan: Interpreter Temporary Slots

Use pre-computed temporary slots for expression intermediates instead of heap allocation.

## Problem

The interpreter allocates temporary slots during analysis but doesn't use them at runtime. All intermediate expression results heap-allocate via `allocate_*()` functions.

## Root Cause

`SlotInfo` (used at runtime) doesn't preserve the `expr` field from `AllocatedSlot` (used during analysis). No way to look up which temp slot belongs to which expression.

## Implementation

### Phase 1: Schema Changes

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

### Phase 2: Interpreter Helpers

**File:** `interp/mod.rs`

Add `get_destination_for_expr()` - returns Destination pointing to temp slot offset.

Add `mark_temp_slot_available()` - marks slot Available after writing.

### Phase 3: Update Expression Evaluation

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

### Phase 4: Cleanup

Existing `cleanup_frame` already handles destroying Available slots. No changes needed.

## Files to Modify

1. `crates/datalove-datafun/src/function_analysis/layout.rs` - SlotInfo schema + helper
2. `crates/datalove-datafun/src/function_analysis/mod.rs` - build_frame_layout
3. `crates/datalove-datafun/src/interp/mod.rs` - helpers + eval_expression_frame
