//! Datalit parser module.
//!
//! Parses datalit source text into AST nodes.

mod state;
mod type_hint;
mod expr;
mod chunk;

#[cfg(test)]
mod tests;

use rmx::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use bct::{
    input::Source,
    bracer::Bracer,
    diagnostic::StoredDiagnostic,
    lexer,
    bracer,
};

use crate::ast;
use state::Parser;

pub use type_hint::{parse_type_hint, TypeHintStream};
pub use chunk::{ChunkItems, ChunkParse, parse_chunk};

/// Parse a Source into a datalit expression with span information.
#[salsa::tracked(returns(copy))]
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ParseResult<'db> {
    let lexed = lexer::lex(db, source);
    let bracer = bracer::bracer(db, lexed);
    // A literal reports its first error and drops what that one led to; see
    // `file_first_error_per_statement`. So nothing is filed until the whole
    // literal is read.
    let kept: Rc<RefCell<Vec<StoredDiagnostic>>> = Rc::default();
    kept.borrow_mut().extend(crate::parser_util::prepass_errors(db, bracer, source));
    let result = parse_bracer(db, bracer, source, Some(kept.C()));
    // A literal is one expression, so one statement, whatever lines it
    // spans: tokens left on a line of their own after it are its error too.
    crate::parser_util::file_first_error_per_statement(db, &[], kept.take());
    result
}

fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
    source_text: bct::input::Source,
    sink: crate::parser_util::Sink,
) -> ast::ParseResult<'db> {
    let mut parser = Parser::from_branch(db, bracer.iter(db), source_text);
    parser.sink = sink;
    parser.plan = crate::chunk_plan::collection_plan(db, bracer.lexed(db));
    let expr = parser.parse_expr_full();
    // A document is one expression, so anything after it was written by
    // mistake rather than left for a caller to read.
    parser.error_if_not_exhausted();
    ast::ParseResult::new(db, expr, parser.take_expr_spans())
}

/// Test-only tracked wrapper around parse() to provide Salsa context.
///
/// Allows tests to call parse() which creates tracked AST nodes.
/// Regular code should call parse() from within a tracked function context.
#[salsa::tracked(returns(clone))]
#[cfg(test)]
pub(crate) fn parse_for_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    parse(db, source).expr(db)
}

/// Public wrapper for integration tests.
///
/// Integration tests are compiled as separate binaries and need pub access.
#[salsa::tracked(returns(clone))]
pub fn parse_integration_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    parse(db, source).expr(db)
}
