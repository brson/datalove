# Datafun Compiler Guide

Reference for the datalove-datafun compiler architecture, crate organization, salsa patterns, and testing.

## Crate Organization

### Core Compiler Crates

| Crate | Purpose |
|-------|---------|
| `datalove-datafun-compiler` | Core pipeline: parsing, typechecking, ownership analysis, IR lowering |
| `datalove-datafun` | High-level facade, `ModuleCompilationPipeline`, `ScriptCompilationContext` |
| `datalove-datafun-parser` | Lexer, parser, bracer |
| `datalove-datafun-ast` | AST types (`Statement`, `Expr`, etc.) |
| `datalove-datafun-tycheck` | Type checking, call resolution |
| `datalove-datafun-ir` | IR types (`IrFunction`, `IrType`, `ValueId`, etc.) |
| `datalove-datafun-interp` | Interpreter, `CallDispatcher`, `ModuleFunctionRegistry` |
| `datalove-datafun-aot-cranelift` | AOT compilation via Cranelift |
| `datalove-datafun-jit` | JIT compilation |
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
[Phase 2: Typecheck]  typecheck_module_graph_with_mode
    |                 - Per-module: typecheck_module [tracked]
    v                 - Output: ModuleGraphTypecheckResult
    |
[Phase 3: Ownership Analysis]  analyze_module_graph_with_mode
    |                          - Per-module: analyze_module [tracked]
    v                          - Output: ModuleGraphAnalysis
    |
[Phase 4: IR Lowering]  lower_module_graph_with_evaluator
    |                   - Per-module: lower_module [tracked]
    v                   - Output: ModuleGraphLoweringResult
    |
ModuleCompilationOutput
```

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
let mut pipeline = ModuleCompilationPipeline::new();
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
| `typecheck_module` | `module`, `parsed`, `requires` | `SingleModuleTypecheckResult` |
| `analyze_module` | `module`, `parsed`, `typecheck` | `SingleModuleAnalysis` |
| `lower_module` | `module`, `ir_idx`, `parsed`, `typecheck`, `ownership_analysis`, `func_ids` | `SingleModuleLoweringResult` |

Graph-level functions (`*_module_graph`) aggregate per-module results.

### Tracked Structs

Result types are `#[salsa::tracked]` for stable identity:
- `ParsedModuleGraph<'db>`
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
| `compiler/src/ownership_analysis.rs` | Drop/ownership analysis |
| `compiler/src/tracked_ownership_analysis.rs` | Salsa-tracked ownership analysis |
| `compiler/src/tracked_lower.rs` | Salsa-tracked IR lowering |
| `compiler/src/lower/` | IR lowering implementation |
| `datafun/src/pipeline.rs` | `ModuleCompilationPipeline` |
| `datafun/src/memo_analysis.rs` | Memoization testing infrastructure |
| `datafun/src/incremental.rs` | `IncrementalModuleWorld` (higher-level) |

## Design Patterns

### Stable Identity
`Module` objects created once and reused. Updates use `set_source()` not recreation.

### Content Hash
`ParsedModuleGraph.module_content_hashes()` computes transitive hashes for dependency tracking.

### Parallel Warming
Rayon warms salsa cache, then tracked function hits cache.

### Deferred Errors
Errors collected as `PendingDiagnostic`, emitted with source location via `SpanLookup`.
