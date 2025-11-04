# Datalove Diagnostic System Design

## Current Status

**Latest Update:** 2025-11-04

- ✅ Phase 1 Complete: Diagnostic crate created and compiles
- ✅ Phase 2 Complete: AST error nodes updated
  - datafun parser: 9/9 error sites updated ✅
  - datalit parser: 21/21 error sites updated ✅
  - ast_serde: Deferred (recommendation: keep serde simple, only serialize message field)
  - All tests passing ✅
- ✅ Phase 3 Complete: Parser diagnostics implemented
  - datafun parser: 9/9 error sites emit ParseDiagnostic ✅
  - datalit parser: 21/21 error sites emit ParseDiagnostic ✅
  - Error codes assigned: P001-P009 (datafun), D001-D020 (datalit)
  - All tests passing ✅
- ✅ Phase 4 Part A Complete: Datalit type checker diagnostics
  - Parser infrastructure complete ✅
  - Type checker diagnostic emission: 45/45 error sites updated ✅
  - Error codes assigned: T001-T046 (datalit type errors)
  - All 800+ tests passing ✅
- 🔄 Phase 4 Part B In Progress: Datafun type checker diagnostics
  - Parser span infrastructure complete ✅
  - TypeContext updates complete ✅
  - Error helper abstraction created ✅ (tycheck.rs:148-226)
  - type_check_for_diagnostics() wrapper created ✅ (tycheck.rs:308-314)
  - Error helpers implemented: F001, F002, F011, F016, F045, F046 ✅
  - Type checker diagnostic emission: 6/~47 error sites updated ✅
  - Error code design: F001-F053 designed ✅
  - CLI integration: run_without_sys working perfectly ✅
  - All 136 tests passing ✅
  - Remaining: ~41 error sites to convert to use helpers
  - Remaining: run_with_sys diagnostic integration
- 🔲 Phase 4 Part C: Documentation (not started)
- 🔲 Phase 5: Resolution diagnostics (not yet started)
- 🔄 Phase 6 In Progress: Driver integration (basic)
  - Basic diagnostic retrieval and rendering in CLI ✅
  - Error testing infrastructure in place ✅
  - Full SourceMap implementation pending
- 🔲 Phase 7: Not yet started
- 🔄 Phase 8 In Progress: Improve diagnostic rendering (mostly complete)
  - Byte-to-line:col utility ✅
  - DiagnosticContext and SourceInfo tracking ✅
  - Custom diagnostic renderer with source snippets and carets ✅
  - 21 parse error test fixtures updated ✅
  - Type diagnostic retrieval in run_without_sys ✅
  - Type diagnostic retrieval in run_with_sys 🔲
  - Type error test fixtures 🔲

**Next Steps:**
1. Phase 4 Part B: Complete datafun diagnostic emission (~41 remaining error sites)
2. Phase 4 Part B: Add run_with_sys diagnostic integration
3. Phase 8: Add type error test fixtures
4. Phase 5: Resolution diagnostics
5. Phase 6: Complete driver integration (full SourceMap for multi-file support)

## Overview

Design a comprehensive diagnostic system inspired by Rust's compiler, using Salsa accumulators for automatic error collection while maintaining error recovery throughout the compilation pipeline.

## Key Design Principles

1. **Non-halting errors** - Errors don't stop compilation, they accumulate
2. **Error nodes in AST** - Parser creates error nodes, later passes handle them gracefully
3. **Rich diagnostics** - Spans, labels, notes, suggestions, error codes
4. **Severity levels** - Error, Warning, Note, Help
5. **Salsa accumulators** - Automatic collection and incremental invalidation
6. **Memoization-pure** - No file paths or line numbers in Salsa queries
7. **Multi-source support** - Single diagnostic can reference multiple source files

## Architecture

### Core Abstraction: Text + ByteSpan

**Inside Salsa (pure, memoizable):**
- Diagnostics reference `Text` (salsa-tracked)
- Never reference `Source` or file paths
- Parser/compiler works only with `Text`/`SubText`

**Outside Salsa (driver/REPL):**
- Maintains `HashMap<Text, SourceMetadata>`
- `SourceMetadata` includes:
  - `path: Option<PathBuf>` - file path (if from file)
  - `display_name: String` - for display (e.g., "<repl-5>" or "main.df")
- Renders diagnostics with file paths and positions

### New Crate: `datalove-diagnostic`

Location: `crates/datalove-diagnostic/`

Dependencies:
- `bct` (for `Text`, `InternedText`)
- `salsa` (for accumulators)

Used by:
- `datalove-datafun` (parser, type checker, etc.)
- `datalove-datalit` (parser, type checker)
- Future language implementations

## Core Types

```rust
// crates/datalove-diagnostic/src/lib.rs

use bct::text::{Text, InternedText};
use std::ops::Range;

pub type ByteSpan = Range<usize>;

/// Diagnostic severity.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[derive(salsa::Update)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

/// A labeled span within a diagnostic.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct DiagnosticLabel<'db> {
    /// Which text this span is in (Text is salsa-tracked, self-identifying).
    pub text: Text<'db>,
    /// Byte offset range within that text.
    pub span: ByteSpan,
    /// Optional label message.
    pub message: Option<InternedText<'db>>,
    /// Primary (main error) or Secondary (related location).
    pub style: LabelStyle,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub enum LabelStyle {
    Primary,
    Secondary,
}

/// Code suggestion for fixes.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Suggestion<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
    pub replacement: Option<InternedText<'db>>,
}

/// The core diagnostic type.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Diagnostic<'db> {
    pub severity: Severity,
    pub code: Option<InternedText<'db>>,  // e.g., "E0425", "P001"
    pub message: InternedText<'db>,
    pub labels: Vec<DiagnosticLabel<'db>>,
    pub notes: Vec<InternedText<'db>>,
    pub suggestions: Vec<Suggestion<'db>>,
}

/// Separate accumulators by compilation phase.
#[salsa::accumulator]
pub struct ParseDiagnostic<'db>(Diagnostic<'db>);

#[salsa::accumulator]
pub struct TypeDiagnostic<'db>(Diagnostic<'db>);

#[salsa::accumulator]
pub struct ResolutionDiagnostic<'db>(Diagnostic<'db>);

#[salsa::accumulator]
pub struct LintDiagnostic<'db>(Diagnostic<'db>);
```

### Builder API

```rust
pub struct DiagnosticBuilder<'db> {
    db: &'db dyn salsa::Database,
    diagnostic: Diagnostic<'db>,
}

impl<'db> DiagnosticBuilder<'db> {
    pub fn error(db: &'db dyn salsa::Database, message: &str) -> Self;
    pub fn warning(db: &'db dyn salsa::Database, message: &str) -> Self;
    pub fn code(mut self, code: &str) -> Self;
    pub fn primary_label(mut self, text: Text<'db>, span: ByteSpan, msg: &str) -> Self;
    pub fn secondary_label(mut self, text: Text<'db>, span: ByteSpan, msg: &str) -> Self;
    pub fn note(mut self, note: &str) -> Self;
    pub fn suggestion(mut self, text: Text<'db>, span: ByteSpan, msg: &str, replacement: Option<&str>) -> Self;

    pub fn emit_parse(self);
    pub fn emit_type(self);
    pub fn emit_resolution(self);
    pub fn emit_lint(self);
}
```

## Enhanced AST Error Nodes

Update existing error nodes to include location information:

```rust
// datafun AST
#[salsa::tracked]
pub struct StmtParseError<'db> {
    pub text: Text<'db>,           // Which text this error is in
    pub span: ByteSpan,             // Byte range of error
    pub message: InternedText<'db>,
}

#[salsa::tracked]
pub struct ExprFunParseError<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
}

// Similar updates for datalit error nodes
```

## Usage Examples

### Parser

```rust
fn parse_statement(&mut self, tokens: &[TreeToken<'db>]) -> Statement<'db> {
    match self.token_word(tokens.first()) {
        Some("let") => self.parse_let(tokens),
        Some("fun") => self.parse_fun(tokens),
        _ => {
            // Emit diagnostic
            if let Some(TreeToken::Token(tok)) = tokens.first() {
                let subtext = tok.text(self.db);
                let text = subtext.text(self.db);
                let span = subtext.range(self.db);

                DiagnosticBuilder::error(self.db, "unexpected statement")
                    .code("P001")
                    .primary_label(text, span, "expected 'let', 'fun', 'ret', etc.")
                    .emit_parse();
            }

            // Still create error node for recovery
            ast::Statement::ParseError(ast::StmtParseError::new(...))
        }
    }
}
```

### Type Checker

```rust
fn check_variable_reference(&mut self, name: InternedText<'db>, text: Text<'db>, span: ByteSpan) {
    if self.lookup_variable(name).is_none() {
        DiagnosticBuilder::error(self.db, format!("cannot find value `{}`", name.as_str(self.db)))
            .code("E0425")
            .primary_label(text, span, "not found in this scope")
            .note("consider importing this function or defining it")
            .emit_type();
    }
}
```

### Multi-Source Diagnostic (Import Error)

```rust
DiagnosticBuilder::error(db, "cannot find function in imported module")
    .code("E0425")
    .primary_label(import_text, import_span, "not found in module")
    .secondary_label(module_text, module_span, "module defined here")
    .note("module does not export this function")
    .emit_type();
```

## Driver Integration (Outside Salsa)

### SourceMetadata

```rust
#[derive(Clone, Debug)]
pub struct SourceMetadata {
    pub path: Option<PathBuf>,
    pub display_name: String,
    pub base_offset: usize,  // Byte offset of this Text in original source
}

pub struct SourceMap<'db> {
    text_metadata: HashMap<Text<'db>, SourceMetadata>,
}

impl<'db> SourceMap<'db> {
    pub fn register_file(&mut self, db: &'db dyn Db, path: PathBuf, content: String) -> Text<'db>;
    pub fn register_repl(&mut self, db: &'db dyn Db, line_num: usize, content: String) -> Text<'db>;
    pub fn register_chunk(&mut self, db: &'db dyn Db, path: PathBuf, content: String, offset_in_original: usize) -> Text<'db>;
}
```

### Rendering

```rust
pub fn render_diagnostic(
    db: &dyn Db,
    source_map: &SourceMap,
    original_sources: &HashMap<PathBuf, String>,
    diagnostic: &Diagnostic,
) {
    for label in &diagnostic.labels {
        let metadata = source_map.text_metadata.get(&label.text).unwrap();

        // Calculate absolute position in original source
        let absolute_byte_offset = metadata.base_offset + label.span.start;

        // Get original source text for line:col conversion
        let source_text = if let Some(path) = &metadata.path {
            original_sources.get(path).unwrap()
        } else {
            label.text.as_str(db)
        };

        let (line, col) = byte_to_line_col(source_text, absolute_byte_offset);

        // Render (can use annotate-snippets crate or custom renderer)
        println!("{}:{}:{}: {}: {}",
            metadata.display_name,
            line,
            col,
            severity_str(diagnostic.severity),
            diagnostic.message.as_str(db)
        );
    }
}

fn byte_to_line_col(text: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in text.char_indices() {
        if i >= byte_offset { break; }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
```

### Retrieving Diagnostics

```rust
// In REPL/CLI driver after compilation:
let parse_diags = parse::accumulated::<ParseDiagnostic>(db, source);
let type_diags = typecheck_script::accumulated::<TypeDiagnostic>(db, script);
let resolution_diags = resolve_functions::accumulated::<ResolutionDiagnostic>(db, script);
let lints = check_lints::accumulated::<LintDiagnostic>(db, script);

let all_diagnostics: Vec<Diagnostic> = parse_diags
    .into_iter()
    .chain(type_diags)
    .chain(resolution_diags)
    .chain(lints)
    .collect();

for diag in all_diagnostics {
    render_diagnostic(db, &source_map, &original_sources, &diag);
}
```

## Error Recovery Pattern

**Current pattern (keep this!):**

1. Parser encounters error
2. Emit diagnostic via accumulator
3. Create error AST node (`Statement::ParseError`, `ExprFunKind::ParseError`)
4. Continue parsing

**Later passes handle error nodes:**

```rust
// Type checker
match statement {
    Statement::ParseError(_) => {
        // Skip type checking, diagnostic already emitted by parser
    }
    Statement::Let(stmt) => {
        // Normal type checking
    }
}

// Interpreter
match expr.kind {
    ExprFunKind::ParseError(err) => {
        // Convert to runtime error
        Err(InterpError::RuntimeError(format!("Parse error: {}", err.message)))
    }
}
```

## Migration Plan

### Phase 1: Create diagnostic crate ✅ COMPLETE
- [x] Create `crates/datalove-diagnostic/`
- [x] Add to workspace Cargo.toml (auto-included via `members = ["crates/*"]`)
- [x] Add dependency on `bct` (for Text, InternedText)
- [x] Define core types: Diagnostic, DiagnosticLabel, Suggestion, Severity, LabelStyle
- [x] Define accumulators: ParseDiagnostic, TypeDiagnostic, ResolutionDiagnostic, LintDiagnostic
- [x] Implement DiagnosticBuilder with builder methods
- [ ] Write basic tests (deferred - will test via integration)

**Implementation Notes:**
- Salsa accumulators cannot store types with lifetime parameters directly
- Solution: Created `StoredDiagnostic` that uses `salsa::Id` instead of typed references
- Conversion methods: `Diagnostic::to_stored()` and `StoredDiagnostic::to_diagnostic()`
- This keeps the public API clean while working within Salsa's constraints
- The crate compiles successfully with only expected dead_code warnings

### Phase 2: Update AST error nodes ✅ COMPLETE
- [x] Add `text: Text<'db>` and `span: ByteSpan` to datafun::StmtParseError
- [x] Add `text: Text<'db>` and `span: ByteSpan` to datafun::ExprFunParseError
- [x] Update datafun parser to populate these fields (9 error sites updated)
- [x] Add similar fields to datalit::TypeHintParseError
- [x] Add similar fields to datalit::ExprParseError
- [x] Update datalit parser to populate these fields (21 error sites completed)
  - 9 TypeHintParseError sites updated
  - 12 ExprParseError sites updated
  - All sub_parser initializations updated to pass source_text
  - Fixed parse_bracer to extract text from tokens instead of private chunk field
- [x] All tests passing

**Implementation Notes:**
- Added `ByteSpan` type alias import to both AST files
- datafun parser: Added `bracer` field to Parser struct and helper methods:
  - `source_text()` - gets Text from bracer
  - `extract_text_span()` - extracts from TreeToken
  - `peek_text_span()` - peeks at next token
- datalit parser: Added `source_text` field to DynParser and helper methods:
  - `get_error_text()` - gets Text for errors
  - `current_text_span()` - extracts from current position
- All error nodes now have the signature: `::new(db, text, span, message)`
- ast_serde: Keeping serde versions simple with just `message` field (text/span are for diagnostic emission, not serialization)
- Source text extraction: Uses first token's SubText to get parent Text (ChunkLex.chunk is private)

### Phase 3: Parser diagnostics ✅ COMPLETE
- [x] Update datafun parser to emit ParseDiagnostic accumulators
- [x] Extract Text + ByteSpan from tokens (SubText)
- [x] Keep error node creation (for error recovery)
- [x] Update datalit parser similarly
- [x] Test that both error nodes and diagnostics are created

**Implementation Notes:**
- Added diagnostic imports to both parsers
- At each error site: emit diagnostic, then create error node
- Used `span.clone()` when passing to DiagnosticBuilder to avoid move errors
- Error codes: P001-P009 for datafun parser, D001-D020 for datalit parser
- All existing tests pass without modifications

### Phase 4: Type checker diagnostics - 🔄 IN PROGRESS
- [x] **Part A: Datalit implementation** - ✅ COMPLETE
  - [x] Add span infrastructure (ParseResult struct)
  - [x] Update datalit parser to collect expr_spans during parsing
  - [x] Update parser return types to ParseResult
  - [x] Update all call sites to use ParseResult.expr (14+ files)
    - [x] Test files: parser_tests, tycheck_tests, pretty_tests, resolve_tests, roundtrip_tests
    - [x] Internal: tydesc_table.rs, instantiate2.rs, resolve.rs
    - [x] External: rt-tests (7 files), cli/main.rs, datafun/parser.rs
    - [x] Parser inline tests (20+ test functions)
  - [x] Fix get_error_text() panic (empty source_text handling)
  - [x] Add ParseResult derive(PartialEq, Eq) for Salsa
  - [x] Create parse_for_test() tracked wrapper
  - [x] Create parse_integration_test() public wrapper
  - [x] Fix all library test failures
    - [x] Parser inline tests (all passing)
    - [x] rt-tests compile helpers
    - [x] tydesc_table.rs compile() helper updated
    - [x] resolve.rs tests updated to use parse_for_test()
    - [x] instantiate2.rs compile_str updated
    - [x] All 85 library tests passing
  - [x] Fix all integration test failures
    - [x] Updated 5 datalit integration tests to use parse_integration_test()
    - [x] Updated 7 rt-tests with #[salsa::tracked] compile functions
    - [x] All integration tests passing
  - [x] Update datalit type checker to emit TypeDiagnostic (45 error sites updated)
  - [x] Assign error codes T001-T046 for datalit type errors
  - [x] Test: `cargo test --all` (800+ tests passing, 0 failures)
- [x] **Part B: Datafun implementation - INFRASTRUCTURE COMPLETE**
  - [x] Add span infrastructure (ParseResult struct with script + expr_spans + datalit_expr_spans)
  - [x] Update datafun parser to collect expr_spans during parsing
  - [x] Update parser to collect datalit expr_spans (critical fix at line 991)
  - [x] Update TypeContext to accept and store spans (HashMap for fast lookup)
  - [x] Update type_check signatures to accept span parameters
  - [x] Update all datafun call sites (CLI, REPL, interp, import_demands, script_world, tests)
  - [x] Pass datalit expr_spans to datalit::resolve::resolve_names() (critical fix at line 715)
  - [x] All production code compiles and builds successfully
  - [x] Production tests: CLI builds and runs, all crates compile
  - [ ] Update datafun type checker to emit TypeDiagnostic (~35 error sites)
  - [ ] Assign error codes F001-F053 for datafun type errors
  - Note: 35 library tests fail with Salsa context issues (calling parse() from non-tracked contexts), but this is purely test infrastructure - production code works correctly
- [ ] **Part C: Documentation**
  - [x] Update plan-diagnostics.md with current status
  - [ ] Document span side table pattern (after implementation complete)
  - [ ] Document error code catalog

**Design (Revised based on implementation):**
- ~~Use BTreeMap - doesn't work (ExprFull doesn't implement Ord)~~
- ~~Use HashMap - doesn't work (HashMap doesn't implement Hash for Salsa)~~
- ✅ Use Vec<(ExprFull, Text, ByteSpan)> for expr_spans
- ✅ ParseResult is a regular struct, not Salsa tracked (lifetime constraints)
- ✅ Parse functions are no longer #[salsa::tracked] (return non-Salsa types)
- ✅ ExprFull is used directly as key (no newtype needed)
- ✅ Track spans for all ExprFull nodes created during parsing
- Keep Vec<TypeError> alongside accumulators during migration (backward compatibility)
- Implementation order: datalit first (datafun depends on it)

**Current Status (2025-10-24 - Latest):**

**Phase 4 Part A (Datalit) - COMPLETE:**
- ✅ Parser infrastructure changes complete
- ✅ All call sites updated to use ParseResult.expr
- ✅ Fixed get_error_text() panic when source_text not available
- ✅ Type checker diagnostic emission complete (45 error sites)
- ✅ Error codes T001-T046 assigned
- ✅ **Total: 800+ tests passing, 0 failures**

**Phase 4 Part B (Datafun) - IN PROGRESS (2025-11-04):**

**Infrastructure (Complete):**
- ✅ ParseResult struct added to datafun ast.rs
  - Contains: script + expr_spans + datalit_expr_spans
  - Regular struct (not Salsa-tracked) due to Vec lifetime constraints
- ✅ Parser span collection implemented
  - Collects datafun expr_spans at parse_expr_full() calls
  - **Critical fix**: Saves datalit expr_spans at line 991 (was previously discarded)
  - Stores both datafun and nested datalit expression spans
- ✅ TypeContext updates
  - Accepts expr_spans and datalit_expr_spans in constructor
  - Converts Vec to HashMap<salsa::Id, (Text, ByteSpan)> for fast lookup
  - Added get_span() method for diagnostic emission
  - **Critical fix**: Passes datalit_expr_spans to datalit::resolve::resolve_names() at line 715
- ✅ Type check signature updates
  - type_check() accepts expr_spans and datalit_expr_spans parameters
  - type_check_with_package_world() updated similarly
  - type_check_for_diagnostics() wrapper created (tycheck.rs:308-314)
- ✅ Call site updates (all production code)
  - CLI: uses type_check_for_diagnostics() for proper accumulation
  - Test helpers: fixed to use parse_for_diagnostics() in tracked context
  - All internal callers updated

**Error Helper Abstraction (Complete):**
- ✅ Error helper methods on TypeContext (tycheck.rs:148-226)
  - Pattern: Methods both emit TypeDiagnostic AND return TypeError
  - Allows error recovery while providing rich diagnostics
  - Takes expr for span lookup, formats nice error messages
- ✅ Implemented helpers:
  - error_undefined_variable() - F001
  - error_undefined_function() - F002
  - error_cannot_synthesize() - F011
  - error_type_mismatch() - F016
  - error_arity_mismatch() - F045
  - error_result_requires_binding() - F046

**Diagnostic Emission (Partial - 6/~47 sites):**
- ✅ F001: Undefined variable (ExprFunKind::Name)
- ✅ F002: Undefined function (synthesize_function_call)
- ✅ F011: Cannot synthesize return type (Statement::Ret)
- ✅ F016: Type mismatch (if/match condition)
- ✅ F045: Function arity mismatch (synthesize_function_call)
- ✅ F046: Result requires error binding (if let destructuring)
- 🔲 Remaining: ~41 error sites (InvalidOperandType: 18, others: 23)

**Error Code Design (Complete):**
- ✅ F001-F010: Name resolution errors
- ✅ F011-F015: Type synthesis errors
- ✅ F016-F025: Type mismatch errors
- ✅ F026-F045: Operator type errors
- ✅ F046-F050: Error handling errors
- ✅ F051-F053: Integration errors

**CLI Integration (run_without_sys Complete):**
- ✅ type_check_for_diagnostics() wrapper working perfectly
- ✅ Diagnostic accumulation via Salsa accumulators
- ✅ Rich error rendering with file:line:col, source snippets, carets
- ✅ Example output working:
  ```
  error[F001]: cannot find value `undefined_var` in this scope
   --> test.dfs:1:14
    |
  1 | let output = undefined_var
    |              ^^^^^^^^^^^^^ not found in this scope
  ```
- 🔲 run_with_sys diagnostic integration pending

**Test Status:**
- ✅ All 136 datafun tests passing
- ✅ CLI tests passing
- ✅ Production code fully functional

**Next Steps:**
- Convert remaining ~41 error sites to use helper methods
- Add remaining helper methods (InvalidOperandType, etc.)
- Update run_with_sys to use type_check_for_diagnostics
- Create type error test fixtures

**Key Implementation Details:**
- ParseResult pattern: Non-Salsa struct with script + 2 span vectors
- Span collection: Record at parse_expr_full() and datalit parse calls
- Critical line 991: Save datalit parse_result.expr_spans (was discarded!)
- Critical line 715: Pass ctx.datalit_expr_spans to datalit::resolve::resolve_names()
- TypeContext.new() takes expr_spans and datalit_expr_spans, converts to HashMap
- All type_check signatures updated to accept span parameters
- parse() must be called from Salsa tracked context (creates tracked Script internally)

### Phase 5: Resolution diagnostics
- [ ] Update resolution pass to emit ResolutionDiagnostic
- [ ] Remove ad-hoc error collection
- [ ] Keep error return values for control flow

### Phase 6: Driver integration
- [x] Retrieve diagnostics via ::accumulated after compilation (basic implementation)
- [x] Implement basic diagnostic renderer for testing
- [ ] Create SourceMap in REPL
- [ ] Create SourceMap in CLI
- [ ] Register Text → SourceMetadata when creating Sources
- [ ] Track base_offset for chunks (if/when we split sources)
- [ ] Improve diagnostic renderer (convert bytes → line:col, show file paths)
- [ ] Consider using annotate-snippets crate for pretty rendering

**Error Testing Infrastructure (2025-10-23):**
- ✅ Added ParseDiagnostic retrieval to CLI script command
- ✅ Implemented basic text-based diagnostic renderer
- ✅ Created error_tests.rs test suite (separate from script_tests)
- ✅ 8 error test fixtures covering parse error codes:
  - D001: Missing parentheses after tuple keyword
  - D002: Missing braces after struct keyword
  - D004: Missing braces after enum name
  - D005: Missing braces after map keyword
  - D006: Missing braces after set keyword
  - D007: Missing brackets after list keyword
  - D013: Unexpected minus sign
  - D019: Unexpected identifier
- ✅ All error tests passing (13 total CLI tests: 5 script + 8 error)
- Note: Error output format is temporary - will be improved with proper SourceMap in Phase 6

### Phase 7: Additional features
- [ ] Add LintDiagnostic support (warnings about code style, etc.)
- [ ] Add suggestion/fix support
- [ ] Error codes catalog (document all error codes)
- [ ] Diagnostic filtering by severity
- [ ] JSON output for IDE integration

## Redundant Error Reporting Analysis (2025-11-03)

### Current Problem

Parse error sites currently do redundant work, calling two separate APIs with nearly identical information:

```rust
// Example from datalit parser.rs:921-923
let message = self.db.intern_text("expected '(' after tuple name");

DiagnosticBuilder::error(self.db, message.as_str(self.db))
    .code("D001")
    .primary_label(keyword_text, keyword_span.clone(), "expected '(' after tuple name")
    .emit_parse();

return ast::Expr::ParseError(ast::ExprParseError::new(self.db, keyword_text, keyword_span, message));
```

**Redundancy:**
1. Message text appears twice (in DiagnosticBuilder and ExprParseError)
2. Text and span appear twice (in primary_label and ExprParseError)
3. Two separate function calls with duplicate information
4. Pattern repeated at 21 datalit sites + 9 datafun sites

**Why Both Exist:**
- **ParseError nodes** (ExprParseError, TypeHintParseError, StmtParseError): Required for error recovery. AST needs error nodes so type checker and other passes can skip invalid nodes gracefully.
- **DiagnosticBuilder/accumulators**: Required for rich error reporting. Collects all diagnostics for display to user with spans, labels, notes, suggestions.

### Solution Options

#### Option 1: Helper Functions in Parser Modules (Recommended)

Create convenience functions in each parser that emit diagnostic and create error node in one call:

```rust
// In datalove-datalit/src/parser.rs
impl<'db> DynParser<'db> {
    fn emit_expr_error(
        &self,
        text: Text<'db>,
        span: ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Expr<'db> {
        let message = self.db.intern_text(message);
        DiagnosticBuilder::error(self.db, message.as_str(self.db))
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message))
    }

    fn emit_type_hint_error(
        &self,
        text: Text<'db>,
        span: ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::TypeHint<'db> {
        let message = self.db.intern_text(message);
        DiagnosticBuilder::error(self.db, message.as_str(self.db))
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message))
    }
}

// In datalove-datafun/src/parser.rs
impl<'db> Parser<'db> {
    fn emit_stmt_error(
        &self,
        text: Text<'db>,
        span: ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Statement<'db> {
        let message = self.db.intern_text(message);
        DiagnosticBuilder::error(self.db, message.as_str(self.db))
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::Statement::ParseError(ast::StmtParseError::new(self.db, text, span, message))
    }

    fn emit_expr_error(
        &self,
        text: Text<'db>,
        span: ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprFun<'db> {
        let message = self.db.intern_text(message);
        DiagnosticBuilder::error(self.db, message.as_str(self.db))
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::ExprFun::ParseError(ast::ExprFunParseError::new(self.db, text, span, message))
    }
}
```

**Usage becomes:**
```rust
// Before (datalit parser.rs:918-923)
let message = self.db.intern_text("expected '(' after tuple name");
DiagnosticBuilder::error(self.db, message.as_str(self.db))
    .code("D001")
    .primary_label(keyword_text, keyword_span.clone(), "expected '(' after tuple name")
    .emit_parse();
return ast::Expr::ParseError(ast::ExprParseError::new(self.db, keyword_text, keyword_span, message));

// After
return self.emit_expr_error(
    keyword_text,
    keyword_span,
    "expected '(' after tuple name",
    "D001",
    "expected '(' after 'tuple'"
);
```

**Pros:**
- Single call site - much cleaner
- No coupling between diagnostic and AST crates
- Type-safe (each helper returns correct type)
- Easy to extend (add note/suggestion parameters)
- Minimal changes to existing code

**Cons:**
- Still creates two separate data structures internally
- Need separate helper for each return type

#### Option 2: Builder Methods That Create Error Nodes

Add methods to DiagnosticBuilder:
```rust
impl<'db> DiagnosticBuilder<'db> {
    pub fn emit_parse_as_expr_error(self, text: Text<'db>, span: ByteSpan) -> ExprParseError<'db> {
        self.emit_parse();
        ExprParseError::new(self.db, text, span, self.diagnostic.message)
    }
}
```

**Pros:**
- Builder already has all the information

**Cons:**
- ❌ Diagnostic crate would depend on AST types (tight coupling)
- ❌ Would need separate method for each error node type
- ❌ Wrong responsibility (diagnostics shouldn't know about AST)

#### Option 3: Keep Both Separate (Current Approach)

Continue with explicit calls at each error site.

**Pros:**
- Clear separation of concerns
- No new abstractions

**Cons:**
- ❌ Verbose and repetitive (5-6 lines per error)
- ❌ Easy to forget one or the other
- ❌ Message text duplicated

#### Option 4: Error Nodes That Emit Diagnostics

Add method to error nodes:
```rust
impl<'db> ExprParseError<'db> {
    pub fn emit_diagnostic(self, db: &'db dyn Db, code: &str, label: &str) {
        DiagnosticBuilder::error(db, self.message.as_str(db))
            .code(code)
            .primary_label(self.text, self.span.clone(), label)
            .emit_parse();
    }
}
```

**Pros:**
- Error node is single source of truth

**Cons:**
- ❌ Still two call sites (create node, then emit)
- ❌ AST types would depend on diagnostic crate (acceptable but less clean)
- ❌ Awkward API (why create node if you're going to emit separately?)

### Recommendation: Option 1 (Helper Functions)

Implement parser helper methods that do both in one call. This is the cleanest approach:

1. No new dependencies or coupling between crates
2. Single, clear call site for each error
3. Type-safe with appropriate return types
4. Easy to extend (add notes/suggestions later)
5. Parser-specific logic stays in parser modules

**Implementation Plan:**
1. Add helper methods to DynParser (datalit)
2. Add helper methods to Parser (datafun)
3. Update all error sites to use helpers (21 datalit + 9 datafun)
4. Verify all tests still pass

**Future Enhancement:**
Add optional parameters for notes and suggestions:
```rust
fn emit_expr_error(
    &self,
    text: Text<'db>,
    span: ByteSpan,
    message: &str,
    code: &str,
    label: &str,
    notes: &[&str],  // Optional notes
    suggestions: &[(&str, &str)],  // Optional (label, replacement) pairs
) -> ast::Expr<'db>
```

### Next Steps

1. Implement helper functions in both parsers
2. Update datalit error sites (21 sites)
3. Update datafun error sites (9 sites)
4. Run test suite to verify
5. Document pattern for future error sites

## Phase 8: Improve Diagnostic Rendering (NEW - 2025-11-03)

### Current Problems

**Parse Error Output Quality:**
```
Parse errors:
error[D001]: expected () after tuple keyword
  --> expected '(' after 'tuple'
     |
Error: 1 parse error(s)
```

**Issues:**
- ❌ No file path
- ❌ No line:column numbers (just shows byte span content)
- ❌ No source code context
- ❌ No caret (^) pointing to exact location
- ❌ No surrounding lines

**Type Error Output:**
```
Error: Script has 1 typecheck error(s):
DatalitError("TypeMismatch { expected: \"u32\", actual: \"string\" }")
```

**Issues:**
- ❌ Just raw Debug output of TypeError enum
- ❌ Not using TypeDiagnostic accumulators
- ❌ No source location information at all

### Root Causes

1. **render_diagnostics() is minimal** (crates/datalove-cli/src/main.rs:312-338)
   - Just prints error code and message
   - Doesn't convert byte spans to line:column
   - Doesn't show file path
   - Doesn't display source snippets

2. **Type diagnostics aren't retrieved** (main.rs:469-474)
   - TypeDiagnostic accumulators exist but aren't used
   - CLI uses old `tycheck_result.errors()` with Debug formatting
   - Need to call `accumulated::<TypeDiagnostic>()`

3. **No SourceMap implementation**
   - Need to track Text → file path + line:column mapping
   - Required for proper diagnostic rendering

### Target Output Quality

**Good parse error:**
```
error[D001]: expected () after tuple keyword
 --> 01_tuple_missing_parens.dfs:1:18
  |
1 | let output: tuple Foo = @5
  |                  ^^^ expected '(' after 'tuple'
```

**Good type error:**
```
error[T022]: mismatched types
 --> test.dfs:1:17
  |
1 | let x: @u32 = @"hello"
  |        ----   ^^^^^^^ expected u32, found string
  |        |
  |        expected due to this type annotation
```

### Implementation Plan

**Part 1: Byte → Line:Col Conversion Utility**
- Add to `crates/datalove-diagnostic/src/lib.rs`
- `byte_to_line_col(text: &str, byte_offset: usize) -> (usize, usize)`
- Helper to convert byte offsets to 1-based line:column numbers

**Part 2: Basic SourceInfo for Single Files**
- Add to `crates/datalove-cli/src/main.rs`
- `struct SourceInfo` with file path and original source text
- Store alongside Source/Text for diagnostic rendering
- Simple version: one file at a time (not multi-file yet)

**Part 3: Improved Diagnostic Renderer**
- Replace `render_diagnostics()` with proper formatting
- Show file path and line:col (e.g., `file.dfs:1:18`)
- Display source line with error
- Add caret (^) pointing to error location
- Show labels and notes
- **Approach**: Custom simple renderer (can use annotate-snippets later)

**Part 4: Retrieve Type Diagnostics**
- Update `ScriptCommand::run_without_sys()` and `run_with_sys()`
- Change from `tycheck_result.errors()` to `accumulated::<TypeDiagnostic>()`
- Add `render_type_diagnostics()` similar to parse diagnostics
- Stop using Debug formatting

**Part 5: Fix lit-tycheck Salsa Context**
- Currently panics: "cannot accumulate values outside of an active tracked function"
- Create `parse_for_tycheck()` tracked wrapper in datalit
- Or: accept that lit-tycheck doesn't emit accumulators (low priority)

**Part 6: Update Error Test Expectations**
- Update `.out.expected` files in `crates/datalove-cli/tests/fixtures/error/`
- Add file paths, line:col, source snippets with carets

**Part 7: Create Type Error Tests**
- Add test fixtures for type errors (type mismatch, unresolved name, etc.)
- Test TypeDiagnostic emission and rendering end-to-end

### Implementation Order

1. Part 1: Byte → line:col utility (foundation)
2. Part 2: Basic SourceInfo tracking (needed for file paths)
3. Part 3: Custom diagnostic renderer (improve parse error output)
4. Part 6: Update parse error test expectations (validate Part 3)
5. Part 4: Retrieve type diagnostics (show we can get them)
6. Part 7: Add type error tests (validate end-to-end)
7. Part 5: Fix lit-tycheck (optional, low priority)

### Current Status (2025-11-03)

**Implemented:**
- ✅ **Part 1 COMPLETE**: `byte_to_line_col()` utility in `datalove-diagnostic/src/lib.rs:358-375`
- ✅ **Part 2 COMPLETE**: `DiagnosticContext` and `SourceInfo` structs in `datalove-cli/src/main.rs:15-67`
  - Constructors: `from_file()`, `from_test()`, `from_repl()`
  - Tracks file path and display name for rendering
- ✅ **Part 3 COMPLETE**: `render_single_diagnostic()` in `datalove-cli/src/main.rs:394-467`
  - Shows file:line:col (e.g., `01_tuple_missing_parens.dfs:1:13`)
  - Displays source line with error
  - Adds caret (^^^) pointing to error location
  - Shows labels and notes
  - Handles multi-line spans
- ✅ **Part 6 COMPLETE**: 21 error test fixtures updated with proper output format
  - Example: `error[D001]: expected () after tuple keyword` with file location and source snippet

**Partially Implemented:**
- 🟡 **Part 4 PARTIAL**: Type diagnostic retrieval
  - ✅ `run_without_sys()` retrieves and renders TypeDiagnostic (main.rs:493-500)
  - ❌ `run_with_sys()` still uses old-style `tycheck_result.errors()` Debug output (main.rs:594-598)
  - ❌ Old-style fallback still exists at main.rs:504-506

**Not Started:**
- ❌ **Part 7**: No type error test fixtures
  - No `crates/datalove-cli/tests/fixtures/type_error/` directory
  - Need end-to-end tests for TypeDiagnostic rendering
- ⚠️ **Part 5 DEFERRED**: lit-tycheck Salsa context issue (low priority utility command)

### Remaining Work

1. **Complete Part 4**: Add TypeDiagnostic retrieval to `run_with_sys()`
   - Replace old-style error handling in package world typecheck
   - Remove fallback at lines 504-506
2. **Complete Part 7**: Create type error test fixtures
   - Add `crates/datalove-cli/tests/fixtures/type_error/` directory
   - Create test cases for common type errors (mismatch, undefined variable, etc.)
   - Validate TypeDiagnostic rendering end-to-end
3. **Optional Part 5**: Fix lit-tycheck (deferred - low priority)

### Success Criteria

- ✅ **Achieved**: Parse errors show file:line:col, source snippet, caret
- ✅ **Achieved**: Parse error tests updated with proper expectations (21 fixtures)
- ✅ **Achieved**: Error output quality matches Rust compiler style
- 🟡 **Partial**: Type errors use TypeDiagnostic in `run_without_sys`, not yet in `run_with_sys`
- ❌ **Pending**: Type error test fixtures not created
- 🟡 **Partial**: Ready to add datafun type diagnostics (rendering works, needs complete integration and tests)

### Out of Scope (Future)

- Multi-file diagnostics (imports, modules) - requires full SourceMap
- Secondary labels across files
- Suggestion/fix rendering
- JSON output for IDE integration
- Color support

**Rationale**: Fix diagnostic rendering before adding more datafun type diagnostics. Otherwise we're just generating more diagnostics that will be rendered poorly.

## Benefits

1. ✅ **Memoization-pure** - Parser never touches Source, only Text
2. ✅ **Future-proof** - Chunks can be refactored without breaking diagnostics
3. ✅ **Multi-source** - Diagnostics can reference multiple Texts (import errors, etc.)
4. ✅ **Salsa accumulators** - Automatic collection & incremental invalidation
5. ✅ **Reusable** - New crate usable by datafun, datalit, future languages
6. ✅ **Error recovery** - AST error nodes still exist for graceful degradation
7. ✅ **REPL-friendly** - Text can be stdin, repl input, files, chunks, etc.
8. ✅ **Rust-quality** - Rich diagnostics with spans, labels, notes, suggestions
9. ✅ **Separation of concerns** - Diagnostics separate from computation results
10. ✅ **Multiple severities** - Errors, warnings, lints all supported

## Open Questions / Future Considerations

1. **Error codes catalog** - Should we maintain a central registry of all error codes?
2. **Diagnostic rendering** - Use annotate-snippets crate or implement custom renderer?
3. **IDE integration** - What format for LSP diagnostics?
4. **Diagnostic configuration** - Allow users to configure severity levels?
5. **Macro expansion** - How do diagnostics work with future macro support?
6. **Performance** - Does accumulator overhead matter? (likely negligible)

## References

- Rust compiler diagnostics: https://doc.rust-lang.org/rustc/
- annotate-snippets crate: https://docs.rs/annotate-snippets/
- Salsa book on accumulators: https://salsa-rs.github.io/salsa/
