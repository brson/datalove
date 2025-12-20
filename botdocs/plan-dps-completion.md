# DPS Completion Plan

Complete destination-passing style and fix Value semantic confusion.

## Current State

**64 non-DPS locations** across mod.rs and script.rs:
- 22 calls passing `None` for dest
- 19 places checking `if dest.is_some()`
- 23 direct `allocate_*` fallback calls

**Expression types ignoring dest:**
- Float, Hex, String, Bool literals (always allocate)
- Set, Map (have `_dest` param but ignore it)
- Data wrapper, TryOption, TryResult

**ValueLocation confusion:**
- `Borrowed` conflates "don't free structure" with "not semantically owned"
- Pattern of checking `ptr == dest.ptr` to decide ownership
- Move semantics determined at runtime instead of using static analysis

## Goals

1. All expression evaluation uses DPS when dest provided
2. `return_dest: Destination` (not `Option<Destination>`)
3. Remove all `allocate_*` fallback paths
4. Simplify Value to reflect static analysis decisions

---

## Phase 1: Add DPS to Remaining Literals

**Files:** `interp/literals.rs`, `interp/mod.rs`, `interp/script.rs`

Add `write_*_to_dest` functions for:
- `write_bool_to_dest(dest, value)`
- `write_f32_to_dest(dest, value)`
- `write_u32_to_dest(dest, value)` (for Hex)
- `write_string_to_dest(ctx, dest, string_expr)`

Update eval match arms to use DPS when dest provided.

---

## Phase 2: Add DPS to Set/Map

**Files:** `interp/mod.rs`, `interp/collections.rs`

Currently `eval_inline_set` and `eval_inline_map` have `_dest` params they ignore.

Options:
- A) Evaluate elements with DPS into pre-allocated collection buffer
- B) Keep current collect-then-allocate but write final result to dest

Recommend B - simpler, Set/Map have complex internal structure.

---

## Phase 3: Fix Coercion Check Paths (7.2)

**Problem:** Let statements and return expressions evaluate with `None` first to check if coercion needed.

**Locations:**
- `mod.rs:1115` - let statement coercion check
- `mod.rs:1612` - return expression coercion check
- `script.rs:337, 360, 365` - script let statements

**Fix:** Always evaluate with dest. Check coercion *after* by comparing value type to dest type. If mismatch, coerce in place.

---

## Phase 4: Fix Operand Evaluation

**Problem:** BinOp, UnaryOp, TryOption, TryResult evaluate operands with `None`.

**Current:** Operands go to temp slots, operator produces result.

**Fix:** These already use temp slots from static analysis. The `None` is correct for operands (they write to their assigned temp slot). But the *result* should use caller's dest.

Verify: `execute_binop`/`execute_unop` already take dest param and use it.

---

## Phase 5: Fix Data/Er Wrappers

**Problem:** `Data` and `Er` expressions evaluate inner value with `None` then wrap.

**Fix:**
- `Data`: Needs type context - evaluate inner to temp, wrap to dest
- `Er`: Already requires dest for type context

These may need to stay as-is due to type erasure in Data.

---

## Phase 6: Make return_dest Non-Optional (7.3)

**Files:** `interp/mod.rs`, `interp/script.rs`, `interp/frame.rs`

1. Change `StackFrame.return_dest: Option<Destination>` to `Destination`
2. Change `execute_function_body(... return_dest: Option<Destination>)` to `Destination`
3. All callers must provide destination:
   - Script scope: allocate before call (done in 7.1)
   - Frame scope: pass caller's dest through

---

## Phase 7: Delete Fallback Code (7.4)

Remove these patterns:
- `if dest.is_some() { ... } else { allocate_*() }`
- Heap clone for Borrowed returns when no dest
- `allocate_option_none`, `allocate_result_err` fallbacks in execute_function_body

After Phase 6, these paths are unreachable.

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

## Execution Order

1. **Phase 1** - Literal DPS (low risk, isolated)
2. **Phase 2** - Set/Map DPS (medium risk)
3. **Phase 3** - Coercion paths (high value, fixes 7.2)
4. **Phase 4** - Verify operand handling (mostly correct already)
5. **Phase 5** - Data/Er wrappers (may defer)
6. **Phase 6** - Non-optional return_dest (7.3)
7. **Phase 7** - Delete fallbacks (7.4)
8. **Phase 8** - Simplify Value (cleanup)

Test after each phase. Phases 1-4 can be done incrementally.

---

## Key Files

- `crates/datalove-datafun-compiler/src/interp/mod.rs` - main interpreter
- `crates/datalove-datafun-compiler/src/interp/script.rs` - script scope
- `crates/datalove-datafun-compiler/src/interp/literals.rs` - literal DPS helpers
- `crates/datalove-datafun-compiler/src/interp/collections.rs` - collection allocation
- `crates/datalove-datafun-compiler/src/interp/value.rs` - Value/Destination types
- `crates/datalove-datafun-compiler/src/interp/frame.rs` - StackFrame definition
- `crates/datalove-datafun-compiler/src/interp/memory.rs` - destroy/free functions
