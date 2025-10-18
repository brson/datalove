Based on the BCT module and module_resolve,
and package2 and package_resolve2.

See for example how la2 (../la2) uses the bct module system.

## Requirements

Libraries contain packages and packages contain modules.

The "sys" library comes with the compiler,
the "local" library is for the user's workspace.
In the future there will be a library that represents the package ecosystem.

In this repo sys lives at `sys`
and the standard library at `sys/std`.
`std` contains `.dfm` datafun module files.

The compiler scans the sys directory for packages
and modules and loads them as bct needs.

Require syntax for modules looks like

```
require module sys/std/bool
require module sys/std/int
```

Always three parts - lib - pkg - module.

## Implementation Status

### Completed

**Core Infrastructure** (crates/datalove-datafun/src/):
- `package.rs` - Salsa types for module system
  - `PackageWorld` input with main, sys, and local libraries
  - `package_world_map()` creates bct `PackageWorldMap`
  - `import_from_loader()` converts filesystem data to salsa types
- `package_load.rs` - Filesystem scanning for packages/modules
  - Scans directories for `.dfm` files
  - Each package directory must contain main module (e.g., `std/std.dfm`)
  - Async loading using futures/channels
- `import_demands.rs` - Extracts `require module` from AST
  - Parses 3-part paths (lib/pkg/module)
  - Builds bct `ImportDemandMap`
- `package_resolve.rs` - Module resolution and cycle detection
  - `resolve_package_world_with_imports()` wrapper
  - Test verifies sys/std modules load correctly

**AST & Parser**:
- `ast.rs` - Added `import_space`, `package_alias`, `module_alias` to `StmtRequire`
- `parser.rs` - Parses `require module lib/pkg/module` syntax
  - Validates 3-part structure with `/` separators
  - Updated tests to use new syntax

**Directory Structure**:
```
sys/
  std/
    std.dfm      # Main module (required for package)
    bool.dfm
    int.dfm
    list.dfm
```

### Not Yet Implemented

**Module Semantics**:
- Modules don't actually *do* anything yet
- No module namespace or scope isolation
- `require module` statements are parsed but not used during execution
- No way to reference items from imported modules

**Standard Library Content**:
- All `.dfm` files in sys/std are currently empty placeholders
- Need to define actual types, functions, and values

**Integration**:
- REPL/Script mode doesn't integrate with PackageWorld yet
- Only batch/file mode will support modules (future work)
- No CLI support for loading packages

### Next Steps

1. Define module namespace semantics (how to reference imported items)
2. Implement module-aware name resolution in `resolution.rs`
3. Populate sys/std modules with actual definitions
4. Add module support to interpreter
5. Create CLI entry point that uses `package_load::load_world()`
6. Add module support to REPL (lower priority)
