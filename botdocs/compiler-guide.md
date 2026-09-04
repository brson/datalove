# Datafun Compiler Guide

Reference for the datalove-datafun compiler architecture.

## Contents

- [Crate Organization](#user-content-crate-organization)
- [Module Compilation Pipeline](#user-content-module-compilation-pipeline)
  - [Phase 5: IR Lowering Detail](#user-content-phase-5-ir-lowering-detail)
  - [Const Parameter Specialization](#user-content-const-parameter-specialization)
  - [Const Evaluation](#user-content-const-evaluation)
- [Generics](#user-content-generics)
- [Native Riders](#user-content-native-riders)
- [The Shipped Binary](#user-content-the-shipped-binary)
- [Script Compilation Pipeline](#user-content-script-compilation-pipeline)
- [IR Types](#user-content-ir-types)
  - [IDs](#user-content-ids)
  - [Code Unit References](#user-content-code-unit-references)
  - [IrCodeUnit](#user-content-ircodeunit)
  - [Type Layout](#user-content-type-layout)
  - [Parameter Modes](#user-content-parameter-modes)
  - [Enum Instructions](#user-content-enum-instructions)
  - [Intrinsics](#user-content-intrinsics)
- [Salsa Patterns](#user-content-salsa-patterns)
  - [Database](#user-content-database)
  - [Tracked Functions](#user-content-tracked-functions)
  - [Incremental Compilation](#user-content-incremental-compilation)
  - [Parallel Execution](#user-content-parallel-execution)
- [Ownership Analysis](#user-content-ownership-analysis)
  - [Tracking Categories](#user-content-tracking-categories)
  - [Auto-adapt](#user-content-auto-adapt)
  - [Error Codes](#user-content-error-codes)
  - [Drop Schedule](#user-content-drop-schedule)
  - [AOT Tracking Bytes](#user-content-aot-tracking-bytes)
- [Execution Backends](#user-content-execution-backends)
- [Entry Points](#user-content-entry-points)
- [Test Patterns](#user-content-test-patterns)
- [Key Files](#user-content-key-files)

## Crate Organization

### Core Compiler Pipeline

| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-parser` | Lexer, parser, bracer |
| `datalove-datafun-ast` | AST types (`Statement`, `Expr`, etc.) |
| `datalove-datafun-common` | Types shared by resolve and tycheck: `Type`, `TypeFunction`, `ParsedModuleGraph`, `DbClone`, `generics` |
| `datalove-datafun-resolve` | Name resolution (type aliases, function signatures) |
| `datalove-datafun-tycheck` | Type checking, call resolution, type synthesis |
| `datalove-datafun-sema` | Types shared by ownership analysis and lowering: `ExprTypes`, `CallTargets`, `AnalysisError`, `DropSchedule` |
| `datalove-datafun-ownership` | Ownership analysis (move/borrow/drop tracking) |
| `datalove-datafun-lower` | AST to IR lowering |
| `datalove-datafun-const` | Const evaluation (CTFE), const inlining, dead code elimination |
| `datalove-datafun-ir` | IR types (`IrCodeUnit`, `IrType`, `ValueId`, layout, registries) |
| `datalove-datafun-intrinsics` | `IntrinsicId` and signatures for `icall` operations |
| `datalove-datafun-inline` | Function inlining pass over IR, including cross-module and dynamic inlining |
| `datalove-datafun-compiler` | Salsa-tracked pipeline, `Database`, `compile_modules()`, `lower_module_graph_with_evaluator()`, specialization |
| `datalove-datafun` | High-level facade, `ModuleCompilationPipeline`, `ScriptCompiler`, workspaces, rider build/load |

### Execution

| Crate | Responsibility |
|-------|----------------|
| `datalove-datafun-interp` | Interpreter, `CallDispatcher`, `NativeFunctionTable`, CTFE evaluator |
| `datalove-datafun-cranelift` | Shared Cranelift utilities |
| `datalove-datafun-cranelift-aot` | Cranelift AOT compilation |
| `datalove-datafun-cranelift-jit` | JIT compilation, tiering and dynamic inlining dispatcher |
| `datalove-datafun-c-aot` | AOT backend emitting C11 source linked against the runtime |
| `datalove-rt` | The runtime: C ABI (`c`), Rust wrappers (`rust`), implementation (`impls`) |
| `datalove-rtdt` | Runtime type descriptors (`TyDesc`), runtime-side layout, anypack |

### Supporting

| Crate | Responsibility |
|-------|----------------|
| `bcts` (imported as `bct`) | Base compiler toolkit: `ModuleGraph`, `Module`, `ModuleId`, source maps, text interning |
| `datalove-datafun-pkg` | Package loading, `PackageWorld`, worldfile parsing, module resolution |
| `datalove-ct` | Compile-time utilities, query logging |
| `datalove-diagnostic` | Diagnostic/error infrastructure |
| `datalove-datalit` | Data literal types and typechecking |
| `datalove-exampletest` | Snapshot test harness |
| `datalove-worldgen` | Generator for random worldfiles that typecheck |
| `datalove` | Thin facade crate holding a plain salsa `Database` |

### Drivers and tests

| Crate | Responsibility |
|-------|----------------|
| `datalove-cli` | Command line driver: build, run, AOT, native component linking |
| `datalove-repl` | REPL evaluation engine |
| `datalove-repl-rat` | Ratatui REPL application and terminal |
| `datalove-stdlib` | The system library as the binary carries it: embedded sources, linked riders, embedded native component |
| `datalove-native-component` | The runtime and riders as one `staticlib` for AOT-compiled programs to link |
| `datalove-rider-std` (`sys/std/rider`) | Native rider implementations for `sys/std` |
| `datalove-tests` | Workspace-wide test suites |
| `datalove-rt-tests` | Runtime tests, separated so the runtime need not depend on datalit |
| `datalove-bench` | Divan benchmarks |

The `sys/` tree at the repository root is the standard library: `sys/std/*.dfm`
modules, plus `sys/std/rider.dli` and the `sys/std/rider` Rust crate behind it.
`sys/std/rider` is a workspace member like anything under `crates/`.

`datalove-stdlib` and `datalove-native-component` sit above everything else:
nothing in the compiler depends on them, so editing a `.dfm` recompiles no
compiler crate. See [The Shipped Binary](#user-content-the-shipped-binary).

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
    |   Rider sources arrive here as raw strings
    v   Output: ParsedModuleGraph (statements + resolved_riders)
    |
[Phase 2: Name Resolution]
    |   resolve_all_names_with_mode
    |   Per-module: resolve_module_names [tracked]
    |   Also resolve_all_exports, build_all_function_ast_maps
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

**Integer synthesis**: bare integer and hex literals synthesize as `int` (bigint).
With an expected type from context (binding annotation, function parameter,
checked arithmetic operand), the literal checks against that type instead.
Both `datalove-datafun-tycheck` and `datalove-datalit` follow this rule.

Float literals take their precision from context the same way, `f64` being
the fallback when nothing supplies one. Both defaults are the widest of their
family, on the grounds that a type chosen without knowing what it is for
should lose the least. A negation does not stop the
expected type reaching the literal under it: the sign says nothing about the
width, so `-3.9` checks against `f64` exactly as `3.9` does, and `-5` is out
of range for a `u32` rather than merely the wrong type.

The two languages reach that by different routes - datalit carries a sign
inside the literal token, datafun parses a negation as an operator over an
unsigned literal - which is a place they can drift apart, and did.
`literal_type_equiv_tests` holds them to the same answers.

Neither takes an integer literal where a float is expected, or the reverse.
There is no implicit conversion between the two families; see
[Numeric Widening](botspec.md) in the spec, and `f64.from_int` and
`int.from_f64` for the named conversions.

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
let bindings. Used for testing CTFE accuracy. Callers of the high-level pipeline pass the
`ConstInlining` enum rather than a bare bool.

### Const Parameter Specialization

Functions with `const` parameters undergo specialization during phase 5c. The compiler
collects all call sites with const arguments during typechecking, then transforms the IR:

```
Phase 5c: Specialize comptime functions
    |   specialize_comptime_functions
    |   - Resolve const arg values from ResolvedConsts
    |   - Transform functions to union-branch form
    |   - Rewrite ComptimeCall instructions to Call
    v   Output: Specialized code units
```

**Union-branch strategy:** Instead of generating N separate functions (full
monomorphization), the compiler generates one function with N branches dispatching on a
tag.

This was adopted for code size and compile time and delivers neither.
`build_dispatch_blocks` clones every body block once per instantiation, so the output is
the same size as monomorphization and takes the same time to produce, plus a `Switch`.
That `Switch` is on a value every call site passes as a literal, since specialization
emits `Const(discriminant)` followed by `Call` and the const-binding-only restriction
guarantees the value is known. It also merges instantiations into one function, which
the tiering in `optimizing.rs` counts and inlines as a unit, so a hot instantiation
cannot tier separately from a cold one.

Monomorphization is smaller, faster to compile, and gives the tiering what it wants.
This is worth revisiting; see [Generics and Specialization](plan-generics.md).

**Call site handling:** The lowering phase emits `ComptimeCall` instructions for calls to
functions with const parameters. During specialization, these are transformed to emit the
enum discriminant as a `Const` instruction followed by a regular `Call` with modified arguments.

**Testing:** The `skip_specialization` flag (like `skip_const_inlining`) allows differential
testing - comparing specialized vs unspecialized output to verify correctness.

See `const-param-specialization.md` and `const-param-impl-plan.md` for detailed design.

### Const Evaluation

Const evaluation in `evaluate_single_const` has three fast paths:

1. **Simple literals** - booleans, integers, floats, strings, None extracted directly
2. **Const references** - look up already-evaluated const from `resolved_so_far` map
3. **Complex expressions** - lower to a minimal script code unit, execute via CTFE, extract result

Function calls in const expressions work because `lowered_functions` are passed to the CTFE evaluator.

## Generics

A generic function is compiled once, with `data` standing where a type parameter
was written. A parameter the callee owns is converted into that shape at the
call site and the value moved back out on the way back; a parameter it borrows
is not converted at all, and the descriptor saying what the value really is
comes from the call site, recorded in `FunctionContext::descriptor_params`.

How the descriptor is carried is up to the backend. The interpreter needs
nothing, since its values are already a pointer and a descriptor. The compiled
backends take one extra pointer parameter per entry in `descriptor_params`,
after the ordinary parameters and in that order.

The pieces: `Var` in `datalit::tycheck::Type`, built only by datafun-resolve
seeding the alias map; `IrType::from_datalit` mapping it to `data`, which is
the whole of erasure; `bind_type_params` and `substitute_type_params` in
`datafun-common::generics`; `Erase` and `Reify` at call boundaries.

A generic function calling another passes its own type parameter along, and the
value is already in the erased shape when it does. `Erase` and `Reify` are
skipped in that case -- the argument's `IrType` already equals the parameter's
erased shape, and the result type already equals the erased return shape.
Erasing an erased value boxes the box, and the callee finds a `data` where the
value should be.

What it does not do yet -- owned collections, indexing a collection of a type
parameter, bounds -- and why, is in
[Where this stands](plan-generics.md#user-content-where-this-stands). Read that
before assuming something is a bug.

## Native Riders

A rider is a set of native function signatures backed by a Rust crate. A module
writes `require rider <alias>` and then imports the names it wants:

```datalove
require rider std
import std.string_len
```

The signatures live in a `.dli` interface file (`sys/std/rider.dli`), or in a
`rider <name>` worldfile section for tests. Each is a `native fun` declaration,
and they may be generic:

```datalove
native fun string_len(ref self: string): index
native fun list_push<T>(mut self: [T], elem: T)
```

How it flows through the pipeline:

- Rider sources travel as `Vec<(String, String)>` (alias, source) into
  `compile_modules`, because building a `RiderInterface` creates a
  `TypeFunction`, which is `#[salsa::tracked]` and so can only happen inside a
  tracked function. `parse_module_graph` builds them and stores them on
  `ParsedModuleGraph::resolved_riders`.
- Each distinct alias gets a synthetic `ModuleId` with path `@rider/{alias}`,
  created once in `build_resolved_riders_from_sources` and shared by every
  module that requires it. `compute_func_id_map` assigns rider functions
  `IrModuleId`s after the regular modules.
- The typechecker resolves imports from riders in
  `resolve_module_imports_internal`.
- `add_native_rider_units` registers an `IrCodeUnit` per rider function whose
  context is `CodeUnitContext::Native`, carrying the linker symbol
  `dlr_{alias}__{func}` and no blocks.

Execution:

- **Interpreter**: `NativeFunctionTable` (`interp/src/native.rs`) maps linker
  symbol to a closure. The `Call` and `ComptimeCall` handlers check it before
  the regular dispatcher. Arguments follow the interpreter's own conventions -
  `in` params are moved values, `out` params are destinations, `ref`/`mut` are
  borrowed pointers.
- **Loading**: there are two ways a rider's functions reach the table, and
  which one applies depends on where the rider came from.
  - *Linked in.* `sys/std`'s rider is an ordinary dependency of the binary, so
    its functions are already in the process. `sys/std/rider/build.rs` reads
    `rider.dli` and generates a `symbols()` table pairing each declared name
    with `dlr_std__{name} as *const ()`, which is also what forces the linker
    to keep them. `rider_load::register_linked_natives` matches the symbols
    the compiled modules call against that table. No cargo, no dlopen. This
    is what the `datalove` binary does.
  - *Built from source.* A rider discovered on disk - a user package's, or
    `sys/std`'s when a test compiles the tree rather than the embedded copy -
    goes through `pipeline/rider_build.rs`, which synthesizes one
    `datalove-native-component` crate depending on every discovered rider
    crate so the runtime is bundled once rather than per rider. It builds a
    cdylib (dlopened by `rider_load::load_rider_library`) and a staticlib (for
    AOT linking). Builds are cached per work dir and rider set. The
    workspace's `work_dir` is where this happens; a workspace with riders and
    no work dir - a worldfile-derived one, for instance - cannot build them.

  Both end at `register_native`, so the interpreter sees no difference. Both
  also return the raw addresses, which the JIT needs: it calls natives through
  a trampoline built from the address rather than through the interpreter's
  table, so a driver must feed them to `JitEngine::register_native_symbol` as
  well. Missing that is not a fallback to the interpreter but a panic inside
  JIT compilation, which aborts the process.

Rider implementations follow the runtime C ABI:
`extern "C-unwind" fn(rt, arg0_ptr, arg0_tydesc, ..., result_out, result_tydesc) -> u8`,
returning 1 for Ok and 2 for Error.

Design notes: [plan-native-riders.md](plan-native-riders.md).

## The Shipped Binary

An installed `datalove` carries its standard library. It reads no part of the
source tree it was built from and shells out to no toolchain: `repl`, `script`,
`script --jit` and `aot-compile` all work with `sys/` deleted and cargo off
`PATH`.

They did not always. The compiler used to find the library with
`env!("CARGO_MANIFEST_DIR")` at three sites, and built the native riders by
running `cargo build --release` at startup, against crate sources in the same
tree. An installed binary therefore depended on the checkout it was compiled
from still existing, unmoved, with a Rust toolchain and a warm registry. When
any of that was missing the failure was silent - the REPL's engine thread died
and every entry sat at "parsing..." forever.

Three things travel inside the binary, all of them assembled by
`crates/datalove-stdlib`:

| What | How | Where it comes from |
|------|-----|---------------------|
| Module sources | `build.rs` walks `sys/`, emits a table of `include_str!` | `sys/*/*.dfm`, `sys/*/rider.dli` |
| Rider functions | `datalove-rider-std` is a normal dependency; its generated `symbols()` gives addresses | `sys/std/rider` |
| Native component | `build.rs` builds it in a nested cargo, embeds it gzipped | `datalove-native-component` |

`system_library()` assembles the first two into a `SystemLibrary`, which is a
`PackageLibrary` of sources plus `natives: Vec<(String, *const ())>`. Drivers
hand it to `WorkspaceDescriptor::from_system_library` and to
`register_linked_natives`. The raw addresses are not `Send`, which is why
`ThreadedExecutor::spawn` takes `fn() -> SystemLibrary` and calls it on the
worker thread rather than being handed the value.

`native_component_staticlib()` covers AOT, which needs a file for the linker
rather than addresses. It writes the embedded archive to
`$XDG_CACHE_HOME/datalove/lib/{sha256}-libdatalove_native_component.a` on
first use and returns the path. The digest names the file so a later datalove
never links an archive an earlier one left behind.

**The native component has its own cargo profile.** It ends up inside the
programs the AOT backend emits, not inside datalove, so `[profile.native-component]`
in the workspace manifest builds it the same way whatever profile datalove
itself is built in. `lto` is off there: thin LTO pads the archive with bitcode
nothing consumes, which cost 19 MB gzipped against 11 MB without.

It also gets its own target directory, because cargo holds a lock on the one
the outer build is running under. A nested cargo from a build script is
otherwise unremarkable - the package cache lock is long released by the time
build scripts run.

### What this costs in the tree

The embedding sits at the top of the crate graph on purpose. Had it gone where
`load_default_sys` used to live, in `datalove-datafun`, every `.dfm` edit would
invalidate the bottom of the stack: 22 s to rebuild what `just test` compiles,
against 0 s before. From `datalove-stdlib`, which only the CLI and a couple of
test targets depend on, the same edit costs about 7 s, nearly all of it
relinking the debug binary.

The loop that matters for stdlib work is untouched. `std_tests` and
`std_all_tests` compile `sys/` off disk through
`WorkspaceDescriptor::load_sys_dir`, so `cargo test -p datalove-datafun --test
std_tests` after editing a module recompiles nothing at all.

That only holds because the two copies cannot drift.
`crates/datalove-stdlib/tests/embedded_matches_tree.rs` asserts the embedded
table is byte-identical to `sys/`, and that every `native fun` a rider
interface declares is linked in. The REPL's `engine_tests` run against
`system_library()`, so the suite covers the shipped path too.

Editing `datalove-rt`, `datalove-rtdt` or the rider crate re-runs the nested
component build. It is incremental and adds about a second, but it is a
separate target directory from the main build - roughly 900 MB per profile -
and `std_all_tests` still builds its own copy under `target/datalove-work`.
Pointing that suite at the embedded artifacts would collapse the two.

### What is still tied to the tree

- `datalove docs` builds the website out of `mandocs/`, so it keeps its
  `env!("CARGO_MANIFEST_DIR")`. It is a repository tool.
- `cargo install --path` works; `cargo install datalove` from a registry would
  not. `datalove-stdlib`'s build script runs `cargo build -p
  datalove-native-component` from the repository root, which a packaged crate
  would not have.

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
    v   Output: IrCodeUnit with a Script context
```

Script units accumulate via `AccumulatedLowerBindings`:
- Tracks exports from prior units
- Each unit increments `accumulated_unit_specs`
- Functions reference prior units via `CodeRef::External { unit, id }`

### Ownership across units

A unit copies out of the bindings earlier units own rather than taking from
them: lowering emits `Clone` for a consuming use that resolves to an
`ExternalValue`/`ExternalSlot`. Every line of a REPL is a unit, so taking
would mean inspecting a value consumed it and binding it to a new name
emptied the old one. A whole-file script is one unit and has no such uses, so
this changes nothing about running a script.

A unit can still give away a binding it defined itself, and then the name
outlives its value. Ownership analysis reports those names in
`ScriptAnalysisData::dead_exports`, `ScriptCompiler` remembers them in
`dead_externals`, and using one is D013. Assigning to such a name revives it
(`revived_exports`), which is why `unit_end` covers every non-Copy slot rather
than only live ones - a later unit can assign to a slot this one gave away, and
the cleanup list is fixed at lowering time.

An expression unit that is a bare name computes nothing: lowering records the
name in `ScriptContext::result_name` with no result value, and the executor
reads that binding where it lives instead of copying it to print it.

## IR Types

### IDs

```rust
pub struct ValueId(pub u32);    // SSA value (immutable)
pub struct SlotId(pub u32);     // Mutable slot (var bindings)
pub struct ParamId(pub u32);    // Function parameter
pub struct BlockId(pub u32);    // Control flow block
pub struct FuncId(pub u32);     // Module-local function ID
pub struct CodeUnitId(pub u32); // Code unit, local to its containing scope
pub struct CallSiteId(pub u32); // Call site, unique within a code unit
pub struct IrModuleId(pub u32); // Module index (not salsa ModuleId)
```

`CodeUnitId` is the unified addressing scheme that replaced `FuncId` inside the
IR. `FuncId` survives above it, in the compiler's `FuncIdMap` from
`(ModuleId, name)` to `(IrModuleId, FuncId)`; the two are numerically the same
where both appear. `CallSiteId` stays stable across IR transformations like
inlining, which is what the JIT counts for tiering decisions.

### Code Unit References

```rust
pub enum CodeRef {
    Local(CodeUnitId),                             // Same module/unit
    External { unit: u32, id: CodeUnitId },        // Previous script unit
    Module { module: IrModuleId, id: CodeUnitId }, // Different module
}
```

`Module` uses numeric `IrModuleId` (not salsa `ModuleId`) for serializability.

### IrCodeUnit

One type represents functions, script units and native functions. The `context`
field decides execution semantics.

```rust
pub struct IrCodeUnit {
    pub id: CodeUnitId,
    pub name: String,

    // Body.
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub call_site_count: u32,
    pub value_types: Vec<IrType>,
    pub slot_types: Vec<IrType>,
    pub tracked_slots: Vec<SlotId>,
    pub const_values: Vec<(String, ValueId)>,
    pub symbols: SymbolTable,

    // Context.
    pub context: CodeUnitContext,

    // Units defined inside this one.
    pub nested_units: Vec<IrCodeUnit>,
}

pub enum CodeUnitContext {
    Function(FunctionContext),
    Script(ScriptContext),
    Native(NativeContext),
}
```

```rust
pub struct FunctionContext {
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub tracked_params: Vec<ParamId>,    // Out params needing runtime tracking
    pub descriptor_params: Vec<ParamId>, // Descriptors the caller supplies
}

pub struct ScriptContext {
    pub unit_end_values: Vec<ValueId>,
    pub unit_end_slots: Vec<SlotId>,
    pub result: Option<ValueId>,
    pub result_name: Option<String>,
    pub exports: Vec<(String, ExportBinding)>,
}

pub struct NativeContext {
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub symbol: String, // e.g. "dlr_std__list_push"
}
```

An `IrModule` is `{ functions: Vec<IrCodeUnit>, symbols: SymbolTable }`.
Lookup at runtime goes through `ModuleFunctionRegistry` (keyed by
`(IrModuleId, CodeUnitId)`), `UnitFunctionRegistry` (indexed by script unit),
or `FunctionRegistry`, which holds both.

### Type Layout

`datalove_datafun_ir::layout` is the single authority on how an `IrType` is
laid out. Both AOT backends and the CTFE evaluator compile against it, and
`datalove_rtdt::layout` computes the same layouts from runtime type
descriptors, which is what the runtime reads values back through.

Generated code writes at offsets from the first and the runtime reads at
offsets from the second, so a disagreement corrupts values rather than
failing a build. `layout_conformance_tests` walks a corpus of types and
checks size, alignment, field offsets, variant payload offsets and tag
payload offsets in both directions.

Nothing should open-code the arithmetic. Payload offsets have named
functions in both authorities:

| | `ir::layout` | `rtdt::layout` |
|---|---|---|
| enum variant | `enum_payload_offset(ty)` | `enum_payload_offset(align)` |
| `?T` | `option_payload_offset(ty)` | `option_payload_offset(align)` |
| `!T` | `result_payload_offset(ty)` | `result_payload_offset(align)` |

### Parameter Modes

```rust
pub enum ParamMode {
    In,  // Ownership transfers to callee
    Out, // Write-only, callee must initialize
    Ref, // Read-only borrow
    Mut, // Read-write borrow
}
```

Call sites repeat the mode: `ExprFunctionCall.arg_modes` holds the marker
written before each argument, `None` meaning `in`. Typechecking rejects any
disagreement with the callee's declared mode (F057), so later phases can read
the mode off the call site alone. Ownership analysis does exactly that, which
is why it needs no resolved call target to know how an argument is passed.

Reading a parameter is `Operand::Param(p)`, not an instruction. Writing has
four forms:

- `ParamStore { param, value }` - store to Mut (destroys old value)
- `ParamStoreTracked { param, value }` - store to Out (checks tracking byte)
- `ParamSetField { param, field_path, value }` - field write to Mut
- `ParamSetFieldTracked { param, field_path, value }` - field write to Out

`RefStore` and `RefSetField` generalize these to any reference-like operand,
which is what inlining needs to write straight to the caller's location.

For `out` params, the **caller** destroys the existing value before the call via `DropViaRef`.

### Enum Instructions

```rust
// Read u32 discriminant tag from enum value. Borrows src (does not consume).
EnumDiscriminant { dest: ValueId, src: Operand }

// Move payload out of enum into dest. Consumes src.
EnumPayload { dest: ValueId, src: Operand, variant_index: u32 }

// Construct enum value with given variant and optional payload.
EnumVariant { dest: ValueId, variant_index: u32, payload: Option<Operand> }
```

Match lowering emits `EnumDiscriminant` to read the tag, then a chain of
comparisons branching to arm blocks. Atom arms `Drop` the input; term arms
use `EnumPayload` to extract the binding.

### Intrinsics

`icall name(args)` compiles to a single machine operation with no call
overhead. `datalove-datafun-intrinsics` defines `IntrinsicId` with stable
discriminants for serialization, covering bitwise ops, shifts, bit counting and
casts. The interpreter implements them in `interp/src/intrinsics.rs`; the
compiled backends emit instructions directly.

## Salsa Patterns

See [salsa-patterns.md](salsa-patterns.md) for how the four salsa kinds are
used here, why expression tables are keyed on `ExprKey` rather than salsa ids,
and how to measure whether a change memoizes.

### Database

The compiler's database is in `datalove-datafun-compiler/src/lib.rs`:

```rust
#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}
```

Implements `DbClone` - shares `Arc<Zalsa>` global state, clones thread-local state.
`Database::recording(recorder)` returns one that reports every query it runs;
clones report to the same recorder, so work farmed out to rayon is recorded too.

`bcts`, `datalove-datalit` and the `datalove` facade each declare their own
plain salsa `Database` as well. Compiler work uses the datafun one, re-exported
as `datalove_datafun::Database`.

### Tracked Functions

| Function | Crate | Key Inputs | Output |
|----------|-------|------------|--------|
| `parse_module_full` | parser | module | `ParseResult` with spans |
| `parse_module_ast` | parser | module | `ParsedStatements`, projected from `parse_module_full` so a module is parsed once |
| `parse_module_graph` | compiler | graph, requires, rider sources | `ParsedModuleGraph` |
| `resolve_module_names` | resolve | module | `ModuleNameResolution` |
| `typecheck_module` | tycheck | module, parsed, name_resolution, imports | `SingleModuleTypecheckResult` |
| `analyze_module` | compiler | module, parsed, typecheck | `SingleModuleAnalysis` |
| `compute_func_id_map` | compiler | parsed_graph | `FuncIdMap` |
| `lower_module` | compiler | module, ir_module_id, parsed, typecheck, ownership, func_ids, consts, skip_inlining, funcs | `SingleModuleLoweringResult` |
| `analyze_script_fragment_tracked` | compiler | typecheck, statements, adapt mode, dead externals | `ScriptUnitOwnershipResult` |

Graph-level functions aggregate per-module results.

### Incremental Compilation

Key principle: `Module` objects are created once and reused. Updates use `set_source()`.

`IncrementalModuleWorld`:
- Stores `Module` objects in `BTreeMap<String, Module>`
- `add_module()` creates new module once
- `update_source()` preserves identity
- Graph rebuilds reuse existing modules

Memoization behavior:
- Whitespace-only changes: re-parses, but the `parse_module_ast` projection
  returns an equal value and backdates, so name resolution and typechecking do
  not re-run (see the firewall section of salsa-patterns.md)
- AST changes (same types): re-typechecks changed module only
- Type changes: re-typechecks dependents

`ModuleId` is a `#[salsa::input]`, so every `::new()` makes a distinct id even
for the same path. That is why identity is preserved rather than recreated, and
why the rider modules share one synthetic id per alias.

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

Runs after typechecking, produces `DropSchedule` consumed by lowering. The
types it produces live in `datalove-datafun-sema` so lowering can depend on
them without depending on the analysis.

### Tracking Categories

```rust
pub enum TrackingCategory {
    Copy,    // No tracking/drops needed
    Precise, // State statically known
    Tracked, // May vary at runtime (needs tracking byte)
}
```

Tracked bindings: exports, `out` params, conditional moves, mutable slots.

Match arms use `ScopeKind::MatchArm`. Branch consistency (D008) is generalized
across all match arms: if a value is moved in one arm, it must be moved in all.

### Auto-adapt

`AutoAdaptMode::Enabled` accepts the `@`-recoverable errors catalogued in
`report-adapt-cases.md` by supplying the `@` the source left out. Both
analyses record where it belongs as `AdaptSites`, keyed by `ExprKey`, and
lowering emits what an explicit `@` on that expression would - a widening
between fixed ints, a clone for linear types.

Ownership analysis records the *earlier* use, not the one that would have
errored: a value read after a move is already gone, so the repair belongs
where it was given away. That covers D001, D002, and D007, where the clone
restores the binding for the next iteration. The typechecker's adaptations
travel the same way, which is why `ScriptCompiler` typechecks through
`create_batch_spec_with_auto_adapt` rather than `create_batch_spec` and its
hardcoded `Disabled`.

Sites are keyed by expression, so handing a body a set naming expressions
from elsewhere is harmless - no expression there matches.

Module-level auto-adapt is not wired up: `compile_modules` passes
`AutoAdaptMode::Disabled` to both typechecking and ownership analysis, and
`ModuleCompilationPipeline` has no way to ask for anything else.

### Error Codes

Variants of `AnalysisError` in `datalove-datafun-sema`.

| Code | Variant | Trigger |
|------|---------|---------|
| D001 | `UseAfterMove` | Using value after move |
| D002 | `DoubleMove` | Moving value twice |
| D003 | `CannotMoveBorrowed` | Moving `ref`/`mut`/`out` param |
| D004 | `CannotMutFromRef` | Passing `ref` to `mut` param |
| D005 | `ReadUninitialized` | Reading an `out` param or `var` binding before it is set |
| D006 | `OutParamNotInitialized` | Return without initializing `out` |
| D007 | `MoveInLoop` | Moving outer-scoped value in loop |
| D008 | `InconsistentBranchMove` | Value moved in one branch only |
| D009 | `OutParamPartialWrite` | Field write to `out` param |
| D010 | `AliasedMutableArgument` | Two arguments share a place root, one is `mut`/`out` |
| D011 | `CannotMutateImmutable` | `let` binding or `in` param passed as `mut`/`out` |
| D012 | `CannotMutateTemporary` | Non-place argument passed as `mut`/`out` |
| D013 | `UseAfterMoveInEarlierUnit` | Using a binding whose own script unit gave its value away |

D001, D002, D007 and D013 carry an `OwnershipRecoveryHint` and are the ones
auto-adapt can repair. The `D0xx` codes in `datalove-datalit`'s parser are a
separate namespace and unrelated.

### Drop Schedule

`DropSchedule` tells lowering where to emit `Drop` instructions:

```rust
pub struct DropSchedule {
    pub then_branch_exit: BTreeMap<usize, Vec<BindingId>>,
    pub else_branch_exit: BTreeMap<usize, Vec<BindingId>>,
    pub before_return: BTreeMap<usize, Vec<BindingId>>,
    pub before_try_return: BTreeMap<usize, Vec<BindingId>>,
    pub before_set_target_early_return: BTreeMap<usize, Vec<BindingId>>,
    pub loop_body_end: BTreeMap<usize, Vec<BindingId>>,
    pub before_break: BTreeMap<usize, Vec<BindingId>>,
    pub before_continue: BTreeMap<usize, Vec<BindingId>>,
    pub match_arm_exit: BTreeMap<(usize, usize), Vec<BindingId>>,
    pub stmt_order: Vec<StmtKey>,
}
```

`before_set_target_early_return` is computed before RHS moves are analyzed, so
it includes the RHS binding, which is still live when a set-index bounds check
fails.

### AOT Tracking Bytes

Out params get tracking bytes in the frame:

```rust
let param_tracking_base = tracking_offset + tracked_values.len() + tracked_slots.len();
for (i, &pid) in tracked_params.iter().enumerate() {
    params[pid.0 as usize].tracking_byte = Some(param_tracking_base + i as u32);
}
```

Values: `UNINIT = 0x00`, `LIVE = 0x01`, `MOVED = 0x02`

## Execution Backends

All of them consume `IrCodeUnit` and agree with `ir::layout`.

- **Interpreter** (`datalove-datafun-interp`) walks the IR directly.
  `CallDispatcher` is the extension point: it can intercept a call and hand it
  to a JIT or a dynamic inliner, or return `NotHandled` to fall through.
  Native rider calls are checked before the dispatcher.
- **CTFE** (`interp/src/ctfe.rs`) is the interpreter used at compile time.
  `InterpCtfeEvaluator::with_module_registry` gives const expressions access to
  cross-module function calls.
- **JIT** (`datalove-datafun-cranelift-jit`) tiers by call count.
  `OptimizingDispatcher` tracks call sites, picks the best available IR
  (inlined if one exists), executes native code when compiled, and otherwise
  counts toward the threshold. `DispatcherMode::Tuned` uses thresholds;
  `Chaos` makes seeded pseudo-random decisions for testing.
- **Cranelift AOT** (`datalove-datafun-cranelift-aot`) compiles ahead of time.
- **C AOT** (`datalove-datafun-c-aot`) emits C11 in one pass: type descriptors,
  then functions (module functions prefixed `__mod_N_`), then
  `__script_body(void* rt)` and a `main()` that initializes the runtime.
- **Inlining** (`datalove-datafun-inline`) transforms IR under
  `InlineDirective`s, and also drives the interpreter's dynamic inliner.

The runtime's C API (`datalove-rt/src/c.rs`) is entirely `dtlv_rti_*`: calls
only the compiler emits, which may use whatever ABI is convenient. The crate
docs also describe a `dtlv_rt_*` family the language would call under a
restricted ABI, but none exist yet. Everything but `init` takes a runtime
handle, and every value pointer is followed by its tydesc.

## Entry Points

### Module Compilation

**Analysis only** (`compile.rs`):

```rust
pub fn compile_modules<'db>(
    db: &'db dyn DbClone,
    input: ModuleCompilationInput<'db>,
    rider_sources: Vec<(String, String)>,
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
    skip_specialization: bool,
) -> ModuleGraphLoweringResult<'db>
```

### High-Level API

```rust
let mut pipeline = ModuleCompilationPipeline::default();
pipeline.add_module(&db, "local", "pkg", "main", source);
let compiled = pipeline.compile_fresh(&db);

// Incremental. Returns the compiled modules and the reborrowed database.
pipeline.update_source(&mut db, "local", "pkg", "main", new_source);
let (compiled, db) = pipeline.compile(&mut db);
```

`ModuleCompilationPipeline::from_sections(&db, &sections, ConstInlining::Enabled)`
builds one from parsed worldfile sections, which is how most tests construct it.
`compile_fresh_with_mode` and `compile_with_mode` take an explicit
`ParallelMode` instead of reading `DATALOVE_PARALLEL`.

### Workspaces

`WorkspaceDescriptor` is an immutable snapshot of everything compilation reads:
system library (absent under `--no-sys`), user libraries in shadowing order,
`CompilerOptions`, and a `work_dir` the compiler may write to. It is cheap to
clone and two of them can be diffed into a `WorkspaceDelta` for incremental
recompilation. It is pure data; drivers (CLI, REPL, LSP) build one and feed it
to `ModuleCompilationPipeline`.

Two constructors supply the system library, and neither goes looking for it:

```rust
// A driver that carries its own library, which is every shipped command.
let sys = datalove_stdlib::system_library();
let descriptor = WorkspaceDescriptor::from_system_library(&sys);

// A test compiling the tree it lives in.
let descriptor = block_on(WorkspaceDescriptor::load_sys_dir(repo_root.join("sys")))?;
```

`from_package_world` and `from_worldfile_sections` build the other kinds.

`work_dir` is output, not input, so it takes no part in `diff`. Two workspaces
compiled concurrently must not share one or their native components overwrite
each other. Only workspaces that build riders from source need one; a
descriptor built from a `SystemLibrary` has no rider crate directories and
never writes anything.

See [proposal-workspaces.md](proposal-workspaces.md).

### Script Compilation

```rust
let mut script_compiler = compiled.script_compiler_default(db).expect("modules compiled");
let result = script_compiler.compile_fragment(source);
// result.typecheck, result.ownership, result.lowering
```

`script_compiler_default` uses the interpreter as the CTFE evaluator, wired to
the module registry so const expressions can call across modules;
`script_compiler(db, evaluator)` takes a different one. Both return `None` if
module compilation had errors.

`ScriptCompilationResult` has separate results for partial compilation
(typecheck can succeed while lowering fails). `compile_expr` handles a single
expression unit, `compile_fragment` a statement unit.

## Test Patterns

### Exampletest Framework

```rust
ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
    .fixture_subdir("ir_lower")
    .file_extension("world")
    .allow_errors(true)
    .run();
```

- `BLESS=1 cargo test` updates expected output. Unset `RUST_BACKTRACE` first.
- Filter: `cargo test -- filter_name`
- `just test` runs the whole suite; `just test-64` uses 64-bit collection
  indexes, `just test-parallel` sets `DATALOVE_PARALLEL=1`, `just test-slow`
  the `slow_tests` features.

### Test Suites

Most live in `crates/datalove-datafun/tests`.

| Suite | Purpose |
|-------|---------|
| `parser_tests` | Parse and emit AST + diagnostics (in the compiler crate) |
| `tycheck_tests`, `tycheck_world_tests` | Typecheck results |
| `auto_adapt_tests` | `@`-recoverable errors under `AutoAdaptMode::Enabled` |
| `ir_lower_tests` | IR lowering from worldfiles |
| `ir_lower_script_tests` | Script IR lowering |
| `ir_inline_tests` | Inlining transformations |
| `ir_serial_tests` | IR serialization round-trip |
| `interp_tests`, `module_interp_tests` | Interpreter execution |
| `interp_jit_tests`, `interp_dispatch_tuned_tests`, `interp_dispatch_chaos_tests` | JIT tiering and dispatch |
| `interp_specialize_tests`, `interp_constlet_tests` | Specialization and const bindings |
| `aot_tests`, `aot_layout_tests` | Cranelift AOT compilation and layout compatibility |
| `dual_tests`, `c_dual_tests` | Compare interp vs Cranelift AOT, and vs C AOT |
| `layout_conformance_tests` | `ir::layout` against `rtdt::layout`, both directions |
| `native_rider_tests` | End-to-end native rider calls |
| `std_tests`, `std_all_tests` | The `sys/std` library, compiled from `sys/` on disk |
| `embedded_matches_tree` | The embedded stdlib against `sys/`, and every declared native linked (in `datalove-stdlib`) |
| `module_memo_tests`, `incremental_memo_tests`, `no_op_recompile_tests`, `parse_firewall_tests` | Salsa memoization behavior |
| `database_memory_tests` | Database growth |
| `worldgen_tests`, `worldgen_dual_tests` | Generated worldfiles typecheck and run the same both ways |

### Worldfile Format

```datalove
----------
rider testlib
----------
native fun int_add(a: i32, b: i32): i32

----------
module local/pkg/main
----------
require rider testlib
import testlib.int_add

fun main(): i32
    ret int_add(3, 4)
end fun
```

Section headers: `module <lib>/<pkg>/<mod>`, `module-add`, `module-remove`,
`module-change-ws`, `module-change-ast`, `module-change-ty`,
`scriptunit-fragment`, `scriptunit-expr`, `inline-directives`, `rider <name>`.
The three `module-change-*` kinds drive the memoization tests.

## Key Files

| File | Contents |
|------|----------|
| `compiler/src/lib.rs` | `Database`, `DbClone` impl, module exports |
| `compiler/src/compile.rs` | `compile_modules()`, `ModuleCompilationOutput` |
| `compiler/src/module_graph.rs` | `IncrementalModuleWorld`, parsing pipeline, rider interface construction |
| `compiler/src/tracked_lower.rs` | `lower_module_graph_with_evaluator()`, `compute_func_id_map()`, three-phase lowering |
| `compiler/src/tracked_ownership_analysis.rs` | Salsa-tracked ownership analysis |
| `compiler/src/tracked_script_lower.rs` | Script lowering, `collect_const_graph()` |
| `compiler/src/tracked_script_ownership.rs` | Script ownership analysis |
| `compiler/src/specialize.rs` | Const parameter specialization, `build_dispatch_blocks()` |
| `sema/src/lib.rs` | `AnalysisError`, `DropSchedule`, `TrackingCategory`, `ExprTypes` |
| `ownership/src/lib.rs` | Drop/ownership analysis |
| `lower/src/func.rs` | `lower_function_for_module()` |
| `lower/src/script.rs` | Script unit lowering |
| `lower/src/const_expr.rs` | `lower_const_binding()`, `try_extract_literal()` |
| `const/src/eval.rs` | `evaluate_prepared_const()` |
| `const/src/inline.rs` | `inline_script_consts()`, `inline_module_functions()` |
| `ir/src/lib.rs` | `IrCodeUnit`, `CodeRef`, `IrType`, `Instruction` |
| `ir/src/layout.rs` | The layout authority for `IrType` |
| `ir/src/registry.rs` | `ModuleFunctionRegistry`, `UnitFunctionRegistry` |
| `datafun/src/pipeline/mod.rs` | `ModuleCompilationPipeline` re-exports |
| `datafun/src/pipeline/module_pipeline.rs` | The pipeline itself, `add_native_rider_units()` |
| `datafun/src/pipeline/script_compiler.rs` | Script compilation pipeline |
| `datafun/src/pipeline/workspace.rs` | `WorkspaceDescriptor`, `WorkspaceDelta` |
| `datafun/src/pipeline/rider_build.rs` | Native component synthesis and cargo build, for riders found on disk |
| `datafun/src/pipeline/rider_load.rs` | `register_linked_natives`, `load_rider_library`, the C ABI bridge |
| `cli/src/main.rs` | `register_natives()`, which wires both the interpreter table and the JIT |
| `stdlib/build.rs` | Embeds `sys/` and builds and embeds the native component |
| `stdlib/src/lib.rs` | `system_library()`, `native_component_staticlib()` |
| `sys/std/rider/build.rs` | Generates `symbols()` from `rider.dli` |
| `interp/src/native.rs` | `NativeFunctionTable` |
| `interp/src/dispatch.rs` | `CallDispatcher`, `DispatchResult` |
| `cranelift-jit/src/optimizing.rs` | Tiering and dynamic inlining dispatcher |
