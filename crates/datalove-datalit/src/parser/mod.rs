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
#[salsa::tracked]
pub fn parse<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ParseResult<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer(db, bracer)
}

fn parse_bracer<'db>(
    db: &'db dyn crate::Db,
    bracer: Bracer<'db>,
) -> ast::ParseResult<'db> {
    let chunk_lex = bracer.chunk(db);
    // Get source text from the first token.
    let source_text = chunk_lex.tokens(db).first().map(|token| {
        let subtext = token.text(db);
        subtext.text(db)
    });
    let tokens = bracer.iter(db).filter_map(|t| t.without_space(db)).collect::<Vec<_>>();
    parse_from_tokens_with_source(db, tokens, source_text)
}

/// Parse a datalit expression directly from a vector of tokens.
///
/// Allows other parsers to delegate to the datalit parser without
/// reconstructing source text from tokens.
pub fn parse_from_tokens<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
) -> ast::ParseResult<'db> {
    // Try to extract source text from the first token for better error reporting.
    let source_text = tokens.first().and_then(|token| {
        match token {
            TreeToken::Token(tok) => {
                let subtext = tok.text(db);
                Some(subtext.text(db))
            }
            TreeToken::Branch(_, iter) => {
                // Look inside the branch for a token.
                iter.clone().find_map(|inner| {
                    inner.without_space(db).and_then(|t| {
                        match t {
                            TreeToken::Token(tok) => {
                                let subtext = tok.text(db);
                                Some(subtext.text(db))
                            }
                            TreeToken::Branch(_, _) => None
                        }
                    })
                })
            }
        }
    });
    parse_from_tokens_with_source(db, tokens, source_text)
}

/// Parse a datalit expression from tokens with an optional source Text for error reporting.
fn parse_from_tokens_with_source<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
    source_text: Option<bct::text::Text<'db>>,
) -> ast::ParseResult<'db> {
    let mut parser = Parser::new(db, tokens, source_text);
    let expr = parser.parse_expr_full();
    ast::ParseResult::new(db, expr, parser.take_expr_spans())
}

/// Parse a type hint and heap from a vector of tokens.
///
/// Returns the parsed type hint and the number of tokens consumed.
pub fn parse_type_hint_and_heap_from_tokens<'db>(
    db: &'db dyn crate::Db,
    tokens: Vec<TreeToken<'db>>,
) -> (ast::TypeHintAndHeap<'db>, usize) {
    let mut parser = Parser::new(db, tokens, None);
    let type_hint = parser.parse_type_hint_and_heap();
    let consumed = parser.pos();
    (type_hint, consumed)
}

/// Test-only tracked wrapper around parse() to provide Salsa context.
///
/// Allows tests to call parse() which creates tracked AST nodes.
/// Regular code should call parse() from within a tracked function context.
#[salsa::tracked]
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
#[salsa::tracked]
pub fn parse_integration_test<'db>(
    db: &'db dyn crate::Db,
    source: Source,
) -> ast::ExprFull<'db> {
    parse(db, source).expr(db)
}
