# Plan: Extract Package Code from datafun Core

Goal: Core compiler knows about modules, not packages/libraries.
Package loading and resolution moves to a separate crate.

## Current Architecture

### Package-related files in datafun (~1270 lines)

| File | Lines | Purpose |
|------|-------|---------|
| `package.rs` | 84 | Salsa wrapper for bct types |
| `package_load.rs` | ~166 | Filesystem loading |
| `package_load_worldfile.rs` | ~519 | Worldfile parsing (tests) |
| `package_resolve.rs` | ~443 | Resolution orchestration |
| `import_demands.rs` | ~56 | Import demand extraction |

### Core type dependencies

- Base types (`Package`, `PackageModule`, `PackageWorldMap`, etc.) from `bct::package2`
- datafun's `PackageWorld` is a Salsa wrapper around two `BTreeMap<PackageName, Package>`

### Critical Coupling Points

1. `tycheck::typecheck_package_world(PackageWorldModuleGraph) -> PackageWorldTypecheckResult`
2. `interp::execute_script(script, PackageWorld, typecheck_result) -> ScriptResult`
3. `InterpContext::new_with_typecheck()` stores PackageWorld for module alias resolution
4. `worldfile_analysis::analyze_worldfile()` builds full PackageWorld for tests

### What the Core Actually Needs

The core doesn't need "packages" or "libraries" - it needs:
- A graph of modules with dependency order
- Module source text
- Module exports (function signatures)

Currently `PackageWorldModuleGraph` from bct provides this but carries package/library hierarchy.

## Extraction Plan

### Phase 1: Define Module-Level Abstraction in Core

Create simpler abstraction the core compiler uses:

```rust
// In datafun core
#[salsa::input]
pub struct ModuleGraph {
    modules: BTreeMap<ModuleId, ModuleSource>,
    dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>>,
}

#[salsa::input]
pub struct ModuleId {
    name: InternedText,
}
```

Adapt `typecheck_package_world()` -> `typecheck_module_graph()`.

### Phase 2: Create `datalove-datafun-pkg` Crate

Move to new crate:
- `package_load.rs`
- `package_load_worldfile.rs`
- `package_resolve.rs`
- `import_demands.rs`
- Adapter: `PackageWorld -> ModuleGraph`

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

1. **bct dependency**: Package types come from bct. May want to keep them there or create minimal module abstraction in datafun.

2. **Salsa integration**: Both PackageWorld and ModuleGraph need to be Salsa inputs. Conversion layer adds complexity.

3. **Test migration**: Most tests use worldfile format assuming packages. Options:
   - Keep package-based testing in -test crate
   - Create simpler module-only test fixtures

## Suggested Order

1. Add `ModuleGraph` abstraction to datafun core
2. Create internal typecheck/interp versions working on ModuleGraph
3. Have existing package code convert PackageWorld -> ModuleGraph, call new internals
4. Once working, extract package files to new crate
