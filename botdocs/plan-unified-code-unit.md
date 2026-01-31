# Unified Code Unit Migration Plan

## Overview

This plan unifies `IrFunction` and `IrScriptUnit` into a single `IrCodeUnit` type,
eliminating the dual-pipeline architecture and all backward-compatibility code.

## Current State Analysis

### IrFunction Fields
```rust
pub struct IrFunction {
    pub id: FuncId,
    pub name: String,
    // Function-specific: signature
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    pub tracked_params: Vec<ParamId>,
    // Common body
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub call_site_count: u32,
    pub value_types: Vec<IrType>,
    pub slot_types: Vec<IrType>,
    pub tracked_slots: Vec<SlotId>,
    pub const_values: Vec<(String, ValueId)>,
}
```

### IrScriptUnit Fields
```rust
pub struct IrScriptUnit {
    // Common body
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub call_site_count: u32,
    pub value_types: Vec<IrType>,
    pub slot_types: Vec<IrType>,
    pub tracked_slots: Vec<SlotId>,
    pub const_values: Vec<(String, ValueId)>,
    // Script-specific: persistence & exports
    pub unit_end_values: Vec<ValueId>,
    pub unit_end_slots: Vec<SlotId>,
    pub functions: Vec<IrFunction>,  // Nested functions
    pub symbols: SymbolTable,
    pub result: Option<ValueId>,
    pub exports: Vec<(String, ExportBinding)>,
}
```

### Key Semantic Differences
1. **Parameters**: Functions have them, scripts use external captures
2. **Return vs UnitEnd**: Different terminator semantics
3. **Exports**: Only scripts export bindings
4. **Persistence**: Script bindings persist in FrameStore
5. **Nesting**: Scripts contain nested functions

---

## Unified Design

### 1. IrCodeUnit - The Unified Type

```rust
/// Unified IR representation for executable code.
///
/// Replaces both IrFunction and IrScriptUnit with a single type.
/// The `context` field determines execution semantics.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IrCodeUnit {
    /// Unique identifier within containing scope.
    pub id: CodeUnitId,
    /// Name for debugging/symbol resolution.
    pub name: String,

    // ========== Body (common to all code units) ==========
    pub blocks: Vec<IrBlock>,
    pub value_count: u32,
    pub slot_count: u32,
    pub call_site_count: u32,
    pub value_types: Vec<IrType>,
    pub slot_types: Vec<IrType>,
    /// Slots requiring runtime tracking.
    pub tracked_slots: Vec<SlotId>,
    /// Const bindings for inlining pass.
    pub const_values: Vec<(String, ValueId)>,
    /// Symbol table for nested function resolution.
    pub symbols: SymbolTable,

    // ========== Context (determines execution semantics) ==========
    pub context: CodeUnitContext,

    // ========== Nested Units ==========
    /// Functions/closures defined inside this unit.
    pub nested_units: Vec<IrCodeUnit>,
}

/// Identifier for a code unit (replaces FuncId for unified addressing).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CodeUnitId(pub u32);

/// Context determining how a code unit executes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeUnitContext {
    /// A callable function with explicit parameters.
    Function(FunctionContext),
    /// A script unit with captures and exports.
    Script(ScriptContext),
}

/// Context for function execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionContext {
    pub params: Vec<ParamId>,
    pub param_modes: Vec<ParamMode>,
    pub param_types: Vec<IrType>,
    pub return_type: IrType,
    /// Out params that need runtime tracking.
    pub tracked_params: Vec<ParamId>,
}

/// Context for script unit execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptContext {
    /// Values that persist for REPL cleanup.
    pub unit_end_values: Vec<ValueId>,
    /// Slots that persist for REPL cleanup.
    pub unit_end_slots: Vec<SlotId>,
    /// Result value for expression units.
    pub result: Option<ValueId>,
    /// Bindings exported to subsequent units.
    pub exports: Vec<(String, ExportBinding)>,
}
```

### 2. Unified Code Reference (replaces FuncRef)

```rust
/// Reference to a code unit.
///
/// Unifies local, external (cross-unit), and module references.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum CodeRef {
    /// Unit defined in the current compilation scope.
    Local(CodeUnitId),
    /// Unit from a previous script execution (for REPL chaining).
    External { unit: u32, id: CodeUnitId },
    /// Unit from a compiled module.
    Module { module: IrModuleId, id: CodeUnitId },
}
```

### 3. Unified Terminator

```rust
pub enum Terminator {
    Goto { target: BlockId, args: Vec<Operand> },
    Branch { cond: Operand, then_block: BlockId, then_args: Vec<Operand>,
             else_block: BlockId, else_args: Vec<Operand> },
    /// Unified return/end terminator.
    /// - For functions: returns to caller
    /// - For scripts: ends unit execution
    Exit { value: Option<Operand> },
    /// Early exit (from ret statement, !, or ? operators).
    /// Only valid in script context.
    EarlyExit { value: Operand },
}
```

### 4. Unified Frame

```rust
/// Runtime frame for code unit execution.
pub struct Frame {
    data: AlignedBuffer,
    layout: IrLayout,

    /// Tracking state for slots (always present for scripts, optional for functions).
    slot_tracking: Option<Vec<bool>>,

    /// Input bindings from caller/environment.
    inputs: Vec<InputSlot>,
}

pub struct InputSlot {
    ptr: *mut u8,
    tydesc: *const TyDesc,
    initialized: bool,
}

impl Frame {
    /// Create frame for any code unit.
    pub fn new(unit: &IrCodeUnit, layout: &IrLayout) -> Self {
        let slot_tracking = match &unit.context {
            CodeUnitContext::Script(_) => Some(vec![false; unit.slot_count as usize]),
            CodeUnitContext::Function(_) => {
                if unit.tracked_slots.is_empty() {
                    None // Precise-only function
                } else {
                    Some(vec![false; unit.slot_count as usize])
                }
            }
        };
        // ... rest of initialization
    }
}
```

### 5. Unified Registry

```rust
/// Registry for code unit resolution.
pub struct CodeUnitRegistry {
    /// Units from compiled modules: (module, id) -> unit
    module_units: HashMap<(IrModuleId, CodeUnitId), IrCodeUnit>,
    /// Units from previous script executions: [unit_index][local_id] -> unit
    script_units: Vec<Vec<IrCodeUnit>>,
}

impl CodeUnitRegistry {
    pub fn resolve(&self, code_ref: &CodeRef) -> Option<&IrCodeUnit> {
        match code_ref {
            CodeRef::Local(_) => None, // Local refs resolved by caller
            CodeRef::External { unit, id } => {
                self.script_units.get(*unit as usize)
                    .and_then(|units| units.iter().find(|u| u.id == *id))
            }
            CodeRef::Module { module, id } => {
                self.module_units.get(&(*module, *id))
            }
        }
    }
}
```

---

## Migration Phases

### Phase 1: Introduce IrCodeUnit (Parallel Structure)

**Goal**: Add new unified types without breaking existing code.

**Files to modify**:
- `crates/datalove-datafun-ir/src/lib.rs`
  - Add `IrCodeUnit`, `CodeUnitId`, `CodeUnitContext`, `FunctionContext`, `ScriptContext`
  - Add `CodeRef` enum
  - Keep `IrFunction`, `IrScriptUnit`, `FuncRef` temporarily

**Add conversion methods**:
```rust
impl IrFunction {
    pub fn into_code_unit(self) -> IrCodeUnit { ... }
}

impl IrScriptUnit {
    pub fn into_code_unit(self) -> IrCodeUnit { ... }
}

impl IrCodeUnit {
    pub fn as_function_context(&self) -> Option<&FunctionContext> { ... }
    pub fn as_script_context(&self) -> Option<&ScriptContext> { ... }
}
```

### Phase 2: Migrate Lowering

**Goal**: Lowering produces `IrCodeUnit` instead of separate types.

**Files to modify**:
- `crates/datalove-datafun-lower/src/context.rs`
  - `LowerCtx` outputs `IrCodeUnit`
  - `FrameState` unchanged (it's already unified)

- `crates/datalove-datafun-lower/src/func.rs`
  - `lower_function_body` returns `IrCodeUnit` with `FunctionContext`

- `crates/datalove-datafun-lower/src/script.rs`
  - `lower_script_fragment_raw` returns `IrCodeUnit` with `ScriptContext`
  - `lower_script_expr` returns `IrCodeUnit` with `ScriptContext`
  - Nested functions stored in `nested_units` instead of `functions`

- `crates/datalove-datafun-lower/src/module.rs`
  - Module lowering produces `Vec<IrCodeUnit>` with `FunctionContext`

### Phase 3: Migrate Interpreter

**Goal**: Interpreter works with `IrCodeUnit` only.

**Files to modify**:
- `crates/datalove-datafun-interp/src/frame.rs`
  - Remove `Frame::new_function` and `Frame::new_script`
  - Single `Frame::new(unit: &IrCodeUnit, ...)` constructor
  - `FrameStore` stores `IrCodeUnit` references

- `crates/datalove-datafun-interp/src/lib.rs`
  - `execute_code_unit(&IrCodeUnit, ...)` replaces both `call_in_context` and `execute_script_unit_in_env`
  - `execute_blocks` handles `Exit` and `EarlyExit` terminators
  - `execute_call` resolves `CodeRef` instead of `FuncRef`

- `crates/datalove-datafun-interp/src/env.rs`
  - `ExecutionContext::get_unit(&CodeRef)` replaces `get_function(&FuncRef)`

- `crates/datalove-datafun-interp/src/dispatch.rs`
  - `CallDispatcher` uses `CodeRef` and `IrCodeUnit`

- `crates/datalove-datafun-interp/src/dynamic.rs`
  - `CallSiteKey` uses `CodeRef`
  - `inlined_functions` becomes `inlined_units: HashMap<CodeRef, IrCodeUnit>`

### Phase 4: Migrate Registries

**Goal**: Single unified registry for all code units.

**Files to modify**:
- `crates/datalove-datafun-ir/src/registry.rs`
  - Remove `UnitFunctionRegistry` and `ModuleFunctionRegistry`
  - Add `CodeUnitRegistry` as defined above

- Update all registry users:
  - `crates/datalove-datafun-interp/src/env.rs`
  - `crates/datalove-datafun/src/pipeline/script_executor.rs`

### Phase 5: Migrate AOT/Cranelift

**Goal**: AOT compilation uses `IrCodeUnit` only.

**Files to modify**:
- `crates/datalove-datafun-cranelift/src/codegen/mod.rs`
  - `compile_code_unit(&IrCodeUnit, ...)` replaces function-specific code

- `crates/datalove-datafun-cranelift/src/codegen/calls.rs`
  - `resolve_code_ref(&CodeRef)` replaces `resolve_func_ref`
  - Implement `CodeRef::External` (currently unimplemented for `FuncRef::External`)

- `crates/datalove-datafun-cranelift-aot/src/lib.rs`
  - Remove `script_unit_to_function` conversion
  - `compile_code_unit` works directly with unified type

- `crates/datalove-datafun-cranelift/src/tydesc_emit.rs`
  - Single `collect_types_from_unit` replaces separate functions

### Phase 6: Migrate Const Inlining

**Files to modify**:
- `crates/datalove-datafun-const/src/inline.rs`
  - `inline_const_values_into_unit(&mut IrCodeUnit)` replaces both
  - Works the same for function and script contexts

### Phase 7: Migrate Pipelines

**Goal**: Pipeline code uses unified types throughout.

**Files to modify**:
- `crates/datalove-datafun/src/pipeline/module_pipeline.rs`
  - Produces `Vec<IrCodeUnit>` instead of `Vec<IrFunction>`

- `crates/datalove-datafun/src/pipeline/script_compiler.rs`
  - `compile_fragment` returns `IrCodeUnit`
  - `compile_expr` returns `IrCodeUnit`

- `crates/datalove-datafun/src/pipeline/script_executor.rs`
  - `execute(&IrCodeUnit, ...)` instead of `execute(&IrScriptUnit, ...)`

- `crates/datalove-datafun/src/pipeline/result.rs`
  - Update result types to use `IrCodeUnit`

- `crates/datalove-datafun/src/pipeline/aot.rs`
  - Update AOT pipeline integration

### Phase 8: Delete Old Types

**Goal**: Remove all deprecated types and conversion code.

**Files to modify**:
- `crates/datalove-datafun-ir/src/lib.rs`
  - Delete `IrFunction` struct
  - Delete `IrScriptUnit` struct
  - Delete `FuncRef` enum
  - Delete `IrModule` struct (replace with module containing `Vec<IrCodeUnit>`)

- Delete all `into_code_unit` and `from_code_unit` conversion methods
- Delete all `as_function`/`as_script_unit` methods that were bridges

### Phase 9: Update Tests

**Goal**: All tests use unified types.

**Files to modify**:
- `crates/datalove-datafun-interp/src/tests.rs`
- `crates/datalove-datafun-cranelift-aot/tests/*.rs`
- `crates/datalove-datafun/tests/*.rs`
- `crates/datalove-tests/tests/*.rs`

Update test helpers to construct `IrCodeUnit` directly.

### Phase 10: Update Serialization

**Goal**: RON/JSON serialization uses new types.

**Files to modify**:
- `crates/datalove-datafun-ir/src/lib.rs`
  - Update `to_ron`/`from_ron` for `IrCodeUnit`

- `crates/datalove-datafun/tests/ir_serial_tests.rs`
  - Update serialization tests

---

## Instruction Set Changes

### Terminator Changes

**Before**:
```rust
Terminator::Return { value: Option<Operand> }
Terminator::UnitEnd { result: Option<Operand> }
Terminator::UnitEarlyReturn { value: Operand }
```

**After**:
```rust
Terminator::Exit { value: Option<Operand> }
Terminator::EarlyExit { value: Operand }
```

The interpreter checks `unit.context` to determine:
- Function context: `Exit` returns to caller
- Script context: `Exit` ends unit, `EarlyExit` is early termination

### Call Instruction Changes

**Before**:
```rust
Instruction::Call { site_id, dest, func: FuncRef, args }
```

**After**:
```rust
Instruction::Call { site_id, dest, target: CodeRef, args }
```

---

## Operand Changes

`Operand::ExternalValue` and `Operand::ExternalSlot` remain unchanged.
They represent captures from previous script units and are only valid
when `unit.context` is `ScriptContext`.

The interpreter validates this at execution time (debug mode) or assumes
correct lowering (release mode).

---

## Frame Changes

### Current Dual Constructor
```rust
impl Frame {
    pub fn new_function(value_count, slot_count, layout) -> Self
    pub fn new_script(value_count, slot_count, layout) -> Self
}
```

### Unified Constructor
```rust
impl Frame {
    pub fn new(unit: &IrCodeUnit, layout: &IrLayout) -> Self {
        let needs_value_tracking = matches!(&unit.context, CodeUnitContext::Script(_));
        // ... unified initialization
    }
}
```

---

## FrameStore Changes

`FrameStore` currently stores frames for script units. After unification,
it stores frames for any `IrCodeUnit` with `ScriptContext`.

```rust
pub struct FrameStore {
    frames: Vec<Frame>,
    units: Vec<IrCodeUnit>,  // Changed from storing unit metadata separately
    moved_values: HashSet<(u32, ValueId)>,
    moved_slots: HashSet<(u32, SlotId)>,
}
```

---

## Summary of Deletions

After all phases complete, these are permanently removed:

1. **Types**:
   - `IrFunction` struct
   - `IrScriptUnit` struct
   - `FuncRef` enum
   - `IrModule` struct

2. **Frame Methods**:
   - `Frame::new_function()`
   - `Frame::new_script()`

3. **Interpreter Methods**:
   - `call_in_context()` (replaced by `execute_code_unit`)
   - `execute_script_unit_in_env()` (replaced by `execute_code_unit`)

4. **Registries**:
   - `UnitFunctionRegistry`
   - `ModuleFunctionRegistry`

5. **Terminators**:
   - `Return` (replaced by `Exit`)
   - `UnitEnd` (replaced by `Exit`)
   - `UnitEarlyReturn` (replaced by `EarlyExit`)

6. **AOT Helpers**:
   - `script_unit_to_function()` conversion

---

## Benefits of Unified Design

1. **Single code path**: One lowering flow, one interpreter dispatch, one AOT path
2. **Simpler mental model**: "Code unit" is the universal abstraction
3. **Extensibility**: Closures become `CodeUnitContext::Closure { captures, ... }`
4. **Less code**: ~30% reduction in IR/interpreter/AOT code
5. **Consistent serialization**: One format for all executable code
6. **Easier optimization**: Unified IR enables unified optimization passes

---

## Risks and Mitigations

1. **Performance regression in hot path**
   - Mitigation: Profile function call path before/after
   - The unified `Frame::new` should be as fast as `new_function`

2. **Serialization compatibility**
   - Mitigation: Version the serialization format
   - Old `.ron` files won't load (acceptable for this migration)

3. **Large changeset**
   - Mitigation: Phase the migration, run `just test` after each phase
   - Each phase should leave tests passing

---

## Estimated Scope

- **Files modified**: ~25 files
- **Lines changed**: ~2000 lines (net reduction after cleanup)
- **New code**: ~500 lines (unified types and methods)
- **Deleted code**: ~800 lines (dual-path code)

---

## Detailed Code Specifications

### A. IR Type Definitions (`datalove-datafun-ir/src/lib.rs`)

#### New Types to Add

```rust
// ============================================================================
// Unified Code Unit
// ============================================================================

/// Identifier for a code unit.
///
/// Replaces FuncId for unified addressing. The numeric value is local to
/// the containing scope (module or script execution session).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct CodeUnitId(pub u32);

/// Reference to a code unit.
///
/// Replaces FuncRef with identical semantics but unified naming.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum CodeRef {
    /// Unit in the current compilation scope.
    Local(CodeUnitId),
    /// Unit from a previous script execution.
    External { unit: u32, id: CodeUnitId },
    /// Unit from a compiled module.
    Module { module: IrModuleId, id: CodeUnitId },
}

/// Context for function execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionContext {
    /// Parameter IDs (references to caller's data).
    pub params: Vec<ParamId>,
    /// Parameter modes (In, Out, Ref, Mut).
    pub param_modes: Vec<ParamMode>,
    /// Type for each parameter.
    pub param_types: Vec<IrType>,
    /// Return type.
    pub return_type: IrType,
    /// Out params that need runtime tracking.
    #[serde(default)]
    pub tracked_params: Vec<ParamId>,
}

/// Context for script unit execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptContext {
    /// Values that persist for REPL cleanup.
    #[serde(default)]
    pub unit_end_values: Vec<ValueId>,
    /// Slots that persist for REPL cleanup.
    #[serde(default)]
    pub unit_end_slots: Vec<SlotId>,
    /// Result value for expression units.
    pub result: Option<ValueId>,
    /// Bindings exported to subsequent units.
    #[serde(default)]
    pub exports: Vec<(String, ExportBinding)>,
}

/// Context determining how a code unit executes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CodeUnitContext {
    /// A callable function with parameters.
    Function(FunctionContext),
    /// A script unit with captures and exports.
    Script(ScriptContext),
}

/// Unified IR representation for executable code.
///
/// Represents both functions and script units. The `context` field
/// determines execution semantics (parameter passing vs captures,
/// return vs unit-end, etc.).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IrCodeUnit {
    /// Unique identifier within containing scope.
    pub id: CodeUnitId,
    /// Name for debugging/symbol resolution.
    pub name: String,

    // ========== Body ==========
    /// Basic blocks.
    pub blocks: Vec<IrBlock>,
    /// Number of SSA values.
    pub value_count: u32,
    /// Number of mutable slots.
    pub slot_count: u32,
    /// Number of call sites.
    #[serde(default)]
    pub call_site_count: u32,
    /// Type for each value.
    pub value_types: Vec<IrType>,
    /// Type for each slot.
    pub slot_types: Vec<IrType>,
    /// Slots requiring runtime tracking.
    #[serde(default)]
    pub tracked_slots: Vec<SlotId>,
    /// Const bindings for inlining.
    #[serde(default)]
    pub const_values: Vec<(String, ValueId)>,
    /// Symbol table for nested unit resolution.
    pub symbols: SymbolTable,

    // ========== Context ==========
    /// Determines execution semantics.
    pub context: CodeUnitContext,

    // ========== Nested Units ==========
    /// Code units defined inside this unit.
    #[serde(default)]
    pub nested_units: Vec<IrCodeUnit>,
}

impl IrCodeUnit {
    /// Get the entry block (always block 0).
    pub fn entry_block(&self) -> &IrBlock {
        &self.blocks[0]
    }

    /// Check if this is a function.
    pub fn is_function(&self) -> bool {
        matches!(&self.context, CodeUnitContext::Function(_))
    }

    /// Check if this is a script unit.
    pub fn is_script(&self) -> bool {
        matches!(&self.context, CodeUnitContext::Script(_))
    }

    /// Get function context if this is a function.
    pub fn function_context(&self) -> Option<&FunctionContext> {
        match &self.context {
            CodeUnitContext::Function(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get script context if this is a script unit.
    pub fn script_context(&self) -> Option<&ScriptContext> {
        match &self.context {
            CodeUnitContext::Script(ctx) => Some(ctx),
            _ => None,
        }
    }

    /// Get return type (for functions).
    pub fn return_type(&self) -> Option<&IrType> {
        self.function_context().map(|c| &c.return_type)
    }

    /// Get parameter count (0 for scripts).
    pub fn param_count(&self) -> usize {
        self.function_context().map(|c| c.params.len()).unwrap_or(0)
    }
}

/// Result of lowering a module to IR.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IrModule {
    /// All code units (functions) in this module.
    pub units: Vec<IrCodeUnit>,
    /// Symbol table for the module.
    pub symbols: SymbolTable,
}
```

#### Terminator Changes

```rust
pub enum Terminator {
    /// Unconditional jump.
    Goto { target: BlockId, args: Vec<Operand> },

    /// Conditional branch.
    Branch {
        cond: Operand,
        then_block: BlockId,
        then_args: Vec<Operand>,
        else_block: BlockId,
        else_args: Vec<Operand>,
    },

    /// Exit from code unit.
    ///
    /// For functions: returns value to caller.
    /// For scripts: ends unit, value is result.
    Exit { value: Option<Operand> },

    /// Early exit from script unit.
    ///
    /// From `ret` statement, `!` operator, or `?` operator.
    /// Only valid in script context.
    EarlyExit { value: Operand },
}
```

#### Call Instruction Change

```rust
// In enum Instruction:
Call {
    site_id: CallSiteId,
    dest: ValueId,
    target: CodeRef,  // Was: func: FuncRef
    args: Vec<Operand>,
},
```

### B. Frame Unification (`datalove-datafun-interp/src/frame.rs`)

#### Before
```rust
impl Frame {
    pub fn new_function(layout: IrLayout, param_count: usize) -> Self {
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(...);
        Self {
            data,
            layout,
            value_initialized: None,  // Functions don't track
            slot_initialized: vec![false; slot_count],
            param_ptrs: vec![std::ptr::null_mut(); param_count],
            param_tydescs: vec![std::ptr::null(); param_count],
            param_initialized: vec![false; param_count],
        }
    }

    pub fn new_script(layout: IrLayout) -> Self {
        let value_count = layout.value_offsets.len();
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(...);
        Self {
            data,
            layout,
            value_initialized: Some(vec![false; value_count]),  // Scripts track
            slot_initialized: vec![false; slot_count],
            param_ptrs: Vec::new(),
            param_tydescs: Vec::new(),
            param_initialized: Vec::new(),
        }
    }
}
```

#### After
```rust
impl Frame {
    /// Create a frame for code unit execution.
    ///
    /// Value tracking is enabled for scripts (for DropTracked cleanup).
    /// Parameter slots are allocated for functions.
    pub fn new(unit: &IrCodeUnit, layout: IrLayout) -> Self {
        let slot_count = layout.slot_offsets.len();
        let data = AlignedBuffer::with_align(
            layout.frame_size as usize,
            layout.frame_align as usize,
        );

        let (value_initialized, param_ptrs, param_tydescs, param_initialized) =
            match &unit.context {
                CodeUnitContext::Function(ctx) => {
                    let param_count = ctx.params.len();
                    (
                        None,  // Functions use precise drops
                        vec![std::ptr::null_mut(); param_count],
                        vec![std::ptr::null(); param_count],
                        vec![false; param_count],
                    )
                }
                CodeUnitContext::Script(_) => {
                    let value_count = layout.value_offsets.len();
                    (
                        Some(vec![false; value_count]),  // Scripts track for cleanup
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                    )
                }
            };

        Self {
            data,
            layout,
            value_initialized,
            slot_initialized: vec![false; slot_count],
            param_ptrs,
            param_tydescs,
            param_initialized,
        }
    }
}
```

### C. Interpreter Unification (`datalove-datafun-interp/src/lib.rs`)

#### Before (two execution paths)
```rust
pub fn call_in_context(&mut self, func: &IrFunction, ...) -> ... {
    let layout = IrLayout::for_function(func);
    let mut frame = Frame::new_function(layout, func.params.len());
    // ... setup params ...
    self.execute_blocks(&func.blocks, &mut frame, ...)
}

pub fn execute_script_unit_in_env(&mut self, unit: &IrScriptUnit, ...) -> ... {
    let layout = IrLayout::for_script_unit(unit);
    let mut frame = Frame::new_script(layout);
    self.execute_blocks(&unit.blocks, &mut frame, ...)
    // ... handle unit_end cleanup ...
}
```

#### After (unified)
```rust
pub fn execute_code_unit(&mut self, unit: &IrCodeUnit, ...) -> Result<...> {
    let layout = IrLayout::for_code_unit(unit);
    let mut frame = Frame::new(unit, layout);

    match &unit.context {
        CodeUnitContext::Function(ctx) => {
            // Setup parameter bindings
            for (i, param) in ctx.params.iter().enumerate() {
                frame.bind_param(*param, args[i].ptr, args[i].tydesc);
            }
        }
        CodeUnitContext::Script(_) => {
            // No parameter setup needed
        }
    }

    let result = self.execute_blocks(&unit.blocks, &mut frame, ...)?;

    match &unit.context {
        CodeUnitContext::Function(_) => {
            // Return value to caller (already handled by Exit terminator)
        }
        CodeUnitContext::Script(ctx) => {
            // Store frame in FrameStore for cross-unit access
            self.frame_store.push_unit(frame, unit);
        }
    }

    Ok(result)
}
```

#### Terminator Handling Change

```rust
// Before:
Terminator::Return { value } => { ... }
Terminator::UnitEnd { result } => { ... }
Terminator::UnitEarlyReturn { value } => { ... }

// After:
Terminator::Exit { value } => {
    // Behavior depends on context (checked at call site)
    ...
}
Terminator::EarlyExit { value } => {
    // Only valid in script context
    ...
}
```

### D. Registry Unification (`datalove-datafun-ir/src/registry.rs`)

#### Before (two registries)
```rust
pub struct UnitFunctionRegistry {
    units: Vec<Vec<IrFunction>>,
}

pub struct ModuleFunctionRegistry {
    functions: HashMap<(IrModuleId, FuncId), IrFunction>,
}
```

#### After (unified)
```rust
/// Registry for resolving code unit references.
pub struct CodeUnitRegistry {
    /// Units from compiled modules.
    module_units: HashMap<(IrModuleId, CodeUnitId), IrCodeUnit>,
    /// Units from script execution sessions.
    script_units: Vec<Vec<IrCodeUnit>>,
}

impl CodeUnitRegistry {
    pub fn new() -> Self {
        Self {
            module_units: HashMap::new(),
            script_units: Vec::new(),
        }
    }

    /// Register a module's code units.
    pub fn register_module(&mut self, module_id: IrModuleId, units: Vec<IrCodeUnit>) {
        for unit in units {
            self.module_units.insert((module_id, unit.id), unit);
        }
    }

    /// Add units from a script execution.
    pub fn add_script_units(&mut self, units: Vec<IrCodeUnit>) {
        self.script_units.push(units);
    }

    /// Resolve a code reference to a unit.
    pub fn resolve(&self, code_ref: &CodeRef) -> Option<&IrCodeUnit> {
        match code_ref {
            CodeRef::Local(_) => None,  // Caller handles local refs
            CodeRef::External { unit, id } => {
                self.script_units.get(*unit as usize)
                    .and_then(|units| units.iter().find(|u| u.id == *id))
            }
            CodeRef::Module { module, id } => {
                self.module_units.get(&(*module, *id))
            }
        }
    }
}
```

### E. Lowering Context Changes (`datalove-datafun-lower/src/context.rs`)

#### LowerCtx.func_scope Change
```rust
// Before:
pub(super) func_scope: HashMap<String, FuncRef>,

// After:
pub(super) unit_scope: HashMap<String, CodeRef>,
```

#### Output Type Changes
```rust
// Before:
pub(super) functions: Vec<IrFunction>,

// After:
pub(super) nested_units: Vec<IrCodeUnit>,
```

### F. Lowering Output Changes

#### lower_function_body (`datalove-datafun-lower/src/func.rs`)

```rust
// Before:
pub fn lower_function_body(...) -> Result<IrFunction, LowerError>

// After:
pub fn lower_function_body(...) -> Result<IrCodeUnit, LowerError> {
    // ... lowering logic unchanged ...

    Ok(IrCodeUnit {
        id: CodeUnitId(func_id.0),
        name: func_name,
        blocks: ctx.body.blocks,
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        call_site_count: ctx.body.next_call_site,
        value_types: ctx.body.value_types,
        slot_types: ctx.body.slot_types,
        tracked_slots: ctx.compute_tracked_slots(),
        const_values: ctx.body.const_values,
        symbols: SymbolTable::new(),
        context: CodeUnitContext::Function(FunctionContext {
            params,
            param_modes,
            param_types,
            return_type,
            tracked_params: ctx.compute_tracked_params(),
        }),
        nested_units: Vec::new(),  // Functions don't have nested units
    })
}
```

#### lower_script_fragment_raw (`datalove-datafun-lower/src/script.rs`)

```rust
// Before:
pub fn lower_script_fragment_raw(...) -> Result<IrScriptUnit, LowerError>

// After:
pub fn lower_script_fragment_raw(...) -> Result<IrCodeUnit, LowerError> {
    // ... lowering logic unchanged ...

    Ok(IrCodeUnit {
        id: CodeUnitId(0),  // Script units don't have meaningful IDs
        name: String::new(),
        blocks: ctx.body.blocks,
        value_count: ctx.body.next_value,
        slot_count: ctx.body.next_slot,
        call_site_count: ctx.body.next_call_site,
        value_types: ctx.body.value_types,
        slot_types: ctx.body.slot_types,
        tracked_slots: ctx.compute_tracked_slots(),
        const_values: ctx.body.const_values,
        symbols: ctx.symbols,
        context: CodeUnitContext::Script(ScriptContext {
            unit_end_values: ctx.compute_unit_end_values(),
            unit_end_slots: ctx.compute_unit_end_slots(),
            result: None,  // Fragment has no result
            exports: ctx.exports,
        }),
        nested_units: ctx.nested_units,  // Functions defined in script
    })
}
```

---

## Validation Checklist

After each phase, verify:

1. **Phase 1** (Add types): `cargo build` passes
2. **Phase 2** (Lowering): `just test` passes with conversions
3. **Phase 3** (Interpreter): All interpreter tests pass
4. **Phase 4** (Registries): Cross-unit/module calls work
5. **Phase 5** (AOT): AOT compilation and execution pass
6. **Phase 6** (Const): Const inlining tests pass
7. **Phase 7** (Pipelines): Full pipeline tests pass
8. **Phase 8** (Delete): Build succeeds after deletion
9. **Phase 9** (Tests): All tests use new types
10. **Phase 10** (Serialization): RON round-trip tests pass

---

## Phase Dependency Graph

```
Phase 1 (IR Types)
    │
    ├───────────────────┬───────────────────┐
    ▼                   ▼                   ▼
Phase 2 (Lowering)  Phase 4 (Registries) Phase 6 (Const)
    │                   │                   │
    └───────────────────┼───────────────────┘
                        ▼
                Phase 3 (Interpreter)
                        │
                        ▼
                Phase 5 (AOT)
                        │
                        ▼
                Phase 7 (Pipelines)
                        │
                        ▼
                Phase 8 (Delete Old Types)
                        │
                ┌───────┴───────┐
                ▼               ▼
        Phase 9 (Tests)  Phase 10 (Serialization)
```

**Parallel work possible**:
- Phases 2, 4, 6 can proceed in parallel after Phase 1
- Phases 9 and 10 can proceed in parallel after Phase 8

---

## Future Extensibility

The unified `CodeUnitContext` enum is designed for extension:

### Closures
```rust
CodeUnitContext::Closure(ClosureContext {
    /// Captured bindings from enclosing scope.
    captures: Vec<CaptureBinding>,
    /// Function-like signature.
    params: Vec<ParamId>,
    param_modes: Vec<ParamMode>,
    param_types: Vec<IrType>,
    return_type: IrType,
})

pub struct CaptureBinding {
    pub name: String,
    pub mode: CaptureMode,  // ByValue, ByRef, ByMut
    pub ty: IrType,
}
```

### Continuations (for async/generators)
```rust
CodeUnitContext::Continuation(ContinuationContext {
    /// Block to resume at.
    resume_point: BlockId,
    /// State that persists across suspensions.
    suspended_slots: Vec<SlotId>,
    /// Yield type.
    yield_type: IrType,
    /// Final return type.
    return_type: IrType,
})
```

### Methods (for future OOP support)
```rust
CodeUnitContext::Method(MethodContext {
    /// Receiver parameter (self).
    receiver: ParamId,
    receiver_mode: ParamMode,  // Ref, Mut, In (consuming)
    /// Rest of signature.
    params: Vec<ParamId>,
    param_modes: Vec<ParamMode>,
    param_types: Vec<IrType>,
    return_type: IrType,
})
```

Each extension adds a new variant without changing existing code paths.

---

## File-by-File Change Summary

| File | Changes |
|------|---------|
| `datalove-datafun-ir/src/lib.rs` | Add IrCodeUnit, CodeRef, contexts; delete IrFunction, IrScriptUnit, FuncRef |
| `datalove-datafun-ir/src/registry.rs` | Replace dual registries with CodeUnitRegistry |
| `datalove-datafun-interp/src/frame.rs` | Unify Frame::new; update FrameStore |
| `datalove-datafun-interp/src/lib.rs` | Unify execute_code_unit; update terminator handling |
| `datalove-datafun-interp/src/env.rs` | Update ExecutionContext to use CodeRef |
| `datalove-datafun-interp/src/dispatch.rs` | Update CallDispatcher to use CodeRef |
| `datalove-datafun-interp/src/dynamic.rs` | Update inlining cache to use CodeRef |
| `datalove-datafun-lower/src/context.rs` | Update LowerCtx.unit_scope, nested_units |
| `datalove-datafun-lower/src/func.rs` | Return IrCodeUnit from lower_function_body |
| `datalove-datafun-lower/src/script.rs` | Return IrCodeUnit from lower_script_* |
| `datalove-datafun-lower/src/module.rs` | Update module lowering to produce IrCodeUnit |
| `datalove-datafun-const/src/inline.rs` | Unify const inlining for IrCodeUnit |
| `datalove-datafun-cranelift/src/codegen/calls.rs` | Resolve CodeRef; implement External variant |
| `datalove-datafun-cranelift/src/codegen/mod.rs` | Compile IrCodeUnit |
| `datalove-datafun-cranelift-aot/src/lib.rs` | Remove script_unit_to_function; compile unified |
| `datalove-datafun/src/pipeline/module_pipeline.rs` | Output Vec<IrCodeUnit> |
| `datalove-datafun/src/pipeline/script_compiler.rs` | Use IrCodeUnit throughout |
| `datalove-datafun/src/pipeline/script_executor.rs` | Execute IrCodeUnit |
| `datalove-datafun/src/pipeline/result.rs` | Update result types |
| `datalove-datafun/tests/*.rs` | Update test helpers |
| `datalove-tests/tests/*.rs` | Update test helpers |

---

## Naming Conventions

To maintain consistency after migration:

| Old Name | New Name |
|----------|----------|
| `IrFunction` | `IrCodeUnit` with `FunctionContext` |
| `IrScriptUnit` | `IrCodeUnit` with `ScriptContext` |
| `FuncId` | `CodeUnitId` |
| `FuncRef` | `CodeRef` |
| `func_scope` | `unit_scope` |
| `functions` (in IrScriptUnit) | `nested_units` |
| `func_id_map` | `unit_id_map` |
| `get_function` | `get_unit` |
| `ModuleFunctionRegistry` | `CodeUnitRegistry` (module part) |
| `UnitFunctionRegistry` | `CodeUnitRegistry` (script part) |
| `call_in_context` | `execute_code_unit` |
| `execute_script_unit_in_env` | `execute_code_unit` |
| `Terminator::Return` | `Terminator::Exit` |
| `Terminator::UnitEnd` | `Terminator::Exit` |
| `Terminator::UnitEarlyReturn` | `Terminator::EarlyExit` |
