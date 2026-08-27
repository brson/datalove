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
    module_graph::ModuleId,
    lexer::{Token, TokenKind, Sigil},
    bracer::{Bracer, TreeToken},
    text::{Text, TextSpan},
    source_map,
    lexer,
    bracer,
};

use datalove_datafun_ast::ast;
use datalove_datafun_ast::script;
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;
use state::{Parser, ScriptCounters};

use salsa::Database as Db;

/// Parse a specific unit from a Script.
///
/// Returns the parsed statements for that unit.
/// Salsa will memoize this per unit, so unchanged units don't need re-parsing.
#[salsa::tracked(returns(ref))]
pub fn parse_script_unit<'db>(
    db: &'db dyn Db,
    script: script::Script<'db>,
    unit_index: usize,
) -> ast::ParsedStatements<'db> {
    let units = &script.units(db);
    let unit = units[unit_index];
    let source = unit.source(db);
    // Scripts don't have a ModuleId.
    parse_with_module_id(db, source, None).parsed
}

/// Parse a Source into a datafun script with span information.
///
/// For script parsing (no module context).
#[salsa::tracked(returns(ref))]
pub fn parse<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParseResult<'db> {
    parse_with_module_id(db, source, None)
}

/// Parse a Source into a datafun script with span information and module context.
///
/// The module_id is used to give functions stable identity for memoization.
pub fn parse_with_module_id<'db>(
    db: &'db dyn Db,
    source: Source,
    module_id: Option<ModuleId<'db>>,
) -> ast::ParseResult<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let source_text = chunk.text(db);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    emit_bracer_errors(db, bracer, source_text);
    parse_bracer(db, bracer, source_text, module_id)
}

/// Parse a Source as a single expression.
#[salsa::tracked(returns(copy))]
pub fn parse_expr<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ExprFun<'db> {
    let chunk = source_map::basic_source_map(db, source);
    let source_text = chunk.text(db);
    let chunk_lex = lexer::lex_chunk(db, chunk);
    let bracer = bracer::bracer(db, chunk_lex);
    emit_bracer_errors(db, bracer, source_text);
    // Expressions don't have module context.
    parse_bracer_expr(db, bracer, source_text)
}

fn parse_bracer_expr<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
    source_text: bct::text::Text<'db>,
) -> ast::ExprFun<'db> {
    // Expressions don't have module context.
    let mut parser = Parser::from_branch_with_context(db, bracer.iter(db), source_text, None, None);
    let expr = parser.parse_expr_full();
    parser.error_if_not_exhausted();
    expr
}

/// Emit parse diagnostics for bracer errors (unclosed, mismatched, or stray braces).
fn emit_bracer_errors<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
    source_text: Text<'db>,
) {
    let chunk = bracer.chunk(db);
    let tokens = chunk.tokens(db);

    for (token_range, sigil) in bracer.errors(db) {
        // Single-token range means stray closing brace.
        // Multi-token range means unclosed opening brace.
        let is_stray_close = token_range.len() == 1;

        if is_stray_close {
            // Stray closing brace - no matching open.
            if let Some(token) = tokens.get(token_range.start) {
                let span = token.span(db);
                let ts = TextSpan::new(source_text, span);
                DiagnosticBuilder::error(db, &format!("unmatched '{}'", sigil.as_str()))
                    .code("P050")
                    .primary_label(ts, &format!("this '{}' has no matching '{}'",
                        sigil.as_str(), sigil.open_sigil().as_str()))
                    .emit_parse();
            }
        } else {
            // Unclosed opening brace.
            if let Some(open_token) = tokens.get(token_range.start) {
                let span = open_token.span(db);
                let ts = TextSpan::new(source_text, span);
                DiagnosticBuilder::error(db, &format!("unclosed '{}'", sigil.as_str()))
                    .code("P051")
                    .primary_label(ts, &format!("this '{}' is never closed",
                        sigil.as_str()))
                    .emit_parse();
            }
        }
    }
}

fn parse_bracer<'db>(
    db: &'db dyn Db,
    bracer: Bracer<'db>,
    source_text: Text<'db>,
    module_id: Option<ModuleId<'db>>,
) -> ast::ParseResult<'db> {
    // Get line iterator - newlines inside balanced braces don't count as line breaks.
    // First split on newlines, then filter spaces from each line.
    let lines = bracer.iter(db)
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
        });

    let (statements, spans) = parse_statements(db, lines, source_text, module_id);
    let parsed = ast::ParsedStatements { statements };
    ast::ParseResult {
        parsed,
        expr_spans: spans.expr_spans,
        break_spans: spans.break_spans,
        continue_spans: spans.continue_spans,
        ret_spans: spans.ret_spans,
        set_spans: spans.set_spans,
        fun_spans: spans.fun_spans,
        type_alias_spans: spans.type_alias_spans,
    }
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

/// Parsed statement spans result.
struct ParsedSpans<'db> {
    expr_spans: Vec<ast::ParseSpanEntry<'db>>,
    break_spans: Vec<bct::diagnostic::SpanEntry>,
    continue_spans: Vec<bct::diagnostic::SpanEntry>,
    ret_spans: Vec<bct::diagnostic::SpanEntry>,
    set_spans: Vec<bct::diagnostic::SpanEntry>,
    fun_spans: Vec<bct::diagnostic::SpanEntry>,
    type_alias_spans: Vec<bct::diagnostic::SpanEntry>,
}

/// Parse statements from lines, creating a Parser for each line.
///
/// Returns statements and accumulated spans.
fn parse_statements<'db>(
    db: &'db dyn Db,
    lines: impl Iterator<Item = Vec<TreeToken<'db>>>,
    source_text: bct::text::Text<'db>,
    module_id: Option<ModuleId<'db>>,
) -> (Vec<ast::Statement<'db>>, ParsedSpans<'db>) {
    let mut statements = vec![];
    let mut all_expr_spans = vec![];
    let mut all_break_spans = vec![];
    let mut all_continue_spans = vec![];
    let mut all_ret_spans = vec![];
    let mut all_set_spans = vec![];
    let mut all_fun_spans = vec![];
    let mut all_type_alias_spans = vec![];
    let mut line_iter = lines.enumerate().peekable();
    let mut counters = ScriptCounters::default();

    while let Some((_line_num, line)) = line_iter.next() {
        if line.is_empty() {
            continue;
        }

        let mut parser = Parser::new(db, line, source_text, module_id, counters);
        let statement = parser.parse_statement(&mut line_iter);
        counters = parser.script_counters();
        statements.push(statement);
        all_expr_spans.extend(parser.take_expr_spans());
        all_break_spans.extend(parser.take_break_spans());
        all_continue_spans.extend(parser.take_continue_spans());
        all_ret_spans.extend(parser.take_ret_spans());
        all_set_spans.extend(parser.take_set_spans());
        all_fun_spans.extend(parser.take_fun_spans());
        all_type_alias_spans.extend(parser.take_type_alias_spans());
    }

    let spans = ParsedSpans {
        expr_spans: all_expr_spans,
        break_spans: all_break_spans,
        continue_spans: all_continue_spans,
        ret_spans: all_ret_spans,
        set_spans: all_set_spans,
        fun_spans: all_fun_spans,
        type_alias_spans: all_type_alias_spans,
    };
    (statements, spans)
}

/// Tracked wrapper for parser tests that only need the ParsedStatements.
#[salsa::tracked(returns(ref))]
pub fn parse_for_test<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedStatements<'db> {
    parse(db, source).parsed.clone()
}

/// Public tracked wrapper for integration tests that returns just the ParsedStatements.
///
/// Integration tests are compiled as separate binaries and need pub access.
#[salsa::tracked(returns(ref))]
pub fn parse_integration_test<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedStatements<'db> {
    parse(db, source).parsed.clone()
}

/// Public tracked wrapper for integration code to enable diagnostic accumulation.
///
/// This function should be called before parse() to accumulate diagnostics,
/// then parse() can be called separately to get the full ParseResult.
/// Returns just the ParsedStatements to satisfy Salsa's type requirements.
#[salsa::tracked(returns(ref))]
pub fn parse_for_diagnostics<'db>(
    db: &'db dyn Db,
    source: Source,
) -> ast::ParsedStatements<'db> {
    parse(db, source).parsed.clone()
}

use datalove_datafun_ast::spans::{SpanMapEntry, DatafunSpans};

/// Extract datafun expression spans from a parsed source.
///
/// Reads spans from the ParseResult side table (no accumulators).
pub fn datafun_spans<'db>(
    db: &'db dyn Db,
    source: Source,
) -> DatafunSpans {
    use bct::diagnostic::SpanEntry;

    let parse_result = parse(db, source);
    let entries: Vec<SpanMapEntry> = parse_result.expr_spans
        .iter()
        .map(|e| SpanMapEntry {
            expr_key: e.expr_key,
            entry: SpanEntry::new(e.source, e.span.C()),
        })
        .collect();

    DatafunSpans::with_stmt_spans(
        entries,
        parse_result.break_spans.C(),
        parse_result.continue_spans.C(),
        parse_result.ret_spans.C(),
        parse_result.set_spans.C(),
        parse_result.fun_spans.C(),
        parse_result.type_alias_spans.C(),
    )
}
