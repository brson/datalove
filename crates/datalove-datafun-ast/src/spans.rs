//! Span infrastructure for datafun expressions and statements.
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

/// Datafun expression and statement spans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::Update)]
pub struct DatafunSpans {
    /// Expression spans, keyed by expression salsa ID.
    pub entries: Vec<SpanMapEntry>,
    /// Break statement spans, indexed by local_index.
    pub break_spans: Vec<SpanEntry>,
    /// Continue statement spans, indexed by local_index.
    pub continue_spans: Vec<SpanEntry>,
}

impl DatafunSpans {
    /// Create new DatafunSpans with expression spans only.
    pub fn new(entries: Vec<SpanMapEntry>) -> Self {
        Self {
            entries,
            break_spans: vec![],
            continue_spans: vec![],
        }
    }

    /// Create new DatafunSpans with all span types.
    pub fn with_stmt_spans(
        entries: Vec<SpanMapEntry>,
        break_spans: Vec<SpanEntry>,
        continue_spans: Vec<SpanEntry>,
    ) -> Self {
        Self { entries, break_spans, continue_spans }
    }

    /// Look up span for an expression.
    pub fn lookup<'db>(&self, expr: ExprFun<'db>) -> Option<SpanEntry> {
        use salsa::plumbing::AsId;
        let expr_id = expr.as_id();
        self.entries.iter()
            .find(|e| e.expr_id == expr_id)
            .map(|e| e.entry.clone())
    }

    /// Look up span for a break statement by local_index.
    pub fn lookup_break(&self, index: u32) -> Option<&SpanEntry> {
        self.break_spans.get(index as usize)
    }

    /// Look up span for a continue statement by local_index.
    pub fn lookup_continue(&self, index: u32) -> Option<&SpanEntry> {
        self.continue_spans.get(index as usize)
    }
}
