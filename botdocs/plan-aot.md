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
- `crates/datalove-datafun/tests/fixtures/aot/003_multiple_values.world` - ✓ passes
- `crates/datalove-datafun/tests/fixtures/aot/004_bigint_add_unsupported.world` - Shows clear error for unsupported bigint
- `crates/datalove-datafun/tests/fixtures/aot/005_variable_basic.world` - ✓ passes (mutable variables)

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
- `cd11c73` - Enable position-independent code in AOT compiler
- `e5f7d84` - Implement SlotStore, SlotLoad, and Slot operand for mutable variables

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

**Position-independent code:**
- Added `is_pic=true` to Cranelift settings
- Eliminates DT_TEXTREL linker warnings

**Mutable variables (slots):**
- `SlotStore` instruction stores values to slot offsets in frame
- `SlotLoad` instruction loads values from slot offsets
- `Operand::Slot` support in `get_operand_value` and `get_operand_ptr`
- Handles both scalar (load/store) and aggregate (memcpy) types
- Test: `005_variable_basic.world` demonstrates var declaration, mutation, access

**Remaining known issues:**
1. Wasteful temp stack slots - creates new slot per spill in `get_operand_ptr`
2. UnitEarlyReturn terminator not implemented
3. Clone instruction not implemented
4. No aggregate fields in Pack (needs memcpy)
5. BinOp with bigint not implemented
6. Parameter passing modes (In/Ref/Mut/Out) not implemented

**Not applicable for AOT:**
- ExternalValue/ExternalSlot operands - these are for REPL cross-unit references; AOT does whole-world compilation

**Resolved in later phases:**
- Drop instruction - ✓ Phase 3.7
- Int/String constants - ✓ Phase 3.7
- UnitEnd terminator - ✓ Phase 3.8 (with script-level drops)
- TryReturn terminator - ✓ Phase 3.11
- Option/Result types - ✓ Phase 3.11

### Phase 3.6: Module Function Calls ✓ COMPLETE

**Commit:**
- (pending) - Module function calls via three-pass compilation

**Modified files:**
- `crates/datalove-datafun-aot-cranelift/src/codegen.rs`
  - Added `module_funcs` and `registry` fields to FunctionCompiler
  - Added `set_module_funcs()` method
  - Implemented `FuncRef::Module` case in `resolve_func_ref`
- `crates/datalove-datafun-aot-cranelift/src/lib.rs`
  - Implemented three-pass compilation in `compile_script_unit_with_types`
  - Pass 1: Declare local functions
  - Pass 2: Declare module functions (named `__mod_{module_id}_{func_name}`)
  - Pass 3a: Compile local functions with both maps
  - Pass 3b: Compile module functions with both maps
- `crates/datalove-datafun-ir/src/registry.rs`
  - Added `iter_module_functions_with_ids()` method

**New test:**
- `crates/datalove-datafun/tests/fixtures/aot/007_module_function_call.world`
  - Module with `identity(x: u32): u32` function
  - Script calls module function, output verified as `@42`

**What works:**
- Full pipeline: module function call → AOT → link → execute → correct output
- Three-pass compilation handles mutual recursion between local and module functions
- Module functions get unique names `__mod_0_identity` etc.

### Phase 3.7: Linear Type Construction and Destruction ✓ COMPLETE

**Commits:**
- `d121550` - AOT linear type construction and destruction
- `9e4d9cc` - Bless test expected outputs

**Modified files:**
- `crates/datalove-datafun-aot-cranelift/src/codegen.rs`
  - Added `Drop` instruction: calls `dtlv_rti_any_destroy_local(rt_handle, tydesc, value_ptr)`
  - Added `Const` for `Int` type: calls `dtlv_rti_int_from_i64(rt_handle, i64_value, dest_ptr)`
  - Added `Const` for `String` type: emits inline data, calls `dtlv_rti_string_from_utf8_unchecked(rt_handle, ptr, len, dest_ptr)`
- `crates/datalove-datafun-aot-cranelift/src/runtime.rs`
  - Added `dtlv_rti_any_destroy_local` import
  - Added `dtlv_rti_int_from_i64` import
  - Added `dtlv_rti_string_from_utf8_unchecked` import
- `crates/datalove-datafun-aot-cranelift/src/tydesc_emit.rs`
  - Added `Int` and `String` TyDesc emission
- `crates/datalove-datafun/tests/fixtures/aot/010_function_with_drops.world` - New test
- `crates/datalove-datafun/tests/fixtures/aot/011_function_returns_string.world` - New test

**Features implemented:**
- Linear type destruction via `Drop` instruction calling runtime
- `Int` constant construction via runtime call
- `String` constant construction: inline data emission + runtime call
- TyDesc emission for `Int` and `String` types

**What works:**
- Functions with linear types (Int, String) that require destruction
- Precise drops within function bodies (already via drop_analysis)
- Function return values for linear types

**Known issue addressed later:**
- Script-level bindings (not in functions) leaked because `UnitEnd` didn't emit drops

### Phase 3.8: Precise Drop Analysis for AOT Script-Level Bindings ✓ COMPLETE

**Commit:**
- `39d08b3` - Add precise drop analysis for AOT script-level bindings

**Problem:**
Script-level linear types leaked because `UnitEnd` didn't emit drops. The interpreter uses dynamic init-flag tracking for REPL incremental compilation, but AOT handles only a single script unit where all bindings should be cleaned up.

**Solution:**
Leverage existing `drop_analysis` module with a `for_aot` parameter instead of runtime tracking.

**Modified files:**
- `crates/datalove-datafun-compiler/src/drop_analysis.rs`
  - Added `for_aot: bool` parameter to `analyze_script_statements`
  - Added `unit_end: Vec<BindingId>` field to `ScriptDropAnalysis`
  - When `for_aot=true`, uses `ScopeKind::Function` so top-level bindings are included in drops

- `crates/datalove-datafun-compiler/src/lower/context.rs`
  - Added `unit_end_drops: Vec<BindingId>` field to `LowerCtx`
  - Added `set_unit_end_drops()` and `emit_unit_end_drops()` methods

- `crates/datalove-datafun-compiler/src/lower/script.rs`
  - Added `for_aot: bool` parameter to `lower_script_fragment_raw`, `lower_script_unit`, `lower_script_expr`
  - Calls `ctx.emit_unit_end_drops()` before `UnitEnd` terminator

- `crates/datalove-datafun/src/pipeline.rs`
  - Added `lower_fragment_for_aot()` and `lower_expr_for_aot()` public methods
  - REPL methods use `for_aot=false`, AOT methods use `for_aot=true`

- `crates/datalove-datafun/tests/aot_tests.rs`
  - Changed to use `lower_fragment_for_aot()` and `lower_expr_for_aot()`

- `crates/datalove-datafun/tests/ir_lower_script_tests.rs`
  - Updated to pass `for_aot=false` (tests REPL behavior)

**Re-enabled tests:**
- `008_int_const.world` - Int constant with proper cleanup
- `009_string_const.world` - String constant with proper cleanup

**IR output change:**
Before: no drops at unit end
After: `drop s0` emitted before `unit_end`

**What works:**
- Full pipeline with leak checking passes: `DATALOVE_LEAK_CHECK=panic-backtrace just test`
- Script-level Int and String bindings properly destroyed at unit end
- All 11 AOT test fixtures pass

### Phase 3.9: Collection Type Construction ✓ COMPLETE

**Commit:**
- (pending) - Collection type construction and destruction

**Modified files:**
- `crates/datalove-datafun-aot-cranelift/src/runtime.rs`
  - Added `dtlv_rti_list_create_local`, `dtlv_rti_list_push_local`
  - Added `dtlv_rti_btreeset_create_local`, `dtlv_rti_btreeset_insert_local`
  - Added `dtlv_rti_btreemap_create_local`, `dtlv_rti_btreemap_insert_local`

- `crates/datalove-datafun-aot-cranelift/src/tydesc_emit.rs`
  - Added `emit_list_tydesc`, `emit_set_tydesc`, `emit_map_tydesc` methods
  - TyDescs for collections include relocations pointing to element type TyDescs
  - Updated `can_emit` to recursively check collection element types

- `crates/datalove-datafun-aot-cranelift/src/codegen.rs`
  - Added `compile_list_new` - creates empty list, pushes each element
  - Added `compile_set_new` - creates empty set, inserts each element
  - Added `compile_map_new` - creates empty map, inserts each key-value pair

**New test fixtures:**
- `012_empty_list.world` - empty list
- `013_list_with_elements.world` - list with elements `[@1, @2, @3]`
- `014_empty_set.world` - empty set
- `015_set_with_elements.world` - set with elements `{@10, @20, @30}`
- `016_empty_map.world` - empty map
- `017_map_with_entries.world` - map with entries `{@1 = @100, @2 = @200}`

**What works:**
- `ListNew` instruction: creates empty list, pushes elements via runtime calls
- `SetNew` instruction: creates empty set, inserts elements via runtime calls
- `MapNew` instruction: creates empty map, inserts key-value pairs via runtime calls
- TyDesc emission with relocations for element type pointers
- Collection destruction via existing `Drop` instruction
- All 17 AOT test fixtures pass with leak checking

### Phase 3.10: Code Organization and Documentation Cleanup ✓ COMPLETE

**Goal:** Clean up code organization and documentation for maintainability.

**Modified files:**

- `crates/datalove-datafun-aot-cranelift/src/lib.rs`
  - Improved module doc with architecture overview, key types, and generated code structure
  - Added doc comments to each submodule declaration
  - Removed unused `tydesc_table` field from `AotCompiler`
  - Deduplicated `new_for_host()` to call `new_for_target(Triple::host())`

- `crates/datalove-datafun-aot-cranelift/src/layout.rs`
  - Removed vestigial `TyDescTable` struct (was never used)
  - Now imports `align_up` from `types` module instead of duplicating it

- `crates/datalove-datafun-aot-cranelift/src/codegen/mod.rs`
  - Improved module doc explaining submodule organization, value representation, rt_handle threading
  - Added doc comments to each submodule declaration

**Code organization:**
```
lib.rs           - Entry point: AotCompiler, three-pass compilation
├── types.rs     - IR→Cranelift type mapping, TypeLayout, CraneliftRepr
├── layout.rs    - FrameLayout computation for stack slots
├── runtime.rs   - RuntimeImports: dtlv_rti_* function declarations
├── tydesc_emit.rs - TyDescEmitter: static TyDesc data emission
└── codegen/
    ├── mod.rs       - FunctionCompiler driver
    ├── ops.rs       - BinOp, UnaryOp
    ├── constants.rs - Const (scalar, Int, String)
    ├── collections.rs - ListNew, SetNew, MapNew
    ├── aggregates.rs - Pack, Unpack, Copy
    ├── calls.rs     - Call instruction
    ├── options.rs   - WrapSome, WrapNone, WrapOk, WrapErr, UnwrapOption, UnwrapResult
    ├── slots.rs     - SlotStore, SlotLoad
    ├── runtime.rs   - DebugLog, Drop
    └── terminators.rs - Return, Goto, Branch, TryReturn
```

**Remaining scaffolding (left for future use):**
- `registry`, `slot_vars`, `next_var`, `alloc_var` in FunctionCompiler
  (infrastructure for potential Cranelift Variable-based slot approach)

### Phase 3.11: Option and Result Types ✓ COMPLETE

**Goal:** Support Option<T> and Result<T> types in AOT compilation.

**New files:**
- `crates/datalove-datafun-aot-cranelift/src/codegen/options.rs`
  - `compile_wrap_some()`: Create Option::Some(value) with tag=2, payload copy
  - `compile_wrap_none()`: Create Option::None with tag=1
  - `compile_wrap_ok()`: Create Result::Ok(value) with tag=1, payload copy
  - `compile_wrap_err()`: Create Result::Err(error) with tag=2, payload copy
  - `compile_unwrap_option()`: Read tag, extract payload, return (value, is_some)
  - `compile_unwrap_result()`: Read tag, extract payload, return (ok, err, is_ok)

**Modified files:**
- `crates/datalove-datafun-aot-cranelift/src/tydesc_emit.rs`
  - Added `emit_option_tydesc()`: Emit Option TyDesc with inner type reference
  - Added `emit_result_tydesc()`: Emit Result TyDesc with ok type reference
  - Updated `can_emit()` to include Option and Result types

- `crates/datalove-datafun-aot-cranelift/src/codegen/mod.rs`
  - Added `mod options;`
  - Added dispatch for WrapSome, WrapNone, WrapOk, WrapErr, UnwrapOption, UnwrapResult
  - Fixed aggregate return types: Unit returns nothing, other aggregates return pointer

- `crates/datalove-datafun-aot-cranelift/src/codegen/terminators.rs`
  - Added `TryReturn` terminator (same as Return for functions returning Option/Result)

**New tests:**
- `018_option_some.world`: Function returns Option<u32> with Some, unwrap via if-option
- `019_option_none.world`: Function returns Option<u32> with None, unwrap via if-option
- `020_result_ok.world`: Function returns Result<u32> with Ok, unwrap via try operator (!)

**Memory layout:**
- Option<T>: tag (u8, None=1, Some=2) + padding + payload at align_up(1, align(T))
- Result<T>: tag (u8, Ok=1, Err=2) + padding + max(sizeof(T), sizeof(Error)) payload

**Known limitation:**
- if-result with else clause generates IR with unreachable blocks that have invalid return types
  (this is an IR lowering issue, not an AOT issue)

### Phases 4-8: Feature Development (Test-Driven)

Order TBD based on what the test harness reveals. Expected needs:

**Slots & Variables** ✓ COMPLETE
- `SlotStore`, `SlotLoad` instructions - ✓
- `Operand::Slot` support - ✓
- Mutable variable support - ✓

**Function Calls** ✓ COMPLETE
- `Call` instruction - ✓ implemented for local and module functions
- rt_handle threading - ✓ implicit first param
- Two-pass compilation for local functions - ✓
- Three-pass compilation for module functions - ✓
- External script unit calls - not needed (single script units only)
- Parameter passing modes (In/Ref/Mut/Out) - not yet implemented

**Linear Types** ✓ COMPLETE
- `Drop` instruction - ✓ calls `dtlv_rti_any_destroy_local`
- `Int` constants via `dtlv_rti_int_from_i64` - ✓
- `String` constants via `dtlv_rti_string_from_utf8_unchecked` - ✓
- Script-level drops via `for_aot` drop analysis - ✓

**Option/Result Types**
- `WrapSome`, `WrapOk`, `WrapErr`
- `UnwrapOption`, `UnwrapResult`
- `TryReturn` terminator

**Runtime Types** ✓ COMPLETE
- `Int`/`String` constants - ✓
- Collection construction (List, Set, Map) - ✓
- Collection destruction via `Drop` - ✓

**Advanced**
- Cross-unit references
- Phi nodes for loops
- Clone instruction

---

## Overview

Add a new crate `datalove-datafun-aot-cranelift` as a peer to parser/tycheck/interp that compiles IR directly to native code using Cranelift.

## Crate Architecture

```
datalove-datafun-aot-cranelift/
├── Cargo.toml
└── src/
    ├── lib.rs              # AotCompiler: entry point, three-pass compilation
    ├── types.rs            # IR→Cranelift type mapping, TypeLayout
    ├── layout.rs           # FrameLayout: stack slot offset computation
    ├── runtime.rs          # RuntimeImports: dtlv_rti_* declarations
    ├── tydesc_emit.rs      # TyDescEmitter: static TyDesc data emission
    └── codegen/
        ├── mod.rs          # FunctionCompiler: IR→Cranelift translation
        ├── ops.rs          # BinOp, UnaryOp
        ├── constants.rs    # Const (scalar, Int, String)
        ├── collections.rs  # ListNew, SetNew, MapNew
        ├── aggregates.rs   # Pack, Unpack, Copy
        ├── calls.rs        # Call instruction
        ├── options.rs      # WrapSome, WrapNone, WrapOk, WrapErr, UnwrapOption, UnwrapResult
        ├── slots.rs        # SlotStore, SlotLoad
        ├── runtime.rs      # DebugLog, Drop
        └── terminators.rs  # Return, Goto, Branch, TryReturn
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
