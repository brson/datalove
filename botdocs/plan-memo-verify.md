# Plan: Module Content Hashes and Salsa Memoization Verification

## Goal

Develop a test suite that verifies Salsa memoization is working correctly at the module level. When a module changes, only that module and its dependents should be recomputed.

## Design from botspec (design-notes.md 2026-01-07)

> The module graph forms a DAG. We use this to create strong content hashes for every module instantiation. This content hash includes the source code of a module, and the configuration of that module including which modules the requires/import demands are bound to.

## Progress

- [x] Phase 1: Module content hashes
- [x] Unit tests for hash propagation behavior
- [x] Phase 2: Salsa verification test infrastructure
- [x] Phase 3: Track Salsa recomputations via event logger

## Approach

### Phase 1: Add Module Content Hashes to ParsedModuleGraph [DONE]

Added `module_content_hashes: BTreeMap<ModuleId, u64>` field to `ParsedModuleGraph`.

Each module's content hash is computed as:
```
hash(module_source, sorted([(alias, dep_content_hash) for each resolved_require]))
```

This is recursive: a module's hash includes its dependencies' hashes, so changes propagate up the DAG.

**Implementation:**
- `crates/datalove-datafun-tycheck/src/lib.rs` - Added field to `ParsedModuleGraph`
- `crates/datalove-datafun-compiler/src/module_graph.rs` - Added `compute_module_content_hashes()` function

**Unit tests** in `module_graph.rs` verify:
- Hash changes when source changes
- Identical source produces identical hash
- Dependent hash changes when dependency changes
- Unrelated module hash unchanged
- Transitive dependency changes propagate
- Multiple dependencies all affect hash
- Alias name affects hash

### Phase 2 & 3: Salsa Event Logging Infrastructure [DONE]

Created `LoggingDatabase` in `module_graph.rs` tests that:
- Uses `salsa::Storage::new(Some(callback))` to capture events
- Stores events in `Arc<Mutex<Vec<salsa::Event>>>`
- Provides `executed_queries()` to get queries that ran (not cached)
- Provides `clear_events()` to reset between test phases

**Memoization verification tests:**
- `test_salsa_caches_identical_input` - Verifies second run with same input is fully cached
- `test_salsa_recomputes_on_source_change` - Verifies changed source triggers recomputation
- `test_salsa_memoization_matches_hash_changes` - Verifies hash changes correlate with recomputation

## Verification Strategy

1. **Hash change tracking**: After a change, compare old vs new `module_content_hashes`
2. **Salsa recomputation tracking**: Use Salsa's event logging to see which queries re-executed
3. **Assertion**: `modules_with_hash_changes == modules_that_recomputed`

## Key Invariant

If `module_content_hashes[M]` is unchanged between runs, then:
- `typecheck_module_graph` should NOT re-typecheck M
- The typechecker should return cached results for M

If `module_content_hashes[M]` changed, then:
- Salsa MUST have detected a change in M's inputs
- The typechecker MUST have re-executed for M
