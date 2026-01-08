# Salsa Memoization Testing: Findings and Lessons

## Summary

Successfully implemented per-module parse caching and verified it with invasive query logging.
Discovered and documented a significant limitation with Salsa accumulators.

## What We Built

### Infrastructure

1. **Thread-local query logging** (`datalove-ct` crate)
   - `enable_query_logging()` / `disable_query_logging()` to capture query execution
   - `log_query(name, module_path, phase)` called inside tracked functions
   - `get_executed_modules(log, query_name)` extracts which modules were processed

2. **Per-module parse function** (`parse_module` in `module_graph.rs`)
   - `#[salsa::tracked]` function that takes a single `Module`
   - Returns only `ParsedStatements` (not a tuple with spans)
   - Enables fine-grained caching: unchanged modules don't re-parse

3. **LoggingDatabase** for Salsa event capture
   - Wraps `salsa::Storage::new(Some(callback))` to capture events
   - `executed_queries()` returns queries that actually ran (not cached)
   - `clear_events()` resets between test phases

### Key Changes

**`parse_module` signature (was returning tuple, now single value):**
```rust
#[salsa::tracked]
pub fn parse_module<'db>(db: &'db dyn Database, module: Module) -> ParsedStatements<'db> {
    let module_path = module.id(db).path(db);
    let source = module.source(db);

    log_query("parse", module_path, QueryPhase::Start);
    let parse_result = datalove_datafun_parser::parse(db, source);
    log_query("parse", module_path, QueryPhase::End);

    parse_result.parsed(db)
}
```

**Spans retrieved separately in `parse_module_graph`:**
```rust
for module in graph.iter_modules(db) {
    let parsed = parse_module(db, module);
    // Get spans separately - not memoized due to accumulator pattern issues.
    let spans = datalove_datafun_parser::datafun_spans(db, source);
    parsed_statements.push((module_id, parsed, spans));
}
```

**`DatafunSpans` changed from `tracked` to `interned`:**
```rust
// OLD: #[salsa::tracked]
// NEW:
#[salsa::interned]
pub struct DatafunSpans<'db> {
    #[returns(ref)]
    pub entries: Vec<SpanMapEntry>,
}
```

## What We Learned

### 1. Salsa Mutation Requires `set_text(&mut db).to(...)`

Creating new `Source` objects each time defeats caching.
Must reuse `Source` objects and mutate them:

```rust
// WRONG - creates new Source each run, no caching possible
let source = Source::new(&db, "let x = 1".to_string());

// RIGHT - mutate existing Source to trigger incremental recomputation
source.set_text(&mut db).to("let x = 2".to_string());
```

Requires `use salsa::Setter;` and `let mut db`.

### 2. Salsa Accumulators Break Memoization (Critical Issue)

`#[salsa::accumulator]` types have a fundamental caching problem:
- When the function that emits accumulators is cached, `accumulated()` returns **empty**
- This means span information is lost on subsequent calls

**The problem:**
```rust
// datafun_spans uses accumulators internally
let spans1 = datafun_spans(&db, source_a);  // Has entries
source_b.set_text(&mut db).to("...");        // Change unrelated source
let spans2 = datafun_spans(&db, source_a);  // EMPTY - parse was cached
```

**Root cause:** The underlying `parse_for_diagnostics` is correctly cached,
but `accumulated()` doesn't preserve values from cached executions.

**Workaround:** Include spans in `ParseResult` like datalit does, rather than using accumulators.

### 3. Tracked vs Interned for Return Types

If a tracked function creates and returns a `#[salsa::tracked]` struct:
- Each execution creates a new struct with a new ID
- Even with identical contents, different IDs break downstream caching

Solution: Use `#[salsa::interned]` instead:
- Identical content produces identical ID
- Functions returning interned types can be memoized correctly

### 4. Per-Module vs Graph-Level Caching

`parse_module_graph` is a tracked function. When ANY input changes, the whole function re-runs.
But the inner `parse_module` calls are cached per-module.

The loop iterates all modules, but only changed modules actually re-parse:
```
First run:  parse a, parse b, parse c  (all execute)
Change c:   parse a (cached), parse b (cached), parse c (executes)
```

## Test Coverage

| Test | Verifies |
|------|----------|
| `test_query_log_per_module_caching` | Only changed module re-parses in graph |
| `test_parse_module_direct_caching` | Single module caches correctly |
| `test_parse_module_two_modules_direct` | Two modules cache independently |
| `test_parse_module_no_change_still_cached` | No changes = fully cached |
| `test_datafun_spans_known_caching_issue` | Documents accumulator bug |
| `test_salsa_events_on_change` | Salsa event logging works |

## Known Issues

1. **datafun_spans accumulator bug** - documented in test, workaround in place
2. **Graph-level vs module-level granularity** - graph re-runs loop, inner calls cached

## Potential Next Steps

1. **Fix datafun_spans properly**
   - Include spans in `ParseResult` struct
   - Remove accumulator usage for spans
   - Follow datalit's pattern

2. **Add typecheck per-module caching**
   - Create `typecheck_module` tracked function
   - Add query logging to typecheck path
   - Verify only changed modules re-typecheck

3. **Benchmark incremental performance**
   - Measure actual parse/typecheck times with caching
   - Compare cached vs uncached runs
   - Profile large module graphs

4. **Hash-based verification**
   - Assert that modules with unchanged content hashes are cached
   - Assert that changed hashes trigger recomputation
   - Tie `module_content_hashes` to actual caching behavior

5. **Consider durability for hot-reload**
   - Salsa durability levels for edit-time vs compile-time inputs
   - Optimize for interactive development workflows
