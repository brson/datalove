# Plan: Module Content Hashes and Salsa Memoization Verification

## Goal

Develop a test suite that verifies Salsa memoization is working correctly at the module level. When a module changes, only that module and its dependents should be recomputed.

## Design from botspec (design-notes.md 2026-01-07)

> The module graph forms a DAG. We use this to create strong content hashes for every module instantiation. This content hash includes the source code of a module, and the configuration of that module including which modules the requires/import demands are bound to.

## Approach

### Phase 1: Add Module Content Hashes to ParsedModuleGraph

Add a new field `module_content_hashes: BTreeMap<ModuleId, u64>` to `ParsedModuleGraph`.

Each module's content hash is computed as:
```
hash(module_source, sorted([(alias, dep_content_hash) for each resolved_require]))
```

This is recursive: a module's hash includes its dependencies' hashes, so changes propagate up the DAG.

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

## Files to Modify

### 1. `crates/datalove-datafun-tycheck/src/lib.rs`

Add to `ParsedModuleGraph`:
```rust
#[salsa::tracked]
pub struct ParsedModuleGraph<'db> {
    // ... existing fields ...

    /// Recursive content hashes for each module.
    /// Hash includes: source text + sorted dependency hashes.
    #[returns(ref)]
    pub module_content_hashes: BTreeMap<ModuleId, u64>,
}
```

### 2. `crates/datalove-datafun-compiler/src/module_graph.rs`

Update `parse_module_graph()` to compute content hashes:

```rust
fn compute_module_content_hashes<'db>(
    db: &'db dyn salsa::Database,
    graph: &ModuleGraph,
    resolved_requires: &BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
) -> BTreeMap<ModuleId, u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hashes = BTreeMap::new();

    // Process in dependency order (graph.iter_modules is already sorted)
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let source = module.source(db);

        let mut hasher = DefaultHasher::new();

        // Hash source text
        source.text(db).hash(&mut hasher);

        // Hash resolved requires with their content hashes (sorted for determinism)
        if let Some(requires) = resolved_requires.get(&module_id) {
            let mut dep_hashes: Vec<_> = requires.iter()
                .filter_map(|(alias, target_id)| {
                    hashes.get(target_id).map(|h| (alias.clone(), *h))
                })
                .collect();
            dep_hashes.sort_by(|a, b| a.0.cmp(&b.0));
            dep_hashes.hash(&mut hasher);
        }

        hashes.insert(module_id, hasher.finish());
    }

    hashes
}
```

### 3. `crates/datalove-datafun/tests/salsa_memoization_tests.rs` (new file)

Create test suite:

```rust
//! Tests for verifying Salsa memoization behavior.

/// Test: changing a leaf module only recomputes that module.
fn test_leaf_change_only_recomputes_leaf() { ... }

/// Test: changing a dependency recomputes all dependents.
fn test_dependency_change_recomputes_dependents() { ... }

/// Test: changing unrelated module doesn't affect others.
fn test_unrelated_change_isolated() { ... }
```

### 4. `crates/datalove-datafun/Cargo.toml`

Add test entry:
```toml
[[test]]
name = "salsa_memoization_tests"
path = "tests/salsa_memoization_tests.rs"
harness = false
```

## Test Fixture Structure

```
tests/fixtures/salsa_memoization/
├── base.world           # Worldfile with A -> B -> C dependency chain
├── modules/
│   ├── a.dfm            # Module A (depends on B)
│   ├── b.dfm            # Module B (depends on C)
│   └── c.dfm            # Module C (leaf)
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
