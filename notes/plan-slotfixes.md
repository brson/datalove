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
- **Phase 4.8**: COMPLETE (eliminate fallback cleanup pass)
- **Phase 4.9**: COMPLETE (bug fix: nested block type lookup, move semantics fix)
- **Phase 5.0**: COMPLETE (branch convergence drops - Rust-like drop model)
- **Phases 5.1-8**: Not started (see Phase 5 notes)

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
Phase 5 addresses this by extending InitializationAnalysis to track temps.

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
- Fallback cleanup pass was still needed (addressed in Phase 4.8)

### Phase 4.8: Eliminate Fallback Cleanup Pass ✓ COMPLETE

**Files**: `interp/mod.rs`

**Goal**: Remove the fallback cleanup pass from `cleanup_frame` by destroying temporaries inline.

**What was done:**

1. Mark condition temps as Moved after `evaluate_branch_condition`:
   - Added `mark_temp_slot_moved(ctx, if_s.condition(ctx.db))` after branch evaluation
   - Condition values are destroyed by `evaluate_branch_condition`, slot now marked Moved

2. Destroy return value temps after heap clone:
   - Modified return value handling to always destroy original contents after cloning
   - If ptr is in Available slot: mark it Moved so cleanup_frame skips it
   - If ptr is in Moved slot or external (Reference): destroy was already needed

3. Removed fallback pass from `cleanup_frame`:
   - Deleted the "Pass 2: Fallback" section that scanned for Available+Owned slots
   - Now only drop_points processing remains, using slot_states for conditional moves

**Key insight:**
The fallback pass was catching:
- If-condition temps (now marked Moved inline)
- Return value temps (now destroyed inline with slot marked Moved)

With these handled inline, all temps are properly marked Moved before cleanup_frame runs.

### Phase 4.9: Bug Fix - Nested Block Type Lookup and Move Semantics ✓ COMPLETE

**Files**: `function_analysis/copyability.rs`, `interp/mod.rs`, `interp/memory.rs`

**Discovery**: While investigating Phase 5, a test for conditional moves with branch convergence
revealed a memory leak. The test `296_conditional_move_convergence` exercises:
```
fun test(cond: bool): u32
    let x = [@1, @2]
    if cond
        let _sink = x  // Move x to _sink in then-branch only
    end if
    ret @0
end fun
```

**Bug 1: Nested block type lookup**

`get_local_type` in `copyability.rs` only searched top-level statements in `func.body()`,
missing let-bindings defined in nested blocks (if-then/else, loops). This caused `_sink`
to be incorrectly identified as a copy type (fallback to bool placeholder), so no drop
point was generated.

**Fix**: Added `find_local_type_in_stmts` helper that recursively searches through:
- If statement then-bodies and else-bodies
- Loop statement bodies

**Bug 2: Move semantics for Local slots with dest**

When evaluating a Name expression for a move type (like `x` in `let _sink = x`), the code
ignored the `dest` parameter and just returned a borrowed pointer to the source slot.
This meant:
- x was marked Moved
- But _sink's slot was never written to
- x's slot still had the list but was marked Moved (never cleaned up)
- Memory leak!

**Fix**: Updated Name expression handling for move types in Local/Temporary slots:
- If dest is provided: use new `move_value_to_dest` to shallow-copy the value
- Mark source slot as Moved
- Return borrowed pointer to dest

**New function: `move_value_to_dest`** (memory.rs)

Added a function that does shallow copy (memcpy) instead of deep clone. This transfers
ownership of heap-allocated data (like list buffers) without duplicating them. The source
slot is marked Moved and cleanup skips it; the destination owns the heap data.

**Test added**: `296_conditional_move_convergence.world`

**Result**: 159 interp tests pass with leak checking.

### Phase 5.0: Branch Convergence Drops (Rust-like Model) ✓ COMPLETE

**Files**: `function_analysis/drops.rs`, `interp/mod.rs`

**Discovery**: During Phase 4.9, a conditional move convergence test revealed that when a slot
is moved in one branch but not another, the slot needs to be dropped in the non-moving branch
before the branches converge. This is exactly how Rust handles conditional moves.

**Research**: Rust uses "drop flags" - per-variable boolean flags that track at runtime whether
a value needs dropping. However, Rust also uses compile-time analysis to insert drops at branch
exits when move states differ across branches, minimizing runtime flag checks.

**Implementation**:

1. **Extended drop location model**: Added `DropLocation` enum:
   - `AfterStmt(StmtId)`: Drop after a specific statement (existing behavior)
   - `BlockExit(BlockId)`: Drop when leaving a block via Goto (new)

2. **Added BranchExit drop reason**: New `DropReason::BranchExit` for drops inserted at branch
   convergence points.

3. **Updated `compute_drop_points` with two phases**:
   - Phase 1: For each join point (block with multiple incoming edges), check if any slot has
     different move states across predecessors. If slot is `MoveState::Always` from one
     predecessor and `MoveState::Never` from another, insert `BlockExit` drop in the `Never`
     predecessor.
   - Phase 2: Process function exits (Return/TryReturn) as before.

4. **Added `process_block_exit_drops` function** to interpreter:
   - Called when executing `Terminator::Goto`
   - Finds all `DropLocation::BlockExit` drops for the current block
   - Destroys slot contents and marks slot as Moved
   - Ensures cleanup_frame won't double-drop

5. **Updated `cleanup_frame`** to skip `BranchExit` drops (handled inline).

**Key insight**: This implements the same drop semantics as Rust - a variable moved in one branch
is considered "consumed" in all branches. In the branch where it's not moved, we insert a drop.
After the join point, the variable is effectively gone on all paths.

**Result**:
- Test `296_conditional_move_convergence` passes with leak checking
- All 159 interp tests pass with leak checking
- Memory model now matches Rust's drop semantics for conditional moves

**Remaining work**: `slot_states` is still used as a safety net for runtime checks in
`cleanup_frame`. With branch convergence drops, most conditional move cases are now handled
statically. The remaining uses could potentially be removed if all edge cases are covered.

### Phase 5.1+: Further slot_states Simplification (Future Work)

**Files**: `function_analysis/liveness.rs`, `function_analysis/drops.rs`, `interp/mod.rs`, `interp/frame.rs`

**Current state after Phase 5.0**: Branch convergence drops handle most conditional move cases
statically. `slot_states` is still used as a safety net in `cleanup_frame` for:
- `MoveState::Sometimes` at function exit (rare after convergence drops)
- Slots that may be uninitialized on some paths (`InitState::Sometimes`)

**Potential further work**:

1. **Analyze remaining slot_states uses**: Identify exactly which cases still need runtime checks
   after branch convergence drops. May find that most/all are now covered statically.

2. **Extend InitializationAnalysis to track temps**: Currently only tracks named slots. Adding
   temp tracking would allow drop_points to cover all slots.

3. **Make use-after-move a compile-time error**: Currently runtime-checked. Moving this to
   typecheck would eliminate one source of runtime state dependence.

**Note**: The challenges previously documented about conditional moves with branch convergence
have been addressed in Phase 5.0. The remaining slot_states uses are simpler cases that may
be removable with further analysis.

### Phase 6: Clean Up Argument Passing

**Files**: `interp/mod.rs`, `function_analysis/slot_allocation.rs`

**Problem**: Arguments are heap-allocated, then stored as Reference slots (pointers), requiring
complex cleanup logic in `cleanup_args_after_frame` with `is_copy_type` checks.

**Solution**: Write argument values directly into callee's frame slots using DPS.

1. Current mess:
   - Caller evaluates args → `Vec<Value>` (TempOwned on heap)
   - Creates Reference slots storing pointers to arg values
   - `cleanup_args_after_frame` cleans up `arg_values` with copy-type checks

2. New model:
   - Change parameter slots from Reference to Local (Owned)
   - Caller evaluates arg directly into callee's parameter slot via DPS
   - For `in` args: value written directly to callee frame slot, callee owns it
   - For `out` args: same as `in`, callee writes output value there
   - For `ref`/`mut` args: keep as Reference (pointer to caller's slot)
   - No separate `arg_values` vector needed
   - No `cleanup_args_after_frame` needed - normal drop_points handles params

3. Update `slot_allocation.rs`:
   - Parameters with `in`/`out` mode: allocate as Local (Owned), not Reference
   - Parameters with `ref`/`mut` mode: keep as Reference (Borrowed)

4. Update `execute_function_body`:
   - Instead of storing pointers in Reference slots, evaluate args with dest = param slot
   - Remove `arg_values` vector and `cleanup_args_after_frame`

5. Update `compute_drop_points`:
   - `in` parameter slots may need drop points (if not moved by function)
   - `ref`/`mut` parameter slots still skipped (Borrowed)

6. Run tests - fix regressions

**Benefit**: Eliminates heap allocation for arguments, simplifies cleanup, unifies
parameter handling with normal local variables.

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
   - Return expressions change from NeedsDest to HasDest (caller provides destination)
   - No callee-side temp needed for returns

6. Move Option/Result wrapping to callee side:
   - Currently wrapping happens in `execute_function_body` *after* heap clone
   - Move wrapping into `eval_return_expression_frame` *before* writing to return_dest
   - When return type is `?T` but expression is `T`, write as Some(T) to dest
   - When return type is `!T` but expression is `T`, write as Ok(T) to dest
   - TryReturn (`?`) still uses InterpError mechanism for early return

7. Remove heap allocation path for return values

8. Run tests - fix regressions

**Complexity areas**:
- Nested calls `f(g())`: Works naturally - g's return_dest is f's arg temp slot
- Option/Result wrapping: Callee must know return type to wrap correctly (available from func signature)
- Script-level calls: Pass None for return_dest, keep heap allocation as fallback
- Eventually script scope could also use DPS with a "script output slot"

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
- `crates/datalove-datafun/src/function_analysis/liveness.rs` (InitializationAnalysis)
- `crates/datalove-datafun/src/function_analysis/moves.rs` (MoveInfo, MovedAnalysis)
- `crates/datalove-datafun/src/function_analysis/copyability.rs` (copy type detection, slot type lookup)
- `crates/datalove-datafun/src/interp/mod.rs`
- `crates/datalove-datafun/src/interp/frame.rs`
- `crates/datalove-datafun/src/interp/value.rs`
- `crates/datalove-datafun/src/interp/memory.rs` (clone_value_to_dest, move_value_to_dest)

## Scope

- **Script scope**: Leave alone for now. Unify with frame-based model in a future cleanup.
- **Focus**: Frame-based function execution only.

## Risk Areas

1. **Pattern matching**: `evaluate_branch_condition` has side effects (frees structures). May need rethinking.

2. **Binop/unop borrow semantics**: Currently handled via `eval_expression_in_*_borrow`. Keep as interpreter-level logic for now (always clone for operators).

3. **Drop order**: If multiple slots drop at same point, order may matter for observable effects. Current implementation may have implicit ordering.

4. **Interaction with script scope**: Function calls from script scope pass TempOwned values. Need to ensure handoff still works correctly.
