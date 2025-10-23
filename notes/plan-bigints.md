# Plan: Implement Bigint Math Operations

## Problem
According to README.md design (lines 290-324), bigints should support:
- Bare `+ - *` and unary `-`
- Checked division `/!` (not bare `/` due to div0)

Currently, bigint math ops don't work at all.

## Current State
1. **AST**: Missing bare unary negation operator (only has `-?` and `-!`)
2. **Parser**: Can't parse bare unary `-`
3. **Type Checker**: Recognizes Int as numeric but doesn't specifically handle it
4. **Interpreter**: Only implements bare ops for U32/F32, nothing for Value::Int
5. **Runtime**: No bigint arithmetic functions exist
6. **Tests**: Only two test files use `@int` (for option/result destructuring)

## Implementation Plan

### 1. AST Changes (ast.rs)
- Add `Neg` variant to `UnaryOp` enum for bare unary negation

### 2. Parser Changes (parser.rs)
- Update `parse_expr_primary` to recognize bare `-` as unary negation operator
- Must distinguish between `-` (bare), `-?` (optional), and `-!` (checked)

### 3. Type Checker Changes (tycheck.rs)
- Update `synthesize_unaryop` to handle bare `Neg` operator for Int type
- Update `synthesize_binop` to properly return Int type for Int operands with bare ops
- No changes to fixed int or float behavior

### 4. Runtime Operations (datalove-rt/src/)
- Create new file `int_math.rs` with bigint operations:
  - `int_add(a: *const Int, b: *const Int, rt: &mut RtLocal) -> *mut Int`
  - `int_sub(a: *const Int, b: *const Int, rt: &mut RtLocal) -> *mut Int`
  - `int_mul(a: *const Int, b: *const Int, rt: &mut RtLocal) -> *mut Int`
  - `int_neg(a: *const Int, rt: &mut RtLocal) -> *mut Int`
  - `int_div_checked(...)` for future `/!` support
- Export from `lib.rs`

### 5. Interpreter Changes (eval_datafun.rs)
- Update `eval_add/sub/mul` to handle `Value::Int` variant using runtime functions
- Add `eval_unaryop` handling for bare `Neg` operator with Int values
- Leave U32/F32 behavior unchanged

### 6. Test Cases
- Parser tests: bare unary `-` parsing
- Type check tests: bigint bare ops should type check correctly
- Interpreter tests (new fixtures):
  - `test_int_add.dfs`: `@100 + @200` → `@300`
  - `test_int_sub.dfs`: `@500 - @200` → `@300`
  - `test_int_mul.dfs`: `@10 * @20` → `@200`
  - `test_int_neg.dfs`: `-@42` → `@-42`
  - Test with large bigints that don't fit in fixed ints

## Files to Modify
1. `crates/datalove-datafun/src/ast.rs` - Add Neg to UnaryOp
2. `crates/datalove-datafun/src/parser.rs` - Parse bare `-`
3. `crates/datalove-datafun/src/tycheck.rs` - Handle bare Neg operator
4. `crates/datalove-rt/src/int_math.rs` - NEW: Bigint runtime operations
5. `crates/datalove-rt/src/lib.rs` - Export int_math module
6. `crates/datalove-datafun/src/eval_datafun.rs` - Implement bigint eval
7. `crates/datalove-datafun/tests/parser_tests.rs` - Add parser tests
8. `crates/datalove-datafun/tests/tycheck_tests.rs` - Add type check tests
9. `crates/datalove-datafun/tests/fixtures/interp/*.dfs` - Add interpreter test fixtures
