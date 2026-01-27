# Datafun Compiler Guide

Reference for the datalove-datafun compiler architecture, crate organization, salsa patterns, and testing.

## Crate Organization

### Core Compiler Crates

| Crate | Purpose |
|-------|---------|
| `datalove-datafun-compiler` | Core pipeline: parsing, salsa-tracked phases, module compilation |
| `datalove-datafun` | High-level facade, `ModuleCompilationPipeline`, `ScriptCompilationContext` |
| `datalove-datafun-parser` | Lexer, parser, bracer |
| `datalove-datafun-ast` | AST types (`Statement`, `Expr`, etc.) |
| `datalove-datafun-resolve` | Name resolution (type aliases, function signatures) |
| `datalove-datafun-tycheck` | Type checking, call resolution |
| `datalove-datafun-sema` | Semantic analysis (`FunctionAnalysis`, drop schedules) |
| `datalove-datafun-ownership` | Ownership analysis (move/borrow/drop tracking) |
| `datalove-datafun-lower` | AST to IR lowering |
| `datalove-datafun-const` | Const evaluation (CTFE) and const inlining |
| `datalove-datafun-ir` | IR types (`IrFunction`, `IrType`, `ValueId`, etc.) |
| `datalove-datafun-interp` | Interpreter, `CallDispatcher`, `ModuleFunctionRegistry` |
| `datalove-datafun-cranelift` | Shared Cranelift utilities |
| `datalove-datafun-cranelift-aot` | AOT compilation via Cranelift |
| `datalove-datafun-cranelift-jit` | JIT compilation |
| `datalove-datafun-pkg` | Package loading, `PackageWorld`, module resolution |

### Supporting Crates

| Crate | Purpose |
|-------|---------|
| `bct` | Base compiler toolkit: `ModuleGraph`, `Module`, `ModuleId`, source maps, text interning |
| `datalove-ct` | Compile-time utilities, query logging |
| `datalove-diagnostic` | Diagnostic/error infrastructure |
| `datalove-datalit` | Data literal types and typechecking |
| `datalove-rtdt` | Runtime type descriptors (`TyDesc`) |
| `datalove-exampletest` | Snapshot test harness |

## Compilation Pipeline

```
Source Text
    |
    v
[Phase 1: Parse]  parse_module_graph_with_mode
    |             - Per-module: parse_module_full [tracked]
    v             - Output: ParsedModuleGraph
    |
[Phase 2: Name Resolution]  resolve_all_names_with_mode
    |                       - Per-module: resolve_module_names [tracked]
    v                       - Output: AllModuleNameResolutions
    |                       - Collects type aliases and function signatures
    |
[Phase 3: Typecheck]  typecheck_module_graph_with_mode
    |                 - Per-module: typecheck_module [tracked]
    v                 - Output: ModuleGraphTypecheckResult
    |                 - Uses name resolution via memoization (cache hit)
    |
[Phase 4: Ownership Analysis]  analyze_module_graph_with_mode
    |                          - Per-module: analyze_module [tracked]
    v                          - Output: ModuleGraphAnalysis
    |
[Phase 5: IR Lowering]  lower_module_graph_with_evaluator
    |                   - See "Lower/Const Pipeline" below
    v                   - Per-module: lower_module [tracked]
    |                   - Output: ModuleGraphLoweringResult
    |
ModuleCompilationOutput
```

### Lower/Const Pipeline

Phase 5 has multiple subphases with different flows for modules vs scripts.

**Module lowering** (tracked, incremental):
```
1. Lower all functions once (lower_all_module_functions)
   |   - IrFunction per function, reused for CTFE + final assembly
   v
2. Const evaluation (evaluate_all_module_consts) [outside tracked fn]
   |   - Uses CTFE evaluator to execute const binding expressions
   |   - Function-level consts qualified as "func_name::const_name"
   v
3. lower_module [tracked]
   |   - Accepts pre_resolved_consts + optional lowered_functions
   |   - inline_module_functions() replaces const refs with values
   v
Final IR with consts inlined
```

**Script lowering** (non-incremental, REPL-style):
```
Phase 2a: Lower functions (phase_lower_functions)
    |     - Lower once, reused for const eval + final assembly
    v
Phase 3: Const evaluation (phase_const_eval)
    |     - Script-level: evaluate_script_consts()
    |     - Function-level: evaluate_function_consts()
    v
Phase 4: Assemble IR (phase_assemble_ir)
    |     - Combines lowered functions + module code
    |     - inline_script_consts() replaces const refs
    v
Final IrScriptUnit
```

**Key types:**
- `PreparedConst`: Either `Simple(ConstValue)` or `Unit(IrScriptUnit)` for CTFE
- `ModulePreResolvedConsts`: Pre-evaluated consts passed to tracked `lower_module`
- `ResolvedConsts`: Script-level const values (name -> value)

**Crate responsibilities:**
| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-lower` | AST to IR lowering, `lower_const_binding()` |
| `datalove-datafun-const` | `evaluate_prepared_const()`, `inline_*_consts()` |
| `datalove-datafun-compiler` | `tracked_lower.rs` - salsa-tracked module lowering |
| `datalove-datafun` | `script_compiler.rs` - script compilation pipeline |

### Entry Points

**Module compilation** (`datalove-datafun-compiler/src/compile.rs`):
```rust
pub fn compile_modules<'db>(
    db: &'db dyn DbClone,
    input: ModuleCompilationInput,
    mode: ParallelMode,
) -> ModuleCompilationOutput<'db>
```

**High-level API** (`datalove-datafun`):
```rust
let mut pipeline = ModuleCompilationPipeline::default();
pipeline.add_module(&db, "local", "pkg", "main", source);
let compiled = pipeline.compile_fresh(&db);
// Or incremental:
pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
let compiled = pipeline.compile(&mut db);
```

## Salsa Patterns

See also: [salsa-patterns.md](salsa-patterns.md) for detailed salsa type categories and gotchas.

### Database

Single database struct in `datalove-datafun-compiler/src/lib.rs`:

```rust
#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}
```

Implements `DbClone` for parallel execution (shares `Arc<Zalsa>` global state, clones thread-local state).

### Tracked Functions

Each phase has a tracked function that salsa memoizes:

| Function | Cache Key | Output |
|----------|-----------|--------|
| `parse_module_full` | `module` | `ParseResult` with spans |
| `parse_module_ast` | `module` | `ParsedStatements` (no spans) |
| `resolve_module_names` | `module`, `parsed` | `ModuleNameResolution` (type aliases, functions, ASTs) |
| `typecheck_module` | `module`, `parsed`, `name_resolution`, `imports` | `SingleModuleTypecheckResult` |
| `analyze_module` | `module`, `parsed`, `typecheck` | `SingleModuleAnalysis` |
| `lower_module` | `module`, `ir_idx`, `parsed`, `typecheck`, `ownership_analysis`, `func_id_map`, `pre_resolved_consts`, `skip_const_inlining`, `lowered_functions` | `SingleModuleLoweringResult` |

Graph-level functions (`*_module_graph`) aggregate per-module results.

### Tracked Structs

Result types are `#[salsa::tracked]` for stable identity:
- `ParsedModuleGraph<'db>`
- `ModuleNameResolution<'db>`, `AllModuleNameResolutions<'db>`
- `SingleModuleTypecheckResult<'db>`, `ModuleGraphTypecheckResult<'db>`
- `SingleModuleAnalysis<'db>`, `ModuleGraphAnalysis<'db>`
- `SingleModuleLoweringResult<'db>`, `ModuleGraphLoweringResult<'db>`
- `FuncIdMap<'db>`

### Incremental Compilation

**Key principle**: `Module` objects must be reused, not recreated.

`IncrementalModuleWorld` (`datalove-datafun-compiler/src/module_graph.rs`):
- Stores `Module` objects in `BTreeMap<String, Module>`
- `add_module()`: Creates new module once
- `update_source()`: Uses `module.set_source()` to update without breaking identity
- Graph rebuilds reuse existing `Module` objects

**Memoization behavior**:
- Whitespace-only changes: Parses again, but AST equality prevents re-typecheck
- AST changes (same types): Re-typechecks the changed module only
- Type changes: Re-typechecks dependents too

### Parallel Execution

Enabled via `DATALOVE_PARALLEL=1`. Pattern:

```rust
pub fn parse_module_graph_parallel<'db>(db: &'db dyn DbClone, ...) -> ParsedModuleGraph<'db> {
    // 1. Prepare work items with cloned databases
    let work: Vec<_> = modules.iter()
        .map(|m| (db.dyn_clone(), m))
        .collect();

    // 2. Warm cache in parallel (rayon)
    work.into_par_iter().for_each(|(db_clone, module)| {
        let _ = parse_module_full(db_clone.as_salsa_db(), module);
    });

    // 3. Delegate to tracked function (cache hits)
    parse_module_graph(db.as_salsa_db(), ...)
}
```

Why it works: `dyn_clone()` shares the global memoization cache (`Arc<Zalsa>`).

## Module Graph Handling

### Key Types (from `bct`)

- `ModuleGraph`: DAG of modules in dependency order
- `Module`: Salsa-tracked, has `id()` and `source()` methods
- `ModuleId`: Interned path (e.g., `"local/pkg/main"`)

### Cross-Module Resolution

**Requires**: `BTreeMap<ModuleId, Vec<(String, ModuleId)>>` maps modules to their imports.

**FuncIdMap**: Maps `(ModuleId, func_name)` to `(IrModuleId, FuncId)` for cross-module calls.

## IR Structure

### IDs

```rust
pub struct ValueId(pub u32);   // SSA value (immutable)
pub struct SlotId(pub u32);    // Mutable slot (var bindings)
pub struct ParamId(pub u32);   // Function parameter
pub struct BlockId(pub u32);   // Control flow block
pub struct FuncId(pub u32);    // Module-local function ID
pub struct IrModuleId(pub u32); // Module index
```

### Function References

```rust
pub enum FuncRef {
    Local(FuncId),                              // Same module
    External { unit: u32, func: FuncId },       // Previous script unit
    Module { module: IrModuleId, func: FuncId }, // Different module
}
```

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
}
```

## Error Categories

Errors are tracked separately in `ModuleCompilationOutput`:

```rust
pub struct ModuleCompilationOutput<'db> {
    pub typecheck_errors: BTreeMap<String, Vec<String>>,
    pub ownership_errors: BTreeMap<String, Vec<String>>,
    pub lowering_errors: BTreeMap<String, Vec<String>>,
    // ...
}
```

Modules with typecheck errors skip ownership analysis and lowering.

## Ownership Analysis

Ownership analysis (`ownership_analysis.rs`) runs after typechecking and before IR lowering. It performs:

1. **Ownership tracking**: Values are Live or Moved
2. **Borrow checking**: `ref`/`mut`/`out` params cannot be moved
3. **Initialization tracking**: `out` params must be initialized before return
4. **Drop scheduling**: Computes where Drop instructions should be emitted

### Analysis Errors

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

### Tracking Categories

```rust
pub enum TrackingCategory {
    Copy,    // Copy type - no tracking/drops needed
    Precise, // State statically known at every point
    Tracked, // State may vary at runtime (needs tracking byte)
}
```

**Tracked bindings**: Exports, `out` params, conditional moves, mutable slots.

### Drop Schedule

`DropSchedule` tells lowering where to emit drops:

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

## Parameter Modes in IR

### ParamMode

```rust
pub enum ParamMode {
    In,  // Ownership transfers to callee
    Ref, // Read-only borrow
    Mut, // Read-write borrow
    Out, // Write-only, callee must initialize
}
```

### IR Instructions for Parameters

**Reading params:**
- `ParamLoad { param, value }` - Load value from param slot

**Writing to `mut`/`out` params:**
- `ParamStore { param, value }` - Store to Mut param (always destroys old value)
- `ParamStoreTracked { param, value }` - Store to Out param (checks tracking byte)
- `ParamSetField { param, field_path, value }` - Set field in Mut param
- `ParamSetFieldTracked { param, field_path, value }` - Set field in Out param

**Tracking field:**
- `IrFunction.tracked_params: Vec<ParamId>` - Params needing runtime tracking

### Call Site Semantics

For `out` params, the **caller** destroys the existing value before the call:

```rust
// Lowering emits DropViaRef before passing field projection to out param
Instruction::DropViaRef { ref_value }
```

The callee sees an uninitialized slot and uses tracked instructions that skip destroy on first write.

### AOT Frame Layout

Out params get tracking bytes in the frame:

```rust
// FrameLayout::compute
let param_tracking_base = tracking_offset + tracked_values.len() + tracked_slots.len();
for (i, &pid) in tracked_params.iter().enumerate() {
    params[pid.0 as usize].tracking_byte = Some(param_tracking_base + i as u32);
}
```

Tracking bytes: `UNINIT = 0x00`, `LIVE = 0x01`, `MOVED = 0x02`

## Test Patterns

### Exampletest Framework

Snapshot testing with automatic blessing:

```rust
ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
    .fixture_subdir("ir_lower")
    .file_extension("world")
    .allow_errors(true)
    .run();
```

- `BLESS=1 cargo test` updates expected output
- Tests run in parallel via rayon
- Filter: `cargo test -- filter_name`

### Test Suites

| Suite | Purpose |
|-------|---------|
| `parser_tests` | Parse and emit AST + diagnostics |
| `tycheck_tests` | Typecheck results |
| `ir_lower_tests` | IR lowering from worldfiles |
| `ir_lower_script_tests` | Script IR lowering |
| `interp_tests` | Interpreter execution |
| `aot_tests` | AOT compilation (Cranelift) |
| `dual_tests` | Compare interp vs AOT output |
| `aot_layout_tests` | Verify AOT/interp layout compatibility |
| `module_memo_tests` | Salsa memoization behavior |

### Worldfile Format

Multi-module test cases:

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

**Special sections for memoization tests**:
- `module-change-ws`: Whitespace-only change (should not re-typecheck)
- `module-change-ast`: AST change, same types
- `module-change-ty`: Type-level change (forces dependent re-typecheck)

### Memoization Analysis

`memo_analysis::analyze_memo_worldfile()` tracks which modules were actually re-parsed/re-typechecked/re-lowered at each step, comparing against expected behavior.

## Key Files

| File | Contents |
|------|----------|
| `compiler/src/lib.rs` | `Database`, module exports |
| `compiler/src/compile.rs` | `compile_modules()`, `ModuleCompilationOutput` |
| `compiler/src/module_graph.rs` | `IncrementalModuleWorld`, parsing pipeline |
| `compiler/src/tracked_ownership_analysis.rs` | Salsa-tracked ownership analysis |
| `compiler/src/tracked_lower.rs` | Salsa-tracked IR lowering, `lower_module()`, `lower_all_module_functions()` |
| `compiler/src/const_eval.rs` | `extract_const_value()` - interpreter memory to ConstValue |
| `lower/src/lib.rs` | Lower crate entry, `lower_function_for_module()` |
| `lower/src/const_expr.rs` | `lower_const_binding()`, `try_extract_literal()` |
| `lower/src/script.rs` | Script unit lowering |
| `const/src/eval.rs` | `evaluate_prepared_const()`, `evaluate_consts_prepared()` |
| `const/src/inline.rs` | `inline_script_consts()`, `inline_module_functions()` |
| `datafun/src/pipeline.rs` | `ModuleCompilationPipeline` |
| `datafun/src/pipeline/script_compiler.rs` | Script compilation with const phases |
| `datafun/src/memo_analysis.rs` | Memoization testing infrastructure |
| `datafun/src/incremental.rs` | `IncrementalModuleWorld` (higher-level) |
| `ownership/src/lib.rs` | Drop/ownership analysis (see Ownership Analysis section) |

## Design Patterns

### Stable Identity
`Module` objects created once and reused. Updates use `set_source()` not recreation.

### Content Hash
`ParsedModuleGraph.module_content_hashes()` computes transitive hashes for dependency tracking.

### Parallel Warming
Rayon warms salsa cache, then tracked function hits cache.

### Deferred Errors
Errors collected as `PendingDiagnostic`, emitted with source location via `SpanLookup`.
