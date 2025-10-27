# Bug: Integer Type Hints Not Respected in Check Mode

## Summary

When checking an integer literal with an explicit type hint against an expected type, datalit's type checker ignores the type hint and only checks whether the raw literal value fits in the expected type's range. This breaks numeric widening tests for cross-widening rejection.

## Discovered While

Implementing numeric widening tests for the datalit layer. Attempted to create a negative test that verifies u32 cannot widen to i32 (cross-widening should fail).

## Example That Should Fail But Passes

```datalove
: [@i32] / [: @u32 / @100]
```

**Expected behavior**: Type error - u32 cannot widen to i32 (cross-widening not allowed)

**Actual behavior**: Test passes with root_type `@i32`

The checker sees `@100` can fit in i32 range and allows it, completely ignoring the `: @u32` type hint.

## Root Cause

In `crates/datalove-datalit/src/tycheck.rs`, the `check()` function has explicit patterns for `(Expr::Int(i), Type::*)` that directly check if the literal value fits in the target type's range (lines 801-937).

When checking the inner expression `: @u32 / @100`:
1. The type hint `@u32` causes `synthesize()` to return `u32`
2. The list element check calls `check()` with the expression and expected type `i32`
3. The `check()` function matches on `(Expr::Int(_), Type::I32)` pattern
4. It checks if `100` fits in i32 range (it does)
5. Check passes, ignoring that the expression was explicitly typed as `u32`

## Why This Matters

1. **Widening semantics are broken**: Cannot write tests that verify cross-widening is properly rejected
2. **Type hints are ignored**: User-specified type information is discarded
3. **Potential runtime issues**: Code might accept values that shouldn't type-check

## Correct Behavior

The `check()` function should:

1. If expression has a type hint, synthesize its type (respecting the hint)
2. Check if synthesized type can widen to expected type
3. Only fall back to literal range checking if no type hint exists

## Example of Correct Flow

```datalove
: [@i32] / [: @u32 / @100]
```

Should behave as:
1. Synthesize `@100` with type hint `@u32` → produces `u32`
2. Check if `u32` can widen to `i32` → NO (cross-widening forbidden)
3. Emit type error

## Workaround

Cannot write meaningful cross-widening rejection tests at the datalit layer. The datafun layer's test `202_no_cross_widen_u32_to_i32.dfs` works correctly because it uses variables:

```datalove
let a: @u32 = @10
let b: @i32 = a  // Correctly fails
```

Variables don't have the literal-matching problem.

## Fix Required

Refactor `check()` function to respect type hints on integer literals:

1. Move type-hinted synthesis higher in the checking priority
2. Only use literal range patterns for bare integers without type hints
3. Ensure widening logic runs before literal range checking

This is a broader refactoring beyond the scope of just adding widening support.

## Related Code

- `crates/datalove-datalit/src/tycheck.rs:268` - `synthesize()` function
- `crates/datalove-datalit/src/tycheck.rs:720` - `check()` function
- `crates/datalove-datalit/src/tycheck.rs:801-937` - Integer literal check patterns
- `crates/datalove-datalit/src/tycheck.rs:1398-1426` - Subsumption fallback with widening

## Test File Removed

`crates/datalove-datalit/tests/fixtures/tycheck/63_no_cross_widen_u32_to_i32.*` - Removed because it passed when it should have failed, giving false confidence.
