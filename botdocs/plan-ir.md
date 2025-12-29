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
    v1 = SlotLoad(s0)       // read sum
    v2 = Const(1)
    v3 = BinOp(Add, v1, v2)
    SlotStore(s0, v3)       // sum = sum + 1
    v4 = SlotLoad(s0)
    Return(v4)

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

File: `crates/datalove-datafun-compiler/src/ir/interp.rs`

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
  - BinOpChecked for all integer types (Add, Sub, Mul with overflow flag)
  - UnaryOp: Neg for signed ints and f32, BitNot for all ints, Not for bool
  - SlotStore, SlotLoad (including cross-unit slot writes via SlotDest::External)
  - Pack (tuple/struct construction)
  - Unpack (tuple/struct destructuring)
  - TupleIndex, FieldAccess
  - WrapSome, WrapNone, UnwrapOption
  - WrapOk, WrapErr, UnwrapResult
  - Call (function calls with nested call support, including cross-unit calls)
  - Phi nodes (single-pass execution with prev_block tracking)
  - All terminators: Branch, Goto, Return, TryReturn, UnitEnd, UnitEarlyReturn
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
- Loop/break/continue:
  - `loop_stack` in LowerCtx tracks (continue_target, break_target) for nested loops
  - `break` lowers to `Goto(loop_exit)`
  - `continue` lowers to `Goto(loop_header)`
  - Interpreter follows CFG via Goto terminators

TODO:
- ListNew, SetNew, MapNew (collection creation - requires runtime calls)
- Drop instruction (destructors - requires runtime calls)

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
- `crates/datalove-datafun/tests/fixtures/interp3/` - 22 worldfiles (cross-unit + loop tests)

Working:
- Module-only execution: parse -> typecheck -> lower -> IR interpret -> pretty print
- Script fragment lowering: parse -> typecheck -> lower script unit
- Script expression lowering: parse -> typecheck expr -> lower script unit
- Cross-unit typechecking: bindings from prior units visible in later units
- Cross-unit lowering: ExternalValue/ExternalSlot/FuncRef::External references
- Cross-unit execution: ScriptEnvironment tracks frames/functions across units

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

TODO:
- Keep old interpreter for comparison
- Run full test suite against both interpreters

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
