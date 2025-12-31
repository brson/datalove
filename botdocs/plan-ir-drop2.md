# Precise Drops via Pre-Lowering Analysis

## Status

**IMPLEMENTED** - Core phases (1, 2, 6) complete. All tests pass.

- Phase 1: Analysis module - DONE
- Phase 2: Lowering integration - DONE
- Phase 3: ScopeTracker cleanup - PENDING (disabled when schedule present)
- Phase 4: Worldfile analysis - DONE (automatic via func.rs)
- Phase 5: Runtime tracking removal - PENDING (optional)
- Phase 6: Testing - DONE (all tests pass)

### Implementation Notes

**Key design change from plan**: Schedule keyed by statement index (`usize`) rather than AST node references. Simpler and avoids lifetime issues.

**Consuming vs non-consuming uses**: Added `is_consumed: bool` to `analyze_expr_moves()`. Binary/unary ops don't consume (just read), while let bindings, function args, and returns do consume.

**Backwards compatibility**: `has_drop_schedule()` check allows ScopeTracker fallback for script units.

### Files Changed

- `ir/drop_analysis.rs` - NEW (~845 lines)
- `ir/mod.rs` - Added module export
- `ir/lower/context.rs` - Added drop_schedule fields, emit helpers
- `ir/lower/func.rs` - Runs analysis, sets schedule
- `ir/lower/stmt.rs` - Uses scheduled drops
- `ir/lower/script.rs` - Fixed function call args
- `tests/fixtures/interp3/120_conditional_move_convergence.world` - NEW test

---

## Goal

Design a new analysis system that runs before lowering to:
1. Detect semantic errors (use-after-move, double-move, etc.)
2. Compute precise drop points for branch convergence

The old `function_analysis` is oriented toward the old interpreter. This new analysis is designed for IR lowering - it produces a `DropSchedule` that lowering consumes to emit precise drops.

## Current State

**Runtime tracking (to eliminate for functions):**
- `Frame.value_initialized: Vec<bool>` / `slot_initialized: Vec<bool>`
- `destroy_all()` cleanup pass at function exit
- Script units retain dynamic tracking (top-level bindings persist across units)

**Current ScopeTracker (inadequate):**
- Tracks `(operand, ty, moved: bool)` per scope
- No branch convergence handling
- Results in double-drops or leaks at join points

**The bug (test 120_conditional_move_convergence):**
```datafun
fun test(cond: bool): u32
    let x = [@1, @2]
    if cond
        let _sink = x  // x moved here
    end if
    ret @0
end fun
```
Current IR emits `drop v3` in block1 AND block3, causing double-drop when cond=true.

## Design: Pre-Lowering Analysis

### Approach

1. **Analysis runs before lowering** on AST to detect errors and compute drops
2. **Errors reported early** (use-after-move, double-move, etc.) - lowering skipped if errors
3. **Analysis produces drop schedule** that lowering consumes
4. **Lowering emits precise drops** using analysis results

Analysis operates on AST but is designed for IR's needs - it computes which bindings need drops at which control flow points.

### Analysis Module

New module: `ir/drop_analysis.rs` (or `ir/analysis/`)

```rust
/// Analyze a function AST to compute drop points and detect errors.
pub fn analyze_function<'db>(
    db: &'db dyn Db,
    func: ast::StmtFun<'db>,
    expr_types: &'db [Option<TypeAndHeap<'db>>],
) -> FunctionDropAnalysis<'db>

pub struct FunctionDropAnalysis<'db> {
    /// Errors detected (use-after-move, etc.)
    pub errors: Vec<AnalysisError>,
    /// Drop schedule for lowering to consume
    pub drops: DropSchedule<'db>,
}

pub struct DropSchedule<'db> {
    /// Drops at statement boundaries, keyed by statement AST node
    pub after_stmt: HashMap<ast::Statement<'db>, Vec<BindingId>>,
    /// Drops at branch exits (for convergence)
    pub branch_exits: HashMap<ast::Statement<'db>, Vec<BindingId>>,
    /// Drops before return statements
    pub before_return: HashMap<ast::StmtRet<'db>, Vec<BindingId>>,
    /// Drops before early-return expressions (checked/optional ops)
    pub before_try_return: HashMap<ast::Expr<'db>, Vec<BindingId>>,
}

/// Identifies a binding (parameter or let/var)
#[derive(Clone, Copy, Hash, Eq, PartialEq)]
pub struct BindingId(u32);
```

### Analysis Algorithm

**Step 1: Build CFG from AST**
- Each statement is a node
- Branches create divergent paths
- Track which bindings are in scope at each point

**Step 2: Track binding state**
```rust
enum BindingState {
    Live,       // In scope, not yet moved
    Moved,      // Ownership transferred
    Dropped,    // Already dropped
}
```

**Step 3: Forward analysis**
- At binding creation: state = Live
- At move (into function call, return, etc.): state = Moved
- At scope exit: schedule drop if Live

**Step 4: Handle branch convergence**
At if/match join points:
- If binding Moved on some paths, Live on others
- Schedule drops on Live paths before the join

**Step 5: Detect errors**
- Use after Move -> error
- Double Move -> error
- Move of borrowed value -> error (future)

### Lowering Integration

Lowering receives `DropSchedule` and emits drops:

```rust
pub fn lower_function<'db>(
    db: &'db dyn Db,
    func: ast::StmtFun<'db>,
    expr_types: &'db [Option<TypeAndHeap<'db>>],
    drops: &DropSchedule<'db>,  // From analysis
) -> Result<IrFunction, LowerError>
```

During lowering:
- After each statement, check `drops.after_stmt` and emit Drop
- At branch exits, check `drops.branch_exits` and emit Drop
- Before return, check `drops.before_return` and emit Drop

### Mapping: AST Bindings -> IR Operands

During lowering, maintain:
```rust
binding_to_operand: HashMap<BindingId, Operand>
```

When analysis says "drop BindingId(5)", lowering looks up the operand and emits `Drop { operand }`.

### Integration Flow

```rust
// In worldfile_analysis_ir3.rs
fn process_function(db, func, expr_types) -> Result<IrFunction, Vec<Error>> {
    // 1. Run analysis
    let analysis = ir::drop_analysis::analyze_function(db, func, expr_types);

    // 2. Check for errors
    if !analysis.errors.is_empty() {
        return Err(analysis.errors);
    }

    // 3. Lower with drop schedule
    let ir_func = ir::lower::lower_function(db, func, expr_types, &analysis.drops)?;
    Ok(ir_func)
}
```

**For script units:**
- Keep current runtime tracking (bindings persist across units)
- Analysis only applies to functions initially

## Implementation Plan

### Phase 1: New Analysis Module

Create `crates/datalove-datafun-compiler/src/ir/drop_analysis.rs`:
- `analyze_function()` - main entry point
- Build CFG from AST statements
- Track binding states (Live/Moved/Dropped)
- Compute drops at scope exits and branch convergence
- Detect errors (use-after-move, double-move)
- Return `FunctionDropAnalysis` with errors and drop schedule

### Phase 2: Integrate Analysis into Lowering

Modify `ir/lower/func.rs`:
- Accept `DropSchedule` parameter
- Track `binding_to_operand: HashMap<BindingId, Operand>`
- After each statement, emit scheduled drops
- At branch exits, emit convergence drops
- Before return, emit scheduled drops

### Phase 3: Remove ScopeTracker Drop Logic

Modify `ir/lower/scope.rs`:
- Remove `record_binding`, `mark_moved`, `bindings_to_drop*`
- Keep scope stack for variable shadowing/restore only
- Drops now come from `DropSchedule`, not `ScopeTracker`

### Phase 4: Wire Into Worldfile Analysis

Modify `worldfile_analysis_ir3.rs` and `worldfile_analysis_modules_ir3.rs`:
- Call `drop_analysis::analyze_function()` before lowering
- Report analysis errors
- Pass `DropSchedule` to lowering

### Phase 5: Remove Runtime Tracking for Functions

In interpreter:
- Remove `value_initialized`/`slot_initialized` from function frames
- Remove `destroy_all()` call after function execution
- Script unit frames keep runtime tracking

### Phase 6: Testing

- Verify test 120 passes (branch convergence)
- Add more tests: nested branches, loops, early return
- Verify no memory leaks or double-frees

## Files to Modify

| File | Changes |
|------|---------|
| `ir/drop_analysis.rs` (NEW) | New analysis module |
| `ir/mod.rs` | Export drop_analysis |
| `ir/lower/func.rs` | Accept DropSchedule, emit drops |
| `ir/lower/context.rs` | Add binding_to_operand tracking |
| `ir/lower/scope.rs` | Remove drop tracking, keep scopes |
| `ir/lower/stmt.rs` | Remove emit_drops calls |
| `worldfile_analysis_ir3.rs` | Call analysis, pass schedule |
| `worldfile_analysis_modules_ir3.rs` | Same |
| `ir/interp/frame.rs` | Remove Vec<bool> for functions |
| `ir/interp/mod.rs` | Remove destroy_all for functions |

## Resolved Questions

1. **Slots**: Lowering already emits `Drop` before `SlotStore` when reassigning a slot (see `stmt.rs:55-58`). Analysis only handles final scope-exit drops for slots.

2. **Phi nodes**: Yes, we use Phi instructions. The incoming operand from the taken predecessor is consumed. Analysis must track this - Phi consumes one of its operands based on control flow.

3. **Existing drops**: Lowering currently emits naive drops via ScopeTracker. This will be removed - lowering will emit drops only from `DropSchedule`.

## Edge Cases

**Loops**: Values defined in loop bodies may need drops at:
- Loop exit (break/fallthrough)
- Each iteration (for values from previous iteration)
Loop-carried values need careful handling.

**Early returns**: Return/TryReturn consume return value but need drops for all other live values.

**Checked/Optional operators**: `+?`, `+!`, `-?`, `-!`, `*?`, `*!`, `/!`, unary `-?`, `-!` all emit `TryReturn` on overflow/underflow. These are early-return points **within expressions**.

```datafun
let y = some_list
let z = a +? b   // if overflow -> TryReturn, must drop y first
```

Analysis must track which expressions contain early-return operators. The `DropSchedule` needs:
```rust
/// Drops before early-return expressions (checked/optional ops)
pub before_try_return: HashMap<ast::Expr<'db>, Vec<BindingId>>,
```

During lowering, when emitting `TryReturn` for overflow path, emit scheduled drops first.

**Script units**: Keep runtime tracking. Analysis only applies to functions initially.
