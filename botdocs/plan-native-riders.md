# Native Riders and Workspace Plan

## Overview

Native riders let datalove modules call Rust code through a defined ABI.
Riders belong to packages. A package has at most one rider, shared by all its modules.
The system library (`sys/std`) uses the same mechanism as user packages -- std is not special.

A new compiler driver layer above the current pipeline orchestrates workspace discovery,
rider compilation, and linking.

This is the design document. For what was built, see
[compiler-guide.md](compiler-guide.md) under "Native Riders" and "The Shipped
Binary". The one place the two diverge is loading: `sys/std`'s rider is linked
into the datalove binary and its addresses come from a generated table, rather
than being dlopened out of a library cargo built at startup. Riders found on
disk still take the path described below.

## Structural Model

```
workspace/
  workspace.dlt                 # manifest (or inferred)
  sys/
    std/                        # package sys/std
      bool.dfm                  # module sys/std/bool
      list.dfm                  # module sys/std/list
      u32.dfm                   # module sys/std/u32
      string.dfm                # module sys/std/string
      rider.dli                 # rider interface for package sys/std
      rider/                    # rider crate
        Cargo.toml
        src/lib.rs
  local/
    myapp/                      # package local/myapp
      main.dfm
      utils.dfm
```

One rider per package. The `.dli` and `rider/` crate live alongside the package's modules.
Not every package needs a rider.

## Language Surface

### In a module (`sys/std/list.dfm`)

```datalove
require rider std
import std.list_push
import std.list_len
import std.list_get

fun push_two(mut self: [data], a: data, b: data)
    list_push(mut self, a)
    list_push(mut self, b)
end fun
```

`require rider std` is a new require kind.
The name `std` is symbolic, resolved by the compiler driver.
`import std.list_push` works identically to importing from a module.

### In the rider interface (`rider.dli`)

```datalove
native fun list_push(mut self: [data], elem: data)
native fun list_len(ref self: [data]): index
native fun list_get(ref self: [data], i: index): ?data

native fun string_len(ref self: string): index
native fun string_concat(a: string, b: string): string

native fun int_add(a: int, b: int): int
native fun int_neg(a: int): int
```

Restricted syntax: `native fun` declarations and `type` aliases only.
No requires, no imports, no executable statements.

### In the rider crate (`rider/src/lib.rs`)

```rust
use datalove_rt::c::{LocalRtHandle, RtStatus};
use datalove_rtdt::TyDesc;

#[unsafe(no_mangle)]
pub extern "C-unwind" fn dlr_std__list_push(
    rt: LocalRtHandle,
    self_mut: *mut u8,
    self_tydesc: *const TyDesc,
    elem_in: *mut u8,
    elem_tydesc: *const TyDesc,
) -> RtStatus {
    // calls into datalove_rt internals
    todo!()
}
```

Symbol naming convention: `dlr_{rider_name}__{function_name}`.
Double underscore separates rider from function.
The compiler generates matching declarations.

## Dependency Graph

The preparse phase extracts require demands.
`require rider` is a new demand kind alongside `require module`.

```
Module nodes:  sys/std/list, sys/std/u32, sys/std/string, ...
Rider nodes:   std
Edges:
  sys/std/list    --require rider-->  std
  sys/std/u32     --require rider-->  std
  sys/std/string  --require rider-->  std
  sys/std/list    --require module--> sys/std/u32  (hypothetical)
```

Rider nodes are leaves. They provide signatures but have no outgoing dependencies.
Rider interfaces are parsed trivially before module compilation begins.

## Impact on the Existing Compiler Pipeline

| Phase | Change |
|-------|--------|
| Preparse | Extract `require rider` demands alongside `require module`. |
| Parse | Parse `.dli` files with restricted grammar. Produce `RiderInterface`. |
| Name resolution | Rider names enter scope from `require rider`. `import std.list_push` resolves against the `RiderInterface`. |
| Typecheck | Native functions have known signatures. Call sites typecheck normally. No body to typecheck. |
| Ownership | Skipped for native functions. Declared param modes are trusted. |
| Lowering | Native functions emit `IrCodeUnit` with `NativeContext` (signature + linker symbol, no blocks). Call sites emit normal `Call` instructions. |

The pipeline receives `ModuleCompilationInput` augmented with rider interfaces
and produces IR referencing native symbols.
File discovery and linking are the driver's job, not the pipeline's.

## IR Representation

Builds on the planned unified code unit design:

```rust
pub enum CodeUnitContext {
    Function(FunctionContext),
    Script(ScriptContext),
    Native(NativeContext),
}

pub struct NativeContext {
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub symbol: String,  // e.g. "dlr_std__list_push"
}
```

A native code unit has a signature and a symbol but no blocks.
Call sites reference native functions via `CodeRef::Module { module, id }` like any other.
The interpreter and AOT backends dispatch on context type.

## Interpreter Path

When executing a call to a native code unit:

1. Look up the `IrCodeUnit` in the registry.
2. See `NativeContext`.
3. Look up function pointer in `NativeFunctionTable` by symbol name.
4. Marshal arguments to C-ABI, call through the pointer.

The `NativeFunctionTable` is populated by the driver before execution:
load the rider's `.so`/`.dylib` via `dlopen`, look up each symbol.
For a rider compiled into the same binary, populate with static function pointers.

Both were built. The second is what the datalove binary uses for `sys/std`, so
that an installed binary needs no cargo at startup; the first is for riders
discovered on disk.

## AOT Path

Native functions fit the existing three-pass AOT compilation:

- **Pass 2** (declare functions): native code units get `Linkage::Import` using their symbol name.
  Identical to how `dtlv_rti_*` functions are declared today.
- **Pass 3** (compile): call instructions targeting native functions generate the same code
  as any other call. No special case.
- **Linking**: the rider's `.a` is linked alongside the runtime `.a`. As built,
  the two are one archive - the native component `rider_build` synthesizes for
  whatever riders the module graph holds, built with cargo in the workspace's
  work dir.

## Compiler Driver

New top-level orchestrator above `ModuleCompilationPipeline`:

```
WorkspaceDriver
  1. Load workspace layout
     - Read workspace.dlt or discover by convention
     - Enumerate libraries, packages, modules
     - Identify which packages have riders

  2. Build dependency inputs
     - Preparse all .dfm files -> require demands
     - Parse all .dli files -> RiderInterface objects
     - Resolve rider names to RiderInterface objects
     - Build ModuleGraph with resolved requires (modules + riders)

  3. Compile modules (existing pipeline)
     - Input: ModuleCompilationInput + rider interfaces
     - Output: IR with native code units

  4. Build riders (if needed)
     - Invoke cargo on each rider crate
     - Produce .a (static) and/or .so (dynamic) artifacts
     - Cache artifacts, rebuild only on change

  5. Assemble
     - AOT: link IR output + rider artifacts -> executable
     - Interp: load rider .so, populate NativeFunctionTable, execute
     - JIT: same as interp, compile hot paths on demand
```

The driver is a new crate above `datalove-datafun` in the dependency tree.

## Workspace Manifest

Minimal `workspace.dlt`:

```dlt
{
    name = "my-project",
    libraries = %{
        "local" = { path = "src" },
    },
}
```

The system library is implicit (provided by the toolchain).
Each library is a directory of packages.
Each package is a directory of modules.
Rider presence is detected by `rider.dli` in the package directory.

## Migrating the Runtime

Today the Cranelift codegen hard-codes `dtlv_rti_*` functions.
These can be migrated incrementally to the `sys/std` rider.

**Keep as runtime kernel**: `dtlv_rti_init`, `dtlv_rti_shutdown`,
memory allocation, type descriptors, `debuglog`, `any_destroy`, `any_clone`.
These are operations the rider code itself needs.

**Move to sys/std rider**: list ops, map/set ops, string ops,
int arithmetic, table/tensor ops.

After migration, the codegen's runtime call list shrinks to just the kernel.
Everything else is a normal native function call resolved through the module system.

## Bootstrapping Order

1. **`native fun` syntax + `.dli` parser.**
   Parse rider interfaces, produce `RiderInterface` structs.

2. **`require rider` in preparse + name resolution.**
   Modules declare rider dependencies, import native functions, typecheck calls.
   IR emits `NativeContext` units. Execution panics on native calls.

3. **Interpreter native dispatch.**
   `NativeFunctionTable` + `call_native`. Hand-write a trivial rider crate,
   load it manually. First native function call from datalove.

4. **AOT native dispatch.**
   External symbol imports + linking against rider `.a`.

5. **Workspace discovery.**
   `WorkspaceLoader` scans directories, finds riders, resolves names.

6. **Rider crate build.**
   Driver invokes cargo to build rider crates.

7. **Migrate sys/std to riders.**
   Move domain ops from hard-coded runtime calls to the sys/std rider.

Each step is independently testable.
Steps 1-2 are pure compiler work.
Step 3 is the first end-to-end proof.
Steps 5-7 are incremental payoff.
