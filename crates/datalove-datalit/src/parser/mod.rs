//! Datalit parser module.
//!
//! Parses datalit source text into AST nodes.

mod state;
mod type_hint;
mod expr;

#[cfg(test)]
mod tests;

use bct::{
    input::Source,
    bracer::Bracer,
    source_map,
    lexer,
    bracer,
    parser_util,
};

use crate::ast;
use datalove_diagnostic::DiagnosticBuilderExt;
use state::Parser;

pub use type_hint::{parse_type_hint, TypeHintStream};

/// Parse a Source into a datalit expression with span information.
#[salsa::tracked(returns(copy))]
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ParseResult<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let source_text = chunk.text(db);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    let chunk_text = chunk.text(db).as_str(db);
    for span in parser_util::non_ascii_names(chunk_lex.tokens(db), chunk_text) {
        parser_util::non_ascii_name_error(db, source_text, span, chunk_text)
            .code("D043")
            .emit_parse();
    }
    parse_bracer(db, bracer, source_text)
}

fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
    source_text: bct::text::Text<'db>,
) -> ast::ParseResult<'db> {
    let mut parser = Parser::from_branch(db, bracer.iter(db), source_text);
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
