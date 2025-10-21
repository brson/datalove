//! Diagnostic system for Datalove compiler.
//!
//! Provides rich, Rust-quality diagnostics with:
//! - Byte-span based locations (memoization-pure, no file paths)
//! - Multiple severity levels (Error, Warning, Note, Help)
//! - Labels, notes, and suggestions
//! - Salsa accumulators for automatic collection
//! - Multi-source diagnostic support

#![allow(unused)]

use bct::text::{Text, InternedText};
use salsa::Accumulator;
use salsa::plumbing::AsId;
use std::ops::Range;

pub type ByteSpan = Range<usize>;

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
struct StoredDiagnostic {
    severity: Severity,
    code: Option<salsa::Id>,
    message: salsa::Id,
    labels: Vec<StoredLabel>,
    notes: Vec<salsa::Id>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct StoredLabel {
    text: salsa::Id,
    span: ByteSpan,
    message: Option<salsa::Id>,
    style: LabelStyle,
}

/// Salsa accumulator for parse diagnostics.
#[salsa::accumulator]
pub struct ParseDiagnostic(StoredDiagnostic);

/// Salsa accumulator for type checking diagnostics.
#[salsa::accumulator]
pub struct TypeDiagnostic(StoredDiagnostic);

/// Salsa accumulator for name resolution diagnostics.
#[salsa::accumulator]
pub struct ResolutionDiagnostic(StoredDiagnostic);

/// Salsa accumulator for lint diagnostics.
#[salsa::accumulator]
pub struct LintDiagnostic(StoredDiagnostic);

impl<'db> Diagnostic<'db> {
    /// Convert to stored form for accumulation.
    fn to_stored(&self) -> StoredDiagnostic {
        StoredDiagnostic {
            severity: self.severity,
            code: self.code.map(|c| c.as_id()),
            message: self.message.as_id(),
            labels: self.labels.iter().map(|l| StoredLabel {
                text: l.text.as_id(),
                span: l.span.clone(),
                message: l.message.map(|m| m.as_id()),
                style: l.style,
            }).collect(),
            notes: self.notes.iter().map(|n| n.as_id()).collect(),
        }
    }
}

/// Builder for constructing diagnostics with a fluent API.
pub struct DiagnosticBuilder<'db> {
    db: &'db dyn salsa::Database,
    diagnostic: Diagnostic<'db>,
}

impl<'db> DiagnosticBuilder<'db> {
    /// Create an error diagnostic.
    pub fn error(db: &'db dyn salsa::Database, message: &str) -> Self {
        Self {
            db,
            diagnostic: Diagnostic {
                severity: Severity::Error,
                code: None,
                message: InternedText::new(db, message.to_string()),
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
                message: InternedText::new(db, message.to_string()),
                labels: vec![],
                notes: vec![],
                suggestions: vec![],
            },
        }
    }

    /// Set the error code.
    pub fn code(mut self, code: &str) -> Self {
        self.diagnostic.code = Some(InternedText::new(self.db, code.to_string()));
        self
    }

    /// Add a primary label (the main error location).
    pub fn primary_label(mut self, text: Text<'db>, span: ByteSpan, msg: &str) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text,
            span,
            message: Some(InternedText::new(self.db, msg.to_string())),
            style: LabelStyle::Primary,
        });
        self
    }

    /// Add a secondary label (a related location).
    pub fn secondary_label(mut self, text: Text<'db>, span: ByteSpan, msg: &str) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text,
            span,
            message: Some(InternedText::new(self.db, msg.to_string())),
            style: LabelStyle::Secondary,
        });
        self
    }

    /// Add a primary label without a message.
    pub fn primary_span(mut self, text: Text<'db>, span: ByteSpan) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text,
            span,
            message: None,
            style: LabelStyle::Primary,
        });
        self
    }

    /// Add a secondary label without a message.
    pub fn secondary_span(mut self, text: Text<'db>, span: ByteSpan) -> Self {
        self.diagnostic.labels.push(DiagnosticLabel {
            text,
            span,
            message: None,
            style: LabelStyle::Secondary,
        });
        self
    }

    /// Add a note.
    pub fn note(mut self, note: &str) -> Self {
        self.diagnostic.notes.push(InternedText::new(self.db, note.to_string()));
        self
    }

    /// Add a suggestion with optional replacement text.
    pub fn suggestion(
        mut self,
        text: Text<'db>,
        span: ByteSpan,
        msg: &str,
        replacement: Option<&str>,
    ) -> Self {
        self.diagnostic.suggestions.push(Suggestion {
            text,
            span,
            message: InternedText::new(self.db, msg.to_string()),
            replacement: replacement.map(|r| InternedText::new(self.db, r.to_string())),
        });
        self
    }

    /// Emit this diagnostic as a parse diagnostic.
    pub fn emit_parse(self) {
        ParseDiagnostic(self.diagnostic.to_stored()).accumulate(self.db);
    }

    /// Emit this diagnostic as a type checking diagnostic.
    pub fn emit_type(self) {
        TypeDiagnostic(self.diagnostic.to_stored()).accumulate(self.db);
    }

    /// Emit this diagnostic as a resolution diagnostic.
    pub fn emit_resolution(self) {
        ResolutionDiagnostic(self.diagnostic.to_stored()).accumulate(self.db);
    }

    /// Emit this diagnostic as a lint diagnostic.
    pub fn emit_lint(self) {
        LintDiagnostic(self.diagnostic.to_stored()).accumulate(self.db);
    }
}

/// Helper functions to retrieve accumulated diagnostics and convert them back.
impl StoredDiagnostic {
    /// Convert from stored form back to rich diagnostic.
    pub fn to_diagnostic<'db>(&self, db: &'db dyn salsa::Database) -> Diagnostic<'db> {
        use salsa::plumbing::FromId;

        Diagnostic {
            severity: self.severity,
            code: self.code.map(|id| InternedText::from_id(id)),
            message: InternedText::from_id(self.message),
            labels: self.labels.iter().map(|l| DiagnosticLabel {
                text: Text::from_id(l.text),
                span: l.span.clone(),
                message: l.message.map(|id| InternedText::from_id(id)),
                style: l.style,
            }).collect(),
            notes: self.notes.iter().map(|id| InternedText::from_id(*id)).collect(),
            suggestions: vec![],  // TODO: Handle suggestions
        }
    }
}
