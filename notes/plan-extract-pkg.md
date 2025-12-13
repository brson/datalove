# Plan: Extract Package Code from datafun Core

Goal: Core compiler knows about modules, not packages/libraries.
Package loading and resolution moves to a separate crate.

## Progress Summary

- [x] Phase 1: Define ModuleGraph abstraction in core
- [x] Create typecheck_module_graph function
- [x] Add PackageWorldModuleGraph -> ModuleGraph conversion
- [x] Create interp version working on ModuleGraph
- [ ] Phase 2: Create datalove-datafun-pkg crate
- [ ] Phase 3: Create datalove-datafun-test crate

## Completed Work

### New Files Created

**`module_graph.rs`** (~260 lines) - Package-agnostic module abstraction:

```rust
// Core types
#[salsa::input]
pub struct ModuleId { path: String }  // e.g., "sys/std/u32"

#[salsa::input]
pub struct Module { id: ModuleId, source: Source }

pub struct ResolvedImport {
    local_name: String,
    source_module: ModuleId,
    export_name: String,
}

#[salsa::input]
pub struct ModuleGraph {
    modules: Vec<Module>,              // Dependency order
    module_by_id: BTreeMap<ModuleId, Module>,
    imports: BTreeMap<ModuleId, Vec<ResolvedImport>>,
    dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>>,
}

// Typecheck result types (keyed by ModuleId, not PackageModule)
#[salsa::tracked]
pub struct ModuleExports<'db> { ... }

#[salsa::tracked]
pub struct ModuleImports<'db> { ... }

#[salsa::tracked]
pub struct ModuleGraphTypecheckResult<'db> { ... }

// Builder for constructing ModuleGraph
pub struct ModuleGraphBuilder<'db> { ... }
```

### New Functions

**`tycheck.rs`**:
- `typecheck_module_graph(db, ModuleGraph) -> ModuleGraphTypecheckResult`
  - Package-agnostic typecheck using pre-resolved imports
  - Processes modules in dependency order
  - Builds exports/imports maps keyed by ModuleId

**`package_resolve.rs`**:
- `to_module_graph_with_imports(db, PackageWorld, PackageWorldModuleGraph) -> ModuleGraph`
  - Converts package-based graph to ModuleGraph
  - Extracts function-level imports from AST
  - Resolves module aliases from require statements

### Tests Added

- `module_graph::tests::test_module_graph_builder` - Builder API test
- `package_resolve::tests::test_to_module_graph_with_imports` - Conversion test
- `package_resolve::tests::test_typecheck_module_graph` - End-to-end typecheck test

All 119+ existing tests continue to pass.

## Current Architecture

### Package-related files in datafun (~1270 lines)

| File | Lines | Purpose |
|------|-------|---------|
| `package.rs` | 84 | Salsa wrapper for bct types |
| `package_load.rs` | ~166 | Filesystem loading |
| `package_load_worldfile.rs` | ~519 | Worldfile parsing (tests) |
| `package_resolve.rs` | ~443 | Resolution orchestration + ModuleGraph conversion |
| `import_demands.rs` | ~56 | Import demand extraction |
| `module_graph.rs` | ~260 | **NEW** - Core module abstraction |

### Core type dependencies

- Base types (`Package`, `PackageModule`, `PackageWorldMap`, etc.) from `bct::package2`
- datafun's `PackageWorld` is a Salsa wrapper around two `BTreeMap<PackageName, Package>`
- **NEW**: `ModuleGraph` provides package-agnostic alternative

### Critical Coupling Points

1. `tycheck::typecheck_package_world(PackageWorldModuleGraph) -> PackageWorldTypecheckResult`
   - **DONE**: `typecheck_module_graph(ModuleGraph) -> ModuleGraphTypecheckResult`
2. `interp::execute_script(script, PackageWorld, typecheck_result) -> ScriptResult`
   - **DONE**: Interpreter now supports both PackageModule and ModuleId via `ModuleRef` enum
3. `InterpContext::new_with_typecheck()` stores PackageWorld for module alias resolution
   - **DONE**: Added `InterpContext::new_with_module_graph()` constructor
4. `worldfile_analysis::analyze_worldfile()` builds full PackageWorld for tests
   - Will stay in -test crate

## Remaining Work

### Phase 1 Complete

Interpreter changes added:
- `ModuleRef` enum to represent either `PackageModule` or `ModuleId`
- `ModuleFunctionTableGraph` - parallel function table using `ModuleId`
- `InterpContext::new_with_module_graph()` constructor
- `lookup_function` now checks both Package and ModuleGraph variants
- `execute_function_body` handles both module context types

Tests:
- `package_resolve::tests::test_interp_with_module_graph` - verifies ModuleGraph-based context creation

### Phase 2: Create `datalove-datafun-pkg` Crate

Move to new crate:
- `package_load.rs`
- `package_load_worldfile.rs`
- `package_resolve.rs` (keep conversion functions)
- `import_demands.rs`
- `package.rs`

Dependencies:
- `datalove-datafun` (for ModuleGraph, core types)
- `bct` (for package2 types)

### Phase 3: Create `datalove-datafun-test` Crate

Move test infrastructure:
- Integration tests using packages
- `worldfile_analysis.rs` functionality

Dependencies:
- `datalove-datafun`
- `datalove-datafun-pkg`
- `datalove-exampletest`

## Key Challenges

1. **bct dependency**: Package types come from bct. ModuleGraph abstraction isolates core from this.

2. **Salsa integration**: Both PackageWorld and ModuleGraph are Salsa inputs. Conversion works but adds some overhead.

3. **Test migration**: Most tests use worldfile format assuming packages.
   - Keep package-based testing in -test crate
   - Core can be tested with ModuleGraph directly

## Design Decisions Made

1. **ModuleId uses path strings** (e.g., "sys/std/u32") rather than interned text - simpler, self-describing

2. **Pre-resolved imports** in ModuleGraph - typecheck doesn't need to resolve aliases

3. **Separate typecheck result types** - ModuleGraphTypecheckResult uses ModuleId keys, PackageWorldTypecheckResult uses PackageModule keys

4. **Conversion at boundary** - Package layer converts to ModuleGraph before calling core typecheck
