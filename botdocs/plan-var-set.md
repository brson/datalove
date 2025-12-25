# Implement Mutable Bindings: `var` and `set`

## Problem

Once shadowing is fixed, loop accumulation patterns like:
```
let n: u32 = @0
loop
    let n = n +! @1  // relies on broken semantics
    ...
end loop
```
...will break. With proper shadowing, each `let n = ...` in the loop creates a new slot, but the RHS `n` is statically resolved to the *outer* slot, so the accumulator never updates.

## Solution

Implement explicit mutable bindings:
- `var` declares a mutable variable
- `set` mutates an existing mutable variable

```
var n: u32 = @0
loop
    set n = n +! @1
    if n >= @3
        break
    end if
end loop
ret ok n
```

## Syntax (from demo-datafun-script.dfs)

```
var i = 0
set i += 1

var a = 0
if should_do_case_1
    set a = 1
else
    set a = 3
end if
```

## Implementation

### 1. AST Changes (`ast.rs`)

Add new statement types:
```rust
pub enum Statement<'db> {
    Let(StmtLet<'db>),
    Var(StmtVar<'db>),   // NEW
    Set(StmtSet<'db>),   // NEW
    Fun(StmtFun<'db>),
    // ...
}

#[salsa::tracked]
pub struct StmtVar<'db> {
    pub name: InternedText<'db>,
    pub type_hint: Option<datalit::ast::TypeHintAndHeap<'db>>,
    pub value: ExprFun<'db>,
}

#[salsa::tracked]
pub struct StmtSet<'db> {
    pub name: InternedText<'db>,
    pub op: Option<SetOp>,  // None for `set x = ...`, Some for `set x += ...`
    pub value: ExprFun<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SetOp {
    Add,    // +=
    Sub,    // -=
    Mul,    // *=
    Div,    // /=
    // Add more as needed
}
```

### 2. Parser Changes (`parser.rs`)

Add `var` and `set` keywords. Parse like `let` but with different statement types.

### 3. Slot Allocation (`slot_allocation.rs`)

Add new SlotKind variant:
```rust
pub enum SlotKind {
    Reference,
    Local,      // immutable let binding
    Mutable,    // mutable var binding (NEW)
    Temporary,
}
```

For `StmtVar`: allocate a `Mutable` slot, update scope.
For `StmtSet`: no new slot - just record the assignment for the interpreter.

### 4. Name Resolution

Store `set` statement → slot mappings:
```rust
pub struct FrameLayout<'db> {
    // ... existing fields ...
    pub set_stmt_slots: HashMap<StmtSet<'db>, SlotId>,
}
```

When analyzing `set x = expr`:
1. Resolve `x` in current scope to find the target slot
2. Verify target slot is `Mutable` (error if `Local`)
3. Store mapping for interpreter

### 5. Interpreter (`interp/mod.rs`)

Add `execute_var_statement_frame` - identical to let but uses `Mutable` slot.

Add `execute_set_statement_frame`:
```rust
fn execute_set_statement_frame(..., set_stmt: StmtSet<'db>, ...) {
    let slot_id = layout.get_slot_for_set_stmt(ctx.db, set_stmt)?;
    let slot_info = layout.get_slot(ctx.db, slot_id)?;

    // For linear types: drop old value first
    if !is_copy_type(slot_info.ty(ctx.db)) {
        drop_slot_value(ctx, slot_info)?;
    }

    // Evaluate new value into slot
    let dest = Destination::from_slot_info(ctx, slot_info);
    eval_expression_frame(ctx, set_stmt.value(ctx.db), dest)?;
}
```

For compound assignment (`set x += expr`):
```rust
// Desugar: set x += expr  =>  set x = x + expr
// But read x before evaluating expr (for aliasing)
```

### 6. Type Checking (`tycheck.rs`)

- `StmtVar`: same as `StmtLet`
- `StmtSet`: verify name is mutable, RHS type matches slot type

### 7. Liveness Analysis (`liveness.rs`)

`set` statements:
- Mark target slot as live (being written)
- Analyze RHS for uses

### 8. Move Checking (`moves.rs`)

`set` statements:
- For linear types, the `set` implicitly drops the old value
- Verify slot is not moved before `set`

### 9. Validation (`validation.rs`)

Add checks:
- `set` target must be a `var` binding (error if `let`)
- `set` target must be in scope

## Files to Modify

1. `crates/datalove-datafun-compiler/src/ast.rs`
   - Add `StmtVar`, `StmtSet`, `SetOp`
   - Add variants to `Statement` enum

2. `crates/datalove-datafun-compiler/src/parser.rs`
   - Parse `var` and `set` statements

3. `crates/datalove-datafun-compiler/src/function_analysis/mod.rs`
   - Add `SlotKind::Mutable`

4. `crates/datalove-datafun-compiler/src/function_analysis/slot_allocation.rs`
   - Handle `StmtVar` and `StmtSet`

5. `crates/datalove-datafun-compiler/src/function_analysis/layout.rs`
   - Add `set_stmt_slots` mapping
   - Add `get_slot_for_set_stmt()` method

6. `crates/datalove-datafun-compiler/src/interp/mod.rs`
   - Add `execute_var_statement_frame`
   - Add `execute_set_statement_frame`

7. `crates/datalove-datafun-compiler/src/tycheck.rs`
   - Type check `var` and `set`

8. `crates/datalove-datafun-compiler/src/function_analysis/liveness.rs`
   - Handle `var` and `set`

9. `crates/datalove-datafun-compiler/src/function_analysis/moves.rs`
   - Handle `var` and `set`

10. `crates/datalove-datafun-compiler/src/function_analysis/validation.rs`
    - Validate `set` targets

## Tests to Add

Create test fixtures in `crates/datalove-datafun/tests/fixtures/module_interp/`:

### Basic var/set
1. `var_u32_basic` - `var x: u32 = @1; set x = @2; ret ok x` → `ok @2`
2. `var_int_basic` - `var x: int = @100; set x = @200; ret x` → `@200`
3. `var_string_basic` - `var s = "hello"; set s = "world"; ret s` → `"world"`

### Compound assignment
4. `set_add_u32` - `var x: u32 = @5; set x += @3; ret ok x` → `ok @8`
5. `set_sub_u32` - `var x: u32 = @5; set x -= @3; ret ok x` → `ok @2`
6. `set_add_int` - `var x = @100 + @0; set x += @50; ret x` → `@150`

### Loop accumulation
7. `loop_var_counter` - rewrite 048_loop_counter with var/set
8. `loop_var_sum` - rewrite 049_loop_sum with var/set
9. `loop_var_fibonacci` - rewrite 087_loop_fibonacci with var/set
10. `loop_var_int` - loop with linear int type

### Self-reference
11. `set_self_add_u32` - `var x: u32 = @1; set x = x +! @1; ret ok x` → `ok @2`
12. `set_self_add_int` - `var x = @1000; set x = x + x; ret x` → `@2000` (no leak!)

### Error cases
13. `set_immutable_error` - `let x: u32 = @1; set x = @2` → error
14. `set_undefined_error` - `set x = @1` → error

### Linear types
15. `var_string_reassign` - verify old string freed on set
16. `var_tuple_linear` - tuple containing Int, reassigned
17. `var_option_linear` - option containing Int

### Control flow
18. `set_in_if` - var before if, set in branches
19. `set_in_loop` - var before loop, set in body
20. `set_conditional` - var x, if cond set x else set x

## Order of Implementation

1. AST + Parser (var, set keywords)
2. SlotKind::Mutable
3. Slot allocation for var/set
4. Type checking
5. Interpreter execution
6. Liveness/moves/validation
7. Tests

## Dependencies

Must implement AFTER plan-shadowing.md (proper name resolution) or simultaneously:
- `set` relies on looking up the correct slot for a name
- With broken name lookup, `set x = ...` might hit wrong slot
