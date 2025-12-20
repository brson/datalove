# DPS Completion Plan

Complete destination-passing style and fix Value semantic confusion.

## Progress

- [x] Phase 1: Add DPS to remaining literals
- [x] Phase 2: Add DPS to Set/Map
- [x] Phase 3: Fix coercion check paths (7.2)
- [x] Phase 4: Verify operand handling (verified correct)
- [x] Phase 5: Data/Er wrappers (verified, deferred - type erasure requires current approach)
- [x] Phase 6: Frame scope DPS for function calls
- [x] Phase 7: Delete fallback code paths (7.4)
- [ ] Phase 8: Simplify Value semantics

---

## Phase 1: Add DPS to Remaining Literals [DONE]

**Files:** `interp/literals.rs`, `interp/mod.rs`, `interp/script.rs`

Added `write_*_to_dest` functions:
- `write_bool_to_dest(dest, value)`
- `write_f32_to_dest(dest, value)`
- `write_u32_to_dest(dest, value)` (for Hex)
- `write_string_to_dest(ctx, string_expr, dest)`

Updated eval match arms in mod.rs and script.rs to use DPS when dest provided.

---

## Phase 2: Add DPS to Set/Map [DONE]

**Files:** `interp/mod.rs`

Used option B: Keep collect-then-allocate, copy final result to dest.

- `eval_inline_set`: Allocate set, copy to dest if provided, free temp
- `eval_inline_map`: Same pattern

---

## Phase 3: Fix Coercion Check Paths [DONE]

**Discovery:** The T → Option<T> coercion was dead code. `coerce_value_to_dest` only handles exact type matches (cloning). Tests use explicit `some()`/`ok()` constructors.

**Changes:**
- `eval_let_statement_frame`: Removed 35 lines of coercion check logic, now always uses DPS
- `eval_return_expression_frame`: Removed 60 lines of coercion check logic, uses DPS directly
- `execute_let_statement` in script.rs: Simplified to straightforward DPS for typed lets
- Removed unused `coerce::coerce_value_to_dest` import from mod.rs

---

## Phase 4: Verify Operand Handling [DONE]

**Status:** Verified correct.

BinOp, UnaryOp, TryOption, TryResult evaluate operands with `None` because operands go to temp slots from static analysis. The *result* uses caller's dest.

`execute_binop`/`execute_unop` already take dest param and use it.

---

## Phase 5: Data/Er Wrappers [DONE - Deferred]

**Status:** Verified current approach is correct.

`Data` and `Er` expressions evaluate inner value with `None` then wrap. This is correct because:
- Data is type-erased (fat pointer), inner value must be allocated first
- Er already uses dest when provided for the Result wrapper

---

## Phase 6: Frame Scope DPS for Function Calls [DONE]

**Files:** `interp/mod.rs`

Changed approach from plan: Instead of making return_dest non-optional (which would require dummy destinations for void functions), updated frame scope function call handling to always provide a destination when possible:

```rust
let return_dest = match dest {
    Some(d) => Some(d),
    None => get_destination_for_expr(ctx, expr).ok(),
};
```

This ensures typed function calls always have a destination (caller's dest or temp slot), while void functions can use None.

---

## Phase 7: Delete Fallback Code [DONE]

Replaced fallback code paths with panics to verify they're unreachable:
- Heap clone for Borrowed returns when no dest → panic
- `allocate_option_none` fallback for try-operator → panic
- `allocate_result_err` fallback for try-operator → panic

Removed dead code:
- `allocate_option_none` function
- `allocate_result_err` function
- `value_tydesc_for_option` function
- `value_tydesc_for_result` function
- Unused imports

---

## Phase 8: Simplify Value Semantics

**Current confusion:** `ValueLocation` tracks structure memory but gets conflated with semantic ownership.

**Proposal:** Since all paths use DPS:

1. Expression eval returns `Result<(), InterpError>` when dest provided
   - Caller knows data is at dest
   - No Value returned, no location confusion

2. Keep `Value` only for:
   - Reading from slots (returns ptr to slot)
   - Intermediate values during collection building

3. Replace `ValueLocation` with simpler enum:
   ```rust
   enum ValueSource {
       Slot,      // Points to frame slot - don't free
       HeapTemp,  // Heap-allocated temp - free structure after use
   }
   ```

4. Semantic ownership (move vs copy) determined by:
   - Static analysis `is_copy_type()`
   - SlotState tracking (Moved/Available)
   - NOT by ValueLocation

---

## Key Files

- `crates/datalove-datafun-compiler/src/interp/mod.rs` - main interpreter
- `crates/datalove-datafun-compiler/src/interp/script.rs` - script scope
- `crates/datalove-datafun-compiler/src/interp/literals.rs` - literal DPS helpers
- `crates/datalove-datafun-compiler/src/interp/collections.rs` - collection allocation
- `crates/datalove-datafun-compiler/src/interp/value.rs` - Value/Destination types
- `crates/datalove-datafun-compiler/src/interp/frame.rs` - StackFrame definition
- `crates/datalove-datafun-compiler/src/interp/memory.rs` - destroy/free functions
