//! Generic diagnostic system for compilers.
//!
//! Provides rich, Rust-quality diagnostics with:
//! - Byte-span based locations (memoization-pure, no file paths)
//! - Multiple severity levels (Error, Warning, Note, Help)
//! - Labels, notes, and suggestions
//! - Salsa-compatible storage for accumulator round-tripping

use rmx::prelude::*;
use crate::text::{Text, InternedText, TextSpan, ByteSpan};
use salsa::plumbing::AsId;

/// Single span entry for efficient lookup.
///
/// Stores a text ID and byte span for on-demand conversion to typed references.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanEntry {
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

impl SpanEntry {
    pub fn new(text_id: salsa::Id, span: ByteSpan) -> Self {
        SpanEntry { text_id, span }
    }

    pub fn to_text_and_span<'db>(&self, _db: &'db dyn salsa::Database) -> (Text<'db>, ByteSpan) {
        use salsa::plumbing::FromId;
        (Text::from_id(self.text_id), self.span.C())
    }
}

/// Diagnostic severity level.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[derive(salsa::Update)]
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
#[derive(salsa::Update)]
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
/// This is a simplified representation that can be stored in Salsa accumulators.
/// Uses raw IDs instead of typed salsa references.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct StoredDiagnostic {
    pub severity: Severity,
    pub code: Option<salsa::Id>,
    pub message: salsa::Id,
    pub labels: Vec<StoredLabel>,
    pub notes: Vec<salsa::Id>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct StoredLabel {
    pub text: salsa::Id,
    pub span: ByteSpan,
    pub message: Option<salsa::Id>,
    pub style: LabelStyle,
}

impl<'db> Diagnostic<'db> {
    /// Convert to stored form for accumulation.
    pub fn to_stored(&self) -> StoredDiagnostic {
        StoredDiagnostic {
            severity: self.severity,
            code: self.code.map(|c| c.as_id()),
            message: self.message.as_id(),
            labels: self.labels.iter().map(|l| StoredLabel {
                text: l.text.as_id(),
                span: l.span.C(),
                message: l.message.map(|m| m.as_id()),
                style: l.style,
            }).collect(),
            notes: self.notes.iter().map(|n| n.as_id()).collect(),
        }
    }
}

impl StoredDiagnostic {
    /// Convert from stored form back to rich diagnostic.
    pub fn to_diagnostic<'db>(&self, _db: &'db dyn salsa::Database) -> Diagnostic<'db> {
        use salsa::plumbing::FromId;

        Diagnostic {
            severity: self.severity,
            code: self.code.map(|id| InternedText::from_id(id)),
            message: InternedText::from_id(self.message),
            labels: self.labels.iter().map(|l| DiagnosticLabel {
                text: Text::from_id(l.text),
                span: l.span.C(),
                message: l.message.map(|id| InternedText::from_id(id)),
                style: l.style,
            }).collect(),
            notes: self.notes.iter().map(|id| InternedText::from_id(*id)).collect(),
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
        self.diagnostic.to_stored()
    }
}
