# Fix Slot Resolution: Proper Shadowing

## Problem

`find_slot_by_name` does runtime name lookup, but at runtime all slots exist and we can't distinguish which binding a name refers to.

```
let x = @1          // slot 0
let x = x + @1      // slot 1, but which slot does RHS `x` refer to?
```

At runtime, both slots exist. We need to resolve names at **analysis time**.

This applies to both `let` and `var` bindings. With `var`/`set` now implemented:
- `let` creates immutable slot (SlotKind::Local)
- `var` creates mutable slot (SlotKind::Mutable)
- `set` mutates existing var slot (no new slot)

All four shadowing combinations are valid:
- `let` shadows `let`
- `var` shadows `var`
- `let` shadows `var`
- `var` shadows `let`

## Solution

Resolve name → slot at analysis time, store the mapping, use it at runtime.

Two approaches:

### Approach A: Store resolved slot in Name expressions (cleanest)

During slot allocation, for each `Name(text)` expression, resolve it to a slot and store the mapping.

### Approach B: Store StmtLet in SlotInfo, lookup by statement identity

Store the `StmtLet` that created each slot. Lookup by statement identity instead of name.

## Implementation (Approach A)

### 1. Add name resolution during slot allocation

In `slot_allocation.rs`, track a scope map during analysis:

```rust
struct SlotAllocator<'db> {
    // ... existing fields ...
    /// Maps variable names to their current slot (for name resolution)
    scope: HashMap<InternedText<'db>, SlotId>,
}
```

When allocating a let binding:
```rust
Statement::Let(let_stmt) => {
    // FIRST: Analyze RHS (uses current scope)
    self.analyze_expr(db, let_stmt.value(db), rhs_ctx);

    // THEN: Allocate slot and update scope
    let slot_id = self.alloc_slot(...);
    self.scope.insert(let_stmt.name(db), slot_id);
}
```

Same pattern for var:
```rust
Statement::Var(var_stmt) => {
    // FIRST: Analyze RHS (uses current scope)
    self.analyze_expr(db, var_stmt.value(db), rhs_ctx);

    // THEN: Allocate slot and update scope
    let slot_id = self.alloc_slot(...);
    self.scope.insert(var_stmt.name(db), slot_id);
}
```

For set, resolve the target name to an existing var slot:
```rust
Statement::Set(set_stmt) => {
    // Resolve target to existing slot
    let slot_id = self.scope.get(&set_stmt.name(db));
    self.set_stmt_slots.insert(set_stmt, slot_id);

    // Analyze RHS (uses current scope, which includes the var being set)
    self.analyze_expr(db, set_stmt.value(db), rhs_ctx);
}
```

When analyzing a Name expression:
```rust
fn analyze_expr(&mut self, db, expr, ctx) {
    match expr.expr(db) {
        ExprFunKind::Name(name) => {
            // Resolve name to slot using current scope
            if let Some(&slot_id) = self.scope.get(&name) {
                self.name_resolutions.insert(expr, slot_id);
            }
        }
        // ...
    }
}
```

### 2. Store resolutions in FrameLayout

```rust
pub struct FrameLayout<'db> {
    // ... existing fields ...
    /// Maps Name expressions to their resolved slots
    pub name_resolutions: HashMap<ExprFun<'db>, SlotId>,
    /// Maps let statements to their destination slots
    pub let_stmt_slots: HashMap<StmtLet<'db>, SlotId>,
    /// Maps var statements to their destination slots
    pub var_stmt_slots: HashMap<StmtVar<'db>, SlotId>,
    /// Maps set statements to their target slots
    pub set_stmt_slots: HashMap<StmtSet<'db>, SlotId>,
}
```

### 3. Add lookup method

```rust
impl FrameLayout<'db> {
    pub fn get_slot_for_name_expr(
        self,
        db: &'db dyn Db,
        expr: ExprFun<'db>,
    ) -> Option<SlotInfo<'db>> {
        let slot_id = self.name_resolutions(db).get(&expr)?;
        self.slots(db).iter().find(|s| s.slot_id(db) == *slot_id).copied()
    }
}
```

### 4. Update interpreter

In `eval_operand`:
```rust
ExprFunKind::Name(name) => {
    let slot_info = layout.get_slot_for_name_expr(ctx.db, expr)
        .ok_or_else(|| InterpError::VariableNotFound(...))?;
    // ...
}
```

### 5. Handle statement destinations

For `execute_let_statement_frame` and `execute_var_statement_frame`, we need the NEW slot (just allocated).
For `execute_set_statement_frame`, we need the existing var slot.
Mappings already defined in step 2.

### 6. Update function_analysis callers

The analysis passes (liveness, moves, validation) also need to use the resolved slots instead of name lookup.

## Files to Modify

1. `crates/datalove-datafun-compiler/src/function_analysis/slot_allocation.rs`
   - Add scope tracking during analysis
   - Resolve Name expressions to slots
   - Allocate let binding slot AFTER analyzing RHS
   - Record name resolutions and let_stmt mappings

2. `crates/datalove-datafun-compiler/src/function_analysis/layout.rs`
   - Add `name_resolutions: HashMap<ExprFun, SlotId>` field
   - Add `let_stmt_slots: HashMap<StmtLet, SlotId>` field
   - Add `get_slot_for_name_expr()` and `get_slot_for_let_stmt()` methods

3. `crates/datalove-datafun-compiler/src/interp/mod.rs`
   - Update `eval_operand` to use `get_slot_for_name_expr()`
   - Update `execute_let_statement_frame` to use `get_slot_for_let_stmt()`

4. `crates/datalove-datafun-compiler/src/interp/control.rs`
   - Remove or update `find_slot_by_name` (may still need for if-bindings)

5. `crates/datalove-datafun-compiler/src/function_analysis/liveness.rs`
   - Update to use resolved slots instead of name lookup

6. `crates/datalove-datafun-compiler/src/function_analysis/moves.rs`
   - Update to use resolved slots instead of name lookup

7. `crates/datalove-datafun-compiler/src/function_analysis/validation.rs`
   - Update to use resolved slots instead of name lookup

## Shadowing Tests to Add

Create test fixtures in `crates/datalove-datafun/tests/fixtures/module_interp/`:

### let shadows let
1. **u32 let-let** - `let x: u32 = @1; let x = x +! @1; ret ok x` → `ok @2`
2. **Int let-let** - `let x = @1000 + @0; let x = x + x; ret x` → `@2000` (no leak!)
3. **String let-let** - `let s = "a"; let s = s; ret s`

### var shadows var
4. **u32 var-var** - `var x: u32 = @1; var x = x +! @1; ret ok x` → `ok @2`
5. **Int var-var** - `var x = @1000 + @0; var x = x + x; ret x` → `@2000` (no leak!)

### let shadows var
6. **u32 var-let** - `var x: u32 = @1; let x = x +! @1; ret ok x` → `ok @2`
7. **Int var-let** - `var x = @1000 + @0; let x = x + x; ret x` → `@2000` (no leak!)

### var shadows let
8. **u32 let-var** - `let x: u32 = @1; var x = x +! @1; ret ok x` → `ok @2`
9. **Int let-var** - `let x = @1000 + @0; var x = x + x; ret x` → `@2000` (no leak!)

### Multi-level shadowing
10. **Nested shadowing** - 3+ levels: let then var then let
11. **Loop shadowing** - verify each iteration uses correct slot

### Scope-based shadowing
12. **If-branch shadowing** - shadow same name in both branches
13. **Tuple with linear** - tuple containing Int, shadowed

### set after shadowing
14. **set finds correct var** - `var x = @1; let x = @2; set x = @3` should error (x is now let)
15. **set with outer var** - `var x = @1; { let x = @2 }; set x = @3; ret x` → `@3`

## Existing Tests to Rewrite

After implementing shadowing, these existing tests use broken `let` semantics and need to be rewritten to use `var`/`set`:

- `048_for_loop_conditional_sum.world` - uses `let sum = sum + iter`
- `049_for_loop_count.world` - uses `let count = count + @1`
- `084_for_loop_conditional_sum_labeled.world` - same
- `085_for_loop_count_labeled.world` - same

## Benefits

- Correct shadowing semantics (each `let`/`var` creates new binding)
- Correct `var`/`set` semantics (mutation, not shadowing)
- No memory leaks for linear types
- Direct slot lookup by AST identity (no string comparison at runtime)
- Analysis-time resolution catches undefined variables earlier
