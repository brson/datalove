# Fully CFG-Driven Interpreter with Flat IR

## Goal

Transform the interpreter from hybrid tree-walking to fully CFG-driven execution with flat instructions, inspired by Move's stackless bytecode.

## Move's Stackless Bytecode Design (Reference)

Move uses a flat IR that maps directly to LLVM:

```rust
pub enum Bytecode {
    Assign(AttrId, TempIndex, TempIndex, AssignKind),  // dest = src
    Call(AttrId, Vec<TempIndex>, Operation, Vec<TempIndex>, Option<AbortAction>),
    Ret(AttrId, Vec<TempIndex>),
    Load(AttrId, TempIndex, Constant),
    Branch(AttrId, Label, Label, TempIndex),  // if cond goto L1 else L2
    Jump(AttrId, Label),
    Label(AttrId, Label),
    Abort(AttrId, TempIndex),
    Nop(AttrId),
}
```

Key insight: "TempIndex maps to alloca, Labels to basic blocks."

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
  +-- Vec<Instruction>  (flat sequence)
        +-- execute_instruction() (no recursion)
  +-- Terminator
```

### Instruction Enum

```rust
/// Index into local/temp value table (like Move's TempIndex).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueId(u32);

/// Flat instruction - no nesting, 2-3 operands max.
pub enum Instruction {
    // Loads
    LoadConst { dest: ValueId, value: ConstValue },
    Copy { dest: ValueId, src: ValueId },
    Move { dest: ValueId, src: ValueId },

    // Arithmetic (simple, always succeeds)
    BinOp { dest: ValueId, op: BinOp, lhs: ValueId, rhs: ValueId },
    UnaryOp { dest: ValueId, op: UnaryOp, operand: ValueId },

    // Checked arithmetic (sets overflow flag)
    BinOpChecked { dest: ValueId, overflow: ValueId, op: BinOp, lhs: ValueId, rhs: ValueId },
    UnaryOpChecked { dest: ValueId, overflow: ValueId, op: UnaryOp, operand: ValueId },

    // Function calls
    Call { dest: ValueId, func: FuncId, args: Vec<ValueId> },

    // Struct/tuple operations
    Pack { dest: ValueId, ty: TypeId, fields: Vec<ValueId> },
    Unpack { dests: Vec<ValueId>, src: ValueId },
    FieldAccess { dest: ValueId, base: ValueId, field: FieldId },
    TupleIndex { dest: ValueId, base: ValueId, index: u32 },

    // Option/Result operations
    WrapSome { dest: ValueId, inner: ValueId },
    WrapOk { dest: ValueId, inner: ValueId },
    WrapErr { dest: ValueId, inner: ValueId },
    WrapNone { dest: ValueId },
    UnwrapOption { dest: ValueId, is_some: ValueId, src: ValueId },
    UnwrapResult { dest: ValueId, is_ok: ValueId, src: ValueId },

    // Collections
    ListNew { dest: ValueId, elements: Vec<ValueId> },
    SetNew { dest: ValueId, elements: Vec<ValueId> },
    MapNew { dest: ValueId, entries: Vec<(ValueId, ValueId)> },

    // Memory
    Drop { value: ValueId },

    // Debug/tracking
    Nop,
}
```

### Terminator (largely unchanged)

```rust
pub enum Terminator {
    Goto(BlockId),
    Branch { cond: ValueId, then_block: BlockId, else_block: BlockId },
    Return { value: Option<ValueId> },
    TryReturn { value: Option<ValueId> },  // early return
}
```

### Lowering Pass: AST -> Flat IR

Transform nested expressions into instruction sequences:

```
// Source: let x = (a + b) * c
// Lowered:
  t0 = Copy(a)
  t1 = Copy(b)
  t2 = BinOp(Add, t0, t1)
  t3 = Copy(c)
  t4 = BinOp(Mul, t2, t3)
  x = Move(t4)

// Source: let x = foo()?
// Lowered:
  t0 = Call(foo, [])
  t1, is_some = UnwrapOption(t0)
  Branch(is_some, cont_block, early_return_block)
cont_block:
  x = Move(t1)
  ...
early_return_block:
  WrapNone(return_dest)
  TryReturn
```

### Interpreter Loop (no recursion)

```rust
fn execute(&mut self) -> Result<Value, InterpError> {
    loop {
        let block = &self.cfg.blocks[self.current_block];

        // Execute all instructions (no recursion!)
        for instr in &block.instructions {
            self.execute_instruction(instr)?;
        }

        // Handle terminator
        match &block.terminator {
            Terminator::Goto(target) => self.current_block = *target,
            Terminator::Branch { cond, then_block, else_block } => {
                self.current_block = if self.read_bool(*cond) {
                    *then_block
                } else {
                    *else_block
                };
            }
            Terminator::Return { value } => {
                return Ok(self.read_value(*value));
            }
            Terminator::TryReturn { value } => {
                return Ok(self.read_value(*value));
            }
        }
    }
}

fn execute_instruction(&mut self, instr: &Instruction) -> Result<(), InterpError> {
    match instr {
        Instruction::LoadConst { dest, value } => {
            self.write_value(*dest, value.clone());
        }
        Instruction::BinOp { dest, op, lhs, rhs } => {
            let l = self.read_value(*lhs);
            let r = self.read_value(*rhs);
            let result = eval_binop(*op, l, r)?;
            self.write_value(*dest, result);
        }
        // ... all simple, non-recursive
    }
    Ok(())
}
```

## Implementation Phases

### Phase 1: Define IR Types

New file: `crates/datalove-datafun-compiler/src/ir/mod.rs`
- `ValueId` struct
- `Instruction` enum
- `IrBlock` struct (instructions + terminator)
- `IrFunction` struct (blocks + value table)

### Phase 2: Lowering Pass

New file: `crates/datalove-datafun-compiler/src/ir/lower.rs`
- `lower_function(FunctionDef) -> IrFunction`
- `lower_statement(Statement, &mut LowerCtx) -> Vec<Instruction>`
- `lower_expression(ExprFun, &mut LowerCtx) -> ValueId`
- Emit instructions, allocate temps, handle control flow

### Phase 3: IR Interpreter

New file: `crates/datalove-datafun-compiler/src/ir/interp.rs`
- `IrInterpreter` struct with value table
- `execute(&mut self) -> Result<Value, InterpError>`
- `execute_instruction(&mut self, instr: &Instruction)`
- Simple loop, no recursion

### Phase 4: Integration

- Wire up: parse -> typecheck -> lower -> interpret
- Keep old interpreter for comparison/fallback
- Run test suite against both

### Phase 5: Cleanup

- Remove old tree-walking interpreter (or keep as reference)
- Optimize IR representation
- Add IR pretty-printing for debugging

## Files to Create/Modify

**New files:**
- `crates/datalove-datafun-compiler/src/ir/mod.rs` - IR types
- `crates/datalove-datafun-compiler/src/ir/lower.rs` - AST->IR lowering
- `crates/datalove-datafun-compiler/src/ir/interp.rs` - IR interpreter

**Modify:**
- `crates/datalove-datafun-compiler/src/lib.rs` - add `mod ir`
- Entry points to wire up new pipeline

## Benefits

1. **No EvalResult needed** - control flow is explicit CFG edges
2. **Easier reasoning** - flat instruction stream
3. **Better for optimization** - standard compiler techniques apply
4. **LLVM-ready** - direct mapping like Move
5. **Debugging** - step through individual instructions

## Design Decisions

1. **Value representation**: Keep DPS with slot offsets - reuse existing FrameLayout infrastructure
2. **Migration**: Parallel execution - run both interpreters, compare results for validation
3. **Drop handling**: Keep computed drop points from existing DropPoints analysis
4. **Scope**: Full language from the start

## ValueId Design

ValueId maps to existing slot infrastructure:

```rust
/// References a slot in the frame (reuses FrameLayout slots).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueId(pub SlotId);

impl ValueId {
    pub fn offset(&self, layout: &FrameLayout) -> u32 {
        layout.slots[self.0.0 as usize].offset
    }
}
```

Instructions read/write via slot offsets into `frame_data: Vec<u8>`, exactly like current interpreter.
