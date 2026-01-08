# Salsa Memoization Testing: Findings and Lessons

## Summary

Successfully implemented per-module caching for both parsing and typechecking.
Verified with invasive query logging. Fixed Salsa accumulator limitation using side table pattern.

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

**`parse_module` returns full `ParseResult` with spans:**
```rust
#[salsa::tracked]
pub fn parse_module<'db>(db: &'db dyn Database, module: Module) -> ParseResult<'db> {
    let module_path = module.id(db).path(db);
    let source = module.source(db);

    log_query("parse", module_path, QueryPhase::Start);
    let parse_result = datalove_datafun_parser::parse(db, source);
    log_query("parse", module_path, QueryPhase::End);

    parse_result  // Contains both parsed statements and spans
}
```

**Spans extracted from `ParseResult` in `parse_module_graph`:**
```rust
for module in graph.iter_modules(db) {
    let parse_result = parse_module(db, module);
    let parsed = parse_result.parsed(db);
    // Convert ParseSpanEntry to SpanMapEntry for DatafunSpans.
    let span_entries = parse_result.expr_spans(db).iter()
        .map(|e| SpanMapEntry { ... })
        .collect();
    let spans = DatafunSpans::new(db, span_entries);
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

### Parse Caching Tests

| Test | Verifies |
|------|----------|
| `test_query_log_per_module_caching` | Only changed module re-parses in graph |
| `test_parse_module_direct_caching` | Single module caches correctly |
| `test_parse_module_two_modules_direct` | Two modules cache independently |
| `test_parse_module_no_change_still_cached` | No changes = fully cached |
| `test_datafun_spans_caching_fixed` | Verifies span caching works after fix |
| `test_salsa_events_on_change` | Salsa event logging works |

### Typecheck Caching Tests

| Test | Verifies |
|------|----------|
| `test_typecheck_records_all_modules` | Typecheck logging records all modules |
| `test_typecheck_per_module_caching` | Only changed module re-typechecks |
| `test_typecheck_no_change_fully_cached` | No changes = fully cached |
| `test_typecheck_with_import_caching` | Import relationships preserve caching |

## Fixed Issues (2026-01-08)

### datafun_spans Accumulator Bug - FIXED

Implemented the side table pattern (following datalit):
- Added `ParseSpanEntry` to ast.rs and `expr_spans` field to `ParseResult`
- Parser accumulates spans in a Vec instead of using Salsa accumulators
- Sub-parsers call `merge_spans_from()` to propagate spans to parent
- `parse_module` returns full `ParseResult` (both statements and spans)
- `datafun_spans()` reads from `ParseResult.expr_spans`
- `DatafunSpanAccumulator` removed

**Key insight:** Accumulators are global to a tracked function call, but when the function is cached, `accumulated()` returns empty. The side table pattern embeds spans directly in the return value, avoiding this issue.

## Remaining Known Issues

1. **Graph-level vs module-level granularity** - graph re-runs loop, inner calls cached

## Verified Working (2026-01-08)

### Typecheck Per-Module Caching - VERIFIED

- `typecheck_module` is a `#[salsa::tracked]` function with query logging
- Tests verify only changed modules re-typecheck
- Import relationships don't break caching (resolved imports are stable)

## Potential Next Steps

1. **Benchmark incremental performance**
   - Measure actual parse/typecheck times with caching
   - Compare cached vs uncached runs
   - Profile large module graphs

2. **Hash-based verification**
   - Assert that modules with unchanged content hashes are cached
   - Assert that changed hashes trigger recomputation
   - Tie `module_content_hashes` to actual caching behavior

3. **Consider durability for hot-reload**
   - Salsa durability levels for edit-time vs compile-time inputs
   - Optimize for interactive development workflows
