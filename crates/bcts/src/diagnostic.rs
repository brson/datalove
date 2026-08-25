//! Generic diagnostic system for compilers.
//!
//! Provides rich, Rust-quality diagnostics with:
//! - Byte-span based locations (memoization-pure, no file paths)
//! - Multiple severity levels (Error, Warning, Note, Help)
//! - Labels, notes, and suggestions
//! - Salsa-compatible storage for accumulator round-tripping

use rmx::prelude::*;
use crate::input::Source;
use crate::text::{Text, InternedText, TextSpan, ByteSpan};

/// Single span entry for efficient lookup.
///
/// Names the source rather than the `Text` derived from it. A `Text` is a
/// tracked struct and an `InternedText` is collectable, so an id held for
/// either goes stale: salsa deletes the tracked slot when the query that made
/// it runs again, and recycles an interned slot that has gone unread. Reading
/// a stale id panics, or in a build without debug assertions hands back
/// whatever now occupies the slot. A `Source` is an input, so its id is good
/// for the life of the database.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanEntry {
    pub source: Source,
    pub span: ByteSpan,
}

impl SpanEntry {
    pub fn new(source: Source, span: ByteSpan) -> Self {
        SpanEntry { source, span }
    }

    pub fn to_text_and_span<'db>(&self, db: &'db dyn crate::Db) -> (Text<'db>, ByteSpan) {
        (crate::source_map::basic_source_map(db, self.source).text(db), self.span.C())
    }
}

/// Diagnostic severity level.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[derive(salsa::SalsaValue)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
            Severity::Help => "help",
        }
    }
}

/// Style of a diagnostic label.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub enum LabelStyle {
    /// The primary location of the diagnostic.
    Primary,
    /// A related/secondary location.
    Secondary,
}

/// A labeled span within a diagnostic.
///
/// References a Text (salsa-tracked) + byte range.
/// The outer driver maps Text to source metadata (path, line numbers).
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct DiagnosticLabel<'db> {
    /// Which text this span is in.
    pub text: Text<'db>,
    /// Byte offset range within that text.
    pub span: ByteSpan,
    /// Optional label message.
    pub message: Option<InternedText<'db>>,
    /// Primary (main error) or Secondary (related location).
    pub style: LabelStyle,
}

impl<'db> DiagnosticLabel<'db> {
    pub fn new(
        text: Text<'db>,
        span: ByteSpan,
        message: Option<InternedText<'db>>,
        style: LabelStyle,
    ) -> Self {
        DiagnosticLabel {
            text,
            span,
            message,
            style,
        }
    }
}

/// A code suggestion for a fix.
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Suggestion<'db> {
    pub text: Text<'db>,
    pub span: ByteSpan,
    pub message: InternedText<'db>,
    pub replacement: Option<InternedText<'db>>,
}

impl<'db> Suggestion<'db> {
    pub fn new(
        text: Text<'db>,
        span: ByteSpan,
        message: InternedText<'db>,
        replacement: Option<InternedText<'db>>,
    ) -> Self {
        Suggestion {
            text,
            span,
            message,
            replacement,
        }
    }
}

/// The core diagnostic type.
///
/// Represents a single diagnostic message (error, warning, etc.)
/// with locations, labels, notes, and suggestions.
///
/// This is the rich, user-facing type with full salsa integration.
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Diagnostic<'db> {
    pub severity: Severity,
    pub code: Option<InternedText<'db>>,
    pub message: InternedText<'db>,
    pub labels: Vec<DiagnosticLabel<'db>>,
    pub notes: Vec<InternedText<'db>>,
    pub suggestions: Vec<Suggestion<'db>>,
}

/// Stored diagnostic for accumulator (no lifetimes).
///
/// An accumulator's payload is `Any`, so it cannot hold a salsa handle and
/// keep the database lifetime. It used to hold raw ids instead, which does not
/// work either: an interned or tracked id read after its slot is recycled
/// resolves to whatever now lives there. So the strings are stored as strings
/// and re-interned on the way out, and a label names the `Source` it points
/// into, which is an input and so cannot be recycled.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct StoredDiagnostic {
    pub severity: Severity,
    pub code: Option<String>,
    pub message: String,
    pub labels: Vec<StoredLabel>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct StoredLabel {
    pub source: Source,
    pub span: ByteSpan,
    pub message: Option<String>,
    pub style: LabelStyle,
}

impl<'db> Diagnostic<'db> {
    /// Convert to stored form for accumulation.
    pub fn to_stored(&self, db: &'db dyn crate::Db) -> StoredDiagnostic {
        StoredDiagnostic {
            severity: self.severity,
            code: self.code.map(|c| c.as_str(db).to_string()),
            message: self.message.as_str(db).to_string(),
            labels: self.labels.iter().map(|l| StoredLabel {
                source: l.text.source(db),
                span: l.span.C(),
                message: l.message.map(|m| m.as_str(db).to_string()),
                style: l.style,
            }).collect(),
            notes: self.notes.iter().map(|n| n.as_str(db).to_string()).collect(),
        }
    }
}

impl StoredDiagnostic {
    /// Convert from stored form back to rich diagnostic.
    pub fn to_diagnostic<'db>(&self, db: &'db dyn crate::Db) -> Diagnostic<'db> {
        Diagnostic {
            severity: self.severity,
            code: self.code.as_ref().map(|c| InternedText::new(db, c.C())),
            message: InternedText::new(db, self.message.C()),
            labels: self.labels.iter().map(|l| DiagnosticLabel {
                text: crate::source_map::basic_source_map(db, l.source).text(db),
                span: l.span.C(),
                message: l.message.as_ref().map(|m| InternedText::new(db, m.C())),
                style: l.style,
            }).collect(),
            notes: self.notes.iter().map(|n| InternedText::new(db, n.C())).collect(),
            suggestions: vec![],  // TODO: Handle suggestions in stored form
        }
    }
}

/// Builder for constructing diagnostics with a fluent API.
pub struct DiagnosticBuilder<'db> {
    db: &'db dyn salsa::Database,
    diagnostic: Diagnostic<'db>,
}

impl<'db> DiagnosticBuilder<'db> {
    /// Get the database reference.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }
}

impl<'db> DiagnosticBuilder<'db> {
    /// Create an error diagnostic.
    pub fn error(db: &'db dyn salsa::Database, message: &str) -> Self {
        Self {
            db,
            diagnostic: Diagnostic {
                severity: Severity::Error,
                code: None,
                message: InternedText::new(db, message.S()),
                labels: vec![],
                notes: vec![],
                suggestions: vec![],
            },
        }
    }

    /// Create a warning diagnostic.
    pub fn warning(db: &'db dyn salsa::Database, message: &str) -> Self {
        Self {
            db,
            diagnostic: Diagnostic {
                severity: Severity::Warning,
                code: None,
                message: InternedText::new(db, message.S()),
                labels: vec![],
                notes: vec![],
                suggestions: vec![],
            },
        }
    }

    /// Set the error code.
    pub fn code(mut self, code: &str) -> Self {
        self.diagnostic.code = Some(InternedText::new(self.db, code.S()));
        self
    }

    /// Add a primary label (the main error location).
    pub fn primary_label(mut self, ts: TextSpan<'db>, msg: &str) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text: ts.text,
            span: ts.span,
            message: Some(InternedText::new(self.db, msg.S())),
            style: LabelStyle::Primary,
        });
        self
    }

    /// Add a secondary label (a related location).
    pub fn secondary_label(mut self, ts: TextSpan<'db>, msg: &str) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text: ts.text,
            span: ts.span,
            message: Some(InternedText::new(self.db, msg.S())),
            style: LabelStyle::Secondary,
        });
        self
    }

    /// Add a primary label without a message.
    pub fn primary_span(mut self, ts: TextSpan<'db>) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text: ts.text,
            span: ts.span,
            message: None,
            style: LabelStyle::Primary,
        });
        self
    }

    /// Add a secondary label without a message.
    pub fn secondary_span(mut self, ts: TextSpan<'db>) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text: ts.text,
            span: ts.span,
            message: None,
            style: LabelStyle::Secondary,
        });
        self
    }

    /// Add a note.
    pub fn note(mut self, note: &str) -> Self {
        self.diagnostic.notes.push(InternedText::new(self.db, note.S()));
        self
    }

    /// Add a suggestion with optional replacement text.
    pub fn suggestion(
        mut self,
        ts: TextSpan<'db>,
        msg: &str,
        replacement: Option<&str>,
    ) -> Self {
        self.diagnostic.suggestions.push(Suggestion {
            text: ts.text,
            span: ts.span,
            message: InternedText::new(self.db, msg.S()),
            replacement: replacement.map(|r| InternedText::new(self.db, r.S())),
        });
        self
    }

    /// Build and return the diagnostic.
    pub fn build(self) -> Diagnostic<'db> {
        self.diagnostic
    }

    /// Build and return the stored form for accumulation.
    pub fn build_stored(self) -> StoredDiagnostic {
        self.diagnostic.to_stored(self.db)
    }
}
