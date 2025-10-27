# Implementation Plan: Numeric Widening

Based on README.md:326-359, implementing automatic numeric widening with these rules:

## 1. Basic Widening Rules

- **Unsigned chain**: u8 → u16 → u32 → u64 → int
- **Signed chain**: i8 → i16 → i32 → i64 → int
- **No cross-widening**: unsigned ≠ signed
- **Math operator widening**: Bare operators (+, -, *) widen to `int`

## 2. Core Type System Changes

### File: crates/datalove-datalit/src/tycheck.rs

- Add `fn can_widen_to(from: &Type, to: &Type) -> bool` helper
  - Implements unsigned/signed widening chains
  - Returns false for cross-widening (u32 → i32, etc.)
- Modify `check()` function to attempt widening when exact type match fails
  - After type mismatch, try `can_widen_to(synthesized, expected)`
  - Order: exact match → widening → Option/Result coercion
- Add comprehensive tests for widening in assignment contexts

### File: crates/datalove-datafun/src/tycheck.rs

- Import and use datalit's `can_widen_to` helper
- Modify `check_expr()` to support widening for datafun expressions
- Update `synthesize_binop()` for bare operators:
  - Currently: bare +,-,* only for float/bigint (lines 843-850)
  - New: also accept fixed ints, widen operands to `int`, return `int`
  - Keep checked (+!, etc.) and optional (+?, etc.) unchanged
- Update `is_numeric_type()` and related helpers as needed

## 3. Integer Literal Handling

**Current**: Literals default to u32, must fit in u32 range
**After widening**: When used with bare math, literals should synthesize as int

### File: crates/datalove-datalit/src/tycheck.rs:294-309

- In `synthesize()` for `Expr::Int`: keep u32 default
- Let widening handle promotion to int when needed
- OR: detect if literal is in bare-math context, synthesize as int
  - Need to consider: may require context-sensitive synthesis

## 4. Test Updates

### New tests to add:

**crates/datalove-datalit/tests/tycheck_tests.rs**:
- Widening in assignment: `let a: u16 = @(1 as u8)`
- Widening chains: u8→u16→u32→u64→int
- Widening chains: i8→i16→i32→i64→int
- No cross-widening: u32 ↛ i32

**crates/datalove-datafun/tests/**:
- Bare math widening: `let a = 1 * 2` → int type
- Variable bare math: `let b: u32 = 1; let c = b * b` → int type
- Literals in bare math: `let a = @1 + @2` → int
- Mixed with checked math: `let x = @1 +! @2` → still Result<u32>

### Existing tests to update:

- Search for tests expecting u32 from bare-math literals
- Update to expect int where bare operators are used
- Keep u32 expectations where type hints are present

## 5. Documentation Updates

### File: notes/typing-rules.md

- Move "Numeric widening" from Future Extensions (line 807) to implemented features
- Add widening rules to type equivalence section
- Add Check-Widening rule
- Document bare-math-to-int behavior

## 6. Implementation Order

1. Add `can_widen_to()` helper to datalit/tycheck.rs
2. Modify datalit `check()` to use widening
3. Add datalit widening tests, ensure they pass
4. Modify datafun `check_expr()` to use widening
5. Modify datafun `synthesize_binop()` for bare-math widening
6. Add datafun widening tests, ensure they pass
7. Run full test suite, fix any broken tests
8. Update documentation

## 7. Edge Cases to Handle

- Literal synthesis vs checking: `let a = 1` stays u32, but `let b = 1 + 1` becomes int
- Function parameters: widening should work when passing arguments
- Return values: widening should work for return type matching
- Tuple/struct fields: widening should work in nested contexts
- Option/Result interaction: widening before or after Option/Result coercion?
  - Recommended: exact match → widening → Option/Result wrapping

## 8. Runtime Implications

Note: This is a type-checking feature. No runtime widening code needed yet - that's for the interpreter/compiler implementation phase. This plan focuses only on type checking.

## 9. Key Design Questions

### Q1: Bare math operator behavior

From README:326-359:
```
// this checks to `int` because the `*` binop,
// forcing the literals to be int
let a = 1 * 2
// locals also get coercions
let b: u32 = 1
// another `int`
let c = b * b
```

This means:
- Bare `+`, `-`, `*` on fixed ints → widen both operands to int, return int
- This is different from current behavior where bare ops only work on float/bigint
- The widening happens at the operator level, not just at assignment

### Q2: Literal synthesis context-sensitivity

Should `1` synthesize differently in these contexts?
- `let a = 1` → u32 (current behavior, keep)
- `let b = 1 + 1` → int (because of bare operator)

Options:
- A) Keep literal synthesis as u32, let operator handle widening to int
- B) Make literal synthesis context-aware (complex)

Recommendation: Option A - simpler, follows existing bidirectional typing patterns.

### Q3: Widening vs coercion priority

When checking `expr` against `expected_type`, if exact match fails:
1. Try widening: can_widen_to(synthesized, expected)?
2. Try Option wrapping: expected is ?T and synthesized matches T?
3. Try Result wrapping: expected is !T and synthesized matches T?

Current order in datalit/tycheck.rs:758-774:
- Option wrapping (line 758)
- Result wrapping (line 771)
- Subsume (synthesis + exact match)

Proposed order:
- Exact match via subsume
- Widening
- Option wrapping
- Result wrapping

This ensures `let a: u16 = @(1u8)` uses widening, not Option.
