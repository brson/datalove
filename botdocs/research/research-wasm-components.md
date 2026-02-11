# Datafun to WASM Component Model Mapping

Research notes on how Datafun modules could map to WebAssembly Component Model.

## Core Concepts Alignment

| Datafun | WASM Component Model |
|---------|----------------------|
| Module (e.g., `local/test/utils`) | Component |
| `fun` export | Export function |
| `import utils.helper` | Import interface function |
| `IrType` | WIT interface type |
| `ModuleExports` | WIT world exports |
| `ModuleImports` | WIT world imports |
| `TyDesc` | Component-level canonical ABI type |

## Key Design Questions

### 1. Runtime Handle Threading

Currently every function takes `rt: LocalRtHandle` as first param. Options for WASM:

**a) Component-internal runtime**: Each component instantiates its own runtime
- Simpler, but no sharing of heap across modules
- `dtlv_rti_*` functions compiled into each component

**b) Imported runtime interface**: Runtime is an imported interface
```wit
interface datafun-runtime {
    resource runtime;
    mem-alloc: func(rt: borrow<runtime>, size: u32, align: u32) -> u64;
    list-push: func(rt: borrow<runtime>, list: u64, elem: u64, ...);
}
```
- Enables runtime sharing across components
- More complex component graph

**c) Host-provided runtime**: Runtime as platform capability
- Most natural for embedding in host apps

### 2. Type Mapping to WIT

Datafun types have natural WIT counterparts:

```wit
// Scalars
type bool = bool;
type u8 = u8;
...
type i64 = s64;
type f32 = float32;

// Int (arbitrary precision) - needs special handling
// Option 1: opaque resource
resource int;
// Option 2: list<u32> with sign flag
record int { limbs: list<u32>, negative: bool }

// String
type string = string;

// Collections
type list<T> = list<T>;
type set<T> = list<T>;  // WIT has no native set
type map<K,V> = list<tuple<K,V>>;  // WIT has no native map

// Tuples/structs
record my-struct { field1: u32, field2: string }
tuple<u32, string>

// Option/Result
option<T> = option<T>;  // Direct WIT support
result<T> = result<T, error>;  // Direct WIT support
```

### 3. Collections as Resources

For preserving identity and mutation:
```wit
interface datafun-collections {
    resource list<T> {
        constructor(element-type: type-desc);
        push: func(elem: T);
        get: func(idx: u32) -> option<T>;
        len: func() -> u32;
    }

    resource set<T> {
        constructor(element-type: type-desc);
        insert: func(elem: T) -> bool;
        contains: func(elem: borrow<T>) -> bool;
    }

    resource map<K, V> {
        constructor(key-type: type-desc, val-type: type-desc);
        insert: func(key: K, val: V) -> option<V>;
        get: func(key: borrow<K>) -> option<V>;
    }
}
```

### 4. Module Interface Generation

Given `ModuleExports`, generate WIT:

```datalove
// local/test/utils
fun add(a: u32, b: u32): u32 ... end fun
fun greet(name: String): String ... end fun
```

becomes:

```wit
package local:test;

interface utils {
    add: func(a: u32, b: u32) -> u32;
    greet: func(name: string) -> string;
}

world utils-world {
    export utils;
}
```

### 5. Cross-Module Imports

```datalove
// local/test/main
require module local/test/utils
import utils.add

fun main(): u32
    ret add(@1, @2)
end fun
```

becomes:

```wit
package local:test;

world main-world {
    import local:test/utils;
    export main: func() -> u32;
}
```

## Backend Architecture (Using Eval Kit)

```
                            datalove-datafun-eval
                                    |
          +-------------------------+-------------------------+
          |                         |                         |
datalove-datafun-interp   datalove-datafun-aot-cranelift   datalove-datafun-wasm
          |                         |                         |
      interpreter              native .o/.so          .wasm components
```

**New crate: `datalove-datafun-wasm`**

Responsibilities:
- Implement `InstructionEmitter` for wasm-encoder
- Generate WIT interfaces from `ModuleExports`/`ModuleImports`
- Emit component binary with canonical ABI
- Link runtime (either embedded or imported)

## Implementation Approaches

### Option A: Core WASM + Shims

1. Compile Datafun IR to core WASM (wasm32-unknown-unknown target)
2. Generate adapter shims for component model ABI
3. Bundle as component using `wit-component`

```rust
// In datalove-datafun-wasm
pub struct WasmEmitter {
    module: wasm_encoder::Module,
    types: WasmTypeSection,
    funcs: WasmFunctionSection,
    ...
}

impl InstructionEmitter for WasmEmitter {
    fn emit_const(&mut self, ...) { /* wasm i32.const, i64.const, etc */ }
    fn emit_binop(&mut self, ...) { /* wasm i32.add, f32.mul, etc */ }
    fn emit_call(&mut self, ...) { /* wasm call, call_indirect */ }
}
```

### Option B: Direct Component Generation

Use `wasm-tools`/`wit-component` to generate components directly:

1. Generate WIT from Datafun module signatures
2. Compile function bodies to wasm bytecode
3. Lift/lower using canonical ABI

## Memory Model Considerations

Datafun's ownership model maps reasonably well to component model:

| Datafun | Component Model |
|---------|-----------------|
| `ParamMode::In` (move) | `T` (by value, ownership transfers) |
| `ParamMode::Out` | Return value |
| `ParamMode::Ref` | `borrow<T>` |
| `ParamMode::Mut` | `borrow-mut<T>` (not in WIT yet) |

For heap types, need to decide:
- **Lift to WIT lists**: Copy data across boundary (simple, but copying)
- **Resource handles**: Share underlying memory (efficient, but more complex)

## Open Questions

1. **Runtime linkage**: Should each component have its own runtime, or share?

2. **Collection representation**:
   - Copy on boundary (convert List<T> to WIT list<T>)?
   - Or resource-based (keep internal representation, expose via methods)?

3. **Int (arbitrary precision)**:
   - As opaque resource?
   - As WIT record with limbs list?
   - As string (base-10)?

4. **Target use case**:
   - Embedding Datafun in other WASM hosts?
   - Calling WASM components from Datafun?
   - Both?

5. **Interop with other languages**: Do you want Datafun components to be callable from any WIT-compatible language, or primarily Datafun-to-Datafun?
