//! Span infrastructure for datafun expressions and statements.
//!
//! Provides span types and lookup. The query function that computes spans
//! is in datalove-datafun-compiler since it depends on the parser.

use rmx::prelude::*;
use bct::text::TextSpan;
use crate::ast::{ExprFun, ExprKey};

/// Entry pairing an expression's stable key with its span.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct SpanMapEntry<'db> {
    pub expr_key: ExprKey<'db>,
    pub entry: TextSpan,
}

/// Datafun expression and statement spans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct DatafunSpans<'db> {
    /// Expression spans, keyed by the expression's stable key.
    pub entries: Vec<SpanMapEntry<'db>>,
    /// Break statement spans, indexed by local_index.
    pub break_spans: Vec<TextSpan>,
    /// Continue statement spans, indexed by local_index.
    pub continue_spans: Vec<TextSpan>,
    /// Return statement spans, indexed by local_index.
    pub ret_spans: Vec<TextSpan>,
    /// Set statement spans, indexed by local_index.
    pub set_spans: Vec<TextSpan>,
    /// Function definition spans, indexed by local_index.
    pub fun_spans: Vec<TextSpan>,
    /// Type alias spans, indexed by local_index.
    pub type_alias_spans: Vec<TextSpan>,
    /// Import statement spans, indexed by local_index.
    pub import_spans: Vec<TextSpan>,
    /// Require statement spans, indexed by local_index.
    pub require_spans: Vec<TextSpan>,
    /// Spans of bare names in type position, indexed by local_index.
    ///
    /// A type alias is the one thing in a type hint that the hint cannot
    /// resolve on its own, so it is the one a diagnostic has to point at.
    pub alias_spans: Vec<TextSpan>,
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
            require_spans: vec![],
            alias_spans: vec![],
        }
    }

    /// Create new DatafunSpans with all span types.
    pub fn with_stmt_spans(
        entries: Vec<SpanMapEntry<'db>>,
        break_spans: Vec<TextSpan>,
        continue_spans: Vec<TextSpan>,
        ret_spans: Vec<TextSpan>,
        set_spans: Vec<TextSpan>,
        fun_spans: Vec<TextSpan>,
        type_alias_spans: Vec<TextSpan>,
        import_spans: Vec<TextSpan>,
        require_spans: Vec<TextSpan>,
        alias_spans: Vec<TextSpan>,
    ) -> Self {
        Self {
            entries, break_spans, continue_spans, ret_spans, set_spans, fun_spans,
            type_alias_spans, import_spans, require_spans, alias_spans,
        }
    }

    /// Look up the span of a bare name in type position by local_index.
    pub fn lookup_alias(&self, index: u32) -> Option<&TextSpan> {
        self.alias_spans.get(index as usize)
    }

    /// Look up span for an expression.
    pub fn lookup(&self, expr: ExprFun<'db>) -> Option<TextSpan> {
        self.lookup_key(expr.key())
    }

    /// Look up span by an expression's stable key.
    pub fn lookup_key(&self, expr_key: ExprKey<'db>) -> Option<TextSpan> {
        self.entries.iter()
            .find(|e| e.expr_key == expr_key)
            .map(|e| e.entry.C())
    }

    /// Look up span for a break statement by local_index.
    pub fn lookup_break(&self, index: u32) -> Option<&TextSpan> {
        self.break_spans.get(index as usize)
    }

    /// Look up span for a continue statement by local_index.
    pub fn lookup_continue(&self, index: u32) -> Option<&TextSpan> {
        self.continue_spans.get(index as usize)
    }

    /// Look up span for a return statement by local_index.
    pub fn lookup_ret(&self, index: u32) -> Option<&TextSpan> {
        self.ret_spans.get(index as usize)
    }

    /// Look up span for a set statement by local_index.
    pub fn lookup_set(&self, index: u32) -> Option<&TextSpan> {
        self.set_spans.get(index as usize)
    }

    /// Look up span for a function definition by local_index.
    pub fn lookup_fun(&self, index: u32) -> Option<&TextSpan> {
        self.fun_spans.get(index as usize)
    }

    /// Look up span for a type alias by local_index.
    pub fn lookup_type_alias(&self, index: u32) -> Option<&TextSpan> {
        self.type_alias_spans.get(index as usize)
    }

    /// Look up span for an import statement by local_index.
    pub fn lookup_import(&self, index: u32) -> Option<&TextSpan> {
        self.import_spans.get(index as usize)
    }

    /// Look up span for a require statement by local_index.
    pub fn lookup_require(&self, index: u32) -> Option<&TextSpan> {
        self.require_spans.get(index as usize)
    }
}
