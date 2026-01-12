//! Type checking context.
//!
//! Provides TypeContext for tracking errors during typechecking.

use bct::text::TextSpan;
use crate::ast::ExprFull;
use crate::resolve::ResolvedExpr;
use super::types::TypeError;

/// Context for typechecking.
pub struct TypeContext<'db> {
    pub(crate) db: &'db dyn crate::Db,
    pub(crate) source: bct::input::Source,
    pub(crate) errors: Vec<TypeError>,
}

impl<'db> TypeContext<'db> {
    pub fn new(db: &'db dyn crate::Db, resolved: ResolvedExpr<'db>) -> Self {
        TypeContext {
            db,
            source: resolved.source(db),
            errors: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: TypeError) {
        self.errors.push(error);
    }

    /// Look up the source location for an expression (on-demand).
    pub fn get_span(&self, expr: ExprFull<'db>) -> Option<TextSpan<'db>> {
        let spans = crate::spans::datalit_spans(self.db, self.source);
        spans.get_text_span(self.db, expr)
    }
}
