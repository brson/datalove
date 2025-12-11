# Plan: Algorithmic Error Generation for funlit_equiv Tests

**Status: COMPLETED** (All phases complete - 100% pass rate on discovery test)

## Goal

Add systematic error equivalence testing between datalit and datafun parsers/typecheckers. Currently funlit_equiv only tests valid expressions; we need to verify both systems produce equivalent errors.

## Approach: Mutation-Based Generation

Generate errors algorithmically by:
1. Generate valid AST via existing `ast_gen`
2. Apply targeted mutations to inject specific error classes
3. Verify both parsers/typecheckers produce equivalent errors

Two mutation categories:
- **Source mutations**: Corrupt pretty-printed text (for parse errors D001-D020)
- **AST mutations**: Modify AST before printing (for type errors T001-T046)

## Implementation

### New File: `crates/datalove-datalit/src/mutation_gen.rs`

```rust
pub struct MutationResult {
    pub source: String,
    pub expected_errors: Vec<&'static str>,
    pub description: String,
}

pub enum Mutation {
    // Source-level (parse errors)
    DeleteHeapSigil,      // D009-D012: Remove @ or #
    DeleteBracket,        // D001-D007, D014-D017: Remove (), {}, [], <>
    TruncateSource,       // D020: Cut off mid-expression

    // AST-level (type errors)
    OutOfRangeInt,        // T001, T005-T012: Integer overflow
    WrongElementType,     // T018-T021: Type mismatch in collection
    HeapMismatch,         // T033-T037: Mix @ and # in collection
    ArityMismatch,        // T024, T038-T040: Wrong tuple/struct field count
    RemoveTypeHint,       // T013-T016: Remove required type hint
    WrongVariant,         // T028: Invalid enum variant name
}
```

### Key Mutations

| Mutation | Error Codes | Strategy |
|----------|-------------|----------|
| DeleteHeapSigil | D009-D012 | Regex delete `@` or `#` from source |
| DeleteBracket | D001-D007 | Regex delete `()`, `{}`, `[]`, `<>` after keywords |
| TruncateSource | D020 | Cut source at random valid position |
| OutOfRangeInt | T001, T005-T012 | Replace int with value outside type's range |
| WrongElementType | T018-T021 | Insert wrong-typed element in list/set/map |
| HeapMismatch | T033-T037 | Change one element's heap from @ to # |
| ArityMismatch | T024, T038-T040 | Add/remove tuple or struct field |
| RemoveTypeHint | T013-T016 | Remove type hint from None/empty collection |
| WrongVariant | T028 | Change enum variant to nonexistent name |

### Test File: `crates/datalove-datafun/tests/error_equiv_tests.rs`

```rust
fn test_error_equiv(source: &str, expected_codes: &[&str]) -> Result<(), String> {
    // Parse/typecheck with datalit
    let datalit_errors = get_datalit_errors(source);

    // Parse/typecheck with datafun (wrap in "let _x = ")
    let datafun_errors = get_datafun_errors(source);

    // Verify error code equivalence
    if datalit_errors != datafun_errors {
        return Err(format!("Error mismatch: datalit={:?}, datafun={:?}",
            datalit_errors, datafun_errors));
    }
    Ok(())
}

#[test]
fn test_generated_error_equiv() {
    let db = Database::default();
    let config = make_compatible_config();

    for seed in 0..100 {
        let base_expr = gen_expr_full_seeded(&db, seed, config.clone());
        for mutation in Mutation::all() {
            if let Some(result) = mutation.apply(&db, base_expr) {
                test_error_equiv(&result.source, &result.expected_errors)?;
            }
        }
    }
}
```

## Files to Modify/Create

1. **Create** `crates/datalove-datalit/src/mutation_gen.rs` - Mutation trait and implementations
2. **Modify** `crates/datalove-datalit/src/lib.rs` - Export mutation_gen module
3. **Create** `crates/datalove-datafun/tests/error_equiv_tests.rs` - Error equivalence tests
4. **Modify** `crates/datalove-datafun/src/funlit_equiv.rs` - Add error extraction helpers

## Phased Implementation

### Phase 1: Infrastructure
- Add `mutation_gen.rs` with `Mutation` enum and `MutationResult`
- Implement source-level mutation helpers (regex-based)

### Phase 2: Parse Error Mutations
- Implement `DeleteHeapSigil`, `DeleteBracket`, `TruncateSource`
- Add error_equiv_tests for parse errors

### Phase 3: Type Error Mutations
- Implement AST-level mutations (`OutOfRangeInt`, `WrongElementType`, etc.)
- Add error_equiv_tests for type errors

### Phase 4: Integration
- Run full test suite
- Ensure all error codes have at least one mutation that triggers them

## Implementation Results

### Completed

All phases implemented. Files created/modified:

1. **`crates/datalove-datalit/src/mutation_gen.rs`** - Core mutation infrastructure with 11 mutation types
2. **`crates/datalove-datalit/src/lib.rs`** - Added mutation_gen module export
3. **`crates/datalove-datafun/tests/error_equiv_tests.rs`** - Error equivalence test suite
4. **`crates/datalove-datafun/Cargo.toml`** - Added rand dev-dependency

### Discovered Discrepancies

The tests reveal real differences between datalit and datafun:

| Mutation | Pass Rate | Notes |
|----------|-----------|-------|
| DeleteHeapSigil | 100% | Both parsers handle missing sigils equivalently |
| DeleteOpeningBracket | 45.5% | Some parser panics on malformed input |
| TruncateSource | 24.0% | Parser panics on incomplete input |
| DeleteComma | 50.0% | Mixed results |
| ExtraClosingBracket | 66.0% | Mostly equivalent |
| OutOfRangeInt | 0% | Datafun doesn't check integer ranges against type hints |
| WrongElementType | 0% | Type checking differs |
| HeapMismatch | 0% | Heap checking differs |
| ArityMismatch | 0% | Arity checking differs |

### Technical Notes

- Tests run in separate threads to isolate parser panics
- Fresh database instances per test to avoid salsa state corruption
- Discovery test passes CI; detailed tests are `#[ignore]` for investigation
- Parser panics on malformed input are counted as failures

---

# Remediation Plan

**Status: PHASE 1 COMPLETE**

## Root Causes

1. ~~Datafun doesn't call datalit's `check()` function for typed literals~~ FIXED
2. ~~Datafun doesn't check heap compatibility for typed literals~~ FIXED
3. ~~Datafun doesn't check arity for tuples/structs against type hints~~ FIXED
4. ~~Mutations using AST manipulation cause salsa panics~~ FIXED
5. Parser panics on malformed input instead of returning ParseError nodes (TBD)

## Phase 1: Type Error Fixes - COMPLETED

### Task 1.1: Add specific error types to datafun - DONE

**File:** `crates/datalove-datafun/src/tycheck.rs`

Added datalit-compatible error variants: `IntOutOfRange`, `HeapMismatch`, `MissingField`, `ExtraField`, `FieldOrderMismatch`, `VariantNotFound`

### Task 1.2: Add check() delegation for typed literals - DONE

Added integer range validation when synthesizing typed integer/hex literals with type hints.

### Task 1.3: Add heap compatibility checks for collections - DONE

Added heap compatibility checks to `synthesize_inline_list`, `synthesize_inline_set`, `synthesize_inline_map`, `synthesize_inline_tensor`.

### Task 1.4: Add helper to convert datalit errors - DONE

Added `From<datalit::tycheck::TypeError> for datafun::tycheck::TypeError` implementation.

## Phase 1.5: HeapMismatch Fix - COMPLETED

### Task 1.5.1: Fix mutation_gen to avoid salsa panics - DONE

Converted `apply_heap_mismatch` from AST manipulation to source-level string manipulation to avoid "cannot create tracked struct outside tracked function" errors.

### Task 1.5.2: Add heap checking for type-hinted collections - DONE

Added `check_list_elements`, `check_set_elements`, `check_map_entries`, `check_tensor_elements` helper functions to validate heap compatibility when collections have type hints.

### Task 1.5.3: Add heap checking for type-hinted literals - DONE

Added heap compatibility validation for Int and Hex literals when they have type hints. Added `unwrap_wrapper_heap` and `unwrap_wrapper_heap_datalit` helpers to extract the innermost heap from Option/Result wrapped types.

## Phase 1.6: WrongElementType Fix - COMPLETED

### Task 1.6.1: Fix mutation_gen to avoid salsa panics - DONE

Converted `apply_wrong_element_type` from AST manipulation to source-level string manipulation.

### Task 1.6.2: Fix type hint format - DONE

Changed mutation output from trailing `<type>` format to prefix `: type / expr` format to match parser expectations.

### Task 1.6.3: Add element type checking with coercion - DONE

Added `check_type_coercion` helper function that follows datalit's coercion rules for Option/Result types. Updated `check_list_elements`, `check_set_elements`, `check_map_entries`, and `check_tensor_elements` to use this helper for type checking with proper coercion support.

## Phase 1.7: ArityMismatch Fix - COMPLETED

### Task 1.7.1: Fix mutation_gen to avoid salsa panics - DONE

Converted `apply_arity_mismatch` from AST manipulation to source-level string manipulation to avoid "cannot create tracked struct outside tracked function" errors.

### Task 1.7.2: Add arity checking for tuples - DONE

Added `check_tuple_elements` helper function to validate that tuple element count matches the type hint. Updated `ExprFunKind::AnonTuple` and `ExprFunKind::NamedTuple` handling to call this function when type hints are present.

### Task 1.7.3: Add arity checking for structs - DONE

Added `check_struct_fields` helper function to validate that struct field count and names match the type hint. Updated `ExprFunKind::AnonStruct` and `ExprFunKind::NamedStruct` handling to call this function when type hints are present.

## Phase 1.8: RemoveTypeHint Fix - COMPLETED

### Task 1.8.1: Fix mutation_gen to avoid salsa panics - DONE

Converted `apply_remove_type_hint` from AST manipulation to source-level string building. Now generates source strings directly for `none`, enums, and empty collections without creating `ExprFull` tracked structs.

## Phase 1.9: WrongVariant Fix - COMPLETED

### Task 1.9.1: Fix mutation_gen to avoid salsa panics - DONE

Converted `apply_wrong_variant` from AST manipulation to source-level string building. Now generates source strings directly with the wrong variant name (`NonexistentVariant12345`) without creating `ExprAnonEnum`, `ExprNamedEnum`, or `ExprFull` tracked structs.

## Phase 1.10: HeapMismatch Collection Element Fix - COMPLETED

### Task 1.10.1: Add expression-level heap checking - DONE

Added `get_expr_heap()` helper to extract outer heap from expressions. Updated all collection element validators to check expression's actual heap in addition to synthesized type's heap. This catches cases like `@{...}` with type hint `#{...}`.

### Task 1.10.2: Fix mutation_gen type hint format - DONE

Changed `apply_heap_mismatch` to use prefix type hint syntax (`: type / expr`) instead of trailing `<type>` format, since datafun doesn't parse trailing type hints on collections.

## Phase 2: Parser Panic Fixes - COMPLETE

### Task 2.1: Fix `need_sigil` panic for missing `/` - DONE

Changed `parse_lit_expr_full` to use `eat_sigil` and return ParseError when `/` is missing after type hint.

### Task 2.2: Fix `is_numeric_literal` to distinguish hex from decimal - DONE

The function was treating any hex digit (a-f) as numeric, causing `@e` to be parsed as an integer instead of a parse error. Fixed to only allow hex digits after `0x`/`0X` prefix.

### Task 2.3: Fix `convert_type_hint` error wrapping - DONE

Changed from `DatalitError(format!("{:?}", e))` to `TypeError::from(e)` to properly convert datalit errors.

### Task 2.4: Fix bct bracer panics with unmatched brackets - DONE

Fixed `BracerIter::next2()` in `bct/crates/bct/src/bracer.rs` to skip `removed_closes` that fall behind the iterator position after exiting a branch. The bug occurred because `removed_closes` positions are absolute token indices, but after jumping past a branch the iterator position could skip past recorded removed close positions.

### Task 2.5: Fix has_parse_error to check type hints in datalit - DONE

The `has_parse_error` function in error_equiv_tests.rs only checked expressions for parse errors, not type hints. When a type hint contained a parse error (e.g., from bracer removing unmatched brackets in `{y)7: u8}`), it wasn't detected.

**Files modified:**
- `crates/datalove-datafun/tests/error_equiv_tests.rs` - Added `has_datalit_type_hint_parse_error` helper and updated `has_parse_error` to check type hints

### Task 2.6: Add Check-TypedAnonEnum to validate enum type hints - DONE

When an anonymous enum expression has a direct enum type hint (not wrapped in Option/Result), the type hint must be equivalent to the expected type from context. This catches cases like:
- Type hint: `enum {TestStruct93(i16), GenEnum17}`
- Expected: `enum {TestStruct93i16, GenEnum17}` (different variant structure)

**Files modified:**
- `crates/datalove-datalit/src/tycheck.rs` - Added `Check-TypedAnonEnum` rule (T047 error code)

### Remaining Issues

- DeleteComma: ~90% pass rate due to minor differences in type hint parsing (not worth fixing)

## Implementation Order

1. ~~Task 1.1 - Add error types~~ DONE
2. ~~Task 1.4 - Add error converter~~ DONE
3. ~~Task 1.2 - Add check() delegation (fixes OutOfRangeInt)~~ DONE
4. ~~Task 1.3 - Add heap checks (fixes HeapMismatch for synthesized collections)~~ DONE
5. ~~Task 1.5.1 - Fix mutation_gen salsa panics~~ DONE
6. ~~Task 1.5.2 - Add heap checking for type-hinted collections~~ DONE
7. ~~Task 1.5.3 - Add heap checking for type-hinted literals~~ DONE
8. ~~Task 1.6.1 - Fix mutation_gen salsa panics for WrongElementType~~ DONE
9. ~~Task 1.6.2 - Fix type hint format~~ DONE
10. ~~Task 1.6.3 - Add element type checking with coercion~~ DONE
11. ~~Task 1.7.1 - Fix mutation_gen salsa panics for ArityMismatch~~ DONE
12. ~~Task 1.7.2 - Add tuple arity checking~~ DONE
13. ~~Task 1.7.3 - Add struct arity checking~~ DONE
14. ~~Task 1.8.1 - Fix mutation_gen salsa panics for RemoveTypeHint~~ DONE
15. ~~Task 1.9.1 - Fix mutation_gen salsa panics for WrongVariant~~ DONE
16. ~~Task 1.10.1 - Add expression-level heap checking~~ DONE
17. ~~Task 1.10.2 - Fix HeapMismatch type hint format~~ DONE
18. ~~Task 2.1 - Fix need_sigil panic for missing /~~ DONE
19. ~~Task 2.2 - Fix is_numeric_literal hex vs decimal~~ DONE
20. ~~Task 2.3 - Fix convert_type_hint error wrapping~~ DONE
21. ~~Task 2.4 - Fix bct bracer panics~~ DONE
22. ~~Task 2.5 - Fix has_parse_error to check type hints in datalit~~ DONE
23. ~~Task 2.6 - Add Check-TypedAnonEnum to validate enum type hints~~ DONE

## Current Results

| Mutation | Previous | Current | Test Status |
|----------|----------|---------|-------------|
| OutOfRangeInt | 0% | 100% | ✅ Enabled |
| HeapMismatch | 0% | 100% | ✅ Enabled |
| DeleteHeapSigil | 100% | 100% | ✅ Enabled |
| WrongElementType | 0% | 100% | ✅ Enabled |
| ArityMismatch | 0% | 100% | ✅ Enabled |
| RemoveTypeHint | 0% | 100% | ✅ Enabled |
| WrongVariant | 0% | 100% | ✅ Enabled |
| TruncateSource | 26% | 100% | ✅ Enabled |
| DeleteOpeningBracket | 36.4% | 100% | ✅ Enabled |
| ExtraClosingBracket | 66.0% | 100% | ✅ Enabled |
| DeleteComma | 50.0% | 100% | ✅ Enabled |

**Test Summary:**
- 11 of 11 mutation tests enabled and passing at 100%
- Overall discovery test: 100% pass rate (183/183)

## Success Criteria

- ~~OutOfRangeInt: 0% → 100%~~ ACHIEVED
- ~~HeapMismatch: 0% → 100%~~ ACHIEVED
- ~~WrongElementType: 0% → 100%~~ ACHIEVED
- ~~ArityMismatch: 0% → 100%~~ ACHIEVED
- ~~RemoveTypeHint: 0% → 100%~~ ACHIEVED
- ~~WrongVariant: 0% → 100%~~ ACHIEVED
- ~~TruncateSource: 26% → 100%~~ ACHIEVED
- ~~Source mutations test: enabled~~ ACHIEVED

---

# Phase 3: DeleteComma Parsing Alignment

**Status: COMPLETE**

## Problem Summary

DeleteComma test was passing at ~83% (44/53). The failures fell into distinct categories where datalit and datafun parsed malformed input differently.

## Root Cause Analysis

The parsers use fundamentally different strategies:

### Datalit Approach
- Uses `parse_comma_separated` which **parses incrementally** - calls `parse_fn`, then looks for comma, repeats
- When parsing `{a = val1 b = val2, c = val3}` (missing comma):
  - Parses field `a`, gets value
  - Looks for comma, doesn't find it, **stops**
  - Result: 1 field

### Datafun Approach
- Uses `split_by_comma` first, then parses each group
- When parsing `{a = val1 b = val2, c = val3}` (missing comma):
  - Splits by comma: `["a = val1 b = val2", "c = val3"]`
  - Parses group 1: finds `a`, finds `=`, parses rest as value (including junk)
  - Parses group 2: finds `c`
  - Result: 2 fields

## Failure Categories

### Category 1: Struct/Tuple Field Comma Deletion (Seeds 17, 136, 151)
**Pattern:** Delete comma between struct field values
```
Original: #{y4 = #"value", z1 = #"data", value7 = ...}
Mutated:  #{y4 = #"value" z1 = #"data", value7 = ...}
Datalit:  ArityMismatch { expected: 3, actual: 1 }
Datafun:  ArityMismatch { expected: 3, actual: 2 }
```

### Category 2: Type Hint Comma Deletion (Seeds 102, 155)
**Pattern:** Delete comma in type hint (struct/tuple)
```
Original: @{data3: @i32, value4: @i64, y0: @bool}
Mutated:  @{data3: @i32, value4: @i64 y0: @bool}
Datalit:  ArityMismatch { expected: 2, actual: 3 }
Datafun:  TypeMismatch (different type hint parsing)
```

### Category 3: Tensor Shape Comma Deletion (Seed 126)
**Pattern:** Delete comma in tensor shape
```
Original: @tensor [2, 1] [...]
Mutated:  @tensor [2 1] [...]
Datalit:  ArityMismatch { expected: 2, actual: 1 }
Datafun:  No error (parses "21" as single dimension)
```

### Category 4: Enum Variant Comma Deletion (Seed 143)
**Pattern:** Delete comma between enum variants in type hint
```
Original: @enum {Data40(@i8), MyStruct77, ElementValue92}
Mutated:  @enum {Data40(@i8), MyStruct77 ElementValue92}
Datalit:  VariantNotFound("ElementValue92") - sees 2 variants
Datafun:  No error - sees 3 variants (parses "MyStruct77 ElementValue92" differently)
```

### Category 5: Tensor Element Comma Deletion (Seeds 150, 159)
**Pattern:** Delete comma between tensor elements
```
Original: #tensor [...] [: #u8 / #85, : #u8 / #146]
Mutated:  #tensor [...] [: #u8 / #85 : #u8 / #146]
Datalit:  PARSE_ERROR
Datafun:  No error
```

## Remediation Strategy

**Goal:** Make datafun use the same incremental parsing strategy as datalit for literals.

### Task 3.1: Refactor datafun struct field parsing

Change `parse_comma_separated_struct_fields` from split-first to incremental:

```rust
// Current (datafun):
fn parse_comma_separated_struct_fields(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprStructField<'db>> {
    let groups = self.split_by_comma(all_tokens);
    for group in groups {
        // parse each group
    }
}

// Target (match datalit):
fn parse_comma_separated_struct_fields(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprStructField<'db>> {
    let tokens: Vec<_> = iter.filter_map(|t| t.without_space(self.db)).collect();
    let mut tokens_iter = tokens.into_iter().peekable();
    let mut fields = Vec::new();
    loop {
        fields.push(self.parse_struct_field(&mut tokens_iter));
        if !self.eat_comma(&mut tokens_iter) {
            break;
        }
    }
    fields
}
```

### Task 3.2: Refactor datafun tuple element parsing

Apply same change to tuple parsing.

### Task 3.3: Refactor datafun tensor shape parsing

Current datafun:
```rust
fn parse_tensor_shape(&mut self, tokens: Vec<TreeToken<'db>>) -> Vec<u32> {
    for token in tokens {
        if comma { flush_current } else { append_to_current }
    }
}
```

This treats "2 1" as "21" because it just concatenates word tokens. Should match datalit's `parse_comma_separated(|p| p.parse_u32_literal())`.

### Task 3.4: Refactor datafun tensor element parsing

Current datafun 2D+ tensor parsing:
```rust
fn parse_tensor_data_2d_plus(&mut self, iter: BracerIter<'db>) -> Vec<ast::ExprFun<'db>> {
    let rows = self.split_tokens_by_comma_with_spaces(&all_tokens);
    for row_tokens in rows {
        while row_iter.peek().is_some() {
            elements.push(self.parse_expr_full(&mut row_iter));
        }
    }
}
```

This is close to datalit, but the difference in 1D parsing is the issue.

### Task 3.5: Fix type hint parsing in datafun

Datafun delegates to datalit for type hints via `parse_type_hint_and_heap_from_tokens`. The issue is that datafun collects tokens stopping at `,`, `=`, `/`, then passes them to datalit.

For malformed type hints like `{data3: @i32, value4: @i64 y0: @bool}`:
- Datafun collects until `/`, gets the whole malformed type hint
- Datalit parses it, but may interpret it differently than if parsed in context

**Fix:** Datafun should use datalit's type hint parser directly rather than pre-collecting tokens.

### Task 3.6: Fix enum type hint parsing

The enum variant parsing difference stems from type hint parsing. Same fix as Task 3.5.

## Implementation Order

1. ~~Task 3.1 - Struct field parsing (fixes Seeds 17, 136, 151)~~ DONE
2. ~~Task 3.2 - Tuple element parsing~~ DONE
3. ~~Task 3.3 - Tensor shape parsing (fixes Seed 126)~~ DONE
4. ~~Task 3.4 - Tensor element parsing (fixes Seeds 150, 159)~~ DONE
5. Task 3.5 - Type hint parsing (Seeds 102, 155) - Not implemented (minor issue, ArityMismatch vs TypeMismatch)
6. Task 3.6 - Enum type hint parsing (Seed 143) - Not implemented (minor issue)

## Completed Changes

1. **Refactored `parse_comma_separated_struct_fields`** to use incremental parsing
   - Now parses field, looks for comma, repeats (like datalit)
   - Fixes Seeds 17, 136, 151

2. **Refactored `parse_comma_separated_exprs`** to use incremental parsing
   - Now parses expr, looks for comma, repeats (like datalit)
   - Fixes tuple and general expression parsing

3. **Refactored `parse_tensor_shape`** to use incremental parsing
   - Now parses dimension, looks for comma, repeats (like datalit)
   - Previously concatenated adjacent word tokens (bug: "2 1" -> "21")
   - Fixes Seed 126

4. **Added tensor rank validation** in `check_tensor_shape_and_elements`
   - Now checks that parsed shape rank matches type hint rank
   - Also validates element count matches shape product
   - Fixes Seed 126 type checking

5. **Added tensor row size validation** in `parse_tensor_data_2d_plus`
   - Now validates each row has exactly `row_size` elements (last dimension)
   - Returns ParseError if row size mismatch (like datalit)
   - Fixes Seeds 150, 159

6. **Refactored `parse_datafun_tuple`** to use incremental parsing
   - Now parses element, looks for comma, repeats (like datalit)

7. **Refactored `parse_comma_separated_map_entries`** to use incremental parsing
   - Now parses entry (key=value), looks for comma, repeats (like datalit)

## Results

- DeleteComma: 83% → 100% (53/53)
- Discovery test: 100% (183/183)
- All detailed tests pass

## Phase 3 Additional Fixes

### Task 3.7: Add enum variant checking - DONE

When synthesizing `AnonEnum`/`NamedEnum` expressions with type hints, datafun now verifies the variant exists in the type hint. Previously, it just converted the type hint without checking.

**Files modified:**
- `crates/datalove-datafun/src/tycheck.rs` - Added `check_enum_variant()` helper function

### Task 3.8: Report ArityMismatch for struct/tuple type mismatches - DONE

Modified `check_type_coercion()` to detect when structs/tuples have different field counts and return `ArityMismatch` instead of `TypeMismatch`. This matches datalit's behavior.

**Files modified:**
- `crates/datalove-datafun/src/tycheck.rs`:
  - Added `CoercionError` enum with `TypeMismatch` and `ArityMismatch` variants
  - Added `check_type_arity_or_mismatch()` helper function
  - Updated all `check_type_coercion()` callers to use the new error type
