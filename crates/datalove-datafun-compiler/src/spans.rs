//! Span query functions for datafun expressions.
//!
//! Re-exports from the parser crate which computes spans during parsing.

// Re-export everything from the parser crate.
pub use datalove_datafun_parser::{
    DatafunSpanAccumulator,
    SpanMapEntry,
    DatafunSpans,
    datafun_spans,
};
