# Plan: Remove Package Concepts from datafun-compiler

## Goal

Complete the removal of package concepts (`PackageWorld`, `PackageModule`, `PackageWorldModuleGraph`, etc.) from `datalove-datafun-compiler`, leaving only the package-agnostic `ModuleGraph`/`ModuleId` abstractions.

## Progress

### Phase 3 (Partial) - COMPLETED

Updated datafun crate callers to use ModuleGraph path where possible:

1. **std_tests.rs** - Fully converted to ModuleGraph path:
   - Uses `to_module_graph()` to convert PackageWorld
   - Uses `typecheck_module_graph()` instead of `typecheck_package_world()`
   - Uses `InterpContext::new_with_module_graph()`
   - Calls `populate_script_imports()` for script-level imports
   - All 20 tests pass

2. **tycheck_world_tests.rs** - Fully converted to ModuleGraph path:
   - Uses `typecheck_module_graph()` instead of `typecheck_package_world()`
   - Iterates over `ModuleId` instead of `PackageModule`
   - All 3 tests pass

3. **worldfile_analysis.rs:analyze_script_section()** - Kept package path:
   - Scripts with function definitions require per-unit typechecking with module context
   - The ModuleGraph path's basic `type_check()` doesn't provide proper function analysis
   - Would need `type_check_with_module_graph()` to fully migrate
   - All 158 interp_tests pass

### New Methods Added

1. **InterpContext::populate_script_imports()** - Public method to populate script imports using ModuleGraph

2. **ModuleFunctionTableGraph::populate_script_imports_for_graph()** - Parses require/import statements and resolves against ModuleGraph

3. **build_module_alias_map_for_graph()** - Helper to map module aliases to ModuleId

4. **type_check_with_module_graph()** - Typechecks scripts with module graph support for import resolution (added in Step 1)

### Step 1 COMPLETED: type_check_with_module_graph

Added `type_check_with_module_graph()` to `tycheck.rs`. This function:
- Takes `(db, source, script, graph, graph_typecheck)` parameters
- Builds path-to-id map from the module graph
- Uses `build_module_alias_map_for_graph()` to resolve require statements
- Resolves imports against `graph_typecheck.module_exports()`
- Does standard two-pass typechecking (signatures then statements)

### Step 2 COMPLETED: execute_fun_statement updated

Updated `execute_fun_statement()` to use `type_check_with_module_graph()` when in ModuleGraph mode:
- Checks `ctx.module_graph_typecheck` first
- If present, uses `type_check_with_module_graph()` with the graph from typecheck result
- Falls back to basic `type_check()` for standalone mode

### Step 3 TODO: Add execute_script_with_module_graph

**File:** `crates/datalove-datafun-compiler/src/interp/mod.rs`

Add new function that mirrors `execute_script()` but uses ModuleGraph types:
- Takes `(db, script, ModuleGraphTypecheckResult)` - no PackageWorld
- Uses `InterpContext::new_with_module_graph()`
- Uses `ctx.populate_script_imports(script, graph)`
- Uses `type_check_with_module_graph()` for script unit typechecking
- Has access to `ctx.script_function_analyses` since it's in the compiler crate

This solves the problem of not being able to access private fields from the datafun crate.

### Step 4 TODO: Update analyze_script_section

**File:** `crates/datalove-datafun/src/worldfile_analysis.rs`

Change to use the new `execute_script_with_module_graph()` function instead of
`execute_script()` with PackageWorld.

### Remaining Work After Steps 3-4

- Remove `execute_script()` once no longer needed
- Remove package-world code from compiler (Phase 4 of original plan)

---

## Next Steps: Implementation Details

### Step 1: Add type_check_with_module_graph

**File:** `crates/datalove-datafun-compiler/src/tycheck.rs`

Add `type_check_with_module_graph()` function (~20 lines):
- Reuse existing `build_module_alias_map_for_graph()` (line 2452)
- Copy import resolution pattern from `typecheck_module_graph()` lines 727-763
- Look up exports from `ModuleGraphTypecheckResult.module_exports()`

```rust
pub fn type_check_with_module_graph<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    script: Script<'db>,
    graph: crate::module_graph::ModuleGraph,
    graph_typecheck: crate::module_graph::ModuleGraphTypecheckResult<'db>,
) -> TypecheckResult<'db>
```

### Step 2: Update execute_fun_statement

**File:** `crates/datalove-datafun-compiler/src/interp/mod.rs`

Update `execute_fun_statement()` (lines 476-489) to use the new function:

```rust
// Current code (line 476-489):
let unit_typecheck = if let Some(typecheck_result) = ctx.typecheck_result {
    // Package-world mode.
    crate::tycheck::type_check_with_package_world(...)
} else {
    // ModuleGraph or standalone mode - use basic typecheck.
    crate::tycheck::type_check(ctx.db, unit_source, parsed_unit)
};

// New code:
let unit_typecheck = if let Some(typecheck_result) = ctx.typecheck_result {
    // Package-world mode (legacy).
    crate::tycheck::type_check_with_package_world(...)
} else if let Some(graph_typecheck) = ctx.module_graph_typecheck {
    // ModuleGraph mode - use new function with imports.
    let graph = graph_typecheck.graph(ctx.db);
    crate::tycheck::type_check_with_module_graph(
        ctx.db,
        unit_source,
        parsed_unit,
        graph,
        graph_typecheck,
    )
} else {
    // Standalone mode - basic typecheck.
    crate::tycheck::type_check(ctx.db, unit_source, parsed_unit)
};
```

### Step 3: Update analyze_script_section

**File:** `crates/datalove-datafun/src/worldfile_analysis.rs`

Change `analyze_script_section()` (lines 315-386) to mirror `analyze_scriptunit_section()`:

Current signature:
```rust
fn analyze_script_section(
    db: &dyn salsa::Database,
    package_world: datalove_datafun_pkg::PackageWorld,
    source: &str,
) -> AnyResult<SectionAnalysis>
```

New signature:
```rust
fn analyze_script_section<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    source: &str,
) -> AnyResult<SectionAnalysis>
```

Implementation:
1. Create fresh InterpContext with `new_with_module_graph(db, typecheck_result)`
2. Populate script imports via `ctx.module_functions_graph.populate_script_imports_for_graph()`
3. Execute script units via `execute_script_unit()`
4. Extract output variable, pretty-print, clean up

Update call site in `analyze_worldfile()` line 163.

### Step 4: Remove Package-World Code

After Step 3 works and tests pass, remove the now-unused package-world code:

**tycheck.rs:**
- Remove `type_check_with_package_world()` (lines ~460-529)
- Remove `type_check_with_package_world_for_diagnostics()` (lines ~439-453)
- Remove `typecheck_package_world()` (lines ~565-685)
- Remove `build_script_module_alias_map()` (lines ~2486-2534)
- Remove `PackageWorldTypecheckResult` struct (lines ~131-152)
- Remove `ModuleExports` / `ModuleImports` that use PackageModule (lines ~106-129)

**interp/mod.rs:**
- Remove `execute_script()` (lines ~108-193)
- Update `execute_fun_statement()` to remove package-world branch

**interp/context.rs:**
- Remove `InterpContext::new_with_typecheck()` (lines ~126-161)
- Remove `InterpContext::new_unchecked()` (lines ~167-193)
- Remove `package_world` field
- Remove `typecheck_result` field (PackageWorld version)
- Remove `current_module` field (PackageModule version)
- Remove `module_functions: ModuleFunctionTable` field and struct

### Step 5: Clean Up Imports

- Remove re-exports of `PackageWorldTypecheckResult` from lib.rs
- Remove `bct::package2` imports from compiler files

### Execution Order

1. Step 1 - Add `type_check_with_module_graph` (additive, safe)
2. Step 2 - Update `execute_fun_statement` to use it
3. Run tests - verify everything still works
4. Step 3 - Update `analyze_script_section` (key migration)
5. Run tests - critical checkpoint
6. Step 4 - Remove package-world code (cleanup)
7. Step 5 - Clean up imports
8. Final test run

---

## Current State

The compiler has **two parallel paths**:
1. **Package path** (legacy): Uses `bct::package2::PackageWorld`, `PackageModule`, `PackageWorldModuleGraph`
2. **ModuleGraph path** (new): Uses `bct::module_graph::ModuleGraph`, `ModuleId`

Both paths are functional. The ModuleGraph path is already used by `worldfile_analysis.rs` in the datafun crate. The package path is still used by `execute_script()` and some callers.

## Architecture After Refactor

```
        bct (ModuleGraph, ModuleId, PackageWorld, etc.)
       /    \
      v      v
datafun-compiler    datafun-pkg
(ModuleGraph only)  (loads packages, resolves to ModuleGraph)
      \            /
       v          v
      datafun (glue: uses pkg to get ModuleGraph, runs compiler)
```

The compiler will only know about `ModuleGraph`/`ModuleId`. Package concepts are exclusively handled by `datafun-pkg` and `datafun`.

---

## Phase 1: Remove Package Types from tycheck.rs

### 1.1 Remove package-aware typecheck result types

**Files:** `tycheck.rs`

Remove these types that use `bct::package2::PackageModule`:
- `ModuleExports` (lines 106-115) - uses `package_module: bct::package2::PackageModule`
- `ModuleImports` (lines 118-129) - uses `package_module: bct::package2::PackageModule`
- `PackageWorldTypecheckResult` (lines 131-152) - uses `graph: PackageWorldModuleGraph`, `BTreeMap<PackageModule, ...>`

The equivalents in `module_graph.rs` already exist:
- `module_graph::ModuleExports` with `module_id: ModuleId`
- `module_graph::ModuleImports` with `module_id: ModuleId`
- `module_graph::ModuleGraphTypecheckResult`

### 1.2 Remove package-aware typecheck functions

**Files:** `tycheck.rs`

Remove these functions:
- `typecheck_package_world()` (lines 564-685)
- `type_check_with_package_world()` (lines 460-529)
- `type_check_with_package_world_for_diagnostics()` (lines 439-453)
- `topological_sort_modules()` (lines 2337-2402) - uses PackageWorldModuleGraph
- `build_module_alias_map()` (lines 2406-2446) - takes PackageModule
- `build_script_module_alias_map()` (lines 2486-2534) - uses PackageWorld

Keep:
- `typecheck_module_graph()` (lines 693-808) - already package-agnostic
- `build_module_alias_map_for_graph()` (lines 2452-2481) - already package-agnostic

**Callers to update (in datafun crate):**
- `worldfile_analysis.rs:analyze_script_section()` - currently calls `typecheck_package_world()`
- Move package-aware script typechecking to datafun crate if needed

---

## Phase 2: Remove Package Types from Interpreter

### 2.1 Remove package fields from InterpContext

**Files:** `interp/context.rs`

Remove from `InterpContext`:
- `package_world: PackageWorld` (line 27)
- `current_module: Option<bct::package2::PackageModule>` (line 32)
- `typecheck_result: Option<PackageWorldTypecheckResult<'db>>` (line 34)
- `module_functions: ModuleFunctionTable<'db>` (line 30)

Keep:
- `module_functions_graph: ModuleFunctionTableGraph<'db>` (line 44)
- `current_module_id: Option<ModuleId>` (line 46)
- `module_graph_typecheck: Option<ModuleGraphTypecheckResult<'db>>` (line 48)

### 2.2 Remove package-aware ModuleFunctionTable

**Files:** `interp/context.rs`

Remove:
- `ModuleFunctionTable` struct (lines 62-67) - uses `PackageModule`
- `ModuleFunctionTable::new()`, `build_from_script()`, `build_from_graph()`, `populate_script_imports()`, `get()`, `get_module_functions()`
- `parse_module_functions()` (lines 506-523) - takes `PackageModule`
- `build_module_alias_map()` (lines 528-579) - uses `PackageWorld`, `PackageModule`

Keep:
- `ModuleFunctionTableGraph` - already package-agnostic

### 2.3 Remove package-aware constructors

**Files:** `interp/context.rs`

Remove:
- `InterpContext::new_with_typecheck()` (lines 126-161) - takes `PackageWorld`, `PackageWorldTypecheckResult`
- `InterpContext::new_unchecked()` (lines 167-193) - takes `PackageWorld`

Keep:
- `InterpContext::new_with_module_graph()` (lines 199-240) - already package-agnostic

**Note:** The `new_with_module_graph()` constructor currently creates a dummy `PackageWorld`. Once we remove package fields, it won't need to.

### 2.4 Remove ModuleRef::Package variant

**Files:** `interp/mod.rs`

Remove:
- `ModuleRef::Package(bct::package2::PackageModule)` enum variant (line 94)

Change `ModuleRef` to just use `ModuleId`:
```rust
// Before:
enum ModuleRef {
    Package(bct::package2::PackageModule),
    Graph(ModuleId),
}

// After: Remove the enum entirely, just use ModuleId directly
```

### 2.5 Remove execute_script() function

**Files:** `interp/mod.rs`

Remove:
- `execute_script()` (lines 108-193) - takes `PackageWorld`, `PackageWorldTypecheckResult`

**Callers to update (in datafun crate):**
- `worldfile_analysis.rs:analyze_script_section()` - needs alternative

**Option A:** Move `execute_script()` to datafun crate with same signature
**Option B:** Rewrite callers to use ModuleGraph path

Recommend Option B: Update `analyze_script_section()` to use the ModuleGraph path like `analyze_worldfile()` already does.

---

## Phase 3: Update datafun Crate Callers

### 3.1 Update worldfile_analysis.rs

**Files:** `datalove-datafun/src/worldfile_analysis.rs`

`analyze_script_section()` (lines 308-379) currently:
1. Calls `typecheck_package_world()` (line 337)
2. Calls `execute_script()` (line 340)

Update to:
1. Convert PackageWorld to ModuleGraph (using `to_module_graph()`)
2. Call `typecheck_module_graph()` instead
3. Use `InterpContext::new_with_module_graph()` and execute manually

This matches what `analyze_worldfile()` already does.

---

## Phase 4: Clean Up lib.rs Exports

### 4.1 Update compiler lib.rs

**Files:** `datalove-datafun-compiler/src/lib.rs`

Remove any re-exports of package-related types from the compiler's public API.

### 4.2 Update datafun lib.rs

**Files:** `datalove-datafun/src/lib.rs`

The datafun crate can still re-export package types from `datafun-pkg` for higher-level callers who need them.

---

## Phase 5: Remove bct Imports

### 5.1 Clean up imports

**Files:** All compiler files

Remove imports of:
- `bct::package2::*`
- `bct::package_resolve2::*`

Keep imports of:
- `bct::module_graph::*`
- `bct::input::Source`
- `bct::text::InternedText`

---

## Order of Operations

1. **Phase 3 first:** Update datafun callers to use ModuleGraph path (makes later removals safe)
2. **Phase 2.5:** Remove `execute_script()` from compiler
3. **Phase 2.1-2.4:** Remove package types from interpreter
4. **Phase 1:** Remove package types from tycheck
5. **Phase 4-5:** Clean up exports and imports

---

## AST Consideration

The AST types (`StmtRequireModule` with `import_space`, `package_alias`, `module_alias`) can remain unchanged. These are just parsed text representing what the user wrote; they don't reference Salsa package types. The interpretation of these parsed fields happens in the alias-building functions, which will now build `ModuleId` lookups instead of `PackageModule` lookups.

---

## Test Strategy

1. Run the existing test suite after each phase
2. Key tests: `interp_tests` (worldfile-based tests)
3. The tests already exercise the ModuleGraph path via `worldfile_analysis.rs`

---

## Risk Assessment

**Low risk:** The ModuleGraph path is already fully functional and tested.

**Main risk:** Callers outside the workspace that use `execute_script()` or `typecheck_package_world()` directly. These would need to be updated.

---

## Summary

| Component | Package Types to Remove | Package-Agnostic Replacement |
|-----------|------------------------|------------------------------|
| tycheck.rs | `ModuleExports`, `ModuleImports`, `PackageWorldTypecheckResult`, `typecheck_package_world`, etc. | `module_graph::ModuleExports`, `module_graph::ModuleImports`, `module_graph::ModuleGraphTypecheckResult`, `typecheck_module_graph` |
| interp/context.rs | `InterpContext.package_world`, `.current_module`, `.typecheck_result`, `ModuleFunctionTable` | `InterpContext.current_module_id`, `.module_graph_typecheck`, `ModuleFunctionTableGraph` |
| interp/mod.rs | `ModuleRef::Package`, `execute_script()` | Use `ModuleId` directly, execute via `InterpContext::new_with_module_graph()` |
