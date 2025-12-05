# Plan: New Interpreter Feature Parity

Complete the new interpreter to support all features tested by old interpreter, REPL, and CLI script command.

## Current State

**New interpreter (interp2) supports:** u32, bool, string, int, f32 literals; arithmetic (+,-,*,/); checked (+!,-!,*!,/!); optional (+?,-?,*?,/?); comparison; if/else (bool only); function calls with params; module imports; unary negation (-x for int).

**65 tests passing** in interp2 vs **136 tests** in old interp.

## Progress

**Completed:**
- Phase 1: Recursion tests (60-63: simple_recur, factorial, fibonacci, mutual_recursion)
- Phase 2: Unary negation for int (64-65: int_neg, int_neg_neg)
- Phase 2.5: Typechecker bug fix for checked/optional binary operators

**Key discoveries:**
- Bare operators (`-`, `*`) widen u32 to Int
- Checked operators (`-!`, `*!`) preserve type but require Result return type

## Runtime API Available

Key functions in `datalove-rt` C-ABI:
- `dtlv_rti_mem_alloc_local(rt, tydesc, count)` - allocate by tydesc
- `dtlv_rti_list_create_from_slice_local()` - create list from elements
- `dtlv_rti_btreemap_build_from_sorted_slices_local()` - create map
- `dtlv_rti_btreeset_build_from_sorted_slice_local()` - create set
- Layout helpers in `datalove-rtdt`: `compute_tuple_layout()`, `compute_option_layout()`, `compute_result_layout()`

Option/Result representation: first byte = tag (OptionTag::Some/None, ResultTag::Ok/Err), payload at computed offset.

## Priority Order

### Phase 1: Recursion ✓
**Tests:** 60-63 (simple_recur, factorial, fibonacci, mutual_recursion)

Recursion works with checked operators. Tests added.

### Phase 2: Unary Negation ✓
**Tests:** 64-65 (int_neg, int_neg_neg)

Added `execute_unop()` with `-x` for int calling `dtlv_rti_int_neg()`.

### Phase 2.5: Fix Typechecker Bug ✓

Binary checked/optional operators (`+!`, `-?`, etc.) now validate that the enclosing function has matching return type.

**Fixed in `tycheck.rs`:**
- `AddChecked | SubChecked | MulChecked | DivChecked` require `Result<T>` return type
- `AddOptional | SubOptional | MulOptional | DivOptional` require `Option<T>` return type
- Reuses F047/F049 error pattern from try operators

**Test updates completed:**
- interp2 tests 25, 28-36: Changed to bare operators with `int` types
- old_interp tests 204-205: Wrapped in functions with `!@int` return type
- tycheck tests 02, 06: Updated for new semantics
- tycheck.rs unit tests: Updated to use Result return types

### Phase 3: Collection Literals
**Tests:** 09-19, 162-171

**Tuple** `@(@42, @"hello")`:
1. Eval each element
2. Allocate tuple via `dtlv_rti_mem_alloc_local()`
3. Compute layout: `compute_tuple_layout(tydesc)`
4. Write each field at `ptr.add(field_offset)` via `copy_nonoverlapping`

**List** `@[@1, @2, @3]`:
1. Eval elements to temp array
2. Call `dtlv_rti_list_create_from_slice_local()`

**Struct** `@{x = @1}`:
- Same as tuple but fields ordered by name

**Map** `@map { @k = @v }`:
1. Eval key-value pairs
2. Sort by key (canonical order)
3. Call `dtlv_rti_btreemap_build_from_sorted_slices_local()`

**Set** `@set { @1, @2 }`:
1. Eval elements
2. Sort (canonical order)
3. Call `dtlv_rti_btreeset_build_from_sorted_slice_local()`

### Phase 4: Option/Result Basics
**Tests:** 51-56

**`@none` literal:**
1. Allocate Option via `dtlv_rti_mem_alloc_local()`
2. Write tag: `*(ptr as *mut u8) = OptionTag::None as u8`

**`@error(@42)` literal:**
1. Allocate Result
2. Write tag: `ResultTag::Err`
3. Write Error struct (tydesc + value ptr) at payload offset

**Coercion (value → Some/Ok):**
1. Allocate Option/Result
2. Write tag: `Some`/`Ok`
3. Write payload at `ptr.add(layout.payload_offset)`

### Phase 5: If Destructuring
**Tests:** 57-101

Syntax: `if opt |value| ... else ... end if`

1. Eval condition to Option/Result
2. Read tag: `*(ptr as *const u8)`
3. If Some/Ok:
   - Get payload offset from layout
   - Move payload out: read ptr at offset, create Value pointing to it
   - Mark Option/Result as "payload moved" (or just don't free payload on container destroy)
   - Bind moved value to `value` variable in scope
   - Execute then-branch
   - Value ownership transfers to scope, freed when scope exits
4. If None/Err:
   - Execute else-branch
5. Free container (but not moved payload)

### Phase 6: Try Operators
**Tests:** 129-141, 162-164

**`val?` (try-option):**
1. Eval operand to Option
2. Read tag; if None → return `InterpError::EarlyReturnNone`
3. If Some → move payload out (read ptr at offset), free container shell, return payload
4. Caller catches EarlyReturnNone, wraps as Option::None

**`val!` (try-result):**
1. Eval operand to Result
2. Read tag; if Err → move Error out, return `InterpError::EarlyReturnErr(error)`
3. If Ok → move payload out, free container shell, return payload
4. Caller catches EarlyReturnErr, wraps as Result::Err

### Phase 7: Optional/Result Unary Ops
**Tests:** 158-161

- `-?x`: checked negation, early-returns None on overflow
- `-!x`: checked negation, early-returns Err on overflow
- Use Rust's `checked_neg()`:
  - Success → return negated value directly (not wrapped)
  - Overflow → return `InterpError::EarlyReturnNone` or `EarlyReturnErr`
- Function must have `?T` or `!T` return type

### Phase 8: Other Integer Types
**Tests:** 94-101

Add to tydesc_table and allocation:
- i8, i16, i32, i64: signed integers
- u8, u16, u64: more unsigned

### Phase 9: Coercion Tests
**Tests:** 117-128

Type-directed coercion during `let` binding:
- If target type is `?T` and value is `T`, wrap in Some
- If target type is `!T` and value is `T`, wrap in Ok

### Phase 10: Test Migration
Convert old tests to worldfile format with correct expected outputs.

## Key Files

- `crates/datalove-datafun/src/interp/mod.rs` - interpreter impl
- `crates/datalove-rtdt/src/layout.rs` - layout computation
- `crates/datalove-rt/src/c.rs` - runtime C-ABI
- `crates/datalove-datafun/tests/fixtures/interp2/` - new tests

## Notes

Old interpreter wraps checked ops in Result - WRONG. Correct: yield T directly, early-return on overflow.

**Known issue:** Script sections in worldfile tests bypass typechecking. The `execute_script` function calls `type_check_with_package_world` but doesn't check for errors before proceeding. This is why tests 60-63 (which use checked operators in script sections) pass despite having type errors - the errors are silently ignored. Module sections are properly typechecked.
