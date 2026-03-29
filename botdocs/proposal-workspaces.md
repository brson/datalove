# Proposal: Workspace descriptor

2026/03/29


## Summary

The workspace is the complete declarative description of all inputs
to a compilation session. It is an immutable snapshot that can be
diffed against a previous snapshot to drive incremental recompilation.

The same descriptor serves AOT compilation, interactive script execution,
and LSP-style incremental recompilation. The execution mode is not part
of the descriptor -- it is a decision made by the driver that consumes it.


## Descriptor structure

```rust
/// Immutable snapshot of all compilation inputs.
///
/// Cheaply cloneable (inner data is Arc'd).
/// Two descriptors can be diffed to produce a WorkspaceDelta
/// that the pipeline applies incrementally.
struct WorkspaceDescriptor {
    /// System package library (e.g. sys/std, sys/collections).
    /// None when --no-sys.
    system_library: Option<PackageLibrary>,

    /// User package libraries (e.g. local/).
    /// Ordered; earlier libraries shadow later ones on name collision.
    user_libraries: Vec<PackageLibrary>,

    /// Compiler options that affect all compilation.
    options: CompilerOptions,
}

struct PackageLibrary {
    /// Library name (e.g. "sys", "local").
    name: String,

    /// Packages in this library, keyed by package name.
    packages: BTreeMap<String, PackageDescriptor>,
}

struct PackageDescriptor {
    /// Package name (e.g. "std", "myapp").
    name: String,

    /// Modules in this package, keyed by module name.
    modules: BTreeMap<String, ModuleDescriptor>,

    /// Native rider for this package, if any.
    rider: Option<RiderDescriptor>,
}

struct ModuleDescriptor {
    /// Module name (e.g. "list", "main").
    name: String,

    /// Source text of the module.
    source: Arc<str>,

    /// Filesystem path, if loaded from disk.
    /// Used for diagnostics and file-watching, not compilation.
    origin: Option<PathBuf>,
}

struct RiderDescriptor {
    /// Interface source text (.dli content).
    interface_source: Arc<str>,

    /// Path to the rider Cargo crate directory.
    /// None for synthetic or inline riders.
    crate_dir: Option<PathBuf>,
}

struct CompilerOptions {
    const_inlining: bool,
    skip_specialization: bool,
    // Future: feature flags, target config, etc.
}
```


## Scripts are not part of the descriptor

Scripts (`.dfs` files, REPL input, interactive sessions) are
compiled _against_ a workspace, not _within_ it. The workspace
describes the module world that scripts can `require` and `import` from.
This matches the current architecture where `ScriptCompiler`
is created from `CompiledModules` and maintains its own
incremental state (accumulated unit specs, bindings).

A driver (CLI, REPL, LSP) holds a workspace descriptor
and one or more script sessions independently.


## Diffing and incremental application

```rust
/// Changes between two workspace snapshots.
struct WorkspaceDelta {
    modules_added: Vec<(ModulePath, ModuleDescriptor)>,
    modules_removed: Vec<ModulePath>,
    modules_changed: Vec<(ModulePath, ModuleDescriptor)>,
    riders_added: Vec<(String, RiderDescriptor)>,
    riders_removed: Vec<String>,
    riders_changed: Vec<(String, RiderDescriptor)>,
    options_changed: Option<CompilerOptions>,
}

impl WorkspaceDescriptor {
    /// Diff two snapshots. The delta can be applied to a pipeline
    /// to incrementally recompile only what changed.
    fn diff(&self, newer: &WorkspaceDescriptor) -> WorkspaceDelta;
}
```

The delta maps directly to `IncrementalModuleWorld` operations:
- `modules_added` -> `add_module()`
- `modules_removed` -> `remove_module()`
- `modules_changed` -> `update_source()` (preserves salsa identity)
- `riders_changed` -> rebuild native component

The pipeline does not interpret the delta itself. A thin
`apply_delta(pipeline, delta)` function translates delta operations
into the pipeline's existing mutation API.


## Construction

Descriptors are built from various sources, all producing
the same type:

- **Filesystem scan**: `PackageWorld` discovery -> descriptor.
- **Worldfile**: parsed sections -> descriptor.
- **LSP**: file open/change/close events -> descriptor patches
  (construct delta directly without full rescan).
- **Programmatic**: tests build descriptors directly.

```rust
impl WorkspaceDescriptor {
    /// From a loaded PackageWorld (filesystem discovery).
    fn from_package_world(
        system: Option<&PackageWorld>,
        local: Option<&PackageWorld>,
        options: CompilerOptions,
    ) -> Self;

    /// From parsed worldfile sections.
    fn from_worldfile_sections(
        sections: &[WorldfileSection],
        options: CompilerOptions,
    ) -> Self;
}
```


## Native component relationship

The native component build (`build_native_component()`) is derived
from the descriptor's rider information but is not part of the
descriptor itself. Like execution mode, it is a downstream concern:

- The descriptor says "package std has a rider at this path."
- The driver decides whether to build it as cdylib (interpreter),
  staticlib (AOT), or both.
- The `WorkspaceDelta` tells the driver whether riders changed
  and a rebuild is needed.


## Relationship to current code

The descriptor replaces the ad-hoc accumulation currently spread across
`ModuleCompilationPipeline` fields. Today the pipeline is both
the input description and the compilation engine. The descriptor
separates these concerns:

| Current | Proposed |
|---------|----------|
| `pipeline.add_module()` | Build a `WorkspaceDescriptor` |
| `pipeline.rider_sources` | `PackageDescriptor::rider` |
| `pipeline.rider_crate_dirs` | `RiderDescriptor::crate_dir` |
| `pipeline.load_sys_library_default()` | `WorkspaceDescriptor::from_package_world()` |
| `pipeline.compile_fresh()` | `pipeline.compile(descriptor)` |
| (no equivalent) | `old.diff(new)` -> `apply_delta(pipeline, delta)` |

The `IncrementalModuleWorld` and salsa machinery remain as-is.
The descriptor is a pure-data layer above them that makes the
input declaration explicit and diffable.
