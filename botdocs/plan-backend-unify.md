# Interpreter/AOT Code Sharing Refactor

## Overview

Refactor to share code between the interpreter (`datalove-datafun-interp`) and AOT compiler (`datalove-datafun-aot-cranelift`), preparing for a future third backend.

## Crate Structure

**One new crate:** `datalove-datafun-eval` - Kit for building datafun evaluators

```
datalove-datafun-ir        (existing - IR definitions)
        |
        v
datalove-datafun-eval      (NEW - evaluator kit)
        |
        +---> datalove-datafun-interp      (existing - uses kit)
        |
        +---> datalove-datafun-aot-cranelift (existing - uses kit)

datalove-rt                (existing - runtime library)
```

**`datalove-datafun-eval` contains:**
- `layout` - Type/frame layout computation
- `ops` - Operation semantics (categories, valid types)
- `emitter` - Abstract instruction emitter trait
- `rt_spec` - Runtime function specifications

**Dependencies:**
- `datalove-datafun-eval` depends on `datalove-datafun-ir`
- `datalove-datafun-interp` depends on `datalove-datafun-eval`
- `datalove-datafun-aot-cranelift` depends on `datalove-datafun-eval`

---

## Phase 1: Create `datalove-datafun-eval` Crate

### Structure: `crates/datalove-datafun-eval/`

```
src/
  lib.rs          - Re-exports all modules
  layout.rs       - Type sizes, alignment, frame layout
  ops.rs          - BinOp/UnaryOp semantics
  emitter.rs      - InstructionEmitter trait
  dispatch.rs     - dispatch_instruction() function
  rt_spec/
    mod.rs        - Runtime spec types
    funcs.rs      - All runtime function specs
```

### `layout.rs` - Type and Frame Layout

```rust
pub struct TypeLayout { pub size: u32, pub align: u32 }
pub struct ItemLayout { pub offset: u32, pub size: u32, pub align: u32 }
pub struct FrameLayout {
    pub params: Vec<ItemLayout>,
    pub values: Vec<ItemLayout>,
    pub slots: Vec<ItemLayout>,
    pub frame_size: u32,
    pub frame_align: u32,
}

pub fn type_layout(ty: &IrType) -> TypeLayout;
pub fn compute_frame_layout(param_types, value_types, slot_types) -> FrameLayout;
pub fn tuple_field_offsets(fields: &[IrType]) -> Vec<u32>;
pub fn struct_field_offsets(fields: &[(String, IrType)]) -> Vec<u32>;
```

### `ops.rs` - Operation Semantics

```rust
pub enum BinOpCategory {
    Arithmetic,        // Add, Sub, Mul
    ArithmeticChecked, // Div, Mod (may error)
    Comparison,        // Lt, Le, Gt, Ge, Eq, Ne -> Bool
    Logical,           // And, Or (Bool only)
    Bitwise,           // BitAnd, BitOr, BitXor
    Shift,             // Shl, Shr
}

pub enum UnaryOpCategory { Negate, LogicalNot, BitwiseNot }
pub enum TypeClass { SignedInt, UnsignedInt, BigInt, Float, Bool }

pub struct BinOpSemantics {
    pub category: BinOpCategory,
    pub valid_type_classes: &'static [TypeClass],
    pub result_is_bool: bool,
    pub can_overflow: bool,
}

impl BinOp { pub fn semantics(&self) -> BinOpSemantics; }
impl UnaryOp { pub fn semantics(&self) -> UnaryOpSemantics; }
```

### `emitter.rs` - Abstract Instruction Emitter

```rust
pub trait InstructionEmitter {
    type Error: std::error::Error;
    type Context<'a>;

    fn emit_const(&mut self, ctx: &mut Self::Context<'_>, dest: ValueId, value: &ConstValue) -> Result<(), Self::Error>;
    fn emit_binop(&mut self, ctx: &mut Self::Context<'_>, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> Result<(), Self::Error>;
    // ... all ~30 instruction types ...
}

pub fn dispatch_instruction<E: InstructionEmitter>(
    emitter: &mut E,
    ctx: &mut E::Context<'_>,
    instruction: &Instruction,
) -> Result<(), E::Error>;
```

### `rt_spec/` - Runtime Function Specifications

```rust
pub enum RtParamType { RtHandle, Ptr, U8, U32 }
pub struct RtParam { pub name: &'static str, pub ty: RtParamType }
pub struct RtFuncSpec {
    pub name: &'static str,
    pub params: &'static [RtParam],
    pub ret: Option<RtParamType>,
}

pub mod funcs {
    pub mod list { pub const CREATE_LOCAL: RtFuncSpec = ...; }
    pub mod set { ... }
    pub mod map { ... }
    pub mod int { ... }
}
```

---

## Phase 2: Add Collection Bulk Construction to Runtime

**Goal:** Bulk runtime functions to construct collections from slices.

### New runtime functions in `crates/datalove-rt/src/c.rs`:

```rust
// List: takes ownership of element buffer, builds list
pub unsafe extern "C-unwind" fn dtlv_rti_list_build_from_slice_local(
    rt: LocalRtHandle,
    elements_ptr: *mut u8,
    count: u32,
    element_tydesc: *const TyDesc,
    list_out: *mut u8,
    list_tydesc: *const TyDesc,
) -> RtStatus;

// Set: takes ownership, sorts internally, builds B-tree
pub unsafe extern "C-unwind" fn dtlv_rti_set_build_from_slice_local(
    rt: LocalRtHandle,
    elements_ptr: *mut u8,
    count: u32,
    element_tydesc: *const TyDesc,
    set_out: *mut u8,
    set_tydesc: *const TyDesc,
) -> RtStatus;

// Map: takes ownership of key/value buffers, sorts, builds B-tree
pub unsafe extern "C-unwind" fn dtlv_rti_map_build_from_slices_local(
    rt: LocalRtHandle,
    keys_ptr: *mut u8,
    values_ptr: *mut u8,
    count: u32,
    key_tydesc: *const TyDesc,
    value_tydesc: *const TyDesc,
    map_out: *mut u8,
    map_tydesc: *const TyDesc,
) -> RtStatus;
```

### Files:
- `crates/datalove-rt/src/c.rs` - Add new C ABI functions
- `crates/datalove-rt/src/impls/list.rs` - Add `list_build_from_slice_impl`
- `crates/datalove-rt/src/impls/set.rs` - Add `set_build_from_slice_impl` with sort helper
- `crates/datalove-rt/src/impls/btreemap.rs` - Add `map_build_from_slices_impl`
- `crates/datalove-datafun-aot-cranelift/src/runtime.rs` - Add imports
- `crates/datalove-datafun-aot-cranelift/src/codegen/collections.rs` - Use bulk functions

---

## Phase 3: Migrate Backends to Use Eval Kit

### Interpreter (`datalove-datafun-interp`)

1. Add dependency on `datalove-datafun-eval`
2. Replace local `layout.rs` with imports from eval kit
3. Implement `InstructionEmitter` trait
4. Replace `execute_instruction` match with `dispatch_instruction`

### AOT (`datalove-datafun-aot-cranelift`)

1. Add dependency on `datalove-datafun-eval`
2. Replace local `layout.rs` and `types.rs` layout code with imports
3. Use `rt_spec` to declare runtime imports
4. Use bulk collection functions
5. Implement `InstructionEmitter` trait
6. Replace `compile_instruction` match with `dispatch_instruction`

---

## Implementation Order

1. **Phase 1** - Create `datalove-datafun-eval` crate with all modules
2. **Phase 2** - Add bulk collection functions to `datalove-rt`
3. **Phase 3** - Migrate interpreter to use eval kit
4. **Phase 4** - Migrate AOT to use eval kit

Each phase is independently testable. Existing tests should pass after each phase.

---

## Files Summary

**Create (new crate):**
```
crates/datalove-datafun-eval/
  Cargo.toml
  src/lib.rs
  src/layout.rs
  src/ops.rs
  src/emitter.rs
  src/dispatch.rs
  src/rt_spec/mod.rs
  src/rt_spec/funcs.rs
```

**Modify (runtime - bulk collections):**
- `crates/datalove-rt/src/c.rs`
- `crates/datalove-rt/src/impls/list.rs`
- `crates/datalove-rt/src/impls/set.rs`
- `crates/datalove-rt/src/impls/btreemap.rs`

**Modify (interpreter):**
- `crates/datalove-datafun-interp/Cargo.toml`
- `crates/datalove-datafun-interp/src/lib.rs`
- `crates/datalove-datafun-interp/src/layout.rs`

**Modify (AOT):**
- `crates/datalove-datafun-aot-cranelift/Cargo.toml`
- `crates/datalove-datafun-aot-cranelift/src/types.rs`
- `crates/datalove-datafun-aot-cranelift/src/layout.rs`
- `crates/datalove-datafun-aot-cranelift/src/runtime.rs`
- `crates/datalove-datafun-aot-cranelift/src/codegen/mod.rs`
- `crates/datalove-datafun-aot-cranelift/src/codegen/collections.rs`
