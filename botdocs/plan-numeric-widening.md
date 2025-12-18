# Plan: Complete Fixed-Width Integer Support in Interpreter

## Goal

Extend the interpreter to handle all 8 fixed-width integer types (u8, i8, u16, i16, u32, i32, u64, i64) for arithmetic operations, matching the typechecker's existing support.

## Progress

**Last updated:** 2025-12-17

| Phase | Status | Description |
|-------|--------|-------------|
| 1 | ✅ Done | Type helpers in types.rs |
| 2 | ✅ Done | Widening function in alloc.rs |
| 3 | ✅ Done | Bare arithmetic widening |
| 4 | ✅ Done | Result writers |
| 5 | ✅ Done | Checked arithmetic |
| 6 | ✅ Done | Optional arithmetic |
| 7 | ✅ Done | Direct comparison |
| 8 | ✅ Done | Unary negation completion |
| 9 | Pending | Tests |

## Current State

| Feature | u32 | i32 | u8/i8/u16/i16/u64/i64 |
|---------|-----|-----|------------------------|
| Literal storage | ✓ | ✓ | ✓ |
| Bare arithmetic (`+ - *`) widening to int | ✓ | ✓ | ✓ |
| Checked arithmetic (`+! -! *! /!`) | ✓ | ✓ | ✓ |
| Optional arithmetic (`+? -? *? /?`) | ✓ | ✓ | ✓ |
| Direct comparison | ✓ | ✓ | ✓ |
| Unary `-?` | n/a | ✓ | i8/i16/i64 |
| Unary `-!` | ✓ | ✓ | ✓ |

## Design Decisions

Per botspec section 2.3:
- **Bare `+ - *`**: All fixed ints widen to `int`, return `int`. Requires matching types.
- **Mixed fixed + int**: Allowed. Widen the fixed-int side to int (matches current u32 + int behavior).
- **Mixed fixed + fixed**: Type error (e.g., u8 + u16 fails). Conservative for now.
- **Checked `+! -! *! /!`**: Return `!T` (same type), early-return on overflow. Requires matching types.
- **Optional `+? -? *? /?`**: Return `?T` (same type), early-return None on overflow. Requires matching types.
- **Unary `-`**: Only `int` (bare negation).
- **Unary `-?`**: Signed fixed ints only (i8, i16, i32, i64).
- **Unary `-!`**: All fixed ints.

## Implementation Plan

### Phase 1: Type Helpers (`types.rs`) ✅ DONE

Added predicates for fixed-width integer classification:

```rust
pub(super) fn is_fixed_int_value(value: Value) -> bool
pub(super) fn is_signed_fixed_int_value(value: Value) -> bool
pub(super) fn get_type_tag(value: Value) -> TyTag
```

**File:** `crates/datalove-datafun-compiler/src/interp/types.rs`

### Phase 2: Widening Functions (`alloc.rs`) ✅ DONE

Added `widen_fixed_int_to_int()`:
- Handles all 8 fixed-width types (u8/i8/u16/i16/u32/i32/u64/i64)
- Proper sign handling for signed types (extracts magnitude, tracks sign)
- Special case for i64::MIN (cannot be negated without overflow)
- Uses 1 limb for values ≤ u32::MAX, 2 limbs for larger 64-bit values

Removed old `widen_u32_to_int()` (superseded).

**File:** `crates/datalove-datafun-compiler/src/interp/alloc.rs`

### Phase 3: Bare Arithmetic Widening (`arith_widening.rs`) ✅ DONE

Updated `eval_add`, `eval_sub`, `eval_mul`, `eval_div` to handle all fixed-int types:
- Same-type requirement enforced with clear error messages
- Mixed fixed-int + int widening works (widens the fixed-int side)
- All 162 existing tests pass

**File:** `crates/datalove-datafun-compiler/src/interp/arith_widening.rs`

### Phase 4: Result Writers (`arith.rs`) ✅ DONE

Added typed result writers for all fixed-width types:
- `write_u8_result`, `write_i8_result`, `write_u16_result`, `write_i16_result`
- `write_i32_result`, `write_u64_result`, `write_i64_result`
- Generic `write_option_fixed_int_result` for Option wrappers

**File:** `crates/datalove-datafun-compiler/src/interp/arith.rs`

### Phase 5: Checked Arithmetic (`arith.rs`) ✅ DONE

Extended all checked arithmetic functions to handle all 8 fixed-width types:
- `eval_add_checked`, `eval_sub_checked`, `eval_mul_checked`, `eval_div_checked`
- Same-type requirement enforced with clear error messages
- Uses Rust's checked arithmetic for overflow detection

**File:** `crates/datalove-datafun-compiler/src/interp/arith.rs`

### Phase 6: Optional Arithmetic (`arith.rs`) ✅ DONE

Extended all optional arithmetic functions to handle all 8 fixed-width types:
- `eval_add_optional`, `eval_sub_optional`, `eval_mul_optional`, `eval_div_optional`
- Returns `Option<T>` wrapped in runtime Option type

**File:** `crates/datalove-datafun-compiler/src/interp/arith.rs`

### Phase 7: Direct Comparison (`arith.rs`) ✅ DONE

Added fast paths for all fixed-int types in `eval_comparison`:
- Direct comparison for same-type operands (u8, i8, u16, i16, u32, i32, u64, i64)
- Falls back to runtime `dtlv_rti_cmp_total_local` for mixed types

**File:** `crates/datalove-datafun-compiler/src/interp/arith.rs`

### Phase 8: Unary Negation (`arith_widening.rs`) ✅ DONE

Extended unary negation for 64-bit types:
- `eval_neg_optional`: Added i64 support
- `eval_neg_result`: Added i64 and u64 support
- Added `write_typed_int_result_64` helper for 64-bit results

**File:** `crates/datalove-datafun-compiler/src/interp/arith_widening.rs`

### Phase 9: Tests

Add interpreter test fixtures in `crates/datalove-datafun/tests/fixtures/interp/`:

**Bare arithmetic (widening):**
- `300_u8_add.world`, `301_i8_add.world`, `302_u16_add.world`, etc.
- Test that result type is `int`

**Checked arithmetic:**
- `310_u8_add_checked.world`, `311_u8_add_checked_overflow.world`
- Per-type overflow boundary tests

**Optional arithmetic:**
- `320_u8_add_optional.world`, `321_u8_add_optional_overflow.world`

**Comparison:**
- `330_u8_cmp.world`, `331_i8_cmp.world`, etc.

**Unary negation:**
- `340_i64_neg_optional.world`, `341_u64_neg_result.world`

## Files to Modify

1. `crates/datalove-datafun-compiler/src/interp/types.rs` - Type predicates
2. `crates/datalove-datafun-compiler/src/interp/alloc.rs` - Widening functions
3. `crates/datalove-datafun-compiler/src/interp/arith.rs` - Checked/optional arithmetic, comparison
4. `crates/datalove-datafun-compiler/src/interp/arith_widening.rs` - Bare arithmetic, unary negation
5. `crates/datalove-datafun/tests/fixtures/interp/*.world` - New test fixtures

## Order of Implementation

1. Phase 1 (types.rs) - Foundation
2. Phase 2 (alloc.rs) - Widening support
3. Phase 3 (arith_widening.rs) - Bare arithmetic (most impactful)
4. Phases 4-6 (arith.rs) - Result writers, checked/optional arithmetic
5. Phase 7 (arith.rs) - Direct comparison
6. Phase 8 (arith_widening.rs) - Unary negation completion
7. Phase 9 - Tests throughout

## Reference Code

- `literals.rs:50-89` - Multi-type pattern for reading/writing integers
- `arith_widening.rs:593-676` - Existing multi-type unary negation
- `arith_widening.rs:124-193` - Existing mixed u32+int widening pattern
