# Plan: Complete funlit_equiv Test Coverage

The `make_compatible_config()` currently disables most types. Here's the remediation plan.

## Currently Enabled (weight > 0)
- `bool_type: 10` - working
- `u8_type: 5` - working
- `u16_type: 5` - working
- `u32_type: 10` - working
- `string_type: 10` - working
- `anon_tuple_type: 10` - working
- `anon_struct_type: 10` - working

## Disabled Types and Remediation

### Group 1: Signed Integers (i8, i16, i32)
**Issue:** "negative literal handling differs"
**Root cause:** Datalit generates `-5` as a single `Int` token with value `-5`. Datafun parses `-5` as `UnaryOp(Neg, Int(5))`.
**Fix:** The `datafun_expr_to_datalit_serde` already handles `UnaryOp(Neg, Int)` -> negative Int conversion (lines 281-306). Enable and test.

### Group 2: 64-bit Integers (u64, i64)
**Issue:** "large values cause error differences"
**Root cause:** Values like `18446744073709551615` may produce different error messages when they overflow.
**Fix:** Use `NumericStrategy::CornerCases` which limits values to edge cases, or filter generated values to small ranges. Consider adding a `NumericStrategy::SmallValues` option to ast_gen.

### Group 3: Float (f32)
**Issue:** "parsing differences between datalit and datafun"
**Root cause:** Need to investigate - likely float literal format differences.
**Fix:** Ensure both parsers handle the same float formats (e.g., `1.0`, `1.5`, `.5`, `1e10`).

### Group 4: BigInt (int)
**Issue:** "large values cause error differences"
**Fix:** Same as 64-bit - use small value strategy.

### Group 5: List
**Issue:** "empty list type inference differs - datalit: CannotSynthesize for [], datafun: infers List<()>"
**Fix:** Two options:
1. Make datafun match datalit behavior (return CannotSynthesize for empty list)
2. Accept the difference and only test non-empty lists by configuring ast_gen

### Group 6: Map
**Issue:** "Uses special syntax"
**Details:** Datalit pretty-prints as `map {key = value}`. Datafun parser should handle this.
**Fix:** Verify datafun parses `@map {k = v}` syntax. If not, add support.

### Group 7: Set
**Issue:** "Uses special syntax"
**Details:** Datalit pretty-prints as `set {elem}`. Datafun parser should handle this.
**Fix:** Verify datafun parses `@set {elem}` syntax. If not, add support.

### Group 8: Tensor
**Issue:** "Complex syntax"
**Details:** Datalit prints `tensor [shape] [elements]`.
**Fix:** Verify datafun parses this syntax. Add support if missing.

### Group 9: Type-hint Required Types
These require `: type / value` syntax which datafun doesn't support yet:
- `option_type` - needs type hint for `@none`
- `result_type` - needs type hint for `@error`
- `named_tuple_type` - needs type hint to resolve name
- `named_struct_type` - needs type hint to resolve name
- `anon_enum_type` - needs type hint for variant resolution
- `named_enum_type` - needs type hint for enum resolution
- `data_type` - needs type hint
- `error_type` - needs type hint

**Fix:** Add type hint parsing to datafun: `: type / expr` syntax. This is a significant parser change.

## Implementation Order

### Phase 1: Quick Wins (parser already supports, just enable)
1. Enable signed ints (i8, i16, i32) - conversion already handles negation
2. Enable list_type - test non-empty lists only

### Phase 2: Syntax Support (add missing keywords to parser)
3. Add `map {...}` keyword parsing to datafun
4. Add `set {...}` keyword parsing to datafun
5. Add `tensor [shape] [elements]` parsing to datafun

### Phase 3: Numeric Edge Cases
6. Add SmallValues numeric strategy to ast_gen
7. Enable u64, i64, int with small values

### Phase 4: Float Handling
8. Investigate float parsing differences
9. Fix and enable f32

### Phase 5: Type Hint Syntax (largest change)
10. Add `: type / expr` parsing to datafun parser
11. Enable option, result, named tuple/struct, enums, data, error

## Testing Strategy

For each phase:
1. Enable the type(s) in make_compatible_config
2. Run tests to see failures
3. Fix parser/typechecker as needed
4. Verify all tests pass before moving to next phase
