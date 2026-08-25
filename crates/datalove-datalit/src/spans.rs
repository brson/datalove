//! Span infrastructure for datalit expressions.
//!
//! Provides on-demand span lookup via tracked structs and query functions.

use rmx::prelude::*;
use bct::text::TextSpan;
use bct::diagnostic::SpanEntry;
use crate::ast::ExprFull;

/// Entry pairing expression ID with span.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanMapEntry {
    pub expr_id: salsa::Id,
    pub entry: SpanEntry,
}

/// Tracked struct for datalit expression spans.
#[salsa::tracked]
pub struct DatalitSpans<'db> {
    #[returns(clone)]
    pub entries: Vec<SpanMapEntry>,
}

impl<'db> DatalitSpans<'db> {
    /// Look up span for an expression.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: ExprFull<'db>) -> Option<SpanEntry> {
        use salsa::plumbing::AsId;
        let expr_id = expr.as_id();
        self.entries(db).iter()
            .find(|e| e.expr_id == expr_id)
            .map(|e| e.entry.C())
    }

    /// Get text and span for an expression.
    pub fn get_text_span(&self, db: &'db dyn crate::Db, expr: ExprFull<'db>) -> Option<TextSpan<'db>> {
        self.lookup(db, expr).map(|entry| {
            let (text, span) = entry.to_text_and_span(db);
            TextSpan::new(text, span)
        })
    }
}

/// Extract datalit expression spans from a parsed source.
#[salsa::tracked(returns(copy))]
pub fn datalit_spans<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> DatalitSpans<'db> {
    let parse_result = crate::parser::parse(db, source);
    let entries = parse_result.expr_spans(db)
        .iter()
        .map(|e| SpanMapEntry {
            expr_id: e.expr_id,
            entry: SpanEntry::new(e.text_id, e.span.C()),
        })
        .collect();

    DatalitSpans::new(db, entries)
}
