//! Span infrastructure for datafun expressions.
//!
//! Provides on-demand span lookup via tracked structs and query functions.

use rmx::prelude::*;
use datalove_diagnostic::{SpanEntry, ByteSpan};
use crate::ast::ExprFun;
use bct::text::Text;

/// Salsa accumulator for datafun expression spans.
/// Emitted during parsing to record source locations.
#[salsa::accumulator]
pub struct DatafunSpanAccumulator {
    pub expr_id: salsa::Id,
    pub text_id: salsa::Id,
    pub span: ByteSpan,
}

/// Salsa accumulator for datalit expression spans.
/// Emitted during parsing to record source locations.
#[salsa::accumulator]
pub struct DatalitSpanAccumulator {
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

/// Tracked struct for embedded datalit expression spans.
#[salsa::tracked]
pub struct DatalitSpans<'db> {
    pub entries: Vec<SpanMapEntry>,
}

impl<'db> DatalitSpans<'db> {
    /// Look up span for a datalit expression.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: datalove_datalit::ast::ExprFull<'db>) -> Option<SpanEntry> {
        use salsa::plumbing::AsId;
        let expr_id = expr.as_id();
        self.entries(db).iter()
            .find(|e| e.expr_id == expr_id)
            .map(|e| e.entry.clone())
    }
}

/// Extract datafun expression spans from a parsed source.
#[salsa::tracked]
pub fn datafun_spans<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> DatafunSpans<'db> {
    // Trigger parsing to accumulate spans.
    crate::parser::parse_for_diagnostics(db, source);

    // Retrieve accumulated spans.
    let accumulated = crate::parser::parse_for_diagnostics::accumulated::<DatafunSpanAccumulator>(db, source);

    let entries: Vec<SpanMapEntry> = accumulated.iter()
        .map(|acc| SpanMapEntry {
            expr_id: acc.expr_id,
            entry: SpanEntry::new(acc.text_id, acc.span.clone()),
        })
        .collect();

    DatafunSpans::new(db, entries)
}

/// Extract datalit expression spans from a parsed datafun source.
#[salsa::tracked]
pub fn datalit_spans<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> DatalitSpans<'db> {
    // Trigger parsing to accumulate spans.
    crate::parser::parse_for_diagnostics(db, source);

    // Retrieve accumulated spans.
    let accumulated = crate::parser::parse_for_diagnostics::accumulated::<DatalitSpanAccumulator>(db, source);

    let entries: Vec<SpanMapEntry> = accumulated.iter()
        .map(|acc| SpanMapEntry {
            expr_id: acc.expr_id,
            entry: SpanEntry::new(acc.text_id, acc.span.clone()),
        })
        .collect();

    DatalitSpans::new(db, entries)
}
