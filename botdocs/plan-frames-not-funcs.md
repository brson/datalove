# Plan: Generalize Interpreter for Script Execution

## Goal

Divorce the interpreter from function-specific assumptions so it can later support script/REPL execution. This is a refactoring task only - no script execution implementation yet.

## Summary

1. Make unit type `()` the implicit return type of void functions
2. Replace `func: StmtFun` in StackFrame with abstracted context info
3. Update interpreter to use context info instead of `func.xxx()` accessors

---

## Part 1: Unit Type as Implicit Return Type

The unit type `()` already works as a ZST (size 0, align 1). Void functions currently have `return_type: None`. We need to change this so all functions have an explicit return type, with void functions using `()`.

### File: `crates/datalove-datafun-compiler/src/tycheck.rs` or new utility

Add helper to create the unit type:
```rust
pub fn unit_type<'db>(db: &'db dyn Db) -> TypeAndHeap<'db> {
    let unit_tuple = datalit::tycheck::TypeAnonTuple::new(db, vec![]);
    TypeAndHeap::new(
        db,
        datalit::ast::Heap::Omitted,
        Type::Datalit(datalit::tycheck::Type::AnonTuple(unit_tuple))
    )
}
```

### Where to use unit type:

- When a function has no declared return type, use `()` instead of `None`
- When creating script unit contexts (future)

---

## Part 2: StackFrame Refactoring

### File: `crates/datalove-datafun-compiler/src/interp/frame.rs`

**Add new struct** (after line 28):
```rust
/// Context for what's being executed in this frame.
pub struct FrameContext<'db> {
    /// Name for error messages.
    pub context_name: bct::text::InternedText<'db>,
    /// Return type (unit for void functions, never None).
    pub return_type: crate::tycheck::TypeAndHeap<'db>,
}
```

**Modify StackFrame** (lines 31-48):
```rust
pub struct StackFrame<'db> {
    pub frame_data: Vec<u8>,
    pub(super) slot_states: Vec<SlotState>,
    pub slot_tydescs: Vec<*const datalove_datalit::rtdt::TyDesc>,
    pub context: FrameContext<'db>,  // Replace func field
    pub layout: FrameLayout<'db>,
    pub cfg: ControlFlowGraph<'db>,
    pub drop_points: DropPoints<'db>,
    pub return_dest: Option<Destination>,
}
```

---

## Part 3: Interpreter Updates

### File: `crates/datalove-datafun-compiler/src/interp/mod.rs`

**Update StackFrame creation** (around line 353):
- Build `FrameContext` from function's name and return type
- For functions with no declared return type, use unit type `()`

**Update all `func.name()` usages** - replace with `frame.context.context_name`:
- Line 140 (error message)
- Line 259 (error message)
- Line 276 (error message)
- Line 321 (error message)
- Line 384 (error message)
- Line 421 (error message)
- Line 439 (error message)
- Line 504 (error message)

**Update all `func.return_type()` usages** - replace with `frame.context.return_type`:
- Line 407 (try-operator Option wrapping) - check if return type is `Option<T>` or `Result<T>`
- Line 498 (implicit return check) - change from `if func.return_type().is_none()` to checking if return_type is unit `()`

**Keep parameter handling unchanged** in `execute_function_body()`:
- Parameter validation (line 134) still uses `func.params()`
- Parameter slot initialization (lines 313-350) still uses `func.params()`
- These are entry-point-specific, not used during execution

---

## Part 4: Helper for Checking Unit Type

Add a helper to check if a type is the unit type:
```rust
pub fn is_unit_type<'db>(db: &'db dyn Db, ty: TypeAndHeap<'db>) -> bool {
    match ty.ty(db) {
        Type::Datalit(datalit::tycheck::Type::AnonTuple(tuple)) => {
            tuple.fields(db).is_empty()
        }
        _ => false,
    }
}
```

Use this at line 498 to check for implicit return (void functions).

---

## Files to Modify

1. `crates/datalove-datafun-compiler/src/interp/frame.rs` - add FrameContext, modify StackFrame
2. `crates/datalove-datafun-compiler/src/interp/mod.rs` - update all func.xxx() usages
3. `crates/datalove-datafun-compiler/src/tycheck.rs` - add unit_type and is_unit_type helpers

---

## Testing

Run the full test suite to ensure no regressions:
```bash
env DATALOVE_LEAK_CHECK=panic timeout 300 cargo test
```

The existing interpreter tests should pass unchanged since behavior is identical - just the internal representation changes.

---

## Notes

- Unit type `()` is already implemented as ZST (size 0, align 1) - confirmed working
- This refactoring does NOT add script execution, just prepares the interpreter for it
- The `func` field is removed from StackFrame but function parameters are still handled at entry
- Cross-frame bindings for scripts are deferred to a later task
