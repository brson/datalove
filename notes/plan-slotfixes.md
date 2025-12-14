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
- **Phase 3**: COMPLETE (dual-mode comparison identified key findings)
- **Phase 4**: COMPLETE
- **Phase 4.5**: COMPLETE (per-block move analysis for precise conditional drops)
- **Phase 4.6**: COMPLETE (Uninitialized slot state for precise temp tracking)
- **Phase 4.7**: COMPLETE (context-aware temp slot allocation)
- **Phases 5-8**: Not started

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

### Phase 3: Dual-Mode Cleanup (Old + New) ✓ COMPLETE

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
- Some discrepancies: `new_only` has slots (static move analysis too conservative)

### Phase 4: Switch to Drop Points Only ✓ COMPLETE

**Files**: `interp/mod.rs`

**What was done:**
1. Rewrote `cleanup_frame` to use drop_points as primary source of truth
2. Added two-pass cleanup approach:
   - Pass 1: Process slots from drop_points (EndOfScope/EarlyReturn)
   - Pass 2: Fallback - clean Available+Owned slots not in drop_points
3. Still use slot_states to check if slots were moved at runtime
4. Extracted `destroy_slot_contents` helper function
5. All tests pass including leak checks

**Design decision:**
The drop_points analysis is conservative about moves (marks a slot as "moved" if it's
moved in ANY branch). The fallback pass catches cases where static analysis says
"moved" but runtime knows the slot is still Available. This ensures no leaks while
allowing the analysis to be refined later.

### Phase 4.5: Per-Block Move Analysis ✓ COMPLETE

**Files**: `function_analysis/moves.rs`, `function_analysis/drops.rs`, `function_analysis/mod.rs`

**What was done:**
1. Added `MoveState` enum (Always/Sometimes/Never) for per-block move tracking
2. Added `MovedAnalysis` struct with per-block entry/exit states (similar to InitializationAnalysis)
3. Added `analyze_moves_per_block` function that propagates move state through CFG
4. Updated `compute_drop_points` to use `MovedAnalysis` instead of global `moved_slots`
5. Now generates precise drops for conditional branches:
   - Slot moved in then-branch only → generates drop for else-branch exit
   - Slot moved in else-branch only → generates drop for then-branch exit
6. Updated `test_drop_with_conditional` to use non-copy types and verify correct behavior

**Key insight:**
With per-block move analysis, the static analysis correctly handles if/else branches:
- Each exit block now checks its own move state, not global moves
- MoveState::Always means slot is definitely moved on all paths to this exit (no drop)
- MoveState::Sometimes/Never means slot may still have a value (generate drop)

**Remaining limitation:**
The fallback pass in `cleanup_frame` is still needed for temporaries (expression results)
because `InitializationAnalysis` only tracks let-bindings, not expression temporaries.
A future improvement would be to track temporary initialization.

### Phase 4.6: Add Uninitialized Slot State ✓ COMPLETE

**Files**: `interp/frame.rs`, `interp/mod.rs`

**What was done:**
1. Added `Uninitialized` variant to `SlotState` enum:
   - `Uninitialized`: slot has not been written to yet (skip cleanup)
   - `Available`: slot contains a valid value (needs cleanup)
   - `Moved`: slot has been moved from or explicitly destroyed (skip cleanup)

2. Slots now start as `Uninitialized` instead of `Available`

3. Parameter slots are marked `Available` after writing pointers

4. Added `mark_temp_slot_moved()` function to mark slots after `destroy_value()`

5. Updated BinOp/UnaryOp evaluation to:
   - Mark operand temp slots as `Moved` after destroying their contents
   - Mark result temp slots as `Available` after writing results

6. `cleanup_frame` now correctly skips:
   - `Uninitialized` slots (never written to, e.g., unused DPS temp slots)
   - `Moved` slots (already destroyed inline)

**Key insight:**
With DPS (Destination-Passing Style), many temp slots are allocated but never used
because values are written directly to their final destination. Previously all slots
started as `Available`, causing redundant destroy calls on uninitialized memory.
Now slots explicitly track their lifecycle: Uninitialized → Available → Moved.

### Phase 4.7: Context-Aware Temp Slot Allocation ✓ COMPLETE

**Files**: `function_analysis/slot_allocation.rs`

**Goal**: Only allocate temp slots for expressions that actually need them.

**Problem**: Previously `slot_allocation.rs` allocated a temp for every expression, but DPS
meant many were never used (stayed Uninitialized). This wasted frame memory.

**What was done:**

1. Added `ExprContext` enum to slot_allocation:
   - `HasDest`: Parent provides destination (let RHS, tuple element, etc.)
   - `NeedsDest`: Expression must provide its own temp (return, binop operand)

2. Modified `analyze_expr` to take context and only allocate temps for `NeedsDest`:
   - Let statement RHS: HasDest (unless coercion needed)
   - Tuple/list/struct elements: propagate parent's context
   - BinOp/UnaryOp operands: NeedsDest (borrow semantics)
   - BinOp/UnaryOp results: depends on parent context
   - Return expression: NeedsDest
   - If condition: NeedsDest

3. Special case for coercion: if let has Option/Result/Data type hint, RHS uses
   NeedsDest because interpreter evaluates without dest first to check coercion.

4. Name expressions only allocate temp in NeedsDest context (for copy types).

**Key Insight**: Temps are needed exactly when an expression is in a "no-dest position".
This is statically determinable by analyzing expression context at allocation time.

**Result:**
- Significantly fewer temp slots allocated
- Frame memory reduced
- Fallback cleanup pass still needed (for temps like return values, if-conditions)
- But fallback is now efficient because most temps are Moved or never allocated

**Remaining limitation:**
The fallback cleanup pass wasn't removed because InitializationAnalysis doesn't track
temps. However, with context-aware allocation, the fallback is efficient:
- Most temps are marked Moved after consumption (binop/unop operands)
- Only a few temps need fallback cleanup (return values, if-conditions)
- Unused temps (in HasDest context) are never allocated

A future improvement could extend InitializationAnalysis to track temps, eliminating
the fallback entirely.

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

### Phase 7: Return Values as Out Arguments

**Files**: `interp/mod.rs`, `interp/frame.rs`, `function_analysis/slot_allocation.rs`

**Problem**: Return values are currently cloned to heap before frame cleanup because
they point to callee frame memory that's about to be deallocated.

**Solution**: Caller provides return destination; callee writes directly there via DPS.

1. Add `return_dest: Option<Destination>` parameter to `execute_function_body`

2. Store return_dest in StackFrame or thread through execution

3. Modify `eval_return_expression_frame`:
   - If return_dest provided: evaluate with that dest
   - Value lands in caller memory, no heap clone needed

4. Modify call site handling in `eval_expression_frame` for FunctionCall:
   - If HasDest context: pass parent's dest as return_dest
   - If NeedsDest context: use caller's temp slot as return_dest

5. Update slot_allocation for return expressions:
   - Return expressions use HasDest (caller provides destination)
   - No callee-side temp needed for returns

6. Handle Option/Result return type wrapping:
   - When return type is `?T` but expression is `T`, write as Some(T) to dest
   - TryReturn (`?`) still uses InterpError mechanism for early return

7. Remove heap allocation path for return values

8. Run tests - fix regressions

**Complexity areas**:
- Nested calls `f(g())`: g's return writes to f's arg temp
- Option/Result wrapping requires typed destination
- Script-level calls: pass None for return_dest, fall back to current behavior

### Phase 8: Simplify Value

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
