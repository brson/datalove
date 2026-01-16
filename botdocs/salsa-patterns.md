# Salsa Patterns Guide

Patterns learned from implementing memoization and parallelization in the datalove compiler.

## Salsa Type Categories

### 1. Input Types (`#[salsa::input]`)

External data that enters the system. Changes to inputs trigger recomputation.

```rust
#[salsa::input]
pub struct Source {
    #[returns(ref)]
    pub text: String,
}

#[salsa::input]
pub struct Module {
    pub id: ModuleId,
    pub source: Source,
}

#[salsa::input]
pub struct ModuleGraph {
    #[returns(ref)]
    pub modules: Vec<Module>,
}
```

**Key properties:**
- Created with `Type::new(db, fields...)`
- Serve as entry points for computation
- Use `#[returns(ref)]` for borrowed access to contained data

### 2. Tracked Types (`#[salsa::tracked]`)

Computed data that salsa caches. Identity based on all fields.

```rust
#[salsa::tracked]
pub struct TypeAndHeap<'db> {
    pub heap: Heap,
    #[returns(ref)]
    pub ty: Type<'db>,
}

#[salsa::tracked]
pub struct SingleModuleTypecheckResult<'db> {
    pub module_id: ModuleId,
    #[returns(ref)]
    pub errors: Vec<TypeError>,
    #[returns(ref)]
    pub expr_types: Vec<Option<TypeAndHeap<'db>>>,
}
```

**Key properties:**
- Can only be created inside tracked functions
- Identity includes all fields (careful with Vec contents)
- Use `#[returns(ref)]` for collections to avoid cloning

### 3. Interned Types (`#[salsa::interned]`)

Deduplicated values. Equal content = same ID.

```rust
#[salsa::interned]
pub struct InternedText<'db> {
    #[returns(ref)]
    pub text: String,
}
```

**Key properties:**
- Lightweight handles (just an ID)
- Same content always returns same struct
- Great for identifiers, paths, strings

### 4. Accumulators (`#[salsa::accumulator]`)

Side-channel data collection (diagnostics).

```rust
#[salsa::accumulator]
pub struct TypeDiagnostic(StoredDiagnostic);
```

**Usage:**
```rust
TypeDiagnostic(diagnostic).accumulate(db);
```

**Warning:** Accumulators have caching issues - if the tracked function that accumulated them gets a cache hit, the accumulations won't fire. Prefer returning diagnostics in result structs.

## Tracked Functions

### Basic Pattern

```rust
#[salsa::tracked]
pub fn parse_module_full<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
) -> ParseResult<'db> {
    // computation...
}
```

**Memoization key:** All non-db parameters determine cache identity.

### Per-Module Memoization Pattern

For module-level caching, use `Module` as the identity key:

```rust
#[salsa::tracked]
pub fn typecheck_module<'db>(
    db: &'db dyn crate::Db,
    module: Module,                              // Identity key
    parsed: ParsedStatements<'db>,               // Derived data
    spans: DatafunSpans,
    resolved_imports: Vec<ResolvedImportData<'db>>,
    import_errors: Vec<TypeError>,
) -> SingleModuleTypecheckResult<'db> {
    // ...
}
```

This caches per-module even though other parameters change when source changes.

### Graph-Level vs Module-Level Functions

Two patterns for working with module graphs:

1. **Graph-level tracked function** (aggregates results):
```rust
#[salsa::tracked]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    for module in graph.iter_modules(db) {
        // Call per-module tracked function
        let result = typecheck_module(db, module, ...);
        // Aggregate results...
    }
}
```

2. **Per-module tracked function** (cached individually):
```rust
#[salsa::tracked]
pub fn typecheck_module<'db>(
    db: &'db Db,
    module: Module,
    // other params...
) -> SingleModuleTypecheckResult<'db> {
    // Process single module
}
```

## Parallel Execution Pattern

### DbClone Trait

Enable database cloning for parallel execution:

```rust
pub trait DbClone: salsa::Database {
    /// Clone the database for use on another thread.
    fn dyn_clone(&self) -> Box<dyn DbClone + Send>;

    /// Get a reference to self as a salsa::Database trait object.
    fn as_salsa_db(&self) -> &dyn salsa::Database;
}
```

**Implementation:**
```rust
impl DbClone for Database {
    fn dyn_clone(&self) -> Box<dyn DbClone + Send> {
        Box::new(self.clone())
    }

    fn as_salsa_db(&self) -> &dyn salsa::Database {
        self
    }
}
```

### Cache Warming Pattern

Parallel execution warms the cache, then sequential aggregation hits it:

```rust
pub fn typecheck_module_graph_parallel<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    use rayon::prelude::*;

    // Clone databases upfront
    let work: Vec<_> = modules
        .map(|module| (db.dyn_clone(), module))
        .collect();

    // Parallel phase - warms salsa cache
    work.into_par_iter().for_each(|(db_clone, module)| {
        let db_salsa = db_clone.as_salsa_db();
        let _ = typecheck_module(db_salsa, module, ...);
    });

    // Sequential aggregation - hits cached results
    typecheck_module_graph(db.as_salsa_db(), parsed_graph)
}
```

**Key insight:** Database clones share `Arc<Zalsa>` (global cache state) but have separate `ZalsaLocal` (thread-local state). Parallel workers write to shared cache; sequential phase reads from it.

## Verifying Memoization

### Query Logging Infrastructure

Thread-local logging to verify cache behavior:

```rust
// In tracked functions, log execution:
log_query("typecheck", module_path, QueryPhase::Start);
// ... do work ...
log_query("typecheck", module_path, QueryPhase::End);

// In tests:
enable_query_logging();
let result = typecheck_module_graph(db, graph);
let entries = disable_query_logging();
let executed = get_executed_modules(&entries, "typecheck");
assert_eq!(executed.len(), expected_count);
```

### Salsa Event Handler

For direct salsa event observation:

```rust
struct LoggingDatabase {
    storage: salsa::Storage<Self>,
    events: Arc<Mutex<Vec<salsa::Event>>>,
}

impl LoggingDatabase {
    fn new() -> Self {
        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();
        Self {
            storage: salsa::Storage::new(Some(Box::new(move |event| {
                events_clone.lock().unwrap().push(event);
            }))),
            events,
        }
    }

    fn executed_queries(&self) -> Vec<String> {
        self.events.lock().unwrap()
            .iter()
            .filter_map(|event| {
                if let salsa::EventKind::WillExecute { database_key } = &event.kind {
                    Some(format!("{:?}", database_key))
                } else {
                    None
                }
            })
            .collect()
    }
}
```

### Test Pattern

```rust
#[test]
fn test_memoization_works() {
    let mut db = Database::default();

    // First run - executes queries
    enable_query_logging();
    let _ = typecheck_module_graph(&db, graph);
    let first = disable_query_logging();
    assert!(get_executed_modules(&first, "typecheck").len() > 0);

    // Second run - should hit cache (0 queries)
    enable_query_logging();
    let _ = typecheck_module_graph(&db, graph);
    let second = disable_query_logging();
    assert_eq!(get_executed_modules(&second, "typecheck").len(), 0);
}
```

## Parallel Safety: Stable Identifiers

### Problem: Non-Deterministic Salsa IDs

Raw salsa IDs (`expr.as_id().index()`) depend on allocation order, which is non-deterministic in parallel:

```rust
// BAD: Parallel order affects ID values
AnalysisError::UseAfterMove {
    expr_id: expr.as_id().index() as u32,  // Non-deterministic!
    name: name.to_string(),
}
```

### Solution: Use Semantic Indices

Use indices that are deterministic within their containing scope:

```rust
// GOOD: local_index is sequential within function
AnalysisError::UseAfterMove {
    local_index: expr.local_index(db),  // Deterministic
    name: name.to_string(),
}
```

Where `local_index` is the expression's sequential position within its function body.

## Common Pitfalls

### 1. Creating Tracked Structs Outside Tracked Functions

```rust
// BAD: Tracked struct created outside tracked function
fn helper<'db>(db: &'db Db) -> MyTrackedStruct<'db> {
    MyTrackedStruct::new(db, ...)  // Error!
}
```

**Solution:** Return plain data from helpers, create tracked struct in tracked function.

### 2. Accumulator Caching Issues

Accumulators don't fire on cache hits. If a tracked function accumulated diagnostics and later gets a cache hit, those diagnostics won't be accumulated again.

**Solution:** Return diagnostics in the result struct instead.

### 3. Identity Instability

If a tracked struct's fields change when they shouldn't (e.g., different expression order in parallel), memoization breaks.

**Solution:** Ensure all fields are deterministic. Use sorted collections, stable IDs.

### 4. Over-Granular Dependencies

If `resolve_all_exports(db, parsed_graph)` depends on the entire graph, ANY module change invalidates ALL modules' import resolution.

**Solution:** For true per-module caching, tracked functions should take `Module` as key, not the whole graph. Graph-level aggregation then calls per-module functions.

## Summary: The Three-Layer Pattern

1. **Per-module tracked functions**: Maximum caching, cache key is `Module`
   - `parse_module_full(db, module) -> ParseResult`
   - `typecheck_module(db, module, ...) -> SingleModuleTypecheckResult`
   - `resolve_module_imports(db, module, ...) -> ModuleImportResolution`

2. **Graph-level tracked functions**: Aggregate per-module results
   - `parse_module_graph(db, graph) -> ParsedModuleGraph`
   - `typecheck_module_graph(db, parsed_graph) -> ModuleGraphTypecheckResult`

3. **Parallel entry points**: Warm cache, then delegate to graph-level
   - `parse_module_graph_parallel(db, graph) -> ParsedModuleGraph`
   - `typecheck_module_graph_parallel(db, parsed_graph) -> ModuleGraphTypecheckResult`
