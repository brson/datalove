# Implementation Plan: Loop Carry/Bring (Block Parameters)

## Overview

Add `loop carry (...) ... end loop bring (...)` syntax for explicit SSA-style loop induction variables, using block parameters throughout the stack.

**Design reference**: `mandocs/design-notes.md` lines 9-127

## Scope Decisions

- **Basic syntax only**: No `while` condition (add later)
- **No labels**: Only innermost loop break/continue supported
- **Remove Phi**: Delete unused Phi instruction entirely

## Phase 1: Parser & AST

### 1.1 AST Changes (`crates/datalove-datafun-ast/src/ast.rs`)

```rust
/// Carry binding for loop iteration state.
#[salsa::tracked]
pub struct CarryBinding<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<TypeHintAndHeap<'db>>,
    pub init: ExprFun<'db>,
}

/// Bring binding for loop exit values.
#[salsa::tracked]
pub struct BringBinding<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<TypeHintAndHeap<'db>>,
}

/// Loop statement - optionally with carry/bring.
#[salsa::tracked]
pub struct StmtLoop<'db> {
    #[returns(ref)]
    pub carries: Vec<CarryBinding<'db>>,  // empty = no carries
    #[returns(ref)]
    pub body: Vec<Statement<'db>>,
    #[returns(ref)]
    pub brings: Vec<BringBinding<'db>>,   // empty = no brings
}

/// Break with optional values (required if loop has brings).
#[salsa::tracked]
pub struct StmtBreak<'db> {
    #[returns(ref)]
    pub values: Vec<ExprFun<'db>>,  // empty = simple break
}

/// Continue with optional values (required if loop has carries).
#[salsa::tracked]
pub struct StmtContinue<'db> {
    #[returns(ref)]
    pub values: Vec<ExprFun<'db>>,  // empty = simple continue
}
```

### 1.2 Parser Changes (`crates/datalove-datafun-parser/src/statement.rs`)

Modify `parse_loop()`:
1. After `loop`, check for `carry` keyword
2. Parse carry bindings: `(name: type = expr, ...)`
3. Parse body until `end loop`
4. Check for `bring` keyword after `end loop`
5. Parse bring bindings: `(name: type, ...)`

Modify `parse_break()` / `parse_continue()`:
1. After keyword, check for `(` to parse value list

### 1.3 Tests

- `crates/datalove-datafun-compiler/tests/fixtures/parser/`: Add test files for carry/bring syntax variations
- Test: simple loop (no carry/bring), carry only, bring only, both, multiple bindings, type annotations

---

## Phase 2: Type Checker

### 2.1 Type Context Changes (`crates/datalove-datafun-tycheck/src/context.rs`)

```rust
pub struct LoopContext<'db> {
    pub carry_types: Vec<Type<'db>>,  // expected types for continue(...)
    pub bring_types: Vec<Type<'db>>,  // expected types for break(...)
}

// In TypeContext:
pub loop_contexts: Vec<LoopContext<'db>>,  // stack for nested loops
```

### 2.2 Statement Type Checking (`crates/datalove-datafun-tycheck/src/statement.rs`)

For `Statement::Loop`:
1. Type-check carry init expressions
2. Bind carry names in scope with inferred/declared types
3. Push `LoopContext` with carry/bring types
4. Type-check body
5. Pop `LoopContext`
6. Bind bring names in outer scope

For `Statement::Break`:
1. Check loop context exists
2. If values provided, check count and types match `bring_types`
3. If loop has brings but break has no values, error

For `Statement::Continue`:
1. Check loop context exists
2. If values provided, check count and types match `carry_types`
3. If loop has carries but continue has no values, error

### 2.3 Tests

- Type mismatch errors for carry/bring
- Missing values when required
- Scope: carries visible in body, brings visible after loop

---

## Phase 3: Drop Analysis

### 3.1 Changes (`crates/datalove-datafun-compiler/src/drop_analysis.rs`)

Key insight: Carry variables are NOT dropped at loop body end - they're passed to next iteration or to break.

1. Track carry bindings separately from regular loop-body bindings
2. `loop_body_end` drops: exclude carry bindings
3. `before_break` drops: exclude carry bindings (they become bring values)
4. `before_continue` drops: exclude carry bindings (they're passed via continue)

New fields in `DropSchedule` (or reuse existing with modified logic):
- Carry bindings have special lifetime: live from loop entry to continue/break

### 3.2 Tests

- Verify carry variables not double-dropped
- Verify non-carry loop bindings still dropped correctly

---

## Phase 4: IR Changes

### 4.1 Block Parameters (`crates/datalove-datafun-ir/src/lib.rs`)

```rust
pub struct IrBlock {
    pub id: BlockId,
    pub params: Vec<ValueId>,  // NEW: block parameters
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

pub enum Terminator {
    Goto {
        target: BlockId,
        args: Vec<Operand>,  // CHANGED: was Goto(BlockId)
    },
    Branch {
        cond: Operand,
        then_block: BlockId,
        then_args: Vec<Operand>,  // NEW
        else_block: BlockId,
        else_args: Vec<Operand>,  // NEW
    },
    // ... rest unchanged
}
```

### 4.2 Remove Phi Instruction

Delete `Instruction::Phi` entirely - it's currently unused by the compiler. This includes:
- Remove from `Instruction` enum in `lib.rs`
- Remove display code in `display.rs`
- Remove `execute_phi()` from interpreter
- Remove `PhiMissingPredecessor` error variant
- Remove Phi tests from `tests.rs`

### 4.3 Update Display (`crates/datalove-datafun-ir/src/display.rs`)

Print block params: `block_3(v5, v6):`
Print goto args: `goto block_3(v7, v8)`

### 4.4 Tests

- IR display tests showing block params

---

## Phase 5: Lowering

### 5.1 Context Changes (`crates/datalove-datafun-compiler/src/lower/context.rs`)

```rust
pub struct LoopLowerContext {
    pub header: BlockId,
    pub exit: BlockId,
    pub carry_values: Vec<ValueId>,  // current iteration's carry ValueIds
    pub bring_values: Vec<ValueId>,  // ValueIds in exit block params
}

// Change loop_stack from Vec<(BlockId, BlockId)> to:
pub loop_stack: Vec<LoopLowerContext>,
```

### 5.2 Loop Lowering (`crates/datalove-datafun-compiler/src/lower/stmt.rs`)

```
lower_loop():
  1. Create loop_header block with params for each carry
  2. Create loop_exit block with params for each bring
  3. Lower carry init expressions
  4. Emit Goto { target: loop_header, args: carry_inits }
  5. Start loop_header block, bind carry params to ValueIds
  6. Push LoopLowerContext
  7. Lower body statements
  8. Emit loop_body_end drops (excluding carries)
  9. Emit Goto { target: loop_header, args: [current carry values] }
     (but this shouldn't be reached if all paths break/continue)
  10. Pop LoopLowerContext
  11. Start loop_exit block, bind bring params to ValueIds
  12. Bind bring names in scope
```

### 5.3 Break/Continue Lowering

```
lower_break(values):
  1. Lower value expressions
  2. Emit before_break drops (excluding carries)
  3. Emit Goto { target: loop_exit, args: values }

lower_continue(values):
  1. Lower value expressions
  2. Emit before_continue drops (excluding carries)
  3. Emit Goto { target: loop_header, args: values }
```

### 5.4 Tests

- IR output tests showing block params and goto args

---

## Phase 6: Interpreter

### 6.1 Block Entry (`crates/datalove-datafun-interp/src/lib.rs`)

In `execute_blocks()`, when transitioning to a new block:

```rust
// After determining next block from terminator:
let args = match &block.terminator {
    Terminator::Goto { args, .. } => args,
    Terminator::Branch { then_args, else_args, .. } =>
        if took_then { then_args } else { else_args },
    _ => &[],
};

// Write args to target block's params
let target_block = blocks.iter().find(|b| b.id == next_block_id).unwrap();
for (param_id, arg) in target_block.params.iter().zip(args) {
    let src = self.read_operand(arg, frame, frames)?;
    let dest = frame.value_dest(*param_id)?;
    unsafe { self.move_value(&src, dest)?; }
    frame.mark_value_initialized(*param_id);
}
```

### 6.2 Remove Phi Code

(Already covered in Phase 4 - delete `execute_phi()`, Phi case, and PhiMissingPredecessor error)

### 6.3 Tests

- Interpreter tests with carry/bring loops
- Test iteration state correctly passed
- Test break values correctly received

---

## Phase 7: AOT (Cranelift)

### 7.1 Block Parameters (`crates/datalove-datafun-aot-cranelift/src/codegen/mod.rs`)

When creating blocks, add parameters:

```rust
for block in &self.func.blocks {
    let cl_block = builder.create_block();
    for param_id in &block.params {
        let ty = self.value_type(*param_id);
        let cl_ty = self.ir_type_to_cranelift(ty);
        builder.append_block_param(cl_block, cl_ty);
    }
    self.blocks.insert(block.id, cl_block);
}
```

### 7.2 Terminator Args (`crates/datalove-datafun-aot-cranelift/src/codegen/terminators.rs`)

```rust
Terminator::Goto { target, args } => {
    let block = self.blocks[target];
    let cl_args: Vec<_> = args.iter()
        .map(|op| self.get_operand_value(builder, op))
        .collect::<Result<_, _>>()?;
    builder.ins().jump(block, &cl_args);
}
```

### 7.3 Reading Block Params

At block entry, map IR ValueIds to Cranelift block params:

```rust
let cl_block = self.blocks[&block.id];
let cl_params = builder.block_params(cl_block);
for (ir_param, &cl_param) in block.params.iter().zip(cl_params) {
    self.values.insert(*ir_param, cl_param);
}
```

### 7.4 Tests

- AOT compilation tests with carry/bring loops
- Verify generated code executes correctly

---

## Implementation Order

Execute phases sequentially, with full test pass before proceeding:

1. **Parser/AST** - syntax parsing works
2. **Type Checker** - type errors caught correctly
3. **Drop Analysis** - no memory leaks or double-frees
4. **IR** - block params represented correctly
5. **Lowering** - AST->IR produces correct block params
6. **Interpreter** - execution works correctly
7. **AOT** - compiled code works correctly

---

## Key Files

| Phase | Files |
|-------|-------|
| AST | `crates/datalove-datafun-ast/src/ast.rs` |
| Parser | `crates/datalove-datafun-parser/src/statement.rs` |
| Type Check | `crates/datalove-datafun-tycheck/src/{context,statement}.rs` |
| Drop Analysis | `crates/datalove-datafun-compiler/src/drop_analysis.rs` |
| IR | `crates/datalove-datafun-ir/src/{lib,display}.rs` |
| Lowering | `crates/datalove-datafun-compiler/src/lower/{context,stmt}.rs` |
| Interpreter | `crates/datalove-datafun-interp/src/lib.rs` |
| AOT | `crates/datalove-datafun-aot-cranelift/src/codegen/{mod,terminators}.rs` |

## Test Fixtures

New test files in `crates/datalove-datafun-compiler/tests/fixtures/`:
- `parser/70_loop_carry_simple.dfs`
- `parser/71_loop_bring_simple.dfs`
- `parser/72_loop_carry_bring.dfs`
- `parser/73_loop_carry_multi.dfs`
- `interp/` and `interp2/` for execution tests
