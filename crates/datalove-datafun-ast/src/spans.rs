//! Span infrastructure for datafun expressions.
//!
//! Provides span types and lookup. The query function that computes spans
//! is in datalove-datafun-compiler since it depends on the parser.

use rmx::prelude::*;
use datalove_diagnostic::SpanEntry;
use crate::ast::ExprFun;

/// Entry pairing expression ID with span.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanMapEntry {
    pub expr_id: salsa::Id,
    pub entry: SpanEntry,
}

/// Datafun expression spans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct DatafunSpans {
    pub entries: Vec<SpanMapEntry>,
}

impl DatafunSpans {
    /// Create new DatafunSpans.
    pub fn new(entries: Vec<SpanMapEntry>) -> Self {
        Self { entries }
    }

    /// Look up span for an expression.
    pub fn lookup<'db>(&self, expr: ExprFun<'db>) -> Option<SpanEntry> {
        use salsa::plumbing::AsId;
        let expr_id = expr.as_id();
        self.entries.iter()
            .find(|e| e.expr_id == expr_id)
            .map(|e| e.entry.clone())
    }
}
