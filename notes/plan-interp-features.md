# Plan: New Interpreter Feature Parity

Complete the new interpreter to support all features tested by old interpreter, REPL, and CLI script command.

## Current State

**New interpreter (interp2) supports:** u32, bool, string, int, f32 literals; arithmetic (+,-,*,/); checked (+!,-!,*!,/!); optional (+?,-?,*?,/?); comparison; if/else (bool only); function calls with params; module imports; unary negation (-x for int); tuples; anonymous structs; lists.

**89 tests passing** in interp2 vs **136 tests** in old interp.

## Progress

**Completed:**
- Phase 1: Recursion tests (60-63: simple_recur, factorial, fibonacci, mutual_recursion)
- Phase 2: Unary negation for int (64-65: int_neg, int_neg_neg)
- Phase 2.5: Typechecker bug fix for checked/optional binary operators
- Phase 3 (partial): Tuples, structs, and lists (70-77)
- Phase 4: @none literal, @error literal, Option/Result return type wrapping
- Phase 5: If-destructuring for Option (80_if_option_some, 81_if_option_none)
- Phase 5b: If-destructuring for Result (82_if_result_ok, 82a_result_ok_simple) - Ok case works
- Phase 6: Try operators (val? and val!) - tested via 62_fibonacci
- Phase 7: Optional/Result unary negation (-?, -!) - tests 84a, 85, 87
- Phase 8: Other integer types with Option/Result coercion - tests 94-97
- Phase 9: Let binding coercion T → Option<T>/Result<T> - tests 100-105

**Key implementations:**
- `execute_function_body` wraps return values in Some/Ok for `?T`/`!T` return types
- `evaluate_branch_condition` handles Bool/Option/Result conditions with payload binding
- Optional operators (`+?` etc.) return raw values; wrapping happens at function boundary
- Checked operators (`+!` etc.) return raw values; wrapping happens at function boundary
- `write_result_err_to_dest` writes @error literals to Result destinations
- `allocate_result_ok_from_value` / `allocate_result_err` for Result allocation
- `eval_try_option` / `eval_try_result` for try operator unwrapping
- Proper memory management: only free heap-allocated containers, not frame slots
- `eval_neg_optional` / `eval_neg_result` for checked unary negation (-?, -!)
- `write_typed_int_result` preserves operand type when writing result
- `allocate_error_string` creates "overflow" error for ResultErr on overflow
- `write_typed_int_to_dest` writes integer literals based on destination type (u8, i8, u16, i16, etc.)
- `type_hint_to_tydesc` extended for all integer types and Option/Result
- `coerce_value_to_dest` handles T → Option<T> and T → Result<T> coercion
- `eval_return_expression_frame` provides typed destination for @none/@error literals
- Skip double-wrapping when return value is already Option/Result type
- `execute_let_statement` coerces T → Option<T>/Result<T> in script scope
- `execute_let_statement_frame` coerces T → Option<T>/Result<T> in function body

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

### Phase 3: Collection Literals (partial ✓)
**Tests:** 09-19, 162-171

**Tuple** `@(@42, @"hello")`: ✓
- `allocate_tuple_from_values()` in interp/mod.rs
- Eval each element, create tydesc via `tydesc_table.get_or_create_tuple()`
- Compute layout, copy elements at field offsets
- Works in script scope, frame scope, and datalit expressions
- Tests: 70_tuple_simple, 71_tuple_nested, 73_tuple_in_module, 74_datafun_tuple_script

**Struct** `@{x = @1}`: ✓
- `allocate_struct_from_values()` in interp/mod.rs
- Fields sorted by name for canonical order
- Test: 72_struct_simple

**List** `@[@1, @2, @3]`: ✓
- `allocate_list_from_values()` in interp/mod.rs
- `tydesc_table.create_list_from_element_tydesc()` for runtime list tydesc
- Eval elements, build contiguous buffer, call `dtlv_rti_list_create_from_slice_local()`
- Runtime clones elements; originals destroyed after
- Tests: 75_list_simple, 76_list_nested, 77_list_of_tuples

**Map** `@map { @k = @v }`: TODO
1. Eval key-value pairs
2. Sort by key (canonical order)
3. Call `dtlv_rti_btreemap_build_from_sorted_slices_local()`

**Set** `@set { @1, @2 }`: TODO
1. Eval elements
2. Sort (canonical order)
3. Call `dtlv_rti_btreeset_build_from_sorted_slice_local()`

### Phase 4: Option/Result Basics ✓
**Tests:** 51-56

**`@none` literal:** ✓ `write_option_none_to_dest`
**`@error(@42)` literal:** ✓ `write_result_err_to_dest`
**Return type wrapping (value → Some/Ok):** ✓ `allocate_option_some_from_value`, `allocate_result_ok_from_value`

### Phase 5: If Destructuring ✓
**Tests:** 57-101

Syntax: `if opt |value| ... else ... end if`

✓ `evaluate_branch_condition` handles:
- Bool: simple truth check
- Option: Some extracts payload to then_binding, None goes to else
- Result: Ok extracts payload to then_binding, Err goes to else (else_binding TODO)

### Phase 6: Try Operators ✓
**Tests:** 62_fibonacci uses try-result operator successfully

**`val?` (try-option):** ✓
- `eval_try_option()` evaluates operand as Option
- If None → return `InterpError::OptionNone` (caught by function, wrapped as Option::None)
- If Some → extract payload to new allocation, free container, return payload

**`val!` (try-result):** ✓
- `eval_try_result()` evaluates operand as Result
- If Err → extract Error, return `InterpError::ResultErr { tydesc, ptr }`
- If Ok → extract payload to new allocation, free container, return payload

### Phase 7: Optional/Result Unary Ops ✓
**Tests:** 84a_neg_optional_raw, 85_neg_optional, 87_neg_result

**`-?x` (optional negation):** ✓
- `eval_neg_optional()` checks type tag (I8, I16, I32)
- Uses Rust's `checked_neg()` for overflow detection
- Success → return negated value directly (not wrapped)
- Overflow → return `InterpError::OptionNone`

**`-!x` (result negation):** ✓
- `eval_neg_result()` checks type tag (I8, I16, I32, U8, U16, U32)
- Uses Rust's `checked_neg()` for overflow detection
- Success → return negated value directly (not wrapped)
- Overflow → allocate "overflow" string, return `InterpError::ResultErr`

**Typechecker fix:** NegOptional/NegResult now return element type directly (not wrapped) and require matching Option/Result function return type, consistent with checked binary operators.

### Phase 8: Other Integer Types ✓
**Tests:** 94-97 (if_option_u8, if_option_u8_none, if_option_i8, if_option_u16)

Implemented typed integer literals with Option/Result coercion:
- `write_typed_int_to_dest` handles u8, i8, u16, i16, u32, i32, u64, i64 based on dest type
- `type_hint_to_tydesc` extended for all integer types and Option/Result
- Argument coercion: when T is passed where Option<T> expected, wrap in Some
- Return coercion: @none/@error get typed destination from function return type
- Fixed double-wrapping: if return value already Option/Result, don't wrap again

### Phase 9: Coercion Tests ✓
**Tests:** 100-105 (let_coercion_option, let_coercion_result, fun_let_coercion_option, return_coercion_option, return_coercion_result)

Implemented type-directed coercion during `let` binding and return:
- Script scope: `execute_let_statement` detects Option/Result type hints and coerces T → Some(T)/Ok(T)
- Function body: `execute_let_statement_frame` evaluates without destination first, then coerces if needed
- Return coercion already implemented in `execute_function_body`
- Note: Result let coercion in function body triggers function_analysis UseAfterMove bug (test 103 skipped)

### Phase 10: Test Migration
Convert old tests to worldfile format with correct expected outputs.

## Key Files

- `crates/datalove-datafun/src/interp/mod.rs` - interpreter impl
- `crates/datalove-rtdt/src/layout.rs` - layout computation
- `crates/datalove-rt/src/c.rs` - runtime C-ABI
- `crates/datalove-datafun/tests/fixtures/interp2/` - new tests

## Notes

Old interpreter wraps checked ops in Result - WRONG. Correct: yield T directly, early-return on overflow.

**Fixed:** Script sections now properly check for typecheck errors. Tests 60-63 updated to use bare operators with int types.
