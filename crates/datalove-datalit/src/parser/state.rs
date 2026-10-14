//! Parser state and helper methods.

use rmx::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use rustc_hash::FxHashMap;

use bct::{
    bracer::{Cursor, TreeToken},
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;

/// Where a parser reads its tokens: a cursor over a bracket or a part of
/// one, with the two tokens at it held for lookahead.
struct TokenSource<'db> {
    cursor: Cursor<'db>,
    /// Lookahead buffer: [0] = current, [1] = next.
    buffer: [Option<TreeToken<'db>>; 2],
    /// The end of the last token consumed, for span tracking.
    last_end: Option<usize>,
}

impl<'db> TokenSource<'db> {
    fn new(mut cursor: Cursor<'db>) -> Self {
        let first = cursor.next();
        let second = cursor.next();
        TokenSource { cursor, buffer: [first, second], last_end: None }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let result = self.buffer[0].take();
        if let Some(token) = &result {
            self.last_end = Some(token.span().end);
        }
        self.buffer[0] = self.buffer[1].take();
        self.buffer[1] = self.cursor.next();
        result
    }
}

/// Text interned during one parse, shared by a parser and the sub-parsers it
/// makes for branches.
///
/// A data file says the same field names and small numbers over and over, and
/// salsa hashes and probes its map for every occurrence it is asked to intern.
type Interned<'db> = Rc<RefCell<FxHashMap<&'db str, InternedText<'db>>>>;

/// Parser state for datalit parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn crate::Db,
    source: TokenSource<'db>,
    pub(super) expr_spans: Vec<ast::ParseSpanEntry>,
    /// Next position to hand out, shared with sub-parsers so that one parse
    /// numbers its expressions in a single sequence.
    pub(super) expr_counter: u32,
    /// The same for the bare names in type position, whose spans a branch's
    /// own parser collects on behalf of whatever stream asked for the type.
    pub(super) alias_counter: u32,
    pub(super) alias_spans: Vec<bct::diagnostic::SpanEntry>,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::text::Text<'db>,
    /// The same text as a string, which the tokens' spans index.
    text: &'db str,
    interned: Interned<'db>,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens (Vec-backed).
    pub(super) fn new(db: &'db dyn crate::Db, tokens: Cursor<'db>, source_text: bct::text::Text<'db>) -> Self {
        Parser {
            db,
            source: TokenSource::new(tokens),
            expr_spans: Vec::new(),
            expr_counter: 0,
            alias_counter: 0,
            alias_spans: Vec::new(),
            had_error: false,
            source_text,
            text: source_text.as_str(db),
            interned: Interned::default(),
        }
    }

    /// Create a new parser from a Cursor (iterator-backed, zero allocation).
    pub(super) fn from_branch(db: &'db dyn crate::Db, iter: Cursor<'db>, source_text: bct::text::Text<'db>) -> Self {
        Self::from_branch_numbering_aliases(db, iter, source_text, 0)
    }

    /// The same, continuing the alias numbering of the stream that asked.
    ///
    /// What is inside a branch is read by a parser of its own whatever the
    /// outer stream is, so without this each nested type would number its
    /// aliases from zero again and file their spans where no one looks.
    pub(super) fn from_branch_numbering_aliases(
        db: &'db dyn crate::Db,
        iter: Cursor<'db>,
        source_text: bct::text::Text<'db>,
        alias_base: u32,
    ) -> Self {
        Parser {
            db,
            source: TokenSource::new(iter),
            expr_spans: Vec::new(),
            expr_counter: 0,
            alias_counter: alias_base,
            alias_spans: Vec::new(),
            had_error: false,
            source_text,
            text: source_text.as_str(db),
            interned: Interned::default(),
        }
    }

    pub(super) fn take_alias_spans(&mut self) -> Vec<bct::diagnostic::SpanEntry> {
        rmx::std::mem::take(&mut self.alias_spans)
    }

    /// A sub-parser over a braced branch, carrying on this one's numbering.
    pub(super) fn sub_parser_from_branch(&self, iter: Cursor<'db>) -> Self {
        let mut sub = Parser::from_branch(self.db, iter, self.source_text());
        sub.expr_counter = self.expr_counter;
        sub.interned = Rc::clone(&self.interned);
        sub
    }

    /// A sub-parser over a slice of tokens, carrying on this one's numbering.
    pub(super) fn sub_parser_from_tokens(&self, tokens: Cursor<'db>) -> Self {
        let mut sub = Parser::new(self.db, tokens, self.source_text());
        sub.expr_counter = self.expr_counter;
        sub.interned = Rc::clone(&self.interned);
        sub
    }

    /// The next expression position, advancing the counter.
    pub(super) fn next_expr_index(&mut self) -> u32 {
        let index = self.expr_counter;
        self.expr_counter += 1;
        index
    }

    /// Take a sub-parser's spans and its place in the numbering.
    ///
    /// A sub-parser starts where this one had got to and carries on, so the
    /// counter has to come back or the two would hand out the same positions.
    pub(super) fn merge_spans_from(&mut self, sub: &mut Self) {
        self.expr_spans.append(&mut sub.expr_spans);
        self.expr_counter = sub.expr_counter;
    }

    /// Take the accumulated expression spans.
    pub(super) fn take_expr_spans(&mut self) -> Vec<ast::ParseSpanEntry> {
        std::mem::take(&mut self.expr_spans)
    }

    /// Emit both a diagnostic and create an ExprParseError node in one call.
    pub(super) fn emit_expr_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Expr<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.clone(), label)
            .emit_parse();
        ast::Expr::ParseError(ast::ExprParseError { text: ts.text, span: ts.span, message: message_text })
    }

    /// Emit both a diagnostic and create a TypeHintParseError node in one call.
    pub(super) fn emit_type_hint_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::TypeHint<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.clone(), label)
            .emit_parse();
        ast::TypeHint::ParseError(ast::TypeHintParseError { text: ts.text, span: ts.span, message: message_text })
    }

    /// Parse a u32 literal from the current position.
    pub(super) fn parse_u32_literal(&mut self) -> Option<u32> {
        match self.peek() {
            Some(TreeToken::Token(tok)) => {
                if let Some(word) = tok.word_str(self.text) {
                    if let Ok(value) = word.parse::<u32>() {
                        self.next();
                        return Some(value);
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Emit error if tokens remain unconsumed after a successful parse.
    ///
    /// Only emits if the parser succeeded (no prior errors). This catches
    /// both parser bugs and user syntax errors.
    pub(super) fn error_if_not_exhausted(&mut self) {
        if self.peek().is_some() && !self.had_error {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("D021")
                .primary_label(ts, "unexpected token")
                .emit_parse();
        }
    }

    /// Emit error if tokens remain unconsumed after a successful type hint parse.
    ///
    /// Only emits if the parser succeeded (no prior errors). This catches
    /// both parser bugs and user syntax errors.
    pub(super) fn error_if_not_exhausted_type_hint(&mut self) {
        if self.peek().is_some() && !self.had_error {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after type")
                .code("D022")
                .primary_label(ts, "unexpected token")
                .emit_parse();
        }
    }

}

impl<'db> TokenStream<'db> for Parser<'db> {
    fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    fn peek(&self) -> Option<&TreeToken<'db>> {
        self.source.buffer[0].as_ref()
    }

    fn peek_next(&self) -> Option<&TreeToken<'db>> {
        self.source.buffer[1].as_ref()
    }

    fn prev_end(&self) -> Option<usize> {
        self.source.last_end
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        self.source.next()
    }

    fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }

    fn text(&self) -> &'db str {
        self.text
    }

    fn intern(&mut self, text: &'db str) -> InternedText<'db> {
        let db = self.db;
        *self.interned.borrow_mut().entry(text).or_insert_with(|| InternedText::new(db, text))
    }
}
