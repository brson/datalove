# Plan: AOT Cranelift Backend for Datafun

## Progress

### Phase 1: Foundation ✓ COMPLETE

**Commits:**
- `aec153f` - Initial AOT crate structure (types.rs, layout.rs, lib.rs)
- `e831d08` - Layout compatibility tests using interpreter's IrTyDescTable

**Created files:**
- `crates/datalove-datafun-aot-cranelift/Cargo.toml`
- `crates/datalove-datafun-aot-cranelift/src/lib.rs` - AotCompiler with host/target creation
- `crates/datalove-datafun-aot-cranelift/src/types.rs` - IR→Cranelift type mapping with rtdt-compatible layouts
- `crates/datalove-datafun-aot-cranelift/src/layout.rs` - FrameLayout computation

**Test suite:**
- `crates/datalove-datafun-compiler/tests/aot_layout_tests.rs` - 46 tests comparing AOT layouts against interpreter's TyDescTable

**Key decisions made:**
- All parameters passed by pointer (uniform ABI)
- Type layouts use exact rtdt sizes via `std::mem::size_of`
- Composite type layouts computed with matching algorithms

### Phase 2: Basic Codegen ✓ COMPLETE

**Commit:**
- `7afeb0b` - Core codegen driver with FunctionCompiler

**Created files:**
- `crates/datalove-datafun-aot-cranelift/src/codegen.rs` (~690 lines)

**Instructions implemented:**
- `Const` - scalar types (Bool, U8-U64, I8-I64, F32)
- `BinOp` - all operators (Add, Sub, Mul, Div, Mod, comparisons, logical, bitwise, shifts)
- `UnaryOp` - Neg, Not, BitNot
- `Copy`/`Move` - value copying
- `Pack`/`Unpack` - tuple/struct (scalar fields only)

**Terminators implemented:**
- `Return`, `Goto`, `Branch`

**Tests:** 5 codegen smoke tests (const, binop, comparison, unary, branch)

**Deferred:**
- `Int`/`String` constants (need runtime calls)
- Aggregate field packing (needs memcpy)
- Slots, params, function calls

### Phase 2.5: DebugLog Minimal Path ✓ COMPLETE

**Commits:**
- `2428d90` - AOT DebugLog compilation support
- `facdfce` - Link-and-run tests for debuglog

**New files:**
- `crates/datalove-datafun-aot-cranelift/src/runtime.rs` - Runtime function imports (dtlv_rti_*)
- `crates/datalove-datafun-aot-cranelift/src/tydesc_emit.rs` - TyDesc static data emission
- `crates/datalove-datafun-aot-cranelift/tests/aot_debuglog_tests.rs` - Compilation tests
- `crates/datalove-datafun-aot-cranelift/tests/aot_run_tests.rs` - Link-and-run tests

**Modified files:**
- `src/codegen.rs` - Added DebugLog instruction, value-in-memory spilling, rt_handle support
- `src/lib.rs` - Script unit compilation with entry point generation
- `crates/datalove-rt/Cargo.toml` - Added cdylib crate type for shared library linking

**Features implemented:**
- `DebugLog` instruction codegen
- Scalar value spilling to frame for pointer access
- Static TyDesc emission for scalar types
- Script unit → IrFunction conversion
- `main()` entry point generation (init, set_debug_mode, body, shutdown)
- Runtime function imports (init, shutdown, set_debug_mode, debuglog_local)
- Linking with libdatalove_rt.so
- Execution and output verification

**Tests:**
- 3 compilation tests (debuglog_i32, debuglog_bool, multiple_debuglogs)
- 3 link-and-run tests (compile → link → execute → verify stderr)

**What works:**
- Full pipeline: `debuglog @42` → object file → link → execute → prints "42" to stderr
- Multiple debuglogs in sequence
- Bool and i32 scalar types

**Known issues:**
- Linker warnings about DT_TEXTREL (not generating position-independent code yet)

### Phase 3: Full Worldfile Test Harness ✓ HARNESS COMPLETE

**Goal:** Build the test harness NOW. Failing tests guide what to implement next.

**Commits:**
- (pending) - AOT worldfile test harness with pipeline integration

**New files:**
- `crates/datalove-datafun/tests/aot_tests.rs` - Test harness using ExampleTestRunner
- `crates/datalove-datafun/tests/fixtures/aot/001_debuglog_i32.world` - ✓ passes
- `crates/datalove-datafun/tests/fixtures/aot/002_debuglog_bool.world` - ✓ passes
- `crates/datalove-datafun/tests/fixtures/aot/003_arithmetic.world` - AOT compile error (BinOp verifier issue)

**Modified files:**
- `crates/datalove-datafun/Cargo.toml` - Added aot-cranelift and tempfile dev-dependencies
- `crates/datalove-datafun/src/pipeline.rs` - Added `lower_fragment()` and `lower_expr()` methods

**Pipeline implemented:**
```
.world file
    → parse_worldfile_sections()
    → ModuleCompilationPipeline::compile()
    → ScriptCompilationContext::lower_fragment/lower_expr() (IR extraction)
    → AotCompiler::compile_script_unit()
    → link with libdatalove_rt.so
    → execute, capture stderr
    → compare to .out.expected
```

**Error categories tracked:**
- PARSE_ERROR / TYPECHECK_ERROR / LOWER_ERROR - existing pipeline errors
- AOT_COMPILE_ERROR - unsupported IR or codegen bugs
- LINK_ERROR - missing runtime symbol
- RUNTIME_ERROR - crash or bad exit code

**Current status:**
- Tests 001, 002, 003 pass: full compile → link → run → capture output
- Test 004 shows clear error for unsupported bigint arithmetic
- Investigation revealed: `@10 + @32` produces Int (bigint), not U32
- Fixed codegen to detect and report unsupported bigint BinOp
- Next step: Add more fixtures to drive feature development

**3.5: Incremental Feature Development**
Run harness → see what fails → implement that feature → repeat.
Each "unsupported instruction" error becomes the next work item.

### Phase 3.5: Infrastructure Improvements ✓ COMPLETE

**Commits:**
- `5f78320` - Emit TyDescs upfront from full module graph
- `84923ca` - Move FunctionRegistry from interp to IR crate
- `43b78ab` - Use runtime TyDesc layout instead of hardcoded constants
- `16eee19` - Implement Call instruction with implicit rt_handle threading

**TyDesc improvements:**
- Collect types from full module graph (not just single script unit)
- Added `collect_types_from_functions()` helper for world compilation
- Replaced hardcoded layout constants with computed values from rtdt
- Use `size_of`, `align_of`, `offset_of!` on `rtdt::TyDesc`
- Import `TyTag` enum directly instead of duplicating values

**FunctionRegistry shared:**
- Moved `FunctionRegistry` from interp crate to IR crate
- Both interpreter and AOT can now use it without circular deps
- Changed `external_function()` to `get_external_function()` returning `Option`

**Call instruction with rt_handle threading:**
- All functions have implicit rt_handle as first parameter (codegen-only)
- IR stays clean - no rt_handle in `param_types`
- `build_signature` prepends PTR_TYPE for rt_handle
- Entry block extracts rt_handle, user params start at index 1
- `compile_call` threads rt_handle to callees
- Local function lookup via `resolve_func_ref`
- External/Module calls return unsupported (need registry integration)

**Remaining known issues:**
1. Wasteful temp stack slots - creates new slot per spill in `get_operand_ptr`
2. No position-independent code - linker warns about DT_TEXTREL
3. TryReturn/UnitEnd/UnitEarlyReturn terminators not implemented
4. Many instructions not implemented (SlotLoad, SlotStore, Drop, Clone, etc.)
5. No Slot/ExternalValue/ExternalSlot operand support
6. No aggregate fields in Pack (needs memcpy)
7. Int/String constants require runtime calls
8. BinOp with bigint not implemented

### Phases 4-8: Feature Development (Test-Driven)

Order TBD based on what the test harness reveals. Expected needs:

**Slots & Variables**
- `SlotStore`, `SlotLoad` instructions
- Mutable variable support

**Function Calls** (partially done)
- `Call` instruction - ✓ implemented for local functions
- rt_handle threading - ✓ implicit first param
- Two-pass compilation for mutual recursion
- External/Module function calls via FunctionRegistry
- Parameter passing modes (In/Ref/Mut/Out)

**Option/Result Types**
- `WrapSome`, `WrapOk`, `WrapErr`
- `UnwrapOption`, `UnwrapResult`
- `TryReturn` terminator

**Runtime Types**
- `Int`/`String` constants via runtime calls
- Collection operations (List, Set, Map)

**Advanced**
- Drop scheduling
- Cross-unit references
- Phi nodes for loops

---

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

**Implicit rt_handle as first parameter** - all functions receive runtime handle:
- Codegen adds rt_handle (PTR_TYPE) as first param to all signatures
- IR stays clean (no rt_handle in `param_types`)
- Call instruction threads rt_handle to callees
- Interpreter doesn't need this (has direct runtime access)

**All user parameters passed by pointer** - uniform ABI:
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

### Phase 1: Foundation ✓
1. Create crate structure with Cargo.toml
2. Implement types.rs - basic type mapping
3. Implement layout.rs - frame layout computation (port from interp)
4. Set up Cranelift module infrastructure

### Phase 2: Basic Codegen ✓
1. Implement single-block functions (no control flow)
2. Constants, arithmetic, simple binops
3. Pack/Unpack for tuples and structs
4. Return values, Goto, Branch terminators
5. DebugLog instruction with runtime calls

### Phase 3: Full Worldfile Test Harness
1. Add `datalove-datafun-aot-cranelift` as dev-dependency to `datalove-datafun`
2. Create `tests/aot_tests.rs` using ExampleTestRunner
3. Integrate with existing pipeline (parse → typecheck → lower → AOT → link → run)
4. Initial fixtures for debuglog + arithmetic
5. Error categorization (AOT_COMPILE_ERROR guides next features)

### Phase 4+: Test-Driven Feature Development
Order determined by failing tests. Expected features:

**Slots & Variables**
- SlotStore, SlotLoad instructions
- Mutable variable support

**Function Calls**
- Call instruction with ABI setup
- Parameter modes (In/Ref/Mut/Out)
- Local and cross-module calls

**Option/Result**
- WrapSome/WrapOk/WrapErr
- UnwrapOption/UnwrapResult
- TryReturn terminator

**Runtime Types**
- Int/String constants via runtime
- Collection operations

**Advanced**
- Drop scheduling
- Phi nodes for loops
- Cross-unit references

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

**Layout compatibility (implemented):**
- `aot_layout_tests.rs` compares AOT `ir_type_to_cranelift().layout()` against interpreter's `IrTyDescTable.get_or_create()` → `TyDescRef.size()/align()`
- 46 tests covering scalars, runtime types, composites, and deeply nested types
- Guarantees ABI compatibility between AOT and interpreter

**Future testing:**
1. Port existing interp test fixtures to aot tests
2. Compare output with interpreter for correctness
3. Add codegen-specific tests (register allocation, stack layout)
4. Benchmark against interpreter

**Potential enhancement:** Similar test asserting interpreter actually uses rtdt-computed layouts (would give three-way agreement: AOT ↔ interp ↔ rtdt)

## Design Decisions (Confirmed)

1. **Object files first** - Use cranelift-object for AOT compilation to .o files, JIT can be added later
2. **x86-64 only** - Target x86-64 Linux initially
3. **Separate entry point** - New API alongside interp, user chooses execution method
