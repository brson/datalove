# Plan: Implement Bigint Math Operations

## Status: ✅ COMPLETED

Basic bigint math operations (`+ - *` and unary `-`) are now fully implemented and tested.
Bigint pretty printing is also complete.

## Problem
According to README.md design (lines 290-324), bigints should support:
- Bare `+ - *` and unary `-` ✅ IMPLEMENTED
- Checked division `/!` (not bare `/` due to div0) ⏳ FUTURE WORK

## Completed State
1. **AST**: ✅ Added `Neg` variant to `UnaryOp` enum (ast.rs:186)
2. **Parser**: ✅ Parses bare `-` as unary operator (parser.rs:747)
3. **Type Checker**: ✅ Handles bare `Neg` and Int binary ops (tycheck.rs:887)
4. **Interpreter**: ✅ Implements all bare ops for Value::Int (eval_datafun.rs)
5. **Runtime**: ✅ Bigint arithmetic functions using `ibig` crate (int_math.rs)
6. **Pretty Printing**: ✅ Bigints display as decimal values (e.g., `@300`, `@-42`)
7. **Tests**: ✅ 4 interpreter tests, all 140 tests passing

## Implementation Summary

### 1. AST Changes ✅
- **ast.rs:186**: Added `Neg` variant to `UnaryOp` enum for bare unary negation
- **ast_serde.rs:160**: Added `Neg` to serialization enum
- **ast_serde.rs:392**: Added `Neg` case to `from_ast` conversion

### 2. Parser Changes ✅
- **parser.rs:747**: Updated `parse_expr_primary` to recognize bare `-` as `UnaryOp::Neg`
- Successfully distinguishes between `-` (bare), `-?` (optional), and `-!` (checked)

### 3. Type Checker Changes ✅
- **tycheck.rs:887**: Updated `synthesize_unaryop` to handle bare `Neg` returning same type
- **type_table.rs:275**: Added `Neg` case returning operand type descriptor
- Binary ops already handled Int correctly, no changes needed
- U32/F32 behavior unchanged as required

### 4. Runtime Operations ✅
- **int_math.rs** (NEW): Created with bigint operations using `ibig` crate:
  - `dtlv_rti_int_add` - Addition
  - `dtlv_rti_int_sub` - Subtraction
  - `dtlv_rti_int_mul` - Multiplication
  - `dtlv_rti_int_neg` - Negation
- **lib.rs:53**: Exported `int_math` module
- **Cargo.toml**: Added `ibig = "0.3"` dependency

### 5. Interpreter Changes ✅
- **eval_datafun.rs:367-402**: Updated `eval_add` to handle `Value::Int`
- **eval_datafun.rs:405-440**: Updated `eval_sub` to handle `Value::Int`
- **eval_datafun.rs:443-478**: Updated `eval_mul` to handle `Value::Int`
- **eval_datafun.rs:282-310**: Added `eval_neg` for bare negation of Int values
- **eval_datafun.rs:271**: Wired up `Neg` case in `eval_unaryop`
- U32/F32 behavior unchanged

### 6. Pretty Printing ✅
- **int_math.rs**: Made `rtdt_int_to_ibig` public for reuse (int_math.rs:10)
- **pretty.rs:238-251**: Implemented `pretty_int` function
  - Converts rtdt::Int to IBig using existing conversion function
  - Uses IBig's `.to_string()` for decimal representation
  - Formats with `@` prefix (e.g., `@300`, `@-42`)
- Removed `<bigint>` placeholder

### 7. Test Cases ✅
- **200_int_add.dfs**: Tests `@int + @int` addition → outputs `@300`
- **201_int_sub.dfs**: Tests `@int - @int` subtraction → outputs `@300`
- **202_int_mul.dfs**: Tests `@int * @int` multiplication → outputs `@200`
- **203_int_neg.dfs**: Tests `-@int` negation → outputs `@-42`
- **83_if_option_int.dfs**: Tests Option<Int> destructuring → outputs `@999999999999999999999`
- **84_if_result_int.dfs**: Tests Result<Int> destructuring → outputs `@999999999999999999999`
- All 140 interpreter tests passing

## Files Modified
1. ✅ `crates/datalove-datafun/src/ast.rs` - Added Neg to UnaryOp
2. ✅ `crates/datalove-datafun/src/ast_serde.rs` - Added Neg to serialization
3. ✅ `crates/datalove-datafun/src/parser.rs` - Parse bare `-`
4. ✅ `crates/datalove-datafun/src/tycheck.rs` - Handle bare Neg operator
5. ✅ `crates/datalove-datafun/src/type_table.rs` - Handle Neg type descriptor
6. ✅ `crates/datalove-rt/src/int_math.rs` - NEW: Bigint runtime operations, public rtdt_int_to_ibig
7. ✅ `crates/datalove-rt/src/lib.rs` - Export int_math module
8. ✅ `crates/datalove-rt/Cargo.toml` - Add ibig dependency
9. ✅ `crates/datalove-datafun/src/eval_datafun.rs` - Implement bigint eval
10. ✅ `crates/datalove-rt/src/pretty.rs` - Implement bigint pretty printing
11. ✅ `crates/datalove-datafun/tests/fixtures/interp/200_int_add.dfs` - New test
12. ✅ `crates/datalove-datafun/tests/fixtures/interp/201_int_sub.dfs` - New test
13. ✅ `crates/datalove-datafun/tests/fixtures/interp/202_int_mul.dfs` - New test
14. ✅ `crates/datalove-datafun/tests/fixtures/interp/203_int_neg.dfs` - New test
15. ✅ `crates/datalove-datafun/tests/fixtures/interp/83_if_option_int.out.expected` - Updated output
16. ✅ `crates/datalove-datafun/tests/fixtures/interp/84_if_result_int.out.expected` - Updated output

## Implementation Notes

### Design Decisions
- **Bigint Library**: Used `ibig` crate (v0.3) for robust arbitrary-precision arithmetic
- **Conversion Strategy**: Convert between `rtdt::Int` limb representation and `IBig` at runtime boundary
- **Type Inference**: Small integer literals like `@42` default to u32; explicit type hints (`let a: @int = @42`) required for Int type
- **Pretty Printing**: Reuses `rtdt_int_to_ibig` conversion function; leverages IBig's decimal string formatting

### Test Strategy
- All tests use explicit type hints to ensure Int type
- Tests verify basic arithmetic correctness
- All 140 tests passing, including new bigint tests

## Future Work

### High Priority
1. **Checked Division `/!`**
   - Required by design for division-by-zero handling
   - Runtime function signature: `dtlv_rti_int_div_checked`
   - Type checker already supports checked operators

### Lower Priority
2. **Additional Test Coverage**
   - Parser-specific tests for bare `-` parsing edge cases
   - Type checker-specific tests for Int type inference
   - Tests with very large bigints (beyond i128 range)

3. **Optional Bigint Operations**
   - Optional variants (`+?`, `-?`, `*?`) could be useful for consistency
   - Would need runtime functions and interpreter support
