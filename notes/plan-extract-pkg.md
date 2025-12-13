# Plan: Extract Package Code from datafun-compiler

Goal: Diamond dependency where datafun-compiler and datafun-pkg don't know about each other.
The glue crate datafun bridges both.

```
              bct (shared types)
             /    \
            v      v
  datafun-compiler  datafun-pkg
            \      /
             v    v
            datafun (glue)
               |
               v
            caller
```

## Progress Summary

- [x] Phase 1: Define ModuleGraph abstraction in core
- [x] Phase 2: Create datalove-datafun-pkg crate (initial version)
- [x] Phase 3: Move ModuleGraph to bct
- [x] Phase 4: Make pkg independent of datafun
- [x] Phase 5a: Split datafun into datafun + datafun-compiler
- [x] Phase 5b: Move package_world tests to datafun
- [x] Phase 6: Remove package modules from datafun-compiler
- [x] Phase 7: Wire up datafun to use datafun-pkg

## Current State (COMPLETE)

```
bct/src/
  - package2.rs: Package, PackageModule, PackageWorld, package_world_map
  - package_resolve2.rs: ImportDemandMap, resolve_package_world
  - module_graph.rs: ModuleGraph, ModuleGraphBuilder

datalove-datafun-compiler/src/
  - Core: ast, parser, tycheck, interp, module_graph, resolution
  - NO package concepts (uses bct::package2 for types)

datalove-datafun-pkg/src/
  - package.rs: re-exports bct types + import_from_loader
  - package_load.rs: PackageWorldConfig, load_world
  - package_load_worldfile.rs: worldfile parsing
  - package_resolve.rs: resolve_package_world_with_imports, to_module_graph

datalove-datafun/src/
  - lib.rs: re-exports compiler + pkg
  - import_demands.rs: extracts imports using compiler's parser
  - package_resolve.rs: high-level API combining import_demands + pkg resolve
  - worldfile_analysis.rs: testing infrastructure
  - tests/: std_tests, tycheck_world_tests, interp_tests
```

## Target Architecture (ACHIEVED)

```
datalove-datafun-compiler/src/
  - Core only: ast, parser, tycheck, interp, module_graph, resolution
  - NO package concepts (uses bct::package2 for PackageWorld type)

datalove-datafun-pkg/src/
  - package.rs: re-exports from bct, import_from_loader
  - package_load.rs, package_load_worldfile.rs
  - package_resolve.rs (generic, takes ImportDemandMap)
  - to_module_graph() (converts PackageWorldModuleGraph -> bct::ModuleGraph)

datalove-datafun/src/
  - lib.rs: re-exports compiler + pkg
  - import_demands.rs: uses compiler's parser -> ImportDemandMap
  - package_resolve.rs: high-level API that calls import_demands
  - worldfile_analysis.rs: testing infrastructure
```

## Implementation: Phase 6

Remove package modules from datafun-compiler.

### Step 1: Move import_demands.rs to datafun

Create `datalove-datafun/src/import_demands.rs`:
- Copy from compiler
- Change `use crate::` to `use datalove_datafun_compiler::`

### Step 2: Create package_resolve wrapper in datafun

Create `datalove-datafun/src/package_resolve.rs`:
```rust
// High-level resolve that handles import_demands internally
pub fn resolve_package_world_with_imports(db, package_world) {
    let map = datalove_datafun_pkg::package_world_map(db, package_world);
    let demands = crate::import_demands::import_demands(db, map);
    datalove_datafun_pkg::resolve_package_world(db, package_world, demands)
}
```

### Step 3: Move worldfile_analysis.rs to datafun

- Uses package + compiler, belongs in glue crate

### Step 4: Move interp_tests to datafun

- Uses worldfile_analysis, must move with it

### Step 5: Delete from datafun-compiler

Delete these files:
- `src/package.rs`
- `src/package_load.rs`
- `src/package_load_worldfile.rs`
- `src/package_resolve.rs`
- `src/import_demands.rs`
- `src/worldfile_analysis.rs`

Update `src/lib.rs` to remove module declarations.

### Step 6: Update datafun dependencies

`datalove-datafun/Cargo.toml`:
```toml
[dependencies]
datalove-datafun-compiler.path = "../datalove-datafun-compiler"
datalove-datafun-pkg.path = "../datalove-datafun-pkg"
```

### Step 7: Update datafun lib.rs

```rust
// Re-export compiler
pub use datalove_datafun_compiler::*;

// Re-export pkg
pub use datalove_datafun_pkg::{
    PackageWorld, Package, PackageModule,
    PackageWorldConfig, load_world,
    package_world_map,
};

// Bridge modules
pub mod import_demands;
pub mod package_resolve;
pub mod worldfile_analysis;
```

## Verification

- `just test` passes
- `datalove-datafun-compiler` has no `package` in module list
- `datalove-datafun-pkg` has no dep on `datalove-datafun-compiler`
- Grep for `use crate::package` in compiler returns nothing

## Caller Usage After Refactor

```rust
use datalove_datafun as datafun;

// Load packages
let package_world = datafun::load_world(&config).await?;

// Resolve (handles import_demands internally)
let resolution = datafun::package_resolve::resolve_package_world_with_imports(db, package_world);
let graph = resolution.result(db)?;

// Typecheck
let result = datafun::tycheck::typecheck_package_world(db, graph);

// Execute
datafun::interp::execute_script(db, script, package_world, result)?;
```

## Key Considerations

1. **import_demands**: Stays in datafun (uses parser).
   Takes PackageWorldMap, returns ImportDemandMap.

2. **worldfile_analysis**: Testing infra, uses both pkg and compiler.

3. **interp_tests**: Uses worldfile_analysis, must be in datafun or separate test crate.

4. **Re-exports**: datafun re-exports both compiler and pkg for convenience.
