//! Diagnostic system for Datalove compiler.
//!
//! Provides Datalove-specific Salsa accumulators for different compilation phases.

use salsa::Accumulator;
use bct::diagnostic::{Diagnostic, DiagnosticBuilder, StoredDiagnostic};

/// Salsa accumulator for parse diagnostics.
#[salsa::accumulator]
pub struct ParseDiagnostic(StoredDiagnostic);

impl ParseDiagnostic {
    pub fn to_diagnostic<'db>(&self, db: &'db dyn salsa::Database) -> Diagnostic<'db> {
        self.0.to_diagnostic(db)
    }
}

/// Salsa accumulator for type checking diagnostics.
#[salsa::accumulator]
pub struct TypeDiagnostic(StoredDiagnostic);

impl TypeDiagnostic {
    pub fn to_diagnostic<'db>(&self, db: &'db dyn salsa::Database) -> Diagnostic<'db> {
        self.0.to_diagnostic(db)
    }
}

/// Salsa accumulator for name resolution diagnostics.
#[salsa::accumulator]
pub struct ResolutionDiagnostic(StoredDiagnostic);

impl ResolutionDiagnostic {
    pub fn to_diagnostic<'db>(&self, db: &'db dyn salsa::Database) -> Diagnostic<'db> {
        self.0.to_diagnostic(db)
    }
}

/// Salsa accumulator for lint diagnostics.
#[salsa::accumulator]
pub struct LintDiagnostic(StoredDiagnostic);

impl LintDiagnostic {
    pub fn to_diagnostic<'db>(&self, db: &'db dyn salsa::Database) -> Diagnostic<'db> {
        self.0.to_diagnostic(db)
    }
}

// Extension trait to add emit methods to DiagnosticBuilder for datalove accumulators.
pub trait DiagnosticBuilderExt<'db> {
    /// Emit this diagnostic as a parse diagnostic.
    fn emit_parse(self);

    /// Emit this diagnostic as a type checking diagnostic.
    fn emit_type(self);

    /// Emit this diagnostic as a resolution diagnostic.
    fn emit_resolution(self);

    /// Emit this diagnostic as a lint diagnostic.
    fn emit_lint(self);
}

impl<'db> DiagnosticBuilderExt<'db> for DiagnosticBuilder<'db> {
    fn emit_parse(self) {
        let db = self.db();
        ParseDiagnostic(self.build_stored()).accumulate(db);
    }

    fn emit_type(self) {
        let db = self.db();
        TypeDiagnostic(self.build_stored()).accumulate(db);
    }

    fn emit_resolution(self) {
        let db = self.db();
        ResolutionDiagnostic(self.build_stored()).accumulate(db);
    }

    fn emit_lint(self) {
        let db = self.db();
        LintDiagnostic(self.build_stored()).accumulate(db);
    }
}
