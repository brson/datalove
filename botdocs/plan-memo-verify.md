# Plan: Module Content Hashes and Salsa Memoization Verification

## Goal

Develop a test suite that verifies Salsa memoization is working correctly at the module level. When a module changes, only that module and its dependents should be recomputed.

## Design from botspec (design-notes.md 2026-01-07)

> The module graph forms a DAG. We use this to create strong content hashes for every module instantiation. This content hash includes the source code of a module, and the configuration of that module including which modules the requires/import demands are bound to.

## Progress

- [x] Phase 1: Module content hashes
- [x] Unit tests for hash propagation behavior
- [ ] Phase 2: Salsa verification test infrastructure
- [ ] Phase 3: Track Salsa recomputations via event logger

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

### Phase 2: Create Salsa Verification Test Infrastructure

Create a test that:
1. Creates a package world with multiple modules (A depends on B, B depends on C)
2. Runs parse + typecheck
3. Records which module hashes exist
4. Makes a small change to one module
5. Runs parse + typecheck again
6. Compares: modules whose hashes changed should match modules that Salsa recomputed

### Phase 3: Track Salsa Recomputations via Event Logger

Implement Salsa's `Events` trait to log which queries execute vs return cached results.

```rust
struct QueryLogger {
    executed_queries: RefCell<Vec<String>>,
}

impl salsa::Database for TestDb {
    fn salsa_event(&self, event: &dyn Fn() -> salsa::Event) {
        let event = event();
        if let salsa::EventKind::WillExecute { .. } = event.kind {
            // Record that this query is executing (not cached)
            self.logger.executed_queries.borrow_mut().push(format!("{:?}", event));
        }
    }
}
```

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
