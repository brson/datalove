//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    lexer::TokenKind,
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use bct::diagnostic::DiagnosticBuilder;
use datalove_diagnostic::DiagnosticBuilderExt;

/// Token source for parser - either Vec-backed or iterator-backed.
enum TokenSource<'db> {
    /// Vec-backed with position index (original approach).
    Vec {
        tokens: Vec<TreeToken<'db>>,
        pos: usize,
    },
    /// Iterator-backed with lookahead buffer (zero allocation).
    Iter {
        iter: BracerIter<'db>,
        /// Lookahead buffer: [0] = current, [1] = next.
        buffer: [Option<TreeToken<'db>>; 2],
        /// Last consumed token for span tracking.
        last_token: Option<TreeToken<'db>>,
    },
}

/// Parser state for datalit parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn crate::Db,
    source: TokenSource<'db>,
    pub(super) expr_spans: Vec<ast::ParseSpanEntry>,
    /// Next position to hand out, shared with sub-parsers so that one parse
    /// numbers its expressions in a single sequence.
    pub(super) expr_counter: u32,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::text::Text<'db>,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens (Vec-backed).
    pub(super) fn new(db: &'db dyn crate::Db, tokens: Vec<TreeToken<'db>>, source_text: bct::text::Text<'db>) -> Self {
        Parser {
            db,
            source: TokenSource::Vec { tokens, pos: 0 },
            expr_spans: Vec::new(),
            expr_counter: 0,
            had_error: false,
            source_text,
        }
    }

    /// Create a new parser from a BracerIter (iterator-backed, zero allocation).
    pub(super) fn from_branch(db: &'db dyn crate::Db, iter: BracerIter<'db>, source_text: bct::text::Text<'db>) -> Self {
        let mut parser = Parser {
            db,
            source: TokenSource::Iter {
                iter,
                buffer: [None, None],
                last_token: None,
            },
            expr_spans: Vec::new(),
            expr_counter: 0,
            had_error: false,
            source_text,
        };
        parser.fill_iter_buffer();
        parser
    }

    /// Fill the iterator buffer with next non-whitespace tokens.
    fn fill_iter_buffer(&mut self) {
        if let TokenSource::Iter { iter, buffer, .. } = &mut self.source {
            if buffer[0].is_none() {
                buffer[0] = Self::next_non_whitespace(iter);
            }
            if buffer[1].is_none() {
                buffer[1] = Self::next_non_whitespace(iter);
            }
        }
    }

    /// Get next non-whitespace token from iterator.
    fn next_non_whitespace(iter: &mut BracerIter<'db>) -> Option<TreeToken<'db>> {
        loop {
            match iter.next() {
                Some(token) => {
                    if Self::is_non_whitespace(&token) {
                        return Some(token);
                    }
                }
                None => return None,
            }
        }
    }

    /// Check if a token is non-whitespace.
    fn is_non_whitespace(token: &TreeToken<'db>) -> bool {
        match token {
            TreeToken::Token(t) => {
                !matches!(t.kind, TokenKind::Whitespace | TokenKind::Comment)
            }
            TreeToken::Branch { .. } => true,
        }
    }

    /// A sub-parser over a braced branch, carrying on this one's numbering.
    pub(super) fn sub_parser_from_branch(&self, iter: BracerIter<'db>) -> Self {
        let mut sub = Parser::from_branch(self.db, iter, self.source_text());
        sub.expr_counter = self.expr_counter;
        sub
    }

    /// A sub-parser over a slice of tokens, carrying on this one's numbering.
    pub(super) fn sub_parser_from_tokens(&self, tokens: Vec<TreeToken<'db>>) -> Self {
        let mut sub = Parser::new(self.db, tokens, self.source_text());
        sub.expr_counter = self.expr_counter;
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
                if let Some(word) = tok.word_str(self.db) {
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
        match &self.source {
            TokenSource::Vec { tokens, pos } => tokens.get(*pos),
            TokenSource::Iter { buffer, .. } => buffer[0].as_ref(),
        }
    }

    fn peek_next(&self) -> Option<&TreeToken<'db>> {
        match &self.source {
            TokenSource::Vec { tokens, pos } => tokens.get(pos.checked_add(1).X()),
            TokenSource::Iter { buffer, .. } => buffer[1].as_ref(),
        }
    }

    fn prev_end(&self) -> Option<usize> {
        match &self.source {
            TokenSource::Vec { tokens, pos } => {
                tokens.get(pos.checked_sub(1)?).map(|token| token.span().end)
            }
            TokenSource::Iter { last_token, .. } => {
                last_token.as_ref().map(|token| token.span().end)
            }
        }
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        match &mut self.source {
            TokenSource::Vec { tokens, pos } => {
                let token = tokens.get(*pos).cloned();
                if token.is_some() {
                    *pos += 1;
                }
                token
            }
            TokenSource::Iter { iter, buffer, last_token } => {
                // Take from slot 0.
                let result = buffer[0].take();
                // Remember last consumed token.
                *last_token = result.clone();
                // Shift slot 1 to slot 0.
                buffer[0] = buffer[1].take();
                // Fill slot 1 from iterator.
                buffer[1] = Self::next_non_whitespace(iter);
                result
            }
        }
    }

    fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }
}
