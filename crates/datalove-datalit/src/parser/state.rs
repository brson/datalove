//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    lexer::TokenKind,
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;

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
                buffer[0] = Self::next_non_whitespace(self.db, iter);
            }
            if buffer[1].is_none() {
                buffer[1] = Self::next_non_whitespace(self.db, iter);
            }
        }
    }

    /// Get next non-whitespace token from iterator.
    fn next_non_whitespace(db: &'db dyn crate::Db, iter: &mut BracerIter<'db>) -> Option<TreeToken<'db>> {
        loop {
            match iter.next() {
                Some(token) => {
                    if Self::is_non_whitespace(db, &token) {
                        return Some(token);
                    }
                }
                None => return None,
            }
        }
    }

    /// Check if a token is non-whitespace.
    fn is_non_whitespace(db: &'db dyn crate::Db, token: &TreeToken<'db>) -> bool {
        match token {
            TreeToken::Token(t) => {
                !matches!(t.kind(db), TokenKind::Whitespace | TokenKind::Comment)
            }
            TreeToken::Branch(_, _) => true,
        }
    }

    /// Get current position (only valid for Vec-backed parser).
    pub(super) fn pos(&self) -> usize {
        match &self.source {
            TokenSource::Vec { pos, .. } => *pos,
            TokenSource::Iter { .. } => 0, // Not meaningful for iterator-backed.
        }
    }

    /// Peek at the next token (one ahead of current) for lookahead.
    pub(super) fn peek_next(&self) -> Option<&TreeToken<'db>> {
        match &self.source {
            TokenSource::Vec { tokens, pos } => tokens.get(pos + 1),
            TokenSource::Iter { buffer, .. } => buffer[1].as_ref(),
        }
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
        ast::Expr::ParseError(ast::ExprParseError::new(self.db, ts.text, ts.span, message_text))
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
        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, ts.text, ts.span, message_text))
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
                buffer[1] = Self::next_non_whitespace(self.db, iter);
                result
            }
        }
    }

    fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }
}
