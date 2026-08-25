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
    bracer::{Bracer, TreeToken},
    source_map,
    lexer,
    bracer,
};

use crate::ast;
use state::Parser;

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
    parse_bracer(db, bracer, source_text)
}

fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
    source_text: bct::text::Text<'db>,
) -> ast::ParseResult<'db> {
    let mut parser = Parser::from_branch(db, bracer.iter(db), source_text);
    let expr = parser.parse_expr_full();
    ast::ParseResult::new(db, expr, parser.take_expr_spans())
}

/// Parse a type hint from a vector of tokens.
///
/// Returns the parsed type hint and the number of tokens consumed.
pub fn parse_type_hint_from_tokens<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    source_text: bct::text::Text<'db>,
) -> (ast::TypeHint<'db>, usize) {
    let mut parser = Parser::new(db, tokens, source_text);
    let type_hint = parser.parse_type_hint();
    let consumed = parser.pos();
    (type_hint, consumed)
}

/// Test-only tracked wrapper around parse() to provide Salsa context.
///
/// Allows tests to call parse() which creates tracked AST nodes.
/// Regular code should call parse() from within a tracked function context.
#[salsa::tracked(returns(copy))]
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
#[salsa::tracked(returns(copy))]
pub fn parse_integration_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    parse(db, source).expr(db)
}
