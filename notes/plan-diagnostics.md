# Datalove Diagnostic System Design

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
- Diagnostics reference `Text` (salsa-tracked) + `ByteSpan` (byte offsets)
- Never reference `Source` or file paths
- Parser/compiler works only with `Text`/`SubText`
- Future-proof: chunks can be refactored without breaking diagnostics

**Outside Salsa (driver/REPL):**
- Maintains `HashMap<Text, SourceMetadata>`
- `SourceMetadata` includes:
  - `path: Option<PathBuf>` - file path (if from file)
  - `display_name: String` - for display (e.g., "<repl-5>" or "main.df")
  - `base_offset: usize` - where this Text starts in original source
- Converts byte offsets → line:column using original source text
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

### Phase 1: Create diagnostic crate
- [ ] Create `crates/datalove-diagnostic/`
- [ ] Add to workspace Cargo.toml
- [ ] Add dependency on `bct` (for Text, InternedText)
- [ ] Define core types: Diagnostic, DiagnosticLabel, Suggestion, Severity, LabelStyle
- [ ] Define accumulators: ParseDiagnostic, TypeDiagnostic, ResolutionDiagnostic, LintDiagnostic
- [ ] Implement DiagnosticBuilder with builder methods
- [ ] Write basic tests

### Phase 2: Update AST error nodes
- [ ] Add `text: Text<'db>` and `span: ByteSpan` to datafun::StmtParseError
- [ ] Add `text: Text<'db>` and `span: ByteSpan` to datafun::ExprFunParseError
- [ ] Update datafun parser to populate these fields
- [ ] Add similar fields to datalit::TypeHintParseError
- [ ] Add similar fields to datalit::ExprParseError
- [ ] Update datalit parser to populate these fields

### Phase 3: Parser diagnostics
- [ ] Update datafun parser to emit ParseDiagnostic accumulators
- [ ] Extract Text + ByteSpan from tokens (SubText)
- [ ] Keep error node creation (for error recovery)
- [ ] Update datalit parser similarly
- [ ] Test that both error nodes and diagnostics are created

### Phase 4: Type checker diagnostics
- [ ] Remove `Vec<TypeError>` from TypeContext
- [ ] Emit TypeDiagnostic via accumulators instead
- [ ] Track Text + ByteSpan for expressions (may need AST updates)
- [ ] Update all error reporting sites
- [ ] Keep error handling in type checker (return Err, etc.)

### Phase 5: Resolution diagnostics
- [ ] Update resolution pass to emit ResolutionDiagnostic
- [ ] Remove ad-hoc error collection
- [ ] Keep error return values for control flow

### Phase 6: Driver integration
- [ ] Create SourceMap in REPL
- [ ] Create SourceMap in CLI
- [ ] Register Text → SourceMetadata when creating Sources
- [ ] Track base_offset for chunks (if/when we split sources)
- [ ] Retrieve diagnostics via ::accumulated after compilation
- [ ] Implement diagnostic renderer (convert bytes → line:col)
- [ ] Consider using annotate-snippets crate for pretty rendering

### Phase 7: Additional features
- [ ] Add LintDiagnostic support (warnings about code style, etc.)
- [ ] Add suggestion/fix support
- [ ] Error codes catalog (document all error codes)
- [ ] Diagnostic filtering by severity
- [ ] JSON output for IDE integration

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
