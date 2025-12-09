# Plan: Algorithmic Error Generation for funlit_equiv Tests

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
