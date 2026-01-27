# Datafun Compiler Guide

Reference for the datalove-datafun compiler architecture.

## Contents

- [Crate Organization](#crate-organization)
- [Module Compilation Pipeline](#module-compilation-pipeline)
  - [Phase 5: IR Lowering Detail](#phase-5-ir-lowering-detail)
  - [Const Evaluation](#const-evaluation)
- [Script Compilation Pipeline](#script-compilation-pipeline)
- [IR Types](#ir-types)
  - [IDs](#ids)
  - [Function References](#function-references)
  - [IrFunction](#irfunction)
  - [Parameter Modes](#parameter-modes)
- [Salsa Patterns](#salsa-patterns)
  - [Database](#database)
  - [Tracked Functions](#tracked-functions)
  - [Incremental Compilation](#incremental-compilation)
  - [Parallel Execution](#parallel-execution)
- [Ownership Analysis](#ownership-analysis)
  - [Tracking Categories](#tracking-categories)
  - [Error Codes](#error-codes)
  - [Drop Schedule](#drop-schedule)
  - [AOT Tracking Bytes](#aot-tracking-bytes)
- [Entry Points](#entry-points)
- [Test Patterns](#test-patterns)
- [Key Files](#key-files)

## Crate Organization

### Core Compiler Pipeline

| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-parser` | Lexer, parser, bracer |
| `datalove-datafun-ast` | AST types (`Statement`, `Expr`, etc.) |
| `datalove-datafun-resolve` | Name resolution (type aliases, function signatures) |
| `datalove-datafun-tycheck` | Type checking, call resolution |
| `datalove-datafun-ownership` | Ownership analysis (move/borrow/drop tracking) |
| `datalove-datafun-lower` | AST to IR lowering |
| `datalove-datafun-const` | Const evaluation (CTFE) and const inlining |
| `datalove-datafun-ir` | IR types (`IrFunction`, `IrType`, `ValueId`, etc.) |
| `datalove-datafun-compiler` | Salsa-tracked pipeline, `compile_modules()`, `lower_module_graph_with_evaluator()` |
| `datalove-datafun` | High-level facade, `ModuleCompilationPipeline`, `ScriptCompiler` |

### Execution

| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-interp` | Interpreter, `CallDispatcher`, `ModuleFunctionRegistry` |
| `datalove-datafun-cranelift` | Shared Cranelift utilities |
| `datalove-datafun-cranelift-aot` | AOT compilation |
| `datalove-datafun-cranelift-jit` | JIT compilation |

### Supporting

| Crate | Responsibility |
|-------|----------------|
| `bct` | Base compiler toolkit: `ModuleGraph`, `Module`, `ModuleId`, source maps, text interning |
| `datalove-datafun-pkg` | Package loading, `PackageWorld`, module resolution |
| `datalove-ct` | Compile-time utilities, query logging |
| `datalove-diagnostic` | Diagnostic/error infrastructure |
| `datalove-datalit` | Data literal types and typechecking |
| `datalove-rtdt` | Runtime type descriptors (`TyDesc`) |
| `datalove-exampletest` | Snapshot test harness |

## Module Compilation Pipeline

Module compilation runs through five phases. Phases 1-4 are handled by `compile_modules()`,
phase 5 by `lower_module_graph_with_evaluator()`.

```
Source Text
    |
    v
[Phase 1: Parse]
    |   parse_module_graph_with_mode
    |   Per-module: parse_module_full [tracked]
    v   Output: ParsedModuleGraph
    |
[Phase 2: Name Resolution]
    |   resolve_all_names_with_mode
    |   Per-module: resolve_module_names [tracked]
    v   Output: AllModuleNameResolutions
    |
[Phase 3: Typecheck]
    |   typecheck_module_graph_with_mode
    |   Per-module: typecheck_module [tracked]
    v   Output: ModuleGraphTypecheckResult
    |
[Phase 4: Ownership Analysis]
    |   analyze_module_graph_with_mode
    |   Per-module: analyze_module [tracked]
    v   Output: ModuleGraphAnalysis
    |
[Phase 5: IR Lowering]
    |   lower_module_graph_with_evaluator
    v   Output: ModuleGraphLoweringResult
```

Errors propagate between phases: modules with typecheck errors skip ownership analysis and lowering.

### Phase 5: IR Lowering Detail

Lowering has three internal phases that handle const evaluation correctly:

```
Phase 5a: Lower all functions
    |   lower_all_module_functions (non-tracked)
    |   Lowers every function to IR
    v   Reused for both CTFE and final assembly
    |
Phase 5b: Evaluate consts
    |   evaluate_all_module_consts (non-tracked)
    |   Uses CTFE evaluator with lowered functions
    |   Function-level consts qualified as "func_name::const_name"
    v   Output: HashMap<ModuleId, ModulePreResolvedConsts>
    |
Phase 5c: Assemble modules
    |   lower_module [tracked]
    |   Reuses pre-lowered functions
    |   Inlines evaluated const values
    v   Output: ModuleGraphLoweringResult
```

Why this structure:
- Functions are lowered once and reused - const expressions can call functions via CTFE
- Const evaluation happens outside tracked functions (uses interpreter state)
- `lower_module` is tracked with pre-resolved consts as hashable input, enabling memoization

The `skip_const_inlining` flag skips phases 5a and 5b entirely, lowering const bindings as
let bindings. Used for testing CTFE accuracy.

### Const Evaluation

Const evaluation in `evaluate_single_const` has three fast paths:

1. **Simple literals** - booleans, integers, floats, strings, None extracted directly
2. **Const references** - look up already-evaluated const from `resolved_so_far` map
3. **Complex expressions** - lower to minimal `IrScriptUnit`, execute via CTFE, extract result

Function calls in const expressions work because `lowered_functions` are passed to the CTFE evaluator.

## Script Compilation Pipeline

Scripts compile incrementally in REPL-style, accumulating exports across units.

```
[Phase 1: Parse + Resolve + Typecheck]
    |   Standard pipeline through typecheck
    v
[Phase 2: Lower functions]
    |   phase_lower_functions
    |   Lower all functions once
    v   Reused for CTFE and final assembly
    |
[Phase 3: Const evaluation]
    |   phase_const_eval
    |   evaluate_script_consts (script-level)
    |   evaluate_function_consts (function-level, uses script results)
    v
[Phase 4: Assemble IR]
    |   phase_assemble_ir
    |   Combines functions + module code
    |   inline_script_consts replaces const refs
    v   Output: IrScriptUnit
```

Script units accumulate via `AccumulatedLowerBindings`:
- Tracks exports from prior units
- Each unit increments `accumulated_unit_specs`
- Functions reference prior units via `FuncRef::External { unit, func }`

## IR Types

### IDs

```rust
pub struct ValueId(pub u32);    // SSA value (immutable)
pub struct SlotId(pub u32);     // Mutable slot (var bindings)
pub struct ParamId(pub u32);    // Function parameter
pub struct BlockId(pub u32);    // Control flow block
pub struct FuncId(pub u32);     // Module-local function ID
pub struct IrModuleId(pub u32); // Module index (not salsa ModuleId)
```

### Function References

```rust
pub enum FuncRef {
    Local(FuncId),                               // Same module/unit
    External { unit: u32, func: FuncId },        // Previous script unit
    Module { module: IrModuleId, func: FuncId }, // Different module
}
```

`Module` uses numeric `IrModuleId` (not salsa `ModuleId`) for serializability.

### IrFunction

```rust
pub struct IrFunction {
    pub id: FuncId,
    pub name: String,
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub blocks: Vec<BasicBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub value_types: Vec<IrType>,
    pub slot_types: Vec<IrType>,
    pub tracked_params: Vec<ParamId>,  // Params needing runtime tracking
}
```

### Parameter Modes

```rust
pub enum ParamMode {
    In,  // Ownership transfers to callee
    Ref, // Read-only borrow
    Mut, // Read-write borrow
    Out, // Write-only, callee must initialize
}
```

IR instructions for parameters:
- `ParamLoad { param, value }` - load value
- `ParamStore { param, value }` - store to Mut (destroys old value)
- `ParamStoreTracked { param, value }` - store to Out (checks tracking byte)
- `ParamSetField { param, field_path, value }` - field write to Mut
- `ParamSetFieldTracked { param, field_path, value }` - field write to Out

For `out` params, the **caller** destroys the existing value before the call via `DropViaRef`.

## Salsa Patterns

See [salsa-patterns.md](salsa-patterns.md) for detailed type categories.

### Database

Single database in `datalove-datafun-compiler/src/lib.rs`:

```rust
#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}
```

Implements `DbClone` - shares `Arc<Zalsa>` global state, clones thread-local state.

### Tracked Functions

| Function | Key Inputs | Output |
|----------|------------|--------|
| `parse_module_full` | module | `ParseResult` with spans |
| `parse_module_ast` | module | `ParsedStatements` (no spans, for equality checks) |
| `resolve_module_names` | module, parsed | `ModuleNameResolution` |
| `typecheck_module` | module, parsed, name_resolution, imports | `SingleModuleTypecheckResult` |
| `analyze_module` | module, parsed, typecheck | `SingleModuleAnalysis` |
| `lower_module` | module, ir_idx, parsed, typecheck, ownership, func_id_map, consts, skip_inlining, funcs | `SingleModuleLoweringResult` |
| `compute_func_id_map` | parsed_graph | `FuncIdMap` |

Graph-level functions aggregate per-module results.

### Incremental Compilation

Key principle: `Module` objects are created once and reused. Updates use `set_source()`.

`IncrementalModuleWorld`:
- Stores `Module` objects in `BTreeMap<String, Module>`
- `add_module()` creates new module once
- `update_source()` preserves identity
- Graph rebuilds reuse existing modules

Memoization behavior:
- Whitespace-only changes: re-parses, but AST equality prevents re-typecheck
- AST changes (same types): re-typechecks changed module only
- Type changes: re-typechecks dependents

### Parallel Execution

Enabled via `DATALOVE_PARALLEL=1`:

```rust
pub fn parse_module_graph_parallel<'db>(db: &'db dyn DbClone, ...) -> ParsedModuleGraph<'db> {
    // Prepare cloned databases
    let work: Vec<_> = modules.iter()
        .map(|m| (db.dyn_clone(), m))
        .collect();

    // Warm cache in parallel
    work.into_par_iter().for_each(|(db_clone, module)| {
        let _ = parse_module_full(db_clone.as_salsa_db(), module);
    });

    // Tracked function hits cache
    parse_module_graph(db.as_salsa_db(), ...)
}
```

Works because `dyn_clone()` shares the global memoization cache.

## Ownership Analysis

Runs after typechecking, produces `DropSchedule` consumed by lowering.

### Tracking Categories

```rust
pub enum TrackingCategory {
    Copy,    // No tracking/drops needed
    Precise, // State statically known
    Tracked, // May vary at runtime (needs tracking byte)
}
```

Tracked bindings: exports, `out` params, conditional moves, mutable slots.

### Error Codes

| Code | Error | Trigger |
|------|-------|---------|
| D001 | UseAfterMove | Using value after move |
| D002 | DoubleMove | Moving value twice |
| D003 | CannotMoveBorrowed | Moving `ref`/`mut`/`out` param |
| D004 | CannotMutFromRef | Passing `ref` to `mut` param |
| D005 | ReadUninitializedOutParam | Reading `out` before `set` |
| D006 | OutParamNotInitialized | Return without initializing `out` |
| D007 | MoveInLoop | Moving outer-scoped value in loop |
| D008 | InconsistentBranchMove | Value moved in one branch only |
| D009 | OutParamPartialWrite | Field write to `out` param |

### Drop Schedule

`DropSchedule` tells lowering where to emit `Drop` instructions:

```rust
pub struct DropSchedule {
    pub then_branch_exit: BTreeMap<usize, Vec<BindingId>>,
    pub else_branch_exit: BTreeMap<usize, Vec<BindingId>>,
    pub before_return: BTreeMap<usize, Vec<BindingId>>,
    pub before_try_return: BTreeMap<usize, Vec<BindingId>>,
    pub loop_body_end: BTreeMap<usize, Vec<BindingId>>,
    pub before_break: BTreeMap<usize, Vec<BindingId>>,
    pub before_continue: BTreeMap<usize, Vec<BindingId>>,
}
```

### AOT Tracking Bytes

Out params get tracking bytes in the frame:

```rust
let param_tracking_base = tracking_offset + tracked_values.len() + tracked_slots.len();
for (i, &pid) in tracked_params.iter().enumerate() {
    params[pid.0 as usize].tracking_byte = Some(param_tracking_base + i as u32);
}
```

Values: `UNINIT = 0x00`, `LIVE = 0x01`, `MOVED = 0x02`

## Entry Points

### Module Compilation

**Analysis only** (`compile.rs`):

```rust
pub fn compile_modules<'db>(
    db: &'db dyn DbClone,
    input: ModuleCompilationInput,
    mode: ParallelMode,
) -> ModuleCompilationOutput<'db>
```

Returns after phase 4. Check `is_successful()` before lowering.

**With lowering** (`tracked_lower.rs`):

```rust
pub fn lower_module_graph_with_evaluator<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    mode: ParallelMode,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    skip_const_inlining: bool,
) -> ModuleGraphLoweringResult<'db>
```

### High-Level API

```rust
let mut pipeline = ModuleCompilationPipeline::default();
pipeline.add_module(&db, "local", "pkg", "main", source);
let compiled = pipeline.compile_fresh(&db);

// Incremental:
pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
let compiled = pipeline.compile(&mut db);
```

### Script Compilation

```rust
let script_compiler = ScriptCompiler::from_compiled(compiled);
let result = script_compiler.compile_fragment(source);
// result.typecheck, result.ownership, result.lowering
```

`ScriptCompilationResult` has separate results for partial compilation (typecheck can succeed while lowering fails).

## Test Patterns

### Exampletest Framework

```rust
ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
    .fixture_subdir("ir_lower")
    .file_extension("world")
    .allow_errors(true)
    .run();
```

- `BLESS=1 cargo test` updates expected output
- Filter: `cargo test -- filter_name`

### Test Suites

| Suite | Purpose |
|-------|---------|
| `parser_tests` | Parse and emit AST + diagnostics |
| `tycheck_tests` | Typecheck results |
| `ir_lower_tests` | IR lowering from worldfiles |
| `ir_lower_script_tests` | Script IR lowering |
| `interp_tests` | Interpreter execution |
| `aot_tests` | AOT compilation |
| `dual_tests` | Compare interp vs AOT output |
| `aot_layout_tests` | AOT/interp layout compatibility |
| `module_memo_tests` | Salsa memoization behavior |

### Worldfile Format

```
----------
module local/pkg/main
----------
fun foo() end fun

----------
module local/pkg/lib
----------
fun bar() end fun
```

Memoization test sections: `module-change-ws`, `module-change-ast`, `module-change-ty`.

## Key Files

| File | Contents |
|------|----------|
| `compiler/src/lib.rs` | `Database`, module exports |
| `compiler/src/compile.rs` | `compile_modules()`, `ModuleCompilationOutput` |
| `compiler/src/module_graph.rs` | `IncrementalModuleWorld`, parsing pipeline |
| `compiler/src/tracked_lower.rs` | `lower_module_graph_with_evaluator()`, three-phase lowering |
| `compiler/src/tracked_ownership_analysis.rs` | Salsa-tracked ownership analysis |
| `lower/src/lib.rs` | `lower_function_for_module()` |
| `lower/src/const_expr.rs` | `lower_const_binding()`, `try_extract_literal()` |
| `const/src/eval.rs` | `evaluate_prepared_const()` |
| `const/src/inline.rs` | `inline_script_consts()`, `inline_module_functions()` |
| `datafun/src/pipeline.rs` | `ModuleCompilationPipeline` |
| `datafun/src/pipeline/script_compiler.rs` | Script compilation pipeline |
| `ownership/src/lib.rs` | Drop/ownership analysis |
