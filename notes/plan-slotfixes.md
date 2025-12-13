# Interpreter Ownership Model Cleanup

## Goal

Clean up the interpreter's ownership model so that:
1. Slots explicitly track ownership (Owned | Borrowed)
2. Interpreter uses computed drop points from function_analysis
3. Value is purely a transient handle for operations
4. No ad-hoc slot_states scanning - analysis is source of truth

## Current State

- `SlotKind`: Reference | Local | Temporary (ownership implicit)
- `slot_states: Vec<SlotState>` tracks Available/Moved at runtime
- `cleanup_frame` scans all slots checking states
- `cleanup_args_after_frame` separately handles argument cleanup
- `DropPoints` computed but never used by interpreter
- `Value.location` (TempOwned/Borrowed) conflates multiple concerns

## Incremental Phases

### Phase 1: Add Explicit Ownership to Slots

**Files**: `function_analysis/slot_allocation.rs`, `function_analysis/mod.rs`

1. Add `SlotOwnership` enum:
   ```rust
   pub enum SlotOwnership {
       Owned,    // Frame owns this slot, must destroy at drop point
       Borrowed, // Caller owns, frame must not destroy
   }
   ```

2. Add `ownership` field to `SlotInfo` (or `AllocatedSlot`)

3. Derive ownership from SlotKind:
   - Reference → Borrowed
   - Local → Owned
   - Temporary → Owned

4. Run tests - should pass with no behavior change

### Phase 2: Interpreter Reads Ownership from Slots

**Files**: `interp/mod.rs`

1. In `cleanup_frame`, check `slot_info.ownership()` instead of `slot_info.kind() == Reference`

2. Keep slot_states for now - just change what we check

3. Run tests - should pass with no behavior change

### Phase 3: Dual-Mode Cleanup (Old + New)

**Files**: `interp/mod.rs`, `interp/frame.rs`

1. Add current program point tracking to interpreter context:
   - Track which statement we're executing
   - Track position (before/after)

2. Create `execute_drop_points_for_stmt()`:
   - Given current stmt_id and position, find matching drop points
   - Destroy the indicated slots

3. Run BOTH old cleanup AND new drop-point cleanup:
   - At function exit, run both
   - Assert they destroy the same set of slots
   - Log discrepancies for debugging

4. Fix discrepancies one by one until both agree

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
