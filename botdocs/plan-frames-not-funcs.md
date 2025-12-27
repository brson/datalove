# Plan: Generalize Interpreter for Script Execution

## Goal

Divorce the interpreter from function-specific assumptions so it can later support script/REPL execution. This is a refactoring task only - no script execution implementation yet.

## Summary

1. Add first-class unit type `()` with 1-byte size
2. Replace `func: StmtFun` in StackFrame with abstracted context info
3. Update interpreter to use context info instead of `func.xxx()` accessors

---

## Part 1: First-Class Unit Type

The unit type `()` (zero-element tuple) must be 1 byte, not zero-sized.

### File: `crates/datalove-datafun-compiler/src/function_analysis/type_sizing.rs`

**Change `compute_tuple_layout`** (lines 129-154):
```rust
fn compute_tuple_layout<'db>(db, fields: &[TypeAndHeap<'db>]) -> TypeLayout {
    // Unit type (empty tuple) is 1 byte, not zero-sized.
    if fields.is_empty() {
        return TypeLayout { size: 1, align: 1 };
    }
    // ... existing logic for non-empty tuples ...
}
```

### File: `crates/datalove-rtdt/src/layout.rs`

**Check `compute_tuple_layout`** (line 42) for similar empty-tuple handling - ensure it returns size=1 for empty tuples.

### File: `crates/datalove-datalit/src/tydesc_table.rs`

Verify tydesc creation for empty tuples produces size=1.

---

## Part 2: StackFrame Refactoring

### File: `crates/datalove-datafun-compiler/src/interp/frame.rs`

**Add new struct** (after line 28):
```rust
/// Context for what's being executed in this frame.
pub struct FrameContext<'db> {
    /// Name for error messages.
    pub context_name: bct::text::InternedText<'db>,
    /// Return type (with first-class unit, never None).
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
- Line 407 (try-operator Option wrapping)
- Line 498 (implicit return check - with unit type this becomes: check if return_type is unit)

**Keep parameter handling unchanged** in `execute_function_body()`:
- Parameter validation (line 134) still uses `func.params()`
- Parameter slot initialization (lines 313-350) still uses `func.params()`
- These are entry-point-specific, not used during execution

---

## Part 4: Helper Function for Unit Type

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

Use this when:
- A function has no declared return type (currently `None`)
- Creating script unit contexts

---

## Files to Modify

1. `crates/datalove-datafun-compiler/src/function_analysis/type_sizing.rs` - unit type size fix
2. `crates/datalove-rtdt/src/layout.rs` - verify unit type size
3. `crates/datalove-datafun-compiler/src/interp/frame.rs` - add FrameContext, modify StackFrame
4. `crates/datalove-datafun-compiler/src/interp/mod.rs` - update all func.xxx() usages
5. `crates/datalove-datafun-compiler/src/tycheck.rs` - add unit_type helper

---

## Testing

Run the full test suite to ensure no regressions:
```bash
env DATALOVE_LEAK_CHECK=panic timeout 300 cargo test
```

The existing interpreter tests should pass unchanged since behavior is identical - just the internal representation changes.

---

## Notes

- This refactoring does NOT add script execution, just prepares the interpreter for it
- The `func` field is removed from StackFrame but function parameters are still handled at entry
- Cross-frame bindings for scripts are deferred to a later task
