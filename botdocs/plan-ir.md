# Fully CFG-Driven Interpreter with SSA IR

## Goal

Transform the interpreter from hybrid tree-walking to fully CFG-driven execution with flat SSA instructions. IR is SSA-form for clean backend codegen; interpreter uses uniform slot-based execution.

## SSA vs Slots

| Construct | IR Representation | Interpreter | LLVM/Cranelift |
|-----------|-------------------|-------------|----------------|
| Expression temps | SSA Value | frame slot | register |
| `let` bindings | SSA Value | frame slot | register |
| `var` bindings | Mutable Slot | frame slot | alloca/StackSlot |
| Function params | SSA Value | frame slot | register |

SSA values are defined once; mutable slots can be reassigned. Backends emit registers for SSA, stack for slots. Interpreter treats both as frame offsets.

## Current Datalove Architecture

```
CFG Block
  +-- Statement (Let/Var/Set/Ret/...)
        +-- ExprFun tree (recursive)
              +-- eval_expression_frame() walks tree
```

- Statements contain nested expression trees
- Recursive `eval_expression_frame` evaluates expressions
- Destination-passing style (DPS)
- EvalResult for early returns

## Proposed Architecture

```
CFG Block
  +-- Vec<Instruction>  (flat SSA sequence)
        +-- execute_instruction() (no recursion)
  +-- Terminator
```

### Core Types

```rust
/// SSA value - defined exactly once, immutable.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueId(pub u32);

/// Mutable slot - for var bindings, can be reassigned.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct SlotId(pub u32);

/// Operand - either SSA value or mutable slot.
#[derive(Copy, Clone, Debug)]
pub enum Operand {
    Value(ValueId),
    Slot(SlotId),
}
```

### Instruction Enum

```rust
pub enum Instruction {
    // SSA-producing instructions (dest is always ValueId)
    Const { dest: ValueId, value: ConstValue },
    Copy { dest: ValueId, src: Operand },
    Move { dest: ValueId, src: Operand },

    // Arithmetic
    BinOp { dest: ValueId, op: BinOp, lhs: Operand, rhs: Operand },
    UnaryOp { dest: ValueId, op: UnaryOp, operand: Operand },

    // Checked arithmetic (produces value + overflow flag)
    BinOpChecked { dest: ValueId, overflow: ValueId, op: BinOp, lhs: Operand, rhs: Operand },
    UnaryOpChecked { dest: ValueId, overflow: ValueId, op: UnaryOp, operand: Operand },

    // Function calls
    Call { dest: ValueId, func: FuncId, args: Vec<Operand> },

    // Struct/tuple operations
    Pack { dest: ValueId, ty: TypeId, fields: Vec<Operand> },
    Unpack { dests: Vec<ValueId>, src: Operand },
    FieldAccess { dest: ValueId, base: Operand, field: FieldId },
    TupleIndex { dest: ValueId, base: Operand, index: u32 },

    // Option/Result operations
    WrapSome { dest: ValueId, inner: Operand },
    WrapOk { dest: ValueId, inner: Operand },
    WrapErr { dest: ValueId, inner: Operand },
    WrapNone { dest: ValueId },
    UnwrapOption { dest: ValueId, is_some: ValueId, src: Operand },
    UnwrapResult { dest: ValueId, is_ok: ValueId, src: Operand },

    // Collections
    ListNew { dest: ValueId, elements: Vec<Operand> },
    SetNew { dest: ValueId, elements: Vec<Operand> },
    MapNew { dest: ValueId, entries: Vec<(Operand, Operand)> },

    // Slot operations (for var bindings)
    SlotStore { slot: SlotId, value: Operand },
    SlotLoad { dest: ValueId, slot: SlotId },

    // Control flow merge
    Phi { dest: ValueId, incoming: Vec<(BlockId, Operand)> },

    // Memory
    Drop { operand: Operand },

    Nop,
}
```

### Terminator

```rust
pub enum Terminator {
    Goto(BlockId),
    Branch { cond: Operand, then_block: BlockId, else_block: BlockId },
    Return { value: Option<Operand> },
    TryReturn { value: Option<Operand> },
}
```

### Lowering Examples

```
// Source: let a = 1; let b = 2; let c = a + b
// Lowered (all SSA):
block0:
    v0 = Const(1)           // let a - SSA
    v1 = Const(2)           // let b - SSA
    v2 = BinOp(Add, v0, v1) // let c - SSA
    Return(v2)

// Source: var sum = 0; sum = sum + 1; ret sum
// Lowered (slot for var):
block0:
    v0 = Const(0)
    SlotStore(s0, v0)       // var sum = 0
    v1 = Const(1)
    v2 = BinOp(Add, s0, v1) // binop borrows s0 directly
    Drop(s0)                // drop old value before store
    SlotStore(s0, v2)       // sum = sum + 1
    v3 = SlotLoad(s0)       // move from slot for return
    Return(v3)

// Source: let x = if cond { a } else { b }
// Lowered (phi at join):
block0:
    Branch(cond, block1, block2)
block1:
    v0 = Copy(a)
    Goto(block3)
block2:
    v1 = Copy(b)
    Goto(block3)
block3:
    v2 = Phi([(block1, v0), (block2, v1)])  // join point
    // v2 is let x

// Source: let x = foo()?
// Lowered:
block0:
    v0 = Call(foo, [])
    v1, v2 = UnwrapOption(v0)  // v1 = inner, v2 = is_some
    Branch(v2, block1, block2)
block1:
    // x = v1 (the unwrapped value)
    ...
block2:
    v3 = WrapNone()
    TryReturn(v3)
```

### Interpreter: Uniform Slot Execution

Both ValueId and SlotId map to frame buffer offsets:

```rust
pub struct IrLayout {
    value_offsets: Vec<u32>,  // ValueId -> byte offset
    slot_offsets: Vec<u32>,   // SlotId -> byte offset
}

impl Operand {
    pub fn offset(&self, layout: &IrLayout) -> u32 {
        match self {
            Operand::Value(v) => layout.value_offsets[v.0 as usize],
            Operand::Slot(s) => layout.slot_offsets[s.0 as usize],
        }
    }
}
```

Interpreter loop unchanged - reads/writes via offsets:

```rust
fn execute_binop(&mut self, dest: ValueId, op: BinOp, lhs: Operand, rhs: Operand) {
    let l = self.read_operand(lhs);
    let r = self.read_operand(rhs);
    let result = eval_binop(op, l, r);
    self.write_value(dest, result);
}

fn read_operand(&self, op: Operand) -> Value {
    let offset = op.offset(&self.layout);
    self.read_at_offset(offset)
}
```

### Backend Codegen

**LLVM:**
- SSA ValueId -> LLVM SSA register
- SlotId -> alloca (no mem2reg needed - these are true mutables)

**Cranelift:**
- SSA ValueId -> cranelift Value
- SlotId -> StackSlot

No wasted work - mem2reg only sees actual mutable slots.

## Implementation Phases

### Phase 1: Define IR Types - COMPLETE

File: `crates/datalove-datafun-compiler/src/ir/mod.rs`
- `ValueId`, `SlotId`, `Operand` (with External variants)
- `Instruction` enum with SSA semantics
- `IrBlock` struct (instructions + terminator)
- `IrFunction` struct (blocks + value/slot counts)
- `IrScriptUnit` struct (blocks + functions + exports)
- `ExportBinding` enum

File: `crates/datalove-datafun-compiler/src/ir/display.rs`
- Pretty-printing for all IR types

### Phase 2: Lowering Pass - COMPLETE

File: `crates/datalove-datafun-compiler/src/ir/lower.rs`
- `lower_function(FunctionDef) -> IrFunction`
- `lower_script_unit(ScriptLowerContext, ScriptUnitKind) -> IrScriptUnit`
- `ScriptLowerContext` for cross-unit binding tracking
- `ScriptUnitKind::Fragment` and `ScriptUnitKind::Expr`
- Expression temps and `let` bindings -> ValueId
- `var` bindings -> SlotId with SlotStore/SlotLoad
- Function definitions in script units
- Cross-unit references via ExternalValue/ExternalSlot

Tests:
- `tests/ir_lower_tests.rs` - 5 function lowering tests
- `tests/ir_lower_script_tests.rs` - 5 script unit lowering tests

#### Known Hacks - ALL RESOLVED

1. **Expression parsing** - FIXED. Added `parse_expr` to parser.
   - Files modified: `parser.rs`, `ir_lower_script_tests.rs`

2. **Cross-unit slot assignment** - FIXED. Added `SlotDest` enum with `Local` and `External` variants.
   - Files modified: `ir/mod.rs`, `ir/lower.rs`, `ir/display.rs`

3. **Type placeholders** - FIXED. Added proper symbol table with FuncId, TypeId, FuncRef, TypeRef.
   - `Call { func: FuncRef }` - functions resolved via symbol table
   - `Pack { ty: TypeRef }` - uses TypeRef enum for built-in and user-defined types
   - `FieldAccess { field_index: u32 }` - uses index instead of string
   - Added `SymbolTable` with function/type definitions and name resolution
   - Files modified: `ir/mod.rs`, `ir/lower.rs`, `ir/display.rs`

### Phase 3: IR Interpreter - MOSTLY COMPLETE

File: `crates/datalove-datafun-compiler/src/ir/interp/` (modular structure)

Uses proper runtime model:
- Frame = flat `Vec<u8>` byte buffer with computed offsets
- Values = `Value { ptr: *mut u8, tydesc: *const TyDesc }` pairs
- TyDesc from `datalove-rtdt` for size/align/type info
- Operations via raw pointer reads/writes

Implemented:
- `IrType` enum in ir/mod.rs for full type info (unlike minimal `TypeRef`)
- `IrTyDescTable` converts IrType to runtime TyDesc
- `IrLayout` computes ValueId/SlotId -> byte offsets
- `Frame` manages frame data with value/slot initialization tracking
- `ExecutionContext` holds available functions for call resolution
- `ScriptEnvironment` holds frames/functions from previous units for cross-unit execution
- `IrInterpreter` executes IrFunction:
  - Const, Copy, Move instructions
  - BinOp for all types: i8-i64, u8-u64, f32, bool (Add, Sub, Mul, Div, Mod, comparisons, BitAnd/Or/Xor, Shl, Shr)
  - BinOp for Int (bigint): Add, Sub, Mul, Div via runtime calls (`dtlv_rti_int_*`)
  - BinOpChecked for all integer types (Add, Sub, Mul with overflow flag)
  - UnaryOp: Neg for signed ints, f32, and Int (bigint); BitNot for all ints; Not for bool
  - SlotStore, SlotLoad (including cross-unit slot writes via SlotDest::External)
  - Pack (tuple/struct construction)
  - Unpack (tuple/struct destructuring)
  - TupleIndex, FieldAccess
  - WrapSome, WrapNone, UnwrapOption
  - WrapOk, WrapErr, UnwrapResult
  - Call (function calls with nested call support, including cross-unit calls)
  - Phi nodes (single-pass execution with prev_block tracking)
  - All terminators: Branch, Goto, Return, TryReturn, UnitEnd, UnitEarlyReturn
- Bigint (`Int`) support:
  - `ConstValue::Int { limbs: Vec<u32>, negative: bool }` - limbs representation
  - `parse_int_const` / `parse_hex_const` convert decimal/hex to limbs
  - `write_const` allocates limbs via runtime, writes `rtdt::Int` structure
  - `execute_binop` uses `dtlv_rti_int_add`, `_sub`, `_mul`, `_div_checked`
  - `execute_unaryop` uses `dtlv_rti_int_neg`
  - Display impl converts limbs back to decimal string
  - **NOT YET**: Int comparison operations (Eq, Ne, Lt, etc.)
- Type tracking during lowering:
  - `fresh_value(ty: IrType)` pushes type to value_types
  - `fresh_slot(ty: IrType)` pushes type to slot_types
  - Expression types looked up from TypecheckResult via salsa IDs
- Return value storage: return values copied to persistent storage to outlive callee frames
- Cross-unit references:
  - `Operand::ExternalValue` - read let bindings from prior units
  - `Operand::ExternalSlot` - read var bindings from prior units
  - `SlotDest::External` - write to var bindings from prior units
  - `FuncRef::External` - call functions from prior units
  - External function calls use callee's `ExecutionContext` for local function lookups
- Loop/break/continue:
  - `loop_stack` in LowerCtx tracks (continue_target, break_target) for nested loops
  - `break` lowers to `Goto(loop_exit)`
  - `continue` lowers to `Goto(loop_header)`
  - Interpreter follows CFG via Goto terminators

- Collection creation:
  - `ListNew` - creates list, reserves capacity, copies elements via runtime calls
  - `SetNew` - sorts elements, builds B-tree via `dtlv_rti_btreeset_build_from_sorted_slice_local`
  - `MapNew` - sorts by key, builds B-tree via `dtlv_rti_btreemap_build_from_sorted_slices_local`
- Precise drop points:
  - IR lowering emits `Drop` instructions at scope exits via `ScopeTracker`
  - `is_copy_type()` determines if a type needs dropping
  - Scope kinds: `Function`, `ScriptUnit`, `Loop`, `IfThen`, `IfElse`
  - Functions: drops emitted before returns, at scope exits
  - Loops: drops before loop-back and break/continue
  - If blocks: drops before Goto(merge) in each branch
  - Set statements: drop old slot value before store (non-copy types)
  - Return values marked as moved (not dropped)
  - Script unit top-level: NOT dropped (exported, cleaned at finalize)
- Drop instruction execution:
  - `execute_drop` calls `dtlv_rti_any_destroy_local`
  - `mark_value_dropped`/`mark_slot_dropped` prevent double-destroy
- Frame cleanup (script finalize only):
  - `Frame::destroy_all` destroys remaining initialized values/slots
  - `FrameStore::destroy_all` destroys all script unit frames
  - `ScriptEnvironment::destroy_all` called at end of worldfile analysis
  - Function frames: no destroy_all needed (precise drops handle cleanup)
  - Script unit frames: destroy_all cleans up exported bindings at script end
- Memory management fixes:
  - **SlotStore destroys old values**: When storing to already-initialized slot, old value destroyed first
  - **Script unit error cleanup**: Frame destroyed on execution error to clean up initialized values
  - **Return/UnitEnd move semantics**: Use `move_value` + `mark_value_dropped` instead of `copy_value` to prevent double-free (frame will be destroyed by caller)
  - **Unit type NonNull fix**: Empty tuple fields use `NonNull::dangling().as_ptr()` instead of null (Rust 1.78+ requires non-null for empty slices in `from_raw_parts`)
- Operand semantics (copy/move/borrow):
  - **Copy types**: Unit, Bool, U8-U64, I8-I64, F32, tuples/structs of copy types
  - **Non-copy types**: Int, String, Data, Error, List, Set, Map, Tensor
  - **`copy_value`**: Shallow bitwise copy, only for copy types (asserted)
  - **`move_value`**: Shallow copy with ownership transfer, source marked dropped
  - **Borrow semantics for binop/unaryop**: Operands read by reference, not consumed
    - `lower_operand()` returns `Operand` directly for borrowing contexts
    - For Name->Slot: returns `Operand::Slot(s)` (no SlotLoad emitted)
    - For Name->Value: returns `Operand::Value(v)`
    - For compound expressions: lowers to value and wraps
    - Interpreter's `read_operand()` reads without consuming
  - **SlotLoad is destructive**: Moves value out of slot, marks slot as dropped
    - Only emitted for consuming contexts (function args, return, etc.)
    - Binop/unaryop operands DON'T use SlotLoad - they borrow via Operand::Slot
  - **External references**: Copy types use Copy instruction, non-copy use Move
- Expression temporaries:
  - **FIXED**: `expr_temps` in LowerCtx tracks non-copy values created during operand lowering
  - After BinOp/UnaryOp, `emit_expr_temp_drops()` emits Drop for all recorded temps
  - Prevents leaks in loops where literals are created each iteration
- Drop tracking TODO:
  - **Branch convergence** - values moved in one if branch but not other should be dropped in non-moving branch (potential memory leak)

### Phase 4: Integration - MOSTLY COMPLETE

Test suites created:
- `module_interp3_tests` - modules only, runs nullary `main()` via IR interpreter
- `interp3_tests` - modules + scriptunit-fragment + scriptunit-expr sections

Files created:
- `crates/datalove-datafun/src/worldfile_analysis_modules_ir3.rs` - module-only IR3 analysis
- `crates/datalove-datafun/src/worldfile_analysis_ir3.rs` - full worldfile IR3 analysis
- `crates/datalove-datafun/tests/module_interp3_tests.rs` - test harness
- `crates/datalove-datafun/tests/interp3_tests.rs` - test harness
- `crates/datalove-datafun/tests/fixtures/module_interp3/` - 5 worldfiles
- `crates/datalove-datafun/tests/fixtures/interp3/` - 48 worldfiles (cross-unit + loop + collection + error tests)

Working:
- Module-only execution: parse -> typecheck -> lower -> IR interpret -> pretty print
- Script fragment lowering: parse -> typecheck -> lower script unit
- Script expression lowering: parse -> typecheck expr -> lower script unit
- Cross-unit typechecking: bindings from prior units visible in later units
- Cross-unit lowering: ExternalValue/ExternalSlot/FuncRef::External references
- Cross-unit execution: ScriptEnvironment tracks frames/functions across units
- Module error reporting: typecheck errors reported per module, lowering skipped for errored modules

Cross-unit test coverage (tests 010-018):
- `010_crossunit_value.world` - let binding across units
- `011_crossunit_slot.world` - var binding across units
- `012_crossunit_function.world` - function call across units
- `013_crossunit_chain.world` - chained cross-unit references
- `014_sameunit_function.world` - function defined and called in same unit
- `015_crossunit_function_chain.world` - function calling function across units
- `016_module_function_call.world` - calling module function from script
- `017_sameunit_var_mutation.world` - var mutation within same unit
- `018_crossunit_var_mutation.world` - var mutation across units

Loop test coverage (tests 019-022):
- `019_loop_break_script.world` - loop with break in script unit
- `020_loop_continue_script.world` - loop with continue in script unit
- `021_loop_function_script.world` - loop in function defined in script
- `022_loop_module_function.world` - loop in function defined in module

Collection test coverage (tests 023-025):
- `023_list_literal.world` - list creation `@[@1, @2, @3]` -> `[@1, @2, @3]`
- `024_set_literal.world` - set creation `@set { @5, @10, @15 }` -> `{set len=3}`
- `025_map_literal.world` - map creation `@map { @1 = @100, @2 = @200 }` -> `{map len=2}`

Collection in function/module tests (026-031):
- `026_list_in_function.world` - list returned from function
- `027_set_in_function.world` - set returned from function
- `028_map_in_function.world` - map returned from function
- `029_list_in_module.world` - list in module function
- `030_set_in_module.world` - set in module function
- `031_map_in_module.world` - map in module function

Try operator tests (032-041):
- `032_try_option_fails_in_script.world` - `?` operator early return in script
- `033_try_result_works_in_script.world` - `!` operator in script
- `034-041` - Error type and Result er variant tests

Cross-unit function dispatch tests (050-060):
- `050_module_to_module_import.world` - module B requires A, imports and calls A's function
- `051_module_multiple_local_functions.world` - module with add_one, add_two, add_four
- `052_script_multiple_local_functions.world` - script unit with multiple local functions
- `053_script_to_prior_script_call.world` - unit 2 calls function from unit 1
- `054_import_inheritance_across_units.world` - import from unit 1 used in unit 2
- `055_module_to_module_chain.world` - base→mid→top chain
- `056_script_missing_require.world` - error case: import without require
- `057_script_missing_import.world` - error case: call without import
- `058_module_typecheck_error.world` - module typecheck error reporting
- `059_module_undefined_import.world` - module import error without require
- `060_script_u32_function.world` - simple script function with int type

Int comparison tests (061-065):
- `061_int_compare_eq.world` - Int equality (@42 == @42)
- `062_int_compare_lt.world` - Int less than (@10 < @20)
- `063_int_compare_negative.world` - negative Int comparison
- `064_int_loop_compare.world` - loop with Int comparison condition
- `065_int_compare_all_ops.world` - all comparison operators (Eq, Ne, Lt, Le, Gt, Ge)

If-binding tests (066-083 in interp3, 010-016 in module_interp3):
- `066_script_if_option_binding.world` - basic Some case
- `067_script_if_option_none_binding.world` - None case, else branch
- `068_script_if_result_ok_binding.world` - Ok case
- `069_script_if_result_error_binding.world` - Error case
- `070-079` - variable shadowing tests (let/var combinations)
- `080_if_option_int_binding.world` - non-copy type (Int)
- `081_if_option_string_binding.world` - non-copy type (String)
- `082_if_option_binding_shadows.world` - if-binding shadows outer variable
- `083_nested_if_option.world` - nested if-bindings

**Test counts:**
- interp3_tests: 71 tests
- module_interp3_tests: 26 tests
- module_interp_tests: 219 tests
- All tests pass with `DATALOVE_LEAK_CHECK=panic-backtrace`

TODO:
- Keep old interpreter for comparison
- Run full test suite against both interpreters

### Feature Gap Analysis (vs Old Interpreter)

**Completed Features:**

- **If-Bindings** - DONE. Option/Result destructuring in if conditions
  - `if opt_value |x| ... end if` extracts Some payload via `UnwrapOption`
  - `if result_value |ok| else |err| ... end if` extracts Ok/Err via `UnwrapResult`
  - Move semantics: inner value is moved (not cloned) to binding
  - Binding scoped to branch, dropped at scope exit
  - Shadowing: if-binding can shadow outer variables, restored at scope exit
  - Non-copy types: Int, String bindings properly dropped
  - Nested if-bindings work correctly
  - Tests 010-016 (module_interp3), 066-083 (interp3)
- **String Literals** - DONE. `ConstValue::String(String)` in IR
  - Lowering strips quotes from AST string values
  - Interpreter creates strings via `dtlv_rti_string_create_local`/`push_bytes_local`
  - Display uses Rust debug formatting for proper escaping
  - Tests: 035_error_from_string, 069, 081 (string in if-binding)
- **Try Operators (?, !)** - DONE. Tests 032-033 verify early return behavior
- **Bigint (Int)** - DONE. Arithmetic via runtime calls, proper limbs representation
- **Int Comparison** - DONE. Inline `int_compare()` in interpreter (no runtime call needed)
  - All comparison operators: Eq, Ne, Lt, Le, Gt, Ge
  - Loop tests (019-022) now work with Int comparison
  - Tests 061-065 cover Int comparisons
- **Collections (List, Set, Map)** - DONE. Creation and function returns work
- **Operand Semantics** - DONE. Proper borrow/move/copy semantics:
  - Binop/unaryop operands borrow (read by reference, not consumed)
  - SlotLoad is destructive (move semantics for consuming contexts)
  - `lower_operand()` returns slots directly as operands (no unnecessary loads)
  - `IrType::is_copy()` determines copy vs non-copy types
  - `copy_value` asserts on copy types only, `move_value` for ownership transfer

- **Optional Arithmetic (+?, -?, *?, /?)** - DONE. Early-return semantics on overflow/div-zero
  - Uses `BinOpChecked` to get `(value, overflow_flag)`
  - Branches on overflow flag: continue or early return with None
  - Supports all fixed-width integer types (i8-i64, u8-u64)
  - Division handles both div-by-zero and signed overflow (MIN / -1)
  - Tests 027-034 (module_interp3)

**Missing Features (MEDIUM priority):**

3. **Checked Arithmetic Result (+!, -!, *!, /!)**
   - Old: Returns `Result<T, Error>`, Err on overflow/div-zero
   - IR: `BinOpChecked` needs Result wrapping (similar to Optional)

4. **Widening Arithmetic**
   - Bare `+`, `-`, `*` on fixed ints widen both operands to Int (bigint)
   - Example: `a + @3` where a is u32 and @3 is Int results in Int type
   - Typechecker handles widening; IR receives correct types
   - IR interpreter handles Int arithmetic via runtime calls

**Missing Features (LOW priority):**

5. **Unary Checked/Optional (-?, -!)**
   - Old: `NegOptional`, `NegResult` for checked negation
   - IR: `UnaryOp` only has `Neg`, `BitNot`, `Not`

6. **Data Type (@data)**
   - Old: `data(value)` coercion wrapper
   - IR: `IrType::Data` exists, no creation instruction

7. **Hex Literals** - May already parse to int values

**Test Coverage:**

- Old interpreter (module_interp_tests): 219 tests
- IR interpreter (interp3_tests): 87 tests
- IR interpreter (module_interp3_tests): 34 tests
- All tests pass with leak checking enabled

Test matrix (each feature should be tested in):
- expr unit
- script fragment
- cross-unit references
- function in script fragment
- function in module

### Phase 5: Cleanup - NOT STARTED

- Remove old tree-walking interpreter (or keep as reference)
- Optimize IR representation

## Files Created/Modified

**Created:**
- `crates/datalove-datafun-compiler/src/ir/mod.rs` - IR types
- `crates/datalove-datafun-compiler/src/ir/lower.rs` - AST->IR lowering
- `crates/datalove-datafun-compiler/src/ir/display.rs` - IR pretty-printing
- `crates/datalove-datafun-compiler/src/ir/interp.rs` - IR interpreter
- `crates/datalove-datafun/tests/ir_lower_tests.rs` - function lowering tests
- `crates/datalove-datafun/tests/ir_lower_script_tests.rs` - script unit tests
- `crates/datalove-datafun/tests/module_interp3_tests.rs` - module-only IR3 tests
- `crates/datalove-datafun/tests/interp3_tests.rs` - full worldfile IR3 tests
- `crates/datalove-datafun/src/worldfile_analysis_modules_ir3.rs` - module-only IR3 analysis
- `crates/datalove-datafun/src/worldfile_analysis_ir3.rs` - full worldfile IR3 analysis
- `crates/datalove-datafun/tests/fixtures/ir_lower/` - function test fixtures
- `crates/datalove-datafun/tests/fixtures/ir_lower_script/` - script test fixtures
- `crates/datalove-datafun/tests/fixtures/module_interp3/` - module-only worldfiles
- `crates/datalove-datafun/tests/fixtures/interp3/` - full worldfiles (incl. cross-unit tests)

**Modified:**
- `crates/datalove-datafun-compiler/src/lib.rs` - add `pub mod ir`
- `crates/datalove-datafun-compiler/src/tycheck.rs` - batch typechecking types and `type_check_script_units`
- `crates/datalove-datafun-pkg/src/package_load_worldfile.rs` - new section types
- `crates/datalove-datafun/src/worldfile_analysis.rs` - handle new sections
- `crates/datalove-datafun/Cargo.toml` - test configurations

## Benefits

1. **Clean SSA semantics** - proper dataflow for analysis/optimization
2. **Efficient codegen** - no wasted mem2reg on expression temps
3. **Simple interpreter** - uniform slot-based execution
4. **No EvalResult needed** - control flow is explicit CFG edges
5. **LLVM/Cranelift ready** - direct mapping to target IR

## Design Decisions

1. **SSA for immutables**: Expression temps and `let` bindings are SSA values
2. **Slots for mutables**: Only `var` bindings use SlotStore/SlotLoad
3. **Interpreter uniformity**: Both map to frame offsets at runtime
4. **Phi nodes**: Explicit merge at control flow join points
5. **Migration**: Parallel execution for validation

## Cross-Unit Typechecking (Salsa Integration)

Worldfile tests have multiple script units that share bindings. To properly typecheck cross-unit references, all units are processed together in a single salsa-tracked function.

### Types (in `tycheck.rs`)

```rust
/// Kind of script unit for batch typechecking.
#[derive(Clone, Hash, PartialEq, Eq)]
pub enum ScriptUnitKind<'db> {
    Fragment(Script<'db>),
    Expr(ExprFun<'db>),
}

/// Input for batch script unit typechecking.
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct ScriptUnitInput<'db> {
    pub source: bct::input::Source,
    pub kind: ScriptUnitKind<'db>,
}

/// Interned batch of script units for typechecking.
#[salsa::interned]
pub struct ScriptUnitBatch<'db> {
    #[returns(ref)]
    pub units: Vec<ScriptUnitInput<'db>>,
}

/// Result of typechecking one script unit.
#[salsa::tracked]
pub struct UnitTypecheckResultTracked<'db> {
    pub errors: Vec<TypeErrorEntry<'db>>,
    #[returns(ref)]
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
    #[returns(ref)]
    pub call_targets: Vec<Option<ResolvedCallTarget<'db>>>,
}

/// Result of typechecking multiple script units together.
#[salsa::tracked]
pub struct ScriptUnitsTypecheckResultTracked<'db> {
    pub results: Vec<UnitTypecheckResultTracked<'db>>,
}
```

### Entry Point

```rust
#[salsa::tracked]
pub fn type_check_script_units<'db>(
    db: &'db dyn crate::Db,
    batch: ScriptUnitBatch<'db>,
) -> ScriptUnitsTypecheckResultTracked<'db>
```

The function processes units sequentially, accumulating let/var/fn bindings. Prior units' bindings are seeded into each unit's TypeContext. Salsa memoizes the entire batch.

### Accumulated Bindings

- `accumulated_vars: HashMap<InternedText, TypeAndHeap>` - let bindings
- `accumulated_fns: HashMap<InternedText, TypeFunction>` - function types
- `accumulated_fn_asts: HashMap<InternedText, StmtFun>` - function ASTs for call checking

After typechecking each Fragment unit, new let/var/fn bindings are extracted and added to the accumulated maps for use by subsequent units.

## Script Unit Support

Scripts and REPL sessions are sequences of "units" executed sequentially.

### Script Unit Kinds

1. **Statement unit**: `Vec<Statement>` - let/var/fun declarations, control flow
2. **Expression unit**: single `Expression` - evaluated, result kept for REPL

Both lower to the same IR structure; difference is whether there's a result value.

### Return Type Semantics

Script units typecheck as-if they have return type `!()` ("result of unit"). This makes:
- `ret expr` set the unit's result and end the unit
- Early returns (`?`, `+?`, etc.) work naturally - propagating None/Err ends unit early

### Extended Operand Type

```rust
pub enum Operand {
    Value(ValueId),           // local SSA value
    Slot(SlotId),             // local mutable slot
    ExternalValue { unit: u32, value: ValueId },  // let binding from previous unit
    ExternalSlot { unit: u32, slot: SlotId },     // var binding from previous unit
}
```

### IrScriptUnit Structure

```rust
pub struct IrScriptUnit {
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    /// Functions defined in this unit.
    pub functions: Vec<IrFunction>,
    /// Result value of this unit (for bare expressions in REPL).
    pub result: Option<ValueId>,
    /// Names exported to later units.
    pub exports: Vec<(String, ExportBinding)>,
}

pub enum ExportBinding {
    Value(ValueId),
    Slot(SlotId),
    Function(usize),
}
```

### Terminator

```rust
pub enum Terminator {
    Goto(BlockId),
    Branch { cond: Operand, then_block: BlockId, else_block: BlockId },
    Return { value: Option<Operand> },      // function return
    TryReturn { value: Option<Operand> },   // early return (functions)
    UnitEnd { result: Option<Operand> },    // end of script unit
    UnitEarlyReturn { value: Operand },     // early return (script units)
}
```

### Lowering Context

```rust
pub struct ScriptLowerContext {
    pub values: HashMap<String, (u32, ValueId)>,    // let bindings
    pub slots: HashMap<String, (u32, SlotId)>,      // var bindings
    pub functions: HashMap<String, u32>,            // function defs
}
```

### Interpreter: Unit Frame Stack

```rust
pub struct ScriptInterpreter {
    unit_frames: Vec<UnitFrame>,  // kept alive for cross-unit refs
}

impl ScriptInterpreter {
    fn read_operand(&self, current_unit: u32, op: Operand) -> Value {
        match op {
            Operand::Value(v) => self.unit_frames[current_unit].read_value(v),
            Operand::Slot(s) => self.unit_frames[current_unit].read_slot(s),
            Operand::ExternalValue { unit, value } =>
                self.unit_frames[unit as usize].read_value(value),
            Operand::ExternalSlot { unit, slot } =>
                self.unit_frames[unit as usize].read_slot(slot),
        }
    }
}
```

### Key Constraints

- CFG never jumps between units
- Loops/ifs must be complete within a single unit
- Forward-only visibility: unit N sees units 0..N-1
- Values persist across units until moved/dropped
- Functions defined in unit N visible to units N+1..

### Script vs Function

| Aspect | IrFunction | IrScriptUnit |
|--------|------------|--------------|
| Parameters | Yes | None |
| External refs | No | Yes |
| Defines functions | No | Yes |
| Terminator | Return | UnitEnd |
| Frame lifetime | Transient | Persistent |
