# Plan: Extract Package Code from datafun Core

Goal: Diamond dependency where datafun and datafun-pkg don't know about each other.

```
              bct (shared types)
             /    \
            v      v
       datafun    datafun-pkg
            \      /
             v    v
           caller
```

## Progress Summary

- [x] Phase 1: Define ModuleGraph abstraction in core
- [x] Phase 2: Create datalove-datafun-pkg crate (initial version)
- [x] Phase 3: Move ModuleGraph to bct
- [x] Phase 4: Make pkg independent of datafun
- [ ] Phase 5: Remove package types from datafun core (optional cleanup)

## Completed Work

### ModuleGraph Abstraction (in datafun, will move to bct)

```rust
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
    modules: Vec<Module>,
    module_by_id: BTreeMap<ModuleId, Module>,
    imports: BTreeMap<ModuleId, Vec<ResolvedImport>>,
    dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>>,
}

pub struct ModuleGraphBuilder<'db> { ... }
```

### Current pkg Crate

Files in pkg (currently depends on datafun - will be changed):
- `package_load_worldfile.rs` - Worldfile parsing
- `package_resolve.rs` - Resolution + ModuleGraph conversion
- `import_demands.rs` - Import demand extraction

## Target Architecture

### What Moves to bct

From `datalove-datafun/src/module_graph.rs`:
- `ModuleId` - opaque module identifier (path string)
- `Module` - module with source text
- `ResolvedImport` - local_name → source_module + export_name
- `ModuleGraph` - dependency-ordered collection with imports
- `ModuleGraphBuilder` - builder API

These use only bct primitives (`Source`, `InternedText`).

### What Stays in datafun

- `ModuleExports<'db>` - contains `TypeFunction<'db>` (datafun-specific)
- `ModuleImports<'db>` - datafun-specific
- `ModuleGraphTypecheckResult<'db>` - datafun-specific
- `typecheck_module_graph()` - uses datafun's parser/typer
- `import_demands()` - uses datafun's parser to extract require statements
- Interpreter - consumes ModuleGraph via typecheck result

### What pkg Does

- Load packages from filesystem (`package_load.rs`)
- Parse worldfiles (`package_load_worldfile.rs`)
- Define PackageWorld, package_world_map()
- Resolve dependencies (via bct::package_resolve2) - caller provides ImportDemandMap
- Convert PackageWorldModuleGraph → bct::ModuleGraph

Note: pkg does NOT parse source code. Caller extracts import demands using datafun.

## Implementation Phases

### Phase 3: Move ModuleGraph types to bct

Create `bct/crates/bct/src/module_graph.rs` with core types.

Files:
- Create `bct/crates/bct/src/module_graph.rs`
- Update `bct/crates/bct/src/lib.rs` to export it

### Phase 4: Update datafun-pkg to use bct directly

1. Remove dependency on datalove-datafun
2. Import ModuleGraph from bct
3. Move package_load.rs, package.rs from datafun to pkg
4. package_resolve.rs takes ImportDemandMap as parameter (caller provides it)
5. Delete import_demands.rs from pkg

Files:
- `crates/datalove-datafun-pkg/Cargo.toml` (remove datafun dep)
- `crates/datalove-datafun-pkg/src/package_resolve.rs`
- Delete `crates/datalove-datafun-pkg/src/import_demands.rs`

### Phase 5: Remove package types from datafun core

1. Delete package.rs, package_load.rs from datafun (moved to pkg)
2. Keep import_demands.rs in datafun (uses parser, takes bct::PackageWorldMap)
3. Delete package_resolve.rs from datafun
4. Remove PackageModule from interpreter (only ModuleGraph)
5. Remove PackageWorld typecheck path

Files:
- Delete `crates/datalove-datafun/src/package.rs`
- Delete `crates/datalove-datafun/src/package_load.rs`
- Delete `crates/datalove-datafun/src/package_load_worldfile.rs`
- Delete `crates/datalove-datafun/src/package_resolve.rs`
- Keep `crates/datalove-datafun/src/import_demands.rs`
- Update interpreter to use only ModuleGraph
- Update tycheck to remove PackageWorld path

### Phase 6: Example caller usage

```rust
// 1. Load packages using pkg
let package_world = datafun_pkg::load_world(&config);
let package_world_map = datafun_pkg::package_world_map(db, package_world);

// 2. Extract import demands using datafun's parser
let import_demand_map = datafun::import_demands(db, package_world_map);

// 3. Resolve and convert to ModuleGraph using pkg
let module_graph = datafun_pkg::resolve_to_module_graph(db, package_world_map, import_demand_map);

// 4. Compile using datafun
let typecheck_result = datafun::typecheck_module_graph(db, module_graph);

// 5. Execute using datafun
let result = datafun::execute_with_module_graph(db, script, module_graph, typecheck_result);
```

## Dependency After Refactor

```
bct (has ModuleGraph, package2, package_resolve2)
 ↑        ↑
 |        |
datafun  datafun-pkg (independent siblings)
 ↑        ↑
 └───┬────┘
     |
  caller
```

## Key Considerations

1. **Salsa jars**: ModuleGraph uses salsa. bct's Database needs to include these jars.

2. **Import demands**: `import_demands()` stays in datafun.
   - Takes PackageWorldMap (from bct) as input
   - Uses datafun's parser to extract `require module` statements
   - Caller calls `datafun::import_demands()`, passes result to `pkg::resolve()`

3. **WorldfileAnalysis**: Uses both pkg and datafun concepts.
   Belongs in a test crate or caller code.

4. **PackageWorldMap**: Already in bct::package_resolve2, so both can use it.
