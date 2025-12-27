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

### Phase 1: Define IR Types

New file: `crates/datalove-datafun-compiler/src/ir/mod.rs`
- `ValueId`, `SlotId`, `Operand`
- `Instruction` enum with SSA semantics
- `IrBlock` struct (instructions + terminator)
- `IrFunction` struct (blocks + layout)

### Phase 2: Lowering Pass

New file: `crates/datalove-datafun-compiler/src/ir/lower.rs`
- `lower_function(FunctionDef) -> IrFunction`
- Expression temps and `let` bindings -> ValueId
- `var` bindings -> SlotId with SlotStore/SlotLoad
- Control flow joins -> Phi nodes

### Phase 3: IR Interpreter

New file: `crates/datalove-datafun-compiler/src/ir/interp.rs`
- `IrInterpreter` with frame buffer
- `IrLayout` maps ValueId/SlotId to offsets
- Simple loop, no recursion

### Phase 4: Integration

- Wire up: parse -> typecheck -> lower -> interpret
- Keep old interpreter for comparison
- Run test suite against both

### Phase 5: Cleanup

- Remove old tree-walking interpreter
- Add IR pretty-printing

## Files to Create/Modify

**New files:**
- `crates/datalove-datafun-compiler/src/ir/mod.rs` - IR types
- `crates/datalove-datafun-compiler/src/ir/lower.rs` - AST->IR lowering
- `crates/datalove-datafun-compiler/src/ir/interp.rs` - IR interpreter

**Modify:**
- `crates/datalove-datafun-compiler/src/lib.rs` - add `mod ir`
- Entry points to wire up new pipeline

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
