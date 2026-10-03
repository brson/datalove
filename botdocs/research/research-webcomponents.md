# Research: Lowering Datafun to WebAssembly Components

Research date: 2026-01-08

## 1. WebAssembly Component Model Status (2026)

### 1.1 Current State

The WebAssembly Component Model is now mature and widely adopted:

- **WASI 0.2.0** (stable since Jan 2024) - Foundation for component development
- **WASI 0.3.0** (released Feb 2026) - Adds native async support via composable concurrency
- **Wasm 3.0** - Core spec stable with component model integration

The component model solves cross-language interoperability by defining:
- A canonical ABI for data exchange between components
- WIT (WebAssembly Interface Types) for interface definition
- Component composition for building applications from parts

### 1.2 Key Features

**WIT Interface Definition Language:**
```wit
package datalove:datafun@0.1.0;

interface types {
    record point { x: s32, y: s32 }
    variant result-val { ok(string), err(string) }
    type my-list = list<u32>;
}

world script-world {
    import wasi:io/streams;
    export run: func() -> result<string, string>;
}
```

**Canonical ABI Representation:**

| Datafun Type | WIT Type | Linear Memory ABI |
|--------------|----------|-------------------|
| bool | bool | i32 (0 or 1) |
| u8-u64 | u8-u64 | i32/i64 |
| i8-i64 | s8-s64 | i32/i64 |
| f32 | f32 | f32 |
| int (bigint) | `list<u8>` | ptr + len (limbs) |
| string | string | ptr + len (UTF-8) |
| `List<T>` | `list<T>` | ptr + len |
| `Option<T>` | `option<T>` | discriminant + payload |
| `Result<T>` | `result<T, string>` | discriminant + ok/err |
| tuple | tuple | flattened fields |
| struct | record | flattened fields |
| enum | variant | discriminant + payload |

**Memory Allocation:**
- Components export `cabi_realloc(ptr, old_size, align, new_size) -> ptr`
- Host calls this to allocate linear memory for strings/lists
- Zero old_ptr means fresh allocation

### 1.3 Tooling Ecosystem

**Production-ready:**
- `wasm-tools component new` - Wrap core wasm into component format
- `wasm-tools component wit` - Inspect component interfaces
- `wasm-tools compose` - Link components together
- `wit-bindgen` - Generate language bindings from WIT

**Runtimes:**
- Wasmtime (Bytecode Alliance) - Full WASI 0.3 support
- Wasmer - WASI 0.2 support
- jco - JavaScript/Node.js runtime
- Browsers do not natively support WASI (polyfills available)

### 1.4 WASIp3 Async Model

WASI 0.3 introduces:
- Async function ABI (avoids function coloring problem)
- Built-in `stream<T>` and `future<T>` types
- Seamless sync-to-async bridging

This is relevant for datafun if we want to support async I/O in the future.

## 2. Datafun Compiler Architecture

See [compiler-guide.md](../compiler-guide.md): the module pipeline, the IR
(SSA values plus mutable slots, block parameters rather than phis), and the
execution backends. A Wasm backend would sit beside the Cranelift AOT and C
backends, consume the same IR, and call the runtime through the same
`dtlv_rti_*` C ABI that `datalove-rt` exports.

## 3. Lowering Strategy for WebAssembly Components

### 3.1 Architecture Options

**Option A: Generate Core Wasm + Wrap to Component**
```
IR -> Wasm core module -> wasm-tools component new -> Component
```
- Use existing pattern from Cranelift backend
- Generate Wasm instead of native code
- Let wasm-tools handle component wrapping
- Requires embedding or linking a Wasm-compatible runtime

**Option B: Direct Component Generation**
```
IR -> Component binary (with canonical ABI)
```
- More control over component structure
- Direct WIT interface matching
- More complex implementation

**Recommended: Option A** - Follows established ecosystem patterns.

### 3.2 New Crate Structure

```
crates/
  datalove-datafun-aot-wasm/      # New crate
    src/
      lib.rs                       # WasmCompiler entry point
      codegen/                     # IR -> Wasm translation
        mod.rs
        expr.rs                    # Expression codegen
        stmt.rs                    # Statement codegen
        func.rs                    # Function codegen
        types.rs                   # IrType -> Wasm type mapping
      layout.rs                    # Linear memory layout
      runtime.rs                   # Runtime import declarations
      wit/                         # WIT interface generation
        mod.rs
        types.rs                   # IrType -> WIT type
        interface.rs               # Module interface generation
```

### 3.3 Type Mapping

| IrType | Wasm Core | WIT | ABI |
|--------|-----------|-----|-----|
| Unit | (none) | (empty tuple) | - |
| Bool | i32 | bool | 0/1 |
| U8-U32 | i32 | u8-u32 | zero-extended |
| U64 | i64 | u64 | direct |
| I8-I32 | i32 | s8-s32 | sign-extended |
| I64 | i64 | s64 | direct |
| F32 | f32 | f32 | direct |
| F64 | f64 | f64 | direct |
| Int | i32,i32 | `list<u8>` | ptr+len (limbs) |
| String | i32,i32 | string | ptr+len (UTF-8) |
| `List<T>` | i32,i32 | `list<T>` | ptr+len |
| `Set<T>` | i32,i32 | `list<T>` | ptr+len (sorted) |
| `Map<K,V>` | i32,i32 | `list<tuple<K,V>>` | ptr+len |
| `Option<T>` | i32,T* | `option<T>` | discriminant+payload |
| `Result<T>` | i32,T*,i32,i32 | `result<T,string>` | disc+ok+err_ptr+len |
| Tuple(fields) | flattened | `tuple<...>` | flattened |
| Struct(fields) | flattened | record | flattened |
| Enum(variants) | i32,max_payload | variant | disc+max_payload |

**Flattening Rules:**
- Primitives flatten to 1-2 values
- Aggregates flatten recursively up to MAX_FLAT (16 values)
- Beyond MAX_FLAT, pass pointer to linear memory

### 3.4 Runtime Strategy

**Option 1: Embed Runtime in Component**
- Compile datalove-rt to Wasm (via wasm32-wasi target)
- Link into component as internal module
- Pro: Self-contained component
- Con: Larger binary, runtime duplication

**Option 2: Import Runtime from Host**
- Define runtime as WIT interface
- Host provides implementation
- Pro: Shared runtime, smaller components
- Con: Host must implement runtime

**Option 3: Minimal Wasm Runtime**
- Implement simplified runtime in Wasm
- Only essential operations (alloc, collections, bigint)
- Pro: Small, self-contained
- Con: Development effort, feature parity

**Recommended: Option 1** initially, with Option 2 as optimization path.

### 3.5 Memory Layout

Linear memory organization:
```
0x0000 - 0x0FFF: Reserved (null page)
0x1000 - heap_base: Static data (TyDescs, string literals)
heap_base - ...: Dynamic heap (managed by allocator)
```

Stack frame layout (same as Cranelift backend):
- Parameters passed via registers or linear memory
- Locals allocated in linear memory frame
- SSA values in Wasm locals (primitives) or memory (aggregates)

### 3.6 Implementation Phases

**Phase 1: Core Wasm Generation**
- [ ] Create `datalove-datafun-aot-wasm` crate
- [ ] Implement basic IrType -> Wasm type mapping
- [ ] Generate core wasm for simple functions (primitives only)
- [ ] Test with wasmtime

**Phase 2: Runtime Integration**
- [ ] Port datalove-rt to wasm32-wasi target
- [ ] Implement memory allocator for Wasm
- [ ] Add collection operations (list, set, map)
- [ ] Add bigint operations

**Phase 3: Full Type Support**
- [ ] Aggregate types (tuple, struct, enum)
- [ ] Option/Result handling
- [ ] String operations
- [ ] Linear type semantics (drop)

**Phase 4: Component Wrapping**
- [ ] Generate WIT interfaces from module signatures
- [ ] Use wasm-tools to create components
- [ ] Support module imports/exports
- [ ] Test component composition

**Phase 5: Testing Infrastructure**
- [ ] Unit tests for codegen
- [ ] Integration tests with wasmtime
- [ ] Cross-component tests
- [ ] Performance benchmarks

### 3.7 WIT Generation Example

Given a datafun module:
```datalove
fun add(a: u32, b: u32): u32
  ret a + b
end fun

fun greet(name: string): string
  ret "Hello, " + name
end fun
```

Generate WIT:
```wit
package datalove:example@0.1.0;

interface example {
    add: func(a: u32, b: u32) -> u32;
    greet: func(name: string) -> string;
}

world example-world {
    export example;
}
```

### 3.8 Codegen Sketch

```rust
// In datalove-datafun-aot-wasm/src/codegen/func.rs

impl FunctionCompiler {
    fn compile_instruction(&mut self, inst: &Instruction) {
        match inst {
            Instruction::Const { dest, value } => {
                let wasm_val = self.const_to_wasm(value);
                self.set_local(*dest, wasm_val);
            }
            Instruction::BinOp { dest, op, lhs, rhs } => {
                let l = self.get_operand(lhs);
                let r = self.get_operand(rhs);
                let result = match op {
                    BinOp::Add => self.builder.i32_add(l, r),
                    BinOp::Sub => self.builder.i32_sub(l, r),
                    // ...
                };
                self.set_local(*dest, result);
            }
            Instruction::Call { dest, func, args } => {
                let func_idx = self.resolve_func(func);
                let wasm_args: Vec<_> = args.iter()
                    .map(|a| self.get_operand(a))
                    .collect();
                let result = self.builder.call(func_idx, &wasm_args);
                self.set_local(*dest, result);
            }
            // ...
        }
    }
}
```

### 3.9 Challenges and Considerations

**Linear Memory Management:**
- Must implement or port allocator to Wasm
- GC not available in component model (yet)
- Manual drop semantics from IR must be preserved

**Bigint Support:**
- No native i128+ in Wasm
- Must use limb-based representation
- Consider using existing Wasm bigint library

**String Encoding:**
- Datafun uses UTF-8 internally
- Canonical ABI expects UTF-8
- String concatenation requires reallocation

**Error Handling:**
- Result types map cleanly to WIT result
- Early return via `!` operator needs unwinding
- Consider trap vs. return for unrecoverable errors

**Testing Strategy:**
- Roundtrip: IR -> Wasm -> Execute -> Verify
- Compare with interpreter results
- Fuzz testing with property-based tests

### 3.10 The Boundary: Ownership, Collections, Bigints

**Parameter modes** map onto WIT ownership:

| Mode | WIT |
|------|-----|
| `in` (move) | `T`, ownership transfers |
| `out` | return value |
| `ref` | `borrow<T>` |
| `mut` | no equivalent yet; WIT has no mutable borrow |

**Collections** cross one of two ways. Copied, a `[T]` becomes `list<T>`, a
set a sorted `list<T>` and a map a `list<tuple<K, V>>`: simple, but every
crossing copies. Or kept in their runtime representation and exposed as
resource handles, which shares memory and preserves identity under mutation:

```wit
resource list<T> {
    push: func(elem: T);
    get: func(idx: u32) -> option<T>;
    len: func() -> u32;
}
```

**`int`** (arbitrary precision) has no WIT counterpart. The options are an
opaque `resource int`, a `record int { limbs: list<u32>, negative: bool }`, or
a base-10 string. The table in 3.3 assumes limbs.

**Open questions:**

- The use case: embedding datafun in other Wasm hosts, calling components from
  datafun, or both.
- Interop: should a datafun component be callable from any WIT language, or is
  datafun-to-datafun the main case? The answer decides whether collections and
  `int` can stay resources or have to be copied into plain WIT types.
- Runtime linkage: one runtime per component, or a shared one imported as an
  interface (see 3.4).

## 4. References

- [WebAssembly Component Model](https://component-model.bytecodealliance.org/)
- [WIT Reference](https://component-model.bytecodealliance.org/design/wit.html)
- [Canonical ABI](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md)
- [wit-bindgen](https://github.com/bytecodealliance/wit-bindgen)
- [WASI 0.3 Overview](https://www.fermyon.com/blog/looking-ahead-to-wasip3)
- [WASI Interfaces](https://wasi.dev/interfaces)
