# Plan: Fix Optional Operators to Early-Return

## Problem

Optional operators (`+?`, `-?`, `*?`, `/?`) currently return `Option<T>` as a value. Per the botspec (lines 172-183), they should **early-return** `None` on overflow, yielding element type `T` directly on success.

## Botspec Reference (notes/botspec.md lines 172-183)

```
#### Optional Arithmetic (`+? -? *? /?`) - Early-return Option

| Type | `+?` `-?` `*?` | `/?` | Unary `-?` |
|------|----------------|------|------------|
| **Fixed ints** | Returns `?T` (same type) | Returns `?T` | Signed only, returns `?T` |

These operators early-return `none` on overflow/div0.
```

Key point: "early-return" means function returns `?T`, operator yields `T` directly on success.

## Current vs Expected Behavior

| Component | Current | Expected |
|-----------|---------|----------|
| Typechecker | `a +? b` → `Option<T>` | `a +? b` → `T` directly |
| Interpreter | Returns `Some(val)` or `None` as value | Returns `T`, early-returns on overflow |
| Function sig | Not enforced | Must return `?T` |

## Files to Modify

### 1. `crates/datalove-datafun/src/tycheck.rs` (lines 1158-1204)

Current code wraps in Option:
```rust
AddOptional | SubOptional | MulOptional => {
    // ...
    let option_inner = datalit::tycheck::TypeOption::new(db, lhs_datalit_ty);
    let option_ty = Type::Datalit(datalit::tycheck::Type::Option(option_inner));
    TypeAndHeap::new(db, datalit::ast::Heap::Omitted, option_ty)
}
```

Change to return element type directly (same pattern as checked operators):
```rust
AddOptional | SubOptional | MulOptional => {
    if !is_fixed_int_type(operand_ty) {
        return Err(...);
    }
    lhs_ty  // Return element type directly
}
```

### 2. `crates/datalove-datafun/src/interp/mod.rs` (lines 2971-3090)

Current `eval_add_optional` (and similar):
```rust
match a.checked_add(b) {
    Some(result) => {
        let val = allocate_u32_raw(ctx, result)?;
        allocate_option_some_from_value(ctx, val)  // Wraps in Some
    }
    None => allocate_option_none(ctx, inner_tydesc),  // Returns None value
}
```

Change to early-return pattern (like checked operators):
```rust
match a.checked_add(b) {
    Some(result) => write_u32_result(ctx, result, dest),  // Return T directly
    None => Err(InterpError::OptionNone),  // Early-return
}
```

### 3. Add new error variant

In `InterpError` enum (~line 186):
```rust
// Checked arithmetic overflow - triggers early return with Err.
Overflow,
DivisionByZero,

// Optional arithmetic overflow - triggers early return with None.
OptionNone,
```

### 4. Test fixtures to update

- `crates/datalove-datafun/tests/fixtures/interp2/43_optional_add.world` - currently expects Option<T> behavior
- `crates/datalove-datafun/tests/fixtures/interp2/44_optional_div.world`
- Tycheck tests for optional operators

## Implementation Steps

1. Add `InterpError::OptionNone` variant
2. Update typechecker: `AddOptional`, `SubOptional`, `MulOptional`, `DivOptional` → return `lhs_ty`
3. Update interpreter: `eval_add_optional`, `eval_sub_optional`, `eval_mul_optional`, `eval_div_optional` → early-return pattern
4. Run tests, bless expected outputs
5. Update botspec status for optional operators

## Parallel with Checked Operators

This is the exact same fix that was applied to checked operators (`+!`, `-!`, `*!`, `/!`):
- Typechecker was returning `Result<T>`, changed to return `T`
- Interpreter was returning wrapped value, changed to early-return `InterpError::Overflow`

The difference is the wrapper type:
- Checked (`!`) → function returns `!T` (Result), early-return is `Err`
- Optional (`?`) → function returns `?T` (Option), early-return is `None`
