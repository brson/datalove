# Interpreter/AOT Code Sharing Refactor

## Overview

Refactor to share code between the interpreter (`datalove-datafun-interp`) and AOT compiler (`datalove-datafun-aot-cranelift`), preparing for a future third backend.

**New crates to create:**
- `datalove-datafun-ops` - Operation semantics
- `datalove-datafun-backend-common` - Abstract emitter trait
- `datalove-rt-spec` - Runtime function specifications

---

## Phase 1: Unified Type Layout (Proposal 1)

**Goal:** Single source of truth for type sizes, alignment, and frame layout computation.

### Add to IR crate: `crates/datalove-datafun-ir/src/layout.rs`

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
pub fn enum_variant_offsets(variants: &[(String, Option<IrType>)]) -> Vec<u32>;
```

### Files to modify:
- `crates/datalove-datafun-ir/src/lib.rs` - Add `pub mod layout;`
- `crates/datalove-datafun-ir/Cargo.toml` - Add `datalove-rtdt` dependency
- `crates/datalove-datafun-aot-cranelift/src/types.rs` - Use shared `type_layout()`, remove duplicates
- `crates/datalove-datafun-aot-cranelift/src/layout.rs` - Use shared `FrameLayout`, keep `CraneliftRepr` augmentation
- `crates/datalove-datafun-interp/src/layout.rs` - Use shared layout, keep `TyDesc` pointers

---

## Phase 2: Operation Semantics (Proposal 2)

**Goal:** Declarative module defining operation categories, valid types, result types.

### Create crate: `crates/datalove-datafun-ops/`

```rust
// src/lib.rs
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

### Migration:
- Move `BinOp`, `UnaryOp` from IR crate to ops crate
- IR crate re-exports them for backward compatibility

### Files:
- Create `crates/datalove-datafun-ops/Cargo.toml`
- Create `crates/datalove-datafun-ops/src/lib.rs`
- Modify `crates/datalove-datafun-ir/Cargo.toml` - depend on ops
- Modify `crates/datalove-datafun-ir/src/lib.rs` - re-export `BinOp`, `UnaryOp`

---

## Phase 3: Runtime Call Specifications (Proposal 3)

**Goal:** Single source of truth for runtime function signatures.

### Create crate: `crates/datalove-rt-spec/`

```rust
// src/lib.rs
pub enum RtParamType { RtHandle, Ptr, U8, U32 }
pub struct RtParam { pub name: &'static str, pub ty: RtParamType }
pub struct RtFuncSpec {
    pub name: &'static str,
    pub params: &'static [RtParam],
    pub ret: Option<RtParamType>,
}

// src/funcs.rs - organized by category
pub mod list {
    pub const CREATE_LOCAL: RtFuncSpec = ...;
    pub const PUSH_LOCAL: RtFuncSpec = ...;
    pub const BUILD_FROM_SLICE_LOCAL: RtFuncSpec = ...; // New
}
pub mod set { ... }
pub mod map { ... }
pub mod int { ... }
pub mod lifecycle { ... }
```

### AOT integration:
```rust
// runtime.rs
impl RuntimeImports {
    fn declare_from_spec<M: Module>(module: &mut M, spec: &RtFuncSpec) -> FuncId;
}
```

### Files:
- Create `crates/datalove-rt-spec/Cargo.toml`
- Create `crates/datalove-rt-spec/src/lib.rs`
- Create `crates/datalove-rt-spec/src/funcs.rs`
- Modify `crates/datalove-datafun-aot-cranelift/Cargo.toml` - depend on rt-spec
- Modify `crates/datalove-datafun-aot-cranelift/src/runtime.rs` - use specs

---

## Phase 4: Collection Construction Helpers (Proposal 6)

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

## Phase 5: Abstract Instruction Emitter (Proposal 4)

**Goal:** Write dispatch logic once; both backends implement trait.

### Create crate: `crates/datalove-datafun-backend-common/`

```rust
// src/emitter.rs
pub trait InstructionEmitter {
    type Error: std::error::Error;
    type Context<'a>;

    fn emit_const(&mut self, ctx: &mut Self::Context<'_>, dest: ValueId, value: &ConstValue) -> Result<(), Self::Error>;
    fn emit_copy(&mut self, ctx: &mut Self::Context<'_>, dest: ValueId, src: &Operand) -> Result<(), Self::Error>;
    fn emit_move(&mut self, ctx: &mut Self::Context<'_>, dest: ValueId, src: &Operand) -> Result<(), Self::Error>;
    fn emit_binop(&mut self, ctx: &mut Self::Context<'_>, dest: ValueId, op: BinOp, lhs: &Operand, rhs: &Operand) -> Result<(), Self::Error>;
    // ... all instruction types ...
    fn emit_nop(&mut self, ctx: &mut Self::Context<'_>) -> Result<(), Self::Error>;
}

// src/dispatch.rs
pub fn dispatch_instruction<E: InstructionEmitter>(
    emitter: &mut E,
    ctx: &mut E::Context<'_>,
    instruction: &Instruction,
) -> Result<(), E::Error> {
    match instruction {
        Instruction::Const { dest, value } => emitter.emit_const(ctx, *dest, value),
        Instruction::BinOp { dest, op, lhs, rhs } => emitter.emit_binop(ctx, *dest, *op, lhs, rhs),
        // ... all 30+ instruction variants ...
    }
}
```

### Interpreter implementation:
```rust
// InterpContext bundles Frame, FrameStore, etc.
impl InstructionEmitter for IrInterpreter {
    type Error = InterpError;
    type Context<'a> = InterpContext<'a>;
    // Each method delegates to existing execute_* functions
}
```

### AOT implementation:
```rust
impl<'a, M: Module> InstructionEmitter for FunctionCompiler<'a, M> {
    type Error = AotError;
    type Context<'c> = &'c mut FunctionBuilder<'c>;
    // Each method delegates to existing compile_* functions
}
```

### Files:
- Create `crates/datalove-datafun-backend-common/Cargo.toml`
- Create `crates/datalove-datafun-backend-common/src/lib.rs`
- Create `crates/datalove-datafun-backend-common/src/emitter.rs`
- Create `crates/datalove-datafun-backend-common/src/dispatch.rs`
- Modify `crates/datalove-datafun-interp/src/lib.rs` - implement trait, use dispatch
- Modify `crates/datalove-datafun-aot-cranelift/src/codegen/mod.rs` - implement trait, use dispatch

---

## Implementation Order

1. **Phase 1 (Layout)** - Foundation, no breaking changes
2. **Phase 2 (Ops)** - Move enums, add semantics
3. **Phase 3 (RT Spec)** - New crate, incremental AOT adoption
4. **Phase 4 (Collections)** - New runtime functions, update AOT
5. **Phase 5 (Emitter)** - Abstract trait, migrate both backends

Each phase is independently testable. Existing tests should pass after each phase.

---

## New Crate Dependency Graph

```
datalove-datafun-ops (new)
    |
    v
datalove-datafun-ir
    |
    v
datalove-datafun-backend-common (new)
    |
    +---> datalove-datafun-interp
    |
    +---> datalove-datafun-aot-cranelift
                |
                v
          datalove-rt-spec (new)
                |
                v
          datalove-rt
```

---

## Critical Files Summary

**Create:**
- `crates/datalove-datafun-ir/src/layout.rs`
- `crates/datalove-datafun-ops/` (new crate)
- `crates/datalove-rt-spec/` (new crate)
- `crates/datalove-datafun-backend-common/` (new crate)

**Modify (Layout):**
- `crates/datalove-datafun-aot-cranelift/src/types.rs`
- `crates/datalove-datafun-aot-cranelift/src/layout.rs`
- `crates/datalove-datafun-interp/src/layout.rs`

**Modify (Runtime):**
- `crates/datalove-rt/src/c.rs`
- `crates/datalove-rt/src/impls/list.rs`
- `crates/datalove-rt/src/impls/set.rs`
- `crates/datalove-rt/src/impls/btreemap.rs`
- `crates/datalove-datafun-aot-cranelift/src/runtime.rs`
- `crates/datalove-datafun-aot-cranelift/src/codegen/collections.rs`

**Modify (Emitter):**
- `crates/datalove-datafun-interp/src/lib.rs`
- `crates/datalove-datafun-aot-cranelift/src/codegen/mod.rs`
