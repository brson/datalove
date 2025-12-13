# Interpreter Ownership Model Cleanup

## Goal

Clean up the interpreter's ownership model so that:
1. Slots explicitly track ownership (Owned | Borrowed)
2. Interpreter uses computed drop points from function_analysis
3. Value is purely a transient handle for operations
4. No ad-hoc slot_states scanning - analysis is source of truth

## Progress

- **Phase 1**: COMPLETE
- **Phase 2**: COMPLETE
- **Phase 3**: IN PROGRESS (dual-mode comparison working, discrepancies logged)
- **Phases 4-7**: Not started

## Incremental Phases

### Phase 1: Add Explicit Ownership to Slots ✓ COMPLETE

**Files**: `function_analysis/mod.rs`

**What was done:**
1. Added `SlotOwnership` enum (Owned/Borrowed) to `function_analysis/mod.rs`
2. Added `ownership()` method on `SlotKind` that derives ownership:
   - Reference → Borrowed
   - Local → Owned
   - Temporary → Owned
3. All tests pass with no behavior change

### Phase 2: Interpreter Reads Ownership from Slots ✓ COMPLETE

**Files**: `interp/mod.rs`

**What was done:**
1. Updated `cleanup_frame` to use `slot_info.kind(ctx.db).ownership() == SlotOwnership::Borrowed`
   instead of `slot_info.kind(ctx.db) == SlotKind::Reference`
2. Semantically equivalent, uses new ownership abstraction
3. All tests pass with no behavior change

### Phase 3: Dual-Mode Cleanup (Old + New) — IN PROGRESS

**Files**: `interp/mod.rs`, `interp/frame.rs`

**What was done:**
1. Added `drop_points: DropPoints<'db>` field to `StackFrame` in `frame.rs`
2. Updated `execute_function_body` to extract drop_points from analysis and include in frame
3. Modified `cleanup_frame` to compare old and new cleanup approaches:
   - Collects slots old cleanup would destroy (Available + Owned)
   - Collects slots drop_points says to destroy (EndOfScope/EarlyReturn reasons)
   - Logs discrepancies to stderr

**Findings:**
- Old cleanup is too aggressive: destroys ALL Available Owned slots, including copy types
- Drop points analysis is precise: only marks non-copy, initialized, non-moved slots
- Most discrepancies: `old_only` has slots (old would drop copy types unnecessarily)
- Some discrepancies: `new_only` has slots (possible move tracking differences)

**Not yet done:**
- Program point tracking (not needed for function-exit comparison)
- `execute_drop_points_for_stmt()` (deferred - not needed until Phase 4)
- Fix discrepancies to make both agree

### Phase 4: Switch to Drop Points Only

**Files**: `interp/mod.rs`, `interp/frame.rs`

1. Remove old `cleanup_frame` scanning logic

2. Keep `slot_states` temporarily for move tracking (will remove later)

3. Insert drop point execution at:
   - Each statement boundary (check for drops at this point)
   - Function exit (remaining drops)

4. Run tests - fix any regressions

### Phase 5: Remove slot_states for Move Tracking

**Files**: `interp/mod.rs`, `interp/frame.rs`, `function_analysis/moves.rs`

1. Move tracking should come from analysis, not runtime:
   - Analysis knows which reads are last-use moves
   - Interpreter doesn't need to track Moved state

2. Remove `slot_states: Vec<SlotState>` from StackFrame

3. When reading a slot:
   - If analysis says this is a move → don't mark anything, just return
   - If analysis says this is a copy → clone

4. Run tests - fix regressions

### Phase 6: Clean Up Argument Passing

**Files**: `interp/mod.rs`

1. Current mess:
   - Caller evaluates args → `Vec<Value>` (TempOwned)
   - Creates Reference slots with just pointers
   - `cleanup_args_after_frame` cleans up `arg_values`

2. New model:
   - For in/out args: ownership transfers to callee's Owned slot
   - For ref/mut args: callee gets Borrowed slot, caller retains ownership
   - No separate `arg_values` cleanup needed

3. Remove `cleanup_args_after_frame`

4. Run tests - fix regressions

### Phase 7: Simplify Value

**Files**: `interp/value.rs`, `interp/memory.rs`, `interp/mod.rs`

1. Evaluate what `ValueLocation` is still needed for:
   - If slots own everything, Value.location may be unnecessary
   - Or simplify to just "needs structure freeing" for temporaries during eval

2. Possibly remove `ValueLocation` entirely if slots handle all ownership

3. Run tests - fix regressions

## Testing Strategy

- Run full test suite after each sub-step
- Add specific tests for:
  - Slot ownership derivation
  - Drop point execution
  - Move vs copy in function args
  - Nested function calls
  - Early returns
  - Conditional branches with different drop paths

## Key Files

- `crates/datalove-datafun/src/function_analysis/mod.rs`
- `crates/datalove-datafun/src/function_analysis/slot_allocation.rs`
- `crates/datalove-datafun/src/function_analysis/drops.rs`
- `crates/datalove-datafun/src/interp/mod.rs`
- `crates/datalove-datafun/src/interp/frame.rs`
- `crates/datalove-datafun/src/interp/value.rs`
- `crates/datalove-datafun/src/interp/memory.rs`

## Scope

- **Script scope**: Leave alone for now. Unify with frame-based model in a future cleanup.
- **Focus**: Frame-based function execution only.

## Risk Areas

1. **Pattern matching**: `evaluate_branch_condition` has side effects (frees structures). May need rethinking.

2. **Binop/unop borrow semantics**: Currently handled via `eval_expression_in_*_borrow`. Keep as interpreter-level logic for now (always clone for operators).

3. **Drop order**: If multiple slots drop at same point, order may matter for observable effects. Current implementation may have implicit ordering.

4. **Interaction with script scope**: Function calls from script scope pass TempOwned values. Need to ensure handoff still works correctly.
