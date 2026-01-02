//! Span infrastructure for datafun expressions.
//!
//! Provides span types and lookup. The query function that computes spans
//! is in datalove-datafun-compiler since it depends on the parser.

use rmx::prelude::*;
use datalove_diagnostic::{SpanEntry, ByteSpan};
use crate::ast::ExprFun;

/// Salsa accumulator for datafun expression spans.
/// Emitted during parsing to record source locations.
#[salsa::accumulator]
pub struct DatafunSpanAccumulator {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

/// Entry pairing expression ID with span.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanMapEntry {
    pub expr_id: salsa::Id,
    pub entry: SpanEntry,
}

/// Tracked struct for datafun expression spans.
#[salsa::tracked]
pub struct DatafunSpans<'db> {
    pub entries: Vec<SpanMapEntry>,
}

impl<'db> DatafunSpans<'db> {
    /// Look up span for an expression.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: ExprFun<'db>) -> Option<SpanEntry> {
        use salsa::plumbing::AsId;
        let expr_id = expr.as_id();
        self.entries(db).iter()
            .find(|e| e.expr_id == expr_id)
            .map(|e| e.entry.clone())
    }
}
