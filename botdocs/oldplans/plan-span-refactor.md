# Span Refactor: On-Demand Span Queries

## Overview

Refactor type checking to query spans on-demand rather than passing large `Vec` bundles as parameters to tracked functions. This improves Salsa memoization by removing span data from function signatures, while maintaining accurate source location information for diagnostics.

**Current Problem:** Type check functions take span vectors as parameters:
```rust
pub fn type_check(
    db: &dyn Db,
    script: Script,
    expr_spans: Vec<(ExprFun, Text, ByteSpan)>,
    datalit_expr_spans: Vec<(ExprFull, Text, ByteSpan)>,
) -> TypecheckResult
```

**Issue:** Any change to spans invalidates Salsa memoization, even if AST structure is identical (e.g., whitespace-only changes).

**Solution:** Store spans in separate tracked queries, look up on-demand during diagnostic emission:
```rust
pub fn type_check(
    db: &dyn Db,
    source: Source,
    script: Script,
) -> TypecheckResult
```

## Motivation

1. **Cleaner API:** Type check takes `(source, script)` instead of `(script, spans, datalit_spans)`
2. **Better memoization potential:** Span changes don't affect type checking cache key
3. **Lazy evaluation:** Spans only queried when diagnostics are emitted
4. **Separation of concerns:** Type checking logic separated from span management
5. **Future-proof:** Easy path to content-addressed ASTs (green/red trees) later

## Architecture

### Data Flow

```
┌─────────────┐
│   Source    │
└──────┬──────┘
       │
       ├──────────────┬─────────────────┐
       ▼              ▼                 ▼
   ┌────────┐   ┌──────────┐    ┌──────────┐
   │  AST   │   │ DatafunS │    │ DatalitS │
   │  Parse │   │   pans   │    │   pans   │
   └────┬───┘   └──────┬───┘    └─────┬────┘
        │              │              │
        │         (queried on-demand) │
        │              │              │
        └──────►┌──────▼──────────────▼─┐
                │   Type Check          │
                │  (source, script)     │
                └───────────────────────┘
```

### Key Insight

Type checking depends on `source` parameter, so source changes still invalidate the cache. However:
- **Current:** Passing spans directly means memoization key includes large Vec data
- **Proposed:** Source reference is smaller, actual span lookup is deferred
- **Future:** Content-addressed ASTs would make source changes not invalidate if structure unchanged

This refactor is a stepping stone toward better incremental compilation.

## Component Design

### 1. SpanEntry (datalove-diagnostic)

**Location:** `crates/datalove-diagnostic/src/lib.rs`

```rust
/// Single span entry for efficient lookup.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanEntry {
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

impl SpanEntry {
    pub fn new(text_id: salsa::Id, span: ByteSpan) -> Self {
        SpanEntry { text_id, span }
    }

    pub fn to_text_and_span<'db>(&self, db: &'db dyn salsa::Database) -> (bct::text::Text<'db>, ByteSpan) {
        use salsa::plumbing::FromId;
        (bct::text::Text::from_id(self.text_id), self.span.clone())
    }
}
```

**Rationale:**
- Store salsa::Id instead of Text<'db> for Salsa compatibility
- Convert to typed references on retrieval
- Lightweight and hashable for HashMap storage

### 2. Datafun Span Infrastructure

**Location:** `crates/datalove-datafun/src/spans.rs` (new file)

```rust
use rmx::prelude::*;
use std::collections::HashMap;
use datalove_diagnostic::SpanEntry;
use crate::ast::ExprFun;

/// Tracked struct for datafun expression spans.
#[salsa::tracked]
pub struct DatafunSpans<'db> {
    #[return_ref]
    pub entries: HashMap<salsa::Id, SpanEntry>,
}

impl<'db> DatafunSpans<'db> {
    /// Look up span for an expression.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: ExprFun<'db>) -> Option<&SpanEntry> {
        use salsa::plumbing::AsId;
        self.entries(db).get(&expr.as_id())
    }
}

/// Tracked struct for embedded datalit expression spans.
#[salsa::tracked]
pub struct DatalitSpans<'db> {
    #[return_ref]
    pub entries: HashMap<salsa::Id, SpanEntry>,
}

impl<'db> DatalitSpans<'db> {
    /// Look up span for a datalit expression.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: datalove_datalit::ast::ExprFull<'db>) -> Option<&SpanEntry> {
        use salsa::plumbing::AsId;
        self.entries(db).get(&expr.as_id())
    }
}

/// Extract datafun expression spans from a parsed source.
#[salsa::tracked]
pub fn datafun_spans<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> DatafunSpans<'db> {
    let parse_result = crate::parser::parse(db, source);
    let mut entries = HashMap::new();

    for (expr, text, span) in &parse_result.expr_spans {
        use salsa::plumbing::AsId;
        entries.insert(
            expr.as_id(),
            SpanEntry::new(text.as_id(), span.clone())
        );
    }

    DatafunSpans::new(db, entries)
}

/// Extract datalit expression spans from a parsed datafun source.
#[salsa::tracked]
pub fn datalit_spans<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> DatalitSpans<'db> {
    let parse_result = crate::parser::parse(db, source);
    let mut entries = HashMap::new();

    for (expr, text, span) in &parse_result.datalit_expr_spans {
        use salsa::plumbing::AsId;
        entries.insert(
            expr.as_id(),
            SpanEntry::new(text.as_id(), span.clone())
        );
    }

    DatalitSpans::new(db, entries)
}
```

**Rationale:**
- Separate tracked structs for datafun and datalit spans
- HashMap keyed by salsa::Id for O(1) lookup
- Query functions extract spans from existing parse results

### 3. Datalit Span Infrastructure

**Location:** `crates/datalove-datalit/src/spans.rs` (new file)

```rust
use rmx::prelude::*;
use std::collections::HashMap;
use datalove_diagnostic::SpanEntry;
use crate::ast::ExprFull;

/// Tracked struct for datalit expression spans.
#[salsa::tracked]
pub struct DatalitSpans<'db> {
    #[return_ref]
    pub entries: HashMap<salsa::Id, SpanEntry>,
}

impl<'db> DatalitSpans<'db> {
    /// Look up span for an expression.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: ExprFull<'db>) -> Option<&SpanEntry> {
        use salsa::plumbing::AsId;
        self.entries(db).get(&expr.as_id())
    }

    /// Get text and span for an expression.
    pub fn get_text_and_span(&self, db: &'db dyn crate::Db, expr: ExprFull<'db>) -> Option<(bct::text::Text<'db>, datalove_diagnostic::ByteSpan)> {
        self.lookup(db, expr).map(|entry| entry.to_text_and_span(db))
    }
}

/// Extract datalit expression spans from a parsed source.
#[salsa::tracked]
pub fn datalit_spans<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> DatalitSpans<'db> {
    let parse_result = crate::parser::parse(db, source);
    let mut entries = HashMap::new();

    for (expr, text, span) in &parse_result.expr_spans {
        use salsa::plumbing::AsId;
        entries.insert(
            expr.as_id(),
            SpanEntry::new(text.as_id(), span.clone())
        );
    }

    DatalitSpans::new(db, entries)
}
```

**Rationale:**
- Mirrors datafun structure for consistency
- Helper method `get_text_and_span()` for convenience

### 4. Datafun TypeContext Changes

**Location:** `crates/datalove-datafun/src/tycheck.rs`

```rust
/// Context for typechecking with on-demand span lookup.
struct TypeContext<'db> {
    db: &'db dyn crate::Db,
    source: bct::input::Source,  // Changed: store source instead of span vecs
    function_types: HashMap<InternedText<'db>, FunctionSignature<'db>>,
    variables: HashMap<InternedText<'db>, TypeFun<'db>>,
    in_function: bool,
    current_function_return_type: Option<TypeFun<'db>>,
}

impl<'db> TypeContext<'db> {
    fn new(db: &'db dyn crate::Db, source: bct::input::Source) -> Self {
        TypeContext {
            db,
            source,
            function_types: HashMap::new(),
            variables: HashMap::new(),
            in_function: false,
            current_function_return_type: None,
        }
    }

    /// Look up span for a datafun expression (on-demand).
    fn get_span(&self, expr: ExprFun<'db>) -> Option<(Text<'db>, ByteSpan)> {
        let spans = crate::spans::datafun_spans(self.db, self.source);
        spans.lookup(self.db, expr).map(|entry| entry.to_text_and_span(self.db))
    }

    /// Look up span for a datalit expression (on-demand).
    fn get_datalit_span(&self, expr: datalit::ast::ExprFull<'db>) -> Option<(Text<'db>, ByteSpan)> {
        let spans = crate::spans::datalit_spans(self.db, self.source);
        spans.lookup(self.db, expr).map(|entry| entry.to_text_and_span(self.db))
    }

    // ... rest of methods unchanged
}
```

**Rationale:**
- Store source reference instead of large span vectors
- Query spans on-demand via tracked functions
- Separate methods for datafun vs datalit spans

### 5. Datafun Type Check Signatures

**Location:** `crates/datalove-datafun/src/tycheck.rs`

```rust
/// Typecheck a script with on-demand span lookup.
#[salsa::tracked]
pub fn type_check<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    script: Script<'db>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, source);

    // First pass: collect all function signatures.
    for statement in script.statements(db) {
        if let Statement::Fun(stmt) = statement {
            // ... unchanged
        }
    }

    // Second pass: type check all statements.
    for statement in script.statements(db) {
        check_statement(&mut ctx, statement);
    }

    TypecheckResult::new(db, script, ctx.function_types.len() as u32)
}

/// Typecheck a script for diagnostic emission (entry point).
#[salsa::tracked]
pub fn type_check_for_diagnostics<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> TypecheckResult<'db> {
    let parse_result = crate::parser::parse(db, source);
    type_check(db, source, parse_result.script)
}

/// Typecheck with package world support.
#[salsa::tracked]
pub fn type_check_with_package_world<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    script: Script<'db>,
    package_world: crate::package::PackageWorld,
    package_world_typecheck: PackageWorldTypecheckResult<'db>,
) -> TypecheckResult<'db> {
    let mut ctx = TypeContext::new(db, source);

    // Build module alias map from require statements...
    // ... rest unchanged
}

/// Wrapper for package world diagnostic emission.
#[salsa::tracked]
pub fn type_check_with_package_world_for_diagnostics<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    package_world: crate::package::PackageWorld,
    package_world_typecheck: PackageWorldTypecheckResult<'db>,
) -> TypecheckResult<'db> {
    let parse_result = crate::parser::parse(db, source);
    type_check_with_package_world(
        db,
        source,
        parse_result.script,
        package_world,
        package_world_typecheck,
    )
}
```

**Key Changes:**
- Removed `expr_spans` and `datalit_expr_spans` parameters
- Added `source` parameter (for span queries)
- All wrappers simplified

### 6. Datalit Resolve Changes

**Location:** `crates/datalove-datalit/src/resolve.rs`

```rust
/// Resolution result with source reference for span lookup.
#[salsa::tracked]
pub struct ResolvedExpr<'db> {
    pub expr: ExprFull<'db>,
    pub resolutions: Vec<ResolutionEntry<'db>>,
    pub errors: Vec<ResolutionErrorEntry<'db>>,
    pub source: bct::input::Source,  // Changed: store source instead of span vec
}

/// Resolve names with on-demand span lookup.
#[salsa::tracked]
pub fn resolve_names<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    expr: ExprFull<'db>,
) -> ResolvedExpr<'db> {
    let mut scope = Scope::new();
    let mut next_id = 0u32;
    let mut resolutions_map = HashMap::new();
    let mut errors_map = HashMap::new();

    // Pass 1: Collect type hint definitions if present.
    if let Some(type_hint_and_heap) = expr.type_hint(db) {
        collect_type_hint_names(db, type_hint_and_heap, &mut scope, &mut next_id);
    }

    // Pass 2: Resolve expression references.
    let expr_and_heap = expr.expr(db);
    let expr_inner = expr_and_heap.expr(db);
    resolve_expr_refs(db, expr_inner, &scope, &mut resolutions_map, &mut errors_map);

    // Convert to tracked structs.
    let resolutions = resolutions_map
        .into_iter()
        .map(|(name, resolution)| ResolutionEntry::new(db, name, resolution))
        .collect();

    let errors = errors_map
        .into_iter()
        .map(|(name, error)| ResolutionErrorEntry::new(db, name, error))
        .collect();

    ResolvedExpr::new(db, expr, resolutions, errors, source)
}
```

**Key Changes:**
- Removed `expr_spans` parameter from `ResolvedExpr`
- Added `source` field to `ResolvedExpr`
- Updated `resolve_names()` signature

### 7. Datalit TypeContext Changes

**Location:** `crates/datalove-datalit/src/tycheck.rs`

```rust
/// Context for typechecking with on-demand span lookup.
struct TypeContext<'db> {
    db: &'db dyn crate::Db,
    source: bct::input::Source,  // Changed: store source instead of HashMap
    resolutions: HashMap<InternedText<'db>, Resolution<'db>>,
    errors: Vec<TypeError>,
}

impl<'db> TypeContext<'db> {
    fn new(db: &'db dyn crate::Db, resolved: ResolvedExpr<'db>) -> Self {
        let resolutions = resolved
            .resolutions(db)
            .iter()
            .map(|entry| (entry.name(db), entry.resolution(db)))
            .collect();

        TypeContext {
            db,
            source: resolved.source(db),  // Extract source from ResolvedExpr
            resolutions,
            errors: Vec::new(),
        }
    }

    /// Look up span for an expression (on-demand).
    fn get_span(&self, expr: ExprFull<'db>) -> Option<(Text<'db>, ByteSpan)> {
        let spans = crate::spans::datalit_spans(self.db, self.source);
        spans.get_text_and_span(self.db, expr)
    }

    // ... rest unchanged
}
```

**Key Changes:**
- Store `source` instead of `expr_spans` HashMap
- Query spans on-demand via tracked function

### 8. Call Site Updates

**Datafun calling Datalit:**

```rust
// Old:
let resolved = datalit::resolve::resolve_names(
    db,
    datalit_expr,
    ctx.datalit_expr_spans.clone()
);

// New:
let resolved = datalit::resolve::resolve_names(
    db,
    ctx.source,  // Pass source instead of spans
    datalit_expr,
);
```

## Migration Plan

### Phase 1: Add Span Infrastructure
1. Add `SpanEntry` to `datalove-diagnostic/src/lib.rs`
2. Create `crates/datalove-datafun/src/spans.rs`
3. Create `crates/datalove-datalit/src/spans.rs`
4. Add span query functions (datafun_spans, datalit_spans)
5. Run tests to verify infrastructure compiles

### Phase 2: Update Datalit
1. Change `ResolvedExpr` to store `source` instead of `expr_spans`
2. Update `resolve_names()` signature to take `source`
3. Update `TypeContext` to store `source` and query spans on-demand
4. Update datalit test helpers
5. Run datalit tests to verify

### Phase 3: Update Datafun
1. Update `TypeContext` to store `source` instead of span vecs
2. Update `type_check()` signatures to take `source, script` instead of `script, spans, datalit_spans`
3. Update datafun → datalit call sites
4. Update datafun test helpers
5. Run datafun tests to verify

### Phase 4: Update CLI and Integration
1. Update `main.rs` call sites to use new signatures
2. Update integration test helpers
3. Run full test suite
4. Verify all 402+ tests pass

### Phase 5: Cleanup
1. Remove old ParseResult span fields (keep as temporary during migration)
2. Update documentation
3. Final verification

## Benefits

1. **Cleaner API:** `type_check(db, source, script)` instead of `type_check(db, script, expr_spans, datalit_expr_spans)`
2. **Lazy evaluation:** Spans only queried when diagnostics are emitted
3. **Better structure:** Tracked structs with HashMap are more efficient than Vec parameters
4. **Separation of concerns:** Type checking separated from span management
5. **Future-proof:** Easy path to content-addressed ASTs later

## Non-Goals

- Content-addressed ASTs (green/red trees) - can be added later
- Avoiding re-parsing when source changes - not achievable without content addressing
- Span-independent memoization - source changes still invalidate everything

The real win will come later when we implement content-addressed ASTs, where whitespace changes don't change the AST identity. Then span queries become truly independent.

## Open Questions

1. **Performance:** Does on-demand span lookup add noticeable overhead?
   - Likely negligible since we only look up spans during diagnostic emission (error cases)
   - Salsa caching should make repeated lookups essentially free

2. **Testing:** Do we need span-specific tests?
   - Existing diagnostic tests validate end-to-end behavior
   - Could add unit tests for span query functions

3. **Module boundaries:** Should spans.rs be part of existing modules or separate?
   - Separate file cleaner for this focused infrastructure
   - Easy to find and understand

## Success Criteria

- ✅ Type check signatures take `(source, script)` instead of `(script, spans, ...)`
- ✅ All diagnostic emission still works correctly
- ✅ All 800+ tests pass
- ✅ No change to diagnostic output quality
- ✅ Code is cleaner and more maintainable

## Implementation Progress

### Status: COMPLETE ✅

All phases completed successfully. The span refactor is now fully functional with proper diagnostic emission.

### Critical Issue Discovered and Resolved

**Problem:** After initial implementation following the plan, tests showed "Failed to build type table" errors instead of detailed diagnostics. Investigation revealed a fundamental Salsa memoization issue.

**Root Cause:**
- The `parse()` function is NOT a Salsa tracked function - it's a plain Rust function
- Multiple code paths were calling `parse(db, source)` directly, each creating fresh AST nodes with different Salsa IDs
- The original plan assumed `parse()` would be memoized, but it wasn't
- This caused span lookup failures: spans were recorded for expression `Id(2c02)` but type checker looked up `Id(2c01)`

**Solution:** Implemented Salsa Accumulators Pattern

Instead of storing spans in `ParseResult` vectors (which get recreated on each `parse()` call), use Salsa's accumulator pattern:

1. **Created accumulator types** in `spans.rs`:
```rust
#[salsa::accumulator]
pub struct DatafunSpanAccumulator {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

#[salsa::accumulator]
pub struct DatalitSpanAccumulator {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}
```

2. **Emit spans during parsing** instead of collecting in vectors:
```rust
fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, text: Text<'db>, span: ByteSpan) -> ast::ExprFun<'db> {
    use salsa::plumbing::AsId;
    let expr = ast::ExprFun::new(self.db, kind);
    // Emit span as accumulator.
    crate::spans::DatafunSpanAccumulator {
        expr_id: expr.as_id(),
        text_id: text.as_id(),
        span,
    }.accumulate(self.db);
    expr
}
```

3. **Query accumulated spans** in span query functions:
```rust
pub fn datafun_spans<'db>(db: &'db dyn crate::Db, source: Source) -> DatafunSpans<'db> {
    // Trigger parsing to accumulate spans.
    crate::parser::parse_for_diagnostics(db, source);

    // Retrieve accumulated spans.
    let accumulated = crate::parser::parse_for_diagnostics::accumulated::<DatafunSpanAccumulator>(db, source);

    let entries: Vec<SpanMapEntry> = accumulated.iter()
        .map(|acc| SpanMapEntry {
            expr_id: acc.expr_id,
            entry: SpanEntry::new(acc.text_id, acc.span.clone()),
        })
        .collect();

    DatafunSpans::new(db, entries)
}
```

4. **Fixed function call paths** to use tracked `parse_for_diagnostics()`:
```rust
// Changed from:
let parse_result = crate::parser::parse(db, source);
type_check(db, source, parse_result.script)

// To:
let script = crate::parser::parse_for_diagnostics(db, source);
type_check(db, source, script)
```

5. **Removed span vectors** from `ParseResult` and `Parser` structs - no longer needed.

### Final Implementation Details

**Modified Files:**
- `crates/datalove-datafun/src/spans.rs` - Added accumulator types, updated span query functions
- `crates/datalove-datafun/src/parser.rs` - Added `create_expr()` helper, emit accumulators, removed span vectors
- `crates/datalove-datafun/src/ast.rs` - Removed `expr_spans` and `datalit_expr_spans` from `ParseResult`
- `crates/datalove-datafun/src/tycheck.rs` - Updated `type_check_for_diagnostics()` to use `parse_for_diagnostics()`
- `crates/datalove-cli/src/main.rs` - Already correct (no changes needed)

**Key Differences from Original Plan:**
- Plan used `HashMap` storage - actual implementation uses `Vec` with linear search (simpler, sufficient for diagnostic use case)
- Plan assumed `parse()` memoization - reality required Salsa accumulator pattern
- Plan stored spans in tracked structs - reality emits them as accumulators during parsing

### Test Results

All 800+ tests passing:
- 136 datafun unit tests ✅
- 93 datalit unit tests ✅
- 300+ runtime tests (btreemap, btreeset, clone, cmp, destroy, eq, list, roundtrip, tensor) ✅
- 21 error tests ✅
- 8 type error tests ✅ (updated expected output with proper diagnostics)
- 70 tycheck tests ✅
- All integration tests ✅

### Example Output

Before (regression):
```
Error: Failed to build type table: Type errors found: 1 errors
```

After (working):
```
Type errors:
error[F001]: cannot find value `undefined_var` in this scope
 --> 01_undefined_variable.dfs:1:14
  |
1 | let output = undefined_var
  |              ^^^^^^^^^^^^^ not found in this scope

Error: 1 type error(s)
```

### Lessons Learned

1. **Salsa memoization requires tracked functions** - plain Rust functions are not memoized
2. **Accumulator pattern is ideal for span storage** - solves the ID mismatch problem elegantly
3. **Always verify Salsa assumptions** - what seems like it "should" memoize may not
4. **Diagnostic testing is critical** - the regression was only caught because we actually ran the CLI
