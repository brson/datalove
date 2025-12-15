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
- **Phase 5.1**: COMPLETE (sparse slot tracking infrastructure, debug-only use-after-move)
- **Phase 5.2**: COMPLETE (inline destruction tracking - exclude BinOp/UnaryOp/if-condition temps)
- **Phase 6**: NO-OP (frame-based args already use DPS)
- **Phase 7**: IN PROGRESS (Return DPS + unified coercion - remaining non-DPS cases)
- **Phase 8**: Not started (simplify Value)

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

### Phase 5.1: Sparse Slot Tracking Infrastructure ✓ COMPLETE

**Files**: `function_analysis/liveness.rs`, `function_analysis/moves.rs`, `function_analysis/mod.rs`,
`interp/frame.rs`, `interp/mod.rs`

**Goal**: Optimize runtime slot tracking by only tracking slots that need runtime checks.

**What was done:**

1. **Added `conditionally_moved_slots()` to `MovedAnalysis`**:
   - Scans all block entry/exit states for `MoveState::Sometimes`
   - Returns list of slots that have conditional move state anywhere

2. **Added `conditionally_initialized_slots()` to `InitializationAnalysis`**:
   - Scans all block entry/exit states for `InitState::Sometimes`
   - Returns list of slots that have conditional initialization anywhere

3. **Added `tracked_slots` field to `FunctionAnalysis` and `StackFrame`**:
   - Computed during analysis: union of conditionally-initialized, conditionally-moved,
     and all Local/Temporary slots
   - Passed to StackFrame for runtime access

4. **Made use-after-move checks debug-only**:
   - Wrapped runtime use-after-move checks in `#[cfg(debug_assertions)]`
   - Static analysis already catches these errors and blocks execution
   - Debug builds retain checks as safety net

5. **Guarded cleanup checks with `tracked_slots`**:
   - `process_block_exit_drops`: only checks `slot_states` for tracked slots
   - `cleanup_frame`: only checks `slot_states` for tracked slots
   - Non-tracked slots (Reference/parameter slots) trust static analysis

**Limitation discovered:**

Static analysis doesn't track inline destruction of temporaries (binop operands, if-condition
temps, etc.). These are marked `Moved` at runtime but the static analysis doesn't know about it.
As a result, we currently include ALL Local and Temporary slots in `tracked_slots`.

**Current tracking scope:**
- Reference (parameter) slots: NOT tracked (static analysis sufficient)
- Local slots: tracked (may be moved inline in ways static analysis misses)
- Temporary slots: tracked (consumed inline, marked Moved at runtime)
- Conditionally-initialized slots: tracked
- Conditionally-moved slots: tracked

**Result:**
- Use-after-move checks: debug-only ✓
- Parameter cleanup checks: skipped (not in tracked_slots) ✓
- All 162 interp tests pass with leak checking ✓

### Phase 5.2: Inline Destruction Tracking ✓ COMPLETE

**Files**: `function_analysis/slot_allocation.rs`, `function_analysis/drops.rs`, `function_analysis/mod.rs`

**Goal**: Track which temps are destroyed inline by the interpreter, exclude them from runtime tracking.

**What was done:**

1. **Added `SlotDestruction` enum to slot_allocation.rs**:
   - `InlineDestroyed`: Temp destroyed inline by interpreter (no drop point, no runtime tracking)
   - `NormalCleanup`: Slot cleaned up at scope end via drop points

2. **Extended `AllocatedSlot` with `destruction` field**:
   - Stores how the slot's contents will be cleaned up

3. **Marked inline-destroyed temps during slot allocation**:
   - BinOp operand temps: `InlineDestroyed` (destroyed after binop evaluation)
   - UnaryOp operand temps: `InlineDestroyed` (destroyed after unop evaluation)
   - If-condition temps: `InlineDestroyed` (destroyed after branch evaluation)
   - All sub-expressions inherit destruction mode from parent

4. **Updated `tracked_slots` computation in mod.rs**:
   - Only include Local/Temporary slots with `NormalCleanup`
   - Exclude `InlineDestroyed` slots from runtime tracking

5. **Updated `compute_drop_points` in drops.rs**:
   - Skip `InlineDestroyed` slots in both Phase 1 (branch convergence) and Phase 2 (function exits)
   - These slots don't need drop points since they're handled inline

**Key insight:**
Inline-destroyed temps have a deterministic lifecycle - they're created and destroyed within a single
expression evaluation. The interpreter handles their destruction directly (e.g., BinOp destroys
operands after the operation), so:
- No drop points needed (no cleanup at scope end)
- No runtime tracking needed (slot_states checks unnecessary)

**Result:**
- `tracked_slots` now excludes inline-destroyed temps
- Drop point computation skips inline-destroyed temps
- All 162 interp tests pass with leak checking ✓

**Slots now excluded from tracking:**
- Reference (parameter) slots: static analysis sufficient
- InlineDestroyed temps: BinOp/UnaryOp operands, if-conditions

**Slots still tracked:**
- Local slots with NormalCleanup
- Temporary slots with NormalCleanup (e.g., return values, function call results)
- Conditionally-initialized/moved slots

### Phase 6: Argument Passing - NO-OP

Frame-based calls already use DPS with temp slots for arguments.
No changes needed.

### Phase 7: Return DPS + Unified Coercion - IN PROGRESS

**Files**: `interp/mod.rs`, `interp/frame.rs`

**Goal**: Caller provides return destination to callee; return expressions write directly to caller's
memory via DPS. Coercion (T→Option<T>) uses existing mechanism instead of duplicate wrapping code.

**What was done:**

1. **Added `return_dest: Option<Destination>` to `StackFrame`**:
   - Stores destination for return value (caller's memory)
   - If Some, return expressions write directly there
   - If None, falls back to heap allocation (script scope)

2. **Updated `execute_function_body`**:
   - Added `return_dest` parameter
   - Passes it to frame creation
   - When `return_dest` is Some, skips heap cloning of Borrowed return values

3. **Updated `eval_function_call_frame`**:
   - Added `return_dest` parameter
   - Passes caller's `dest` through to `execute_function_body`

4. **Updated `eval_return_expression_frame`**:
   - If `return_dest` is Some: evaluates into caller's memory with coercion
   - For Option/Result destinations: evaluates without dest first, then coerces if needed
   - If `return_dest` is None: falls back to heap allocation with wrapping

5. **Removed duplicate Option/Result wrapping from `execute_function_body`**:
   - Old code wrapped all Ok values in Some/Ok at function boundary
   - Now coercion happens in `eval_return_expression_frame` where it belongs
   - Kept try-operator error handling (`OptionNone`/`ResultErr`) for early returns

**Key insight:**
T→Option<T> coercion is a value conversion that should happen at the point of assignment, not
at function return. With Return DPS, the return expression evaluates into a typed destination,
and coercion kicks in via the existing `coerce_value_to_dest` mechanism.

**Partial result:**
- Return values written directly to caller's memory when `return_dest` provided
- No heap cloning needed for frame-based calls
- Coercion unified through `coerce_value_to_dest` mechanism
- All 162 interp tests pass with leak checking ✓

#### Remaining Non-DPS Cases to Fix

The following code paths still bypass DPS and need to be eliminated:

**1. Script scope calls** (line ~1015)
```rust
execute_function_body(ctx, func, func_module, arg_values, None)
```
- `eval_function_call_in_script_scope()` passes `None` for return_dest
- Falls back to heap allocation
- **Must fix**: Script scope must provide a return destination

**2. Borrowed frame returns without return_dest** (lines ~1265-1326)
- When `return_dest` is None AND return expression yields Borrowed value
- Must clone to heap before frame cleanup since frame will be deallocated
- Uses `dtlv_rti_clone_local()` for types with internal pointers

**3. Option/Result wrapping when return_dest is None** (lines ~2386-2406)
- When function returns T but return type is Option<T> or Result<T>
- Calls `allocate_option_some_from_value()` or `allocate_result_ok_from_value()`
- Allocates wrapper on heap, copies inner value, frees original

**4. Coercion check path** (lines ~2303-2328)
- Even when `return_dest` IS provided, evaluates without dest first
- Checks if T→Option<T> or T→Result<T> coercion needed
- Then copies/coerces to dest afterward
- NOT pure DPS - should evaluate directly into dest with coercion

**5. @none/@error typed literals** (lines ~2354-2378)
- When returning typed `@none` or `@error` literals
- Allocates heap memory for typed destination
- Evaluates into temporary heap buffer

**6. Copy type reads** (lines ~2004-2015)
- Reading copy-type from Reference slot always clones
- Uses `clone_value_to_dest()` regardless of dest availability
- Correct behavior (must preserve original), but not pure DPS

**7. Try-operator early returns** (lines ~1346-1362)
- ? or ! operators trigger early return with Option::None or Result::Err
- Allocates wrapper on heap, bypasses normal return expression evaluation

#### Phase 7 Completion Plan

To fully eliminate non-DPS paths and remove the fallback:

**7.1**: Script scope provides return destination - allocate heap buffer before call, pass as return_dest
**7.2**: Fix coercion check path - evaluate directly into dest with type-aware DPS
**7.3**: Fix try-operator early returns - write directly to return_dest
**7.4**: Fix @none/@error literals - use return_dest directly
**7.5**: Ensure all call paths have return_dest (nested calls, etc.)
**7.6**: Remove `return_dest: Option<Destination>` - make it non-optional `Destination`
**7.7**: Delete all heap allocation fallback code paths

### Phase 8: Simplify Value (Not Yet Started)

**Files**: `interp/value.rs`, `interp/memory.rs`, `interp/mod.rs`

1. Evaluate what `ValueLocation` is still needed for:
   - If slots own everything, Value.location may be unnecessary
   - Or simplify to just "needs structure freeing" for temporaries during eval

2. Possibly remove `ValueLocation` entirely if slots handle all ownership

3. Run tests - fix regressions

## Key Files

- `crates/datalove-datafun-compiler/src/function_analysis/mod.rs`
- `crates/datalove-datafun-compiler/src/function_analysis/slot_allocation.rs`
- `crates/datalove-datafun-compiler/src/function_analysis/drops.rs`
- `crates/datalove-datafun-compiler/src/function_analysis/liveness.rs`
- `crates/datalove-datafun-compiler/src/function_analysis/moves.rs`
- `crates/datalove-datafun-compiler/src/function_analysis/copyability.rs`
- `crates/datalove-datafun-compiler/src/interp/mod.rs`
- `crates/datalove-datafun-compiler/src/interp/frame.rs`
- `crates/datalove-datafun-compiler/src/interp/value.rs`
- `crates/datalove-datafun-compiler/src/interp/memory.rs`

## Scope

- **Script scope**: Now included - must provide return destination like frame-based calls.
- **Goal**: All function calls use DPS for return values. No heap allocation fallbacks.

## Risk Areas

1. **Pattern matching**: `evaluate_branch_condition` has side effects (frees structures). May need rethinking.

2. **Binop/unop borrow semantics**: Currently handled via `eval_expression_in_*_borrow`. Keep as interpreter-level logic for now (always clone for operators).

3. **Drop order**: If multiple slots drop at same point, order may matter for observable effects. Current implementation may have implicit ordering.

4. **Interaction with script scope**: Function calls from script scope pass TempOwned values. Need to ensure handoff still works correctly.
