//! Datafun parser module.
//!
//! Parses datafun source text into AST nodes.

mod state;
mod statement;
mod expr;
mod literal;

use rmx::prelude::*;

use bct::{
    input::Source,
    lexer::{Token, TokenKind, Sigil},
    bracer::{Bracer, TreeToken},
    source_map,
    lexer,
    bracer,
};

use datalove_datafun_ast::ast;
use datalove_datafun_ast::script;
use state::Parser;

/// Re-export Db trait for convenience.
pub use salsa::Database as Db;

/// Parse a specific unit from a Script.
///
/// Returns the parsed statements for that unit.
/// Salsa will memoize this per unit, so unchanged units don't need re-parsing.
#[salsa::tracked]
pub fn parse_script_unit<'db>(
    db: &'db dyn Db,
    script: script::Script,
    unit_index: usize,
) -> ast::Script<'db> {
    let units = &script.units(db);
    let unit = units[unit_index];
    let source = unit.source(db);
    parse(db, source).script(db)
}

/// Parse a Source into a datafun script with span information.
#[salsa::tracked]
pub fn parse<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParseResult<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer(db, bracer)
}

/// Parse a Source as a single expression.
#[salsa::tracked]
pub fn parse_expr<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ExprFun<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    parse_bracer_expr(db, bracer)
}

fn parse_bracer_expr<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
) -> ast::ExprFun<'db> {
    // Collect all tokens, filtering spaces.
    let tokens: Vec<TreeToken<'db>> = bracer.iter(db)
        .filter_map(|token| token.without_space(db))
        .collect();

    let mut parser = Parser::new(db, tokens);
    let expr = parser.parse_expr_full();
    parser.error_if_not_exhausted();
    expr
}

fn parse_bracer<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
) -> ast::ParseResult<'db> {
    // Get line iterator - newlines inside balanced braces don't count as line breaks.
    // First split on newlines, then filter spaces from each line.
    let lines: Vec<Vec<TreeToken<'db>>> = bracer.iter(db)
        .batching(|iter| {
            let mut line = vec![];
            let mut found_newline = false;

            while let Some(token) = iter.next() {
                match token {
                    TreeToken::Token(t) if is_line_separator(db, t) => {
                        found_newline = true;
                        break;
                    }
                    _ => {
                        // Filter spaces here, after newline check.
                        if let Some(t) = token.without_space(db) {
                            line.push(t);
                        }
                    }
                }
            }

            if !line.is_empty() || found_newline {
                Some(line)
            } else {
                None
            }
        })
        .collect();

    let statements = parse_statements(db, lines);
    let script = ast::Script::new(db, statements);
    ast::ParseResult::new(db, script)
}

/// Check if a token acts as a line separator.
///
/// Line separators are newlines or semicolons.
fn is_line_separator<'db>(db: &'db dyn Db, token: Token<'db>) -> bool {
    match token.kind(db) {
        TokenKind::Whitespace => token.text(db).as_str(db).contains("\n"),
        TokenKind::Sigil(Sigil::Semicolon) => true,
        _ => false,
    }
}

/// Parse statements from lines, creating a Parser for each line.
fn parse_statements<'db>(
    db: &'db dyn Db,
    lines: Vec<Vec<TreeToken<'db>>>,
) -> Vec<ast::Statement<'db>> {
    let mut statements = vec![];
    let mut line_iter = lines.into_iter().enumerate().peekable();

    while let Some((_line_num, line)) = line_iter.next() {
        if line.is_empty() {
            continue;
        }

        let mut parser = Parser::new(db, line);
        let statement = parser.parse_statement(&mut line_iter);
        statements.push(statement);
    }

    statements
}

/// Tracked wrapper for parser tests that only need the Script.
#[salsa::tracked]
pub fn parse_for_test<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::Script<'db> {
    parse(db, source).script(db)
}

/// Public tracked wrapper for integration tests that returns just the Script.
///
/// Integration tests are compiled as separate binaries and need pub access.
#[salsa::tracked]
pub fn parse_integration_test<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::Script<'db> {
    parse(db, source).script(db)
}

/// Public tracked wrapper for integration code to enable diagnostic accumulation.
///
/// This function should be called before parse() to accumulate diagnostics,
/// then parse() can be called separately to get the full ParseResult.
/// Returns just the Script to satisfy Salsa's type requirements.
#[salsa::tracked]
pub fn parse_for_diagnostics<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::Script<'db> {
    parse(db, source).script(db)
}
