//! Span query functions for datafun expressions.
//!
//! The span types are defined in datalove-datafun-ast.
//! This module provides the query function that computes spans.

use rmx::prelude::*;
use datalove_diagnostic::SpanEntry;

// Re-export types from AST crate.
pub use datalove_datafun_ast::spans::{
    DatafunSpanAccumulator,
    SpanMapEntry,
    DatafunSpans,
};

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
