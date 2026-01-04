# Plan: AOT Cranelift Backend for Datafun

## Overview

Add a new crate `datalove-datafun-aot-cranelift` as a peer to parser/tycheck/interp that compiles IR directly to native code using Cranelift.

## Crate Architecture

```
datalove-datafun-aot-cranelift/
├── Cargo.toml
└── src/
    ├── lib.rs              # Public API: compile_function, compile_module, etc.
    ├── codegen.rs          # Core codegen driver
    ├── types.rs            # IR type → Cranelift type mapping
    ├── layout.rs           # Value/slot layout (mirrors interp/layout.rs)
    ├── abi.rs              # Calling convention, parameter modes
    ├── instructions.rs     # IR instruction → Cranelift IR translation
    ├── terminators.rs      # Control flow terminators
    ├── runtime.rs          # Runtime function imports (dtlv_rti_*)
    └── module.rs           # Module-level compilation
```

**Dependencies:**
- `datalove-datafun-ir` - IR types (primary input)
- `datalove-rtdt` - Runtime type layouts (Int, String, List, etc.) - **must match exactly**
- `datalove-rt` - Runtime function symbols for linking
- `cranelift-codegen`, `cranelift-frontend`, `cranelift-module`, `cranelift-object`
- `target-lexicon` - Target triple handling

**No dependencies on:** parser, tycheck, interp, ast

## Key Design Decisions

### 1. Type Mapping (types.rs)

Must match rtdt layouts exactly. Reference: `crates/datalove-rtdt/src/lib.rs`

**Scalars (repr(transparent)):**
| IR Type | Cranelift | Size |
|---------|-----------|------|
| Unit | - | 0 |
| Bool | I8 | 1 |
| U8/I8 | I8 | 1 |
| U16/I16 | I16 | 2 |
| U32/I32 | I32 | 4 |
| U64/I64 | I64 | 8 |
| F32 | F32 | 4 |

**Runtime types (repr(C) structs):**
| IR Type | Layout | Size | Notes |
|---------|--------|------|-------|
| Int | `{ data: *u32, size_and_sign: i32, capacity: u32 }` | 16 | Bigint, GMP-style |
| String | `{ data: *u8, size: u32, capacity: u32 }` | 16 | Fat pointer |
| List | `{ data: *u8, size: u32, capacity: u32 }` | 16 | Fat pointer, element-typed |
| Map | `{ root: *MapNode, len: u32 }` | 12 | B-tree, order=6 |
| Set | `{ root: *SetNode, len: u32 }` | 12 | B-tree, order=6 |
| Tensor | `{ ptr_base, cap, offset, shape*, strides*, layout }` | 40 | Multi-pointer |
| Data | `{ primary: *(), secondary: *() }` | 16 | Tagged pointer pair |
| Error | `{ primary: *(), secondary: *() }` | 16 | Tagged pointer pair |

**Composite types (computed layout):**
| IR Type | Layout | Notes |
|---------|--------|-------|
| Tuple | Fields at computed offsets | Use rtdt layout algorithm |
| Struct | Named fields sorted, at offsets | Use rtdt layout algorithm |
| Enum | `{ discriminant: u32, payload: max_variant }` | Discriminant + largest variant |
| Option | `{ tag: u8, [padding], payload: T }` | None=1, Some=2 |
| Result | `{ tag: u8, [padding], payload: max(T, Error) }` | Ok=1, Err=2 |

**Data/Error tagged pointer encoding (anypack):**
- Tag in lower 3 bits of primary pointer
- Tag 0: TwoPointers (tydesc*, value*)
- Tag 1: SmallImmediate (61-bit value, TyTag)
- Tag 4: InlineWithTyDesc (tydesc* tagged, 64-bit immediate)

### 2. Value Layout (layout.rs)

Port or share the interpreter's `IrLayout` computation to ensure identical layouts:
- Reference: `crates/datalove-datafun-interp/src/layout.rs`
- Reference: `crates/datalove-rtdt/src/layout.rs` (type size/align computation)
- Compute frame size, alignment
- Assign stack slot offsets for each ValueId/SlotId
- Multi-word types (String, List, Int, etc.) always in memory, accessed via stack slots
- Small scalars can be kept in Cranelift SSA values (virtual registers)

### 3. Calling Convention (abi.rs)

**All parameters passed by pointer** - uniform ABI:
- **In**: Pass pointer, callee takes ownership (destroys at end)
- **Out**: Pass pointer to uninitialized slot, callee writes
- **Ref**: Pass pointer, read-only borrow
- **Mut**: Pass pointer, read-write borrow

Return value: pointer to caller-allocated space (caller provides return slot).

### 4. Instruction Translation (instructions.rs)

Direct mapping from IR instructions to Cranelift:

| IR Instruction | Cranelift |
|---------------|-----------|
| Const | iconst, f32const, call runtime for Int/String |
| BinOp (Add, Sub, etc.) | iadd, isub, imul, etc. |
| BinOpChecked | iadd + overflow flag check |
| UnaryOp (Neg, Not) | ineg, bnot |
| Copy | load + store (or register copy) |
| Move | load + store + mark dropped |
| Call | call with ABI setup |
| Pack | store fields at computed offsets |
| Unpack | load fields from offsets |
| FieldAccess | load from field offset |
| WrapSome/WrapOk/WrapErr | store discriminant + payload |
| UnwrapOption/UnwrapResult | load discriminant, branch, load payload |
| SlotStore | store to slot offset, call destructor if needed |
| SlotLoad | load from slot, mark borrowed |
| ListNew/SetNew/MapNew | call runtime constructor |
| Phi | handled by Cranelift SSA builder |
| Drop | call dtlv_rti_any_destroy_local |

### 5. Control Flow (terminators.rs)

| Terminator | Cranelift |
|-----------|-----------|
| Goto(block) | jump block |
| Branch { cond, then, else } | brif cond, then, else |
| Return { value } | return value |
| TryReturn | conditional return (early exit) |
| UnitEnd | return result for script units |

Phi nodes: Cranelift's SSA builder handles block parameters automatically.

### 6. Runtime Integration (runtime.rs)

Import runtime functions as external symbols:
- `dtlv_rti_any_destroy_local` - destructor
- `dtlv_rti_int_*` - bigint operations
- `dtlv_rti_string_*` - string operations
- `dtlv_rti_list_*`, `dtlv_rti_set_*`, `dtlv_rti_map_*` - collections
- Type descriptor access for dynamic operations

### 7. Module Compilation (module.rs)

Primary mode: **cranelift-object** for AOT compilation to .o files

```rust
pub fn compile_module(module: &IrModule, target: &TargetIsa) -> ObjectProduct;
pub fn compile_script_unit(unit: &IrScriptUnit, target: &TargetIsa) -> ObjectProduct;
```

The ObjectProduct can be written to .o files, then linked with the runtime library.

## Implementation Phases

### Phase 1: Foundation
1. Create crate structure with Cargo.toml
2. Implement types.rs - basic type mapping
3. Implement layout.rs - frame layout computation (port from interp)
4. Set up Cranelift module infrastructure

### Phase 2: Basic Codegen
1. Implement single-block functions (no control flow)
2. Constants, arithmetic, simple binops
3. Pack/Unpack for tuples and structs
4. Return values

### Phase 3: Control Flow
1. Multi-block functions with Goto
2. Branch terminator
3. Phi node handling via Cranelift SSA builder
4. Loops (while)

### Phase 4: Function Calls
1. In parameter mode (copy and non-copy)
2. Ref/Mut parameter modes
3. Out parameter mode
4. Local function calls
5. External function calls (cross-module)

### Phase 5: Advanced Features
1. Option/Result types
2. Enum variants
3. Try/early return
4. Drop scheduling and destructor calls

### Phase 6: Runtime Integration
1. String operations (runtime calls)
2. Int (bigint) operations
3. Collection operations (List, Set, Map)
4. Tensor operations

### Phase 7: Script Units & Linking
1. Script unit compilation to object files
2. Cross-unit value/slot access via relocations
3. Linker integration for final executable

### Phase 8: Integration & Testing
1. Add as dependency to datalove-datafun-compiler
2. Integration with datalove-datafun pipeline
3. Test harness mirroring interp tests
4. Performance benchmarks

## Files to Modify

**New crate:**
- `crates/datalove-datafun-aot-cranelift/Cargo.toml`
- `crates/datalove-datafun-aot-cranelift/src/*.rs`

**Workspace:**
- `Cargo.toml` - add to members

**Integration (later phases):**
- `crates/datalove-datafun-compiler/Cargo.toml` - add dependency
- `crates/datalove-datafun-compiler/src/lib.rs` - export aot module
- `crates/datalove-datafun/src/pipeline.rs` - add aot compilation path

## Key Reference Files

| Purpose | File |
|---------|------|
| IR definition | `crates/datalove-datafun-ir/src/lib.rs` |
| **Runtime type layouts** | `crates/datalove-rtdt/src/lib.rs` |
| **Type layout computation** | `crates/datalove-rtdt/src/layout.rs` |
| **Tagged pointer encoding** | `crates/datalove-rtdt/src/anypack.rs` |
| Frame layout | `crates/datalove-datafun-interp/src/layout.rs` |
| Instruction execution | `crates/datalove-datafun-interp/src/lib.rs` |
| Value representation | `crates/datalove-datafun-interp/src/value.rs` |
| Type descriptors | `crates/datalove-datafun-interp/src/tydesc.rs` |
| Runtime API | `crates/datalove-rt/src/lib.rs` |

## Testing Strategy

1. Port existing interp test fixtures to aot tests
2. Compare output with interpreter for correctness
3. Add codegen-specific tests (register allocation, stack layout)
4. Benchmark against interpreter

## Design Decisions (Confirmed)

1. **Object files first** - Use cranelift-object for AOT compilation to .o files, JIT can be added later
2. **x86-64 only** - Target x86-64 Linux initially
3. **Separate entry point** - New API alongside interp, user chooses execution method
