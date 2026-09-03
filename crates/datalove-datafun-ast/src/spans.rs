//! Span infrastructure for datafun expressions and statements.
//!
//! Provides span types and lookup. The query function that computes spans
//! is in datalove-datafun-compiler since it depends on the parser.

use rmx::prelude::*;
use bct::diagnostic::SpanEntry;
use crate::ast::{ExprFun, ExprKey};

/// Entry pairing an expression's stable key with its span.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct SpanMapEntry<'db> {
    pub expr_key: ExprKey<'db>,
    pub entry: SpanEntry,
}

/// Datafun expression and statement spans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct DatafunSpans<'db> {
    /// Expression spans, keyed by the expression's stable key.
    pub entries: Vec<SpanMapEntry<'db>>,
    /// Break statement spans, indexed by local_index.
    pub break_spans: Vec<SpanEntry>,
    /// Continue statement spans, indexed by local_index.
    pub continue_spans: Vec<SpanEntry>,
    /// Return statement spans, indexed by local_index.
    pub ret_spans: Vec<SpanEntry>,
    /// Set statement spans, indexed by local_index.
    pub set_spans: Vec<SpanEntry>,
    /// Function definition spans, indexed by local_index.
    pub fun_spans: Vec<SpanEntry>,
    /// Type alias spans, indexed by local_index.
    pub type_alias_spans: Vec<SpanEntry>,
    /// Import statement spans, indexed by local_index.
    pub import_spans: Vec<SpanEntry>,
}

impl<'db> DatafunSpans<'db> {
    /// Create new DatafunSpans with expression spans only.
    pub fn new(entries: Vec<SpanMapEntry<'db>>) -> Self {
        Self {
            entries,
            break_spans: vec![],
            continue_spans: vec![],
            ret_spans: vec![],
            set_spans: vec![],
            fun_spans: vec![],
            type_alias_spans: vec![],
            import_spans: vec![],
        }
    }

    /// Create new DatafunSpans with all span types.
    pub fn with_stmt_spans(
        entries: Vec<SpanMapEntry<'db>>,
        break_spans: Vec<SpanEntry>,
        continue_spans: Vec<SpanEntry>,
        ret_spans: Vec<SpanEntry>,
        set_spans: Vec<SpanEntry>,
        fun_spans: Vec<SpanEntry>,
        type_alias_spans: Vec<SpanEntry>,
        import_spans: Vec<SpanEntry>,
    ) -> Self {
        Self {
            entries, break_spans, continue_spans, ret_spans, set_spans, fun_spans,
            type_alias_spans, import_spans,
        }
    }

    /// Look up span for an expression.
    pub fn lookup(&self, db: &'db dyn salsa::Database, expr: ExprFun<'db>) -> Option<SpanEntry> {
        self.lookup_key(ExprKey::of(db, expr))
    }

    /// Look up span by an expression's stable key.
    pub fn lookup_key(&self, expr_key: ExprKey<'db>) -> Option<SpanEntry> {
        self.entries.iter()
            .find(|e| e.expr_key == expr_key)
            .map(|e| e.entry.C())
    }

    /// Look up span for a break statement by local_index.
    pub fn lookup_break(&self, index: u32) -> Option<&SpanEntry> {
        self.break_spans.get(index as usize)
    }

    /// Look up span for a continue statement by local_index.
    pub fn lookup_continue(&self, index: u32) -> Option<&SpanEntry> {
        self.continue_spans.get(index as usize)
    }

    /// Look up span for a return statement by local_index.
    pub fn lookup_ret(&self, index: u32) -> Option<&SpanEntry> {
        self.ret_spans.get(index as usize)
    }

    /// Look up span for a set statement by local_index.
    pub fn lookup_set(&self, index: u32) -> Option<&SpanEntry> {
        self.set_spans.get(index as usize)
    }

    /// Look up span for a function definition by local_index.
    pub fn lookup_fun(&self, index: u32) -> Option<&SpanEntry> {
        self.fun_spans.get(index as usize)
    }

    /// Look up span for a type alias by local_index.
    pub fn lookup_type_alias(&self, index: u32) -> Option<&SpanEntry> {
        self.type_alias_spans.get(index as usize)
    }

    /// Look up span for an import statement by local_index.
    pub fn lookup_import(&self, index: u32) -> Option<&SpanEntry> {
        self.import_spans.get(index as usize)
    }
}
