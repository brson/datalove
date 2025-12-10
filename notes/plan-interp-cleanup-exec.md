# Interpreter Cleanup Execution Plan

Based on analysis of `crates/datalove-datafun/src/interp/mod.rs` (5816 lines)
and the goals in `plan-interp-cleanup.md`.

## Goals Recap

1. Interpreter should be entirely driven by function analysis, no ad-hoc decisions
2. Eliminate unnecessary clones (language only has moves, no clones yet)
3. Every value/temporary should be accounted for by function analysis
4. No ad-hoc heap allocations outside stack frame slots
5. Test all expression/statement types in three contexts:
   - Script unit statements
   - Functions in script units
   - Functions in modules
6. Functions in script units and modules should share code path
7. Script statements should share code with functions where reasonable

## Current Architecture

### Two Parallel Execution Paths (The Core Problem)

The interpreter has duplicated code for almost every operation:

1. **Script scope path**: `eval_expression_in_script_scope()` (~140 lines)
   - Uses `ScriptScope.variables` HashMap for variable storage
   - Move tracking via `ScriptVarState::Moved`
   - Ad-hoc heap allocation for values
   - Manual type coercion for Option/Result parameters (~220 lines in `eval_function_call_in_script_scope`)

2. **Frame path**: `eval_expression_frame()` (~280 lines)
   - Uses analysis-driven `StackFrame` with `frame_data` buffer
   - Move tracking via `SlotState::Moved` on slots
   - Temp slots from function analysis for sub-expressions
   - Much simpler function call handling (~50 lines)

### Key Structures

- `InterpContext`: Central context with call stack, script scope, module functions
- `ScriptScope`: REPL state - variables HashMap, functions HashMap
- `StackFrame`: Frame with packed `frame_data: Vec<u8>`, `slot_states`, layout from analysis
- `Value`: ptr + tydesc + location (Borrowed vs TempOwned)
- `Destination`: DPS target for expression evaluation

## Identified Issues

### 1. Clone Operations

Current `.clone()` calls in the interpreter:

**Type metadata clones (OK)**:
- `dt.clone()` on `crate::tycheck::Type::Datalit` - these are type descriptors, not values

**Value clones (BUGS per the design goal)**:
- `clone_value()` (lines 2480-2510): Full heap clone via `dtlv_rti_clone_local`
- `clone_value_to_dest()` (lines 2561-2599): Clone to pre-allocated destination
- Copy type handling clones values on every read
- Return value cloning when borrowed value escapes frame

The language doesn't have clones yet, so these represent either:
- Copy types (semantically correct to copy)
- Bugs where moves should be happening

### 2. Ad-hoc Heap Allocations

Script scope execution allocates values on the heap outside of analysis-driven slots:
- `eval_expression_in_script_scope` returns heap-allocated values
- Function call argument evaluation allocates temporary buffers
- These are not tracked by function analysis

### 3. Code Duplication

Duplicated evaluation functions (script scope vs frame):
- `eval_expression_in_script_scope` / `eval_expression_frame`
- `eval_inline_list_script_scope` / `eval_inline_list_frame`
- `eval_inline_set_script_scope` / `eval_inline_set_frame`
- `eval_inline_map_script_scope` / `eval_inline_map_frame`
- `eval_inline_anon_tuple_script_scope` / `eval_inline_anon_tuple_frame`
- `eval_inline_anon_struct_script_scope` / `eval_inline_anon_struct_frame`
- `eval_function_call_in_script_scope` / `eval_function_call_frame`

### 4. Asymmetric Features

Script scope has Option/Result coercion logic (~220 lines) that frame path doesn't have.
This suggests either:
- Missing functionality in frame path
- Over-engineering in script scope (analysis should handle coercion)

## Cleanup Strategy

### Phase 1: Audit and Document Clone Sites

For each clone operation, determine:
- Is this a copy type? (semantically correct)
- Is this a move that should happen without cloning?
- Is this compensating for missing analysis?

Create inventory of all clone sites with classification.

### Phase 2: Unify Expression Evaluation

Extract common expression evaluation logic into a trait-based or enum-based approach:

```rust
enum EvalContext<'a, 'db> {
    ScriptScope(&'a mut ScriptScope<'db>),
    Frame(usize),  // frame index
}

fn eval_expression<'db>(
    ctx: &mut InterpContext<'db>,
    eval_ctx: EvalContext<'_, 'db>,
    expr: ast::ExprFun<'db>,
    dest: Option<Destination>,
) -> Result<Value, InterpError>
```

This lets us share:
- BinOp evaluation
- UnaryOp evaluation
- Literal evaluation
- Collection literal evaluation

While keeping separate:
- Variable lookup (slot vs HashMap)
- Move tracking (SlotState vs ScriptVarState)

### Phase 3: Eliminate Script Scope Heap Allocations

For script-level execution, create a "script frame" that uses the same
analysis-driven slot allocation as function bodies:
- Script statements get temp slots from analysis
- Variables stored in frame slots, not HashMap
- Unified cleanup path

### Phase 4: Simplify Function Call Handling

The 220-line coercion logic in `eval_function_call_in_script_scope` should be
replaced by analysis-driven coercion:
- Function analysis should determine coercion requirements
- Caller just evaluates to the destination the analysis specifies
- No runtime type tag checking

### Phase 5: Test Coverage Audit

Create test matrix tracking coverage for each expression/statement type:

| Feature | Script Stmt | Fun in Script | Fun in Module |
|---------|-------------|---------------|---------------|
| let     | [ ]         | [ ]           | [ ]           |
| if      | N/A         | [ ]           | [ ]           |
| ret     | N/A         | [ ]           | [ ]           |
| binop   | [ ]         | [ ]           | [ ]           |
| unop    | [ ]         | [ ]           | [ ]           |
| tuple   | [ ]         | [ ]           | [ ]           |
| list    | [ ]         | [ ]           | [ ]           |
| map     | [ ]         | [ ]           | [ ]           |
| set     | [ ]         | [ ]           | [ ]           |
| struct  | [ ]         | [ ]           | [ ]           |
| funcall | [ ]         | [ ]           | [ ]           |
| try ?/! | N/A         | [ ]           | [ ]           |

Fill in the matrix by auditing existing tests in `tests/fixtures/interp/*.world`.

## Specific Refactoring Tasks

### Task 1: Extract Shared BinOp/UnaryOp Evaluation

`execute_binop()` and `execute_unop()` are already shared.
No changes needed here.

### Task 2: Extract Literal Evaluation

Create shared functions for literals that take a destination:
- `eval_bool_literal(ctx, value, dest)`
- `eval_int_literal(ctx, expr, dest)`
- `eval_float_literal(ctx, expr, dest)`
- `eval_string_literal(ctx, expr, dest)`

Both paths call these.

### Task 3: Extract Collection Evaluation

Create shared inline collection evaluators:
- `eval_inline_list(ctx, expr, dest, element_evaluator)`
- `eval_inline_set(ctx, expr, dest, element_evaluator)`
- `eval_inline_map(ctx, expr, dest, kv_evaluator)`
- `eval_inline_tuple(ctx, expr, dest, element_evaluator)`
- `eval_inline_struct(ctx, expr, dest, field_evaluator)`

The `*_evaluator` callbacks handle the context-specific sub-expression evaluation.

### Task 4: Unify Variable Read

Create abstraction over variable storage:
- Frame: read from slot at offset, check SlotState
- ScriptScope: read from HashMap, check ScriptVarState

### Task 5: Script Analysis Integration

Extend function analysis to cover script-level statements:
- Generate frame layout for script execution
- Assign temp slots for script expressions
- Use same execution path as functions

### Task 6: Remove Coercion Logic

Move Option/Result coercion to function analysis:
- Analysis determines when coercion needed
- Generates appropriate destination types
- Interpreter just follows analysis

## Test Requirements

After cleanup, verify with existing tests:
```bash
cargo test --package datalove-datafun --test interp_tests
```

Add new tests to fill coverage gaps identified in Phase 5 matrix.

## Success Criteria

1. No `.clone()` calls on runtime values (only type metadata clones OK)
2. All values stored in analysis-driven slots, no ad-hoc heap allocation
3. Single code path for expression evaluation (parameterized by context)
4. All expression/statement types tested in all three contexts
5. All existing tests pass
6. No memory leaks (verified by DATALOVE_LEAK_CHECK=panic)

## Progress (2025-12-10)

### Phase 1: Clone Audit - COMPLETE
Found that existing clones are for:
- Copy types (semantically correct)
- Type metadata (Type::Datalit clones, not values)
- Slot state vectors (for cleanup tracking)
No value clone bugs identified.

### Phase 2: Unified Expression Evaluation - PARTIAL
- Added `EvalContext` enum (ScriptScope | Frame) for dispatch
- Created unified collection evaluators:
  - `eval_inline_list()` - replaces both `_script_scope` and `_frame` versions
  - `eval_inline_set()` - replaces both `_script_scope` and `_frame` versions
  - `eval_inline_map()` - replaces both `_script_scope` and `_frame` versions
  - `eval_inline_anon_tuple()` - replaces both `_script_scope` and `_frame` versions
  - `eval_inline_anon_struct()` - replaces both `_script_scope` and `_frame` versions
- Removed ~100 lines of duplicate code
- Variable lookup/binop/etc remain separate (require deeper architectural changes)

### Phase 3 & 4: Script Scope Analysis - NOT STARTED
Requires extending function_analysis to cover script-level statements.
This is a significant architectural change.

### Phase 5: Test Coverage - COMPLETE
Added 6 new tests for collections in functions:
- 280_map_in_function.world
- 281_set_in_function.world
- 282_struct_in_function.world
- 283_list_in_function.world
- 284_tuple_in_function.world
- 287_struct_in_module.world

Note: Module tests for Map/Set/List not possible yet due to missing type syntax.

### Tests: 145 passing with DATALOVE_LEAK_CHECK=panic
