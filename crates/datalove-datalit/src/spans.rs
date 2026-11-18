//! Span infrastructure for datalit expressions.
//!
//! Provides on-demand span lookup via tracked structs and query functions.

use rmx::prelude::*;
use bct::text::TextSpan;
use crate::ast::ExprFull;

/// Entry pairing an expression's position in the parse with its span.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct SpanMapEntry {
    pub expr_index: u32,
    pub entry: TextSpan,
}

/// Tracked struct for datalit expression spans.
#[salsa::tracked]
pub struct DatalitSpans<'db> {
    #[returns(clone)]
    pub entries: Vec<SpanMapEntry>,
}

impl<'db> DatalitSpans<'db> {
    /// The span of an expression the parser built.
    ///
    /// Every expression the parser builds is numbered and has a span, and
    /// only those reach the typechecker: a generated one is printed and
    /// parsed again first.
    pub fn lookup(&self, db: &'db dyn crate::Db, expr: &ExprFull<'db>) -> TextSpan {
        let expr_index = expr.local_index()
            .expect("an expression with a span was numbered by the parser");
        self.entries(db).iter()
            .find(|e| e.expr_index == expr_index)
            .map(|e| e.entry.C())
            .expect("the parser recorded a span for every expression it numbered")
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
            expr_index: e.expr_index,
            entry: TextSpan::new(e.source, e.span.C()),
        })
        .collect();

    DatalitSpans::new(db, entries)
}
