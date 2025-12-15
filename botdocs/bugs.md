## eval_datafun accepts null inner_tydesc

## Parser doesn't support NaN and infinity for floats

The datalit parser only recognizes float literals in the form `number.number` (e.g., `1.0`, `-3.14`).
It does not support special float values like NaN, infinity, or negative infinity.

This prevents the AST generator from creating complete test coverage for float corner cases.

**Impact:**
- AST generator can't generate NaN, inf, or -inf values
- Tests can't verify proper handling of these special float values
- Roundtrip testing (AST → pretty → parse → AST) fails for these values

**Proposed Solution:**
Add hex literal syntax for bit-perfect float representation:
- `0x7fc00000` for NaN (f32)
- `0x7f800000` for positive infinity (f32)
- `0xff800000` for negative infinity (f32)
- `0x80000000` for negative zero (f32)

This would allow representing any float bit pattern, including all NaN variants,
and enable complete testing of float semantics.

**Related:**
- `crates/datalove-datalit/src/ast_gen.rs` has FIXMEs where NaN/infinity generation is disabled
- `crates/datalove-datalit/tests/ast_gen_tests.rs` test_numeric_corner_cases_generated has note about missing coverage
