//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    lexer::{Sigil, TokenKind},
    bracer::{BracerIter, TreeToken},
    text::InternedText,
};
use salsa::Accumulator;

use datalove_datafun_ast::ast;
use datalove_datafun_ast::spans::DatafunSpanAccumulator;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;

use super::Db;

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

/// Parser state for datafun parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn Db,
    source: TokenSource<'db>,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::text::Text<'db>,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens (Vec-backed).
    pub(super) fn new(db: &'db dyn Db, tokens: Vec<TreeToken<'db>>) -> Self {
        let source_text = Self::extract_source_text_from_slice(db, &tokens);
        Parser {
            db,
            source: TokenSource::Vec { tokens, pos: 0 },
            had_error: false,
            source_text,
        }
    }

    /// Create a new parser from a BracerIter (iterator-backed, zero allocation).
    pub(super) fn from_branch(db: &'db dyn Db, iter: BracerIter<'db>) -> Self {
        let source_text = Self::extract_source_text_from_iter(db, &iter);
        let mut parser = Parser {
            db,
            source: TokenSource::Iter {
                iter,
                buffer: [None, None],
                last_token: None,
            },
            had_error: false,
            source_text,
        };
        // Prime the buffer.
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
    fn next_non_whitespace(db: &'db dyn Db, iter: &mut BracerIter<'db>) -> Option<TreeToken<'db>> {
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
    fn is_non_whitespace(db: &'db dyn Db, token: &TreeToken<'db>) -> bool {
        match token {
            TreeToken::Token(t) => {
                !matches!(t.kind(db), TokenKind::Whitespace | TokenKind::Comment)
            }
            TreeToken::Branch(_, _) => true,
        }
    }

    /// Extract source Text from tokens slice.
    fn extract_source_text_from_slice(db: &'db dyn Db, tokens: &[TreeToken<'db>]) -> bct::text::Text<'db> {
        if let Some(token) = tokens.first() {
            match token {
                TreeToken::Token(tok) => tok.text(db).text(db),
                TreeToken::Branch(_, iter) => {
                    for inner in iter.clone() {
                        if let Some(TreeToken::Token(tok)) = inner.without_space(db) {
                            return tok.text(db).text(db);
                        }
                    }
                    bct::text::Text::new(db, String::new())
                }
            }
        } else {
            bct::text::Text::new(db, String::new())
        }
    }

    /// Extract source Text from BracerIter.
    fn extract_source_text_from_iter(db: &'db dyn Db, iter: &BracerIter<'db>) -> bct::text::Text<'db> {
        if let Some(ts) = iter.text_span() {
            return ts.text;
        }
        bct::text::Text::new(db, String::new())
    }

    /// Get the source text for this parser.
    pub(super) fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }

    /// Emit both a diagnostic and create a StmtParseError node in one call.
    pub(super) fn emit_stmt_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Statement<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.clone(), label)
            .emit_parse();
        ast::Statement::ParseError(ast::StmtParseError::new(self.db, ts.text, ts.span, message_text))
    }

    /// Emit both a diagnostic and create an ExprFun with ParseError kind in one call.
    pub(super) fn emit_expr_error(
        &mut self,
        ts: TextSpan<'db>,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprFun<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(ts.clone(), label)
            .emit_parse();
        ast::ExprFun::new(
            self.db,
            ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(self.db, ts.text, ts.span, message_text))
        )
    }

    /// Check if looking at a colon type hint.
    pub(super) fn peek_colon_type_hint(&self) -> bool {
        self.peek_sigil(Sigil::Colon)
    }

    /// Get the byte position at start of current token (or end of last token).
    pub(super) fn current_byte_pos(&self) -> usize {
        if let Some(token) = self.peek() {
            self.extract_text_span(token).start()
        } else {
            // At end of input, return end of last token.
            match &self.source {
                TokenSource::Vec { tokens, pos } => {
                    if *pos > 0 {
                        if let Some(token) = tokens.get(pos - 1) {
                            return self.extract_text_span(token).end();
                        }
                    }
                    0
                }
                TokenSource::Iter { last_token, .. } => {
                    if let Some(token) = last_token {
                        self.extract_text_span(token).end()
                    } else {
                        0
                    }
                }
            }
        }
    }

    /// Get the byte position at end of previous token (after consuming).
    pub(super) fn last_byte_end(&self) -> usize {
        match &self.source {
            TokenSource::Vec { tokens, pos } => {
                if *pos > 0 {
                    if let Some(token) = tokens.get(pos - 1) {
                        return self.extract_text_span(token).end();
                    }
                }
                0
            }
            TokenSource::Iter { last_token, .. } => {
                if let Some(token) = last_token {
                    self.extract_text_span(token).end()
                } else {
                    0
                }
            }
        }
    }

    /// Create an expression and emit its span as accumulator.
    pub(super) fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, ts: TextSpan<'db>) -> ast::ExprFun<'db> {
        use salsa::plumbing::AsId;
        let expr = ast::ExprFun::new(self.db, kind);
        DatafunSpanAccumulator {
            expr_id: expr.as_id(),
            text_id: ts.text.as_id(),
            span: ts.span,
        }.accumulate(self.db);
        expr
    }

    /// Emit error if tokens remain unconsumed after a successful parse.
    pub(super) fn error_if_not_exhausted(&mut self) {
        if self.peek().is_some() && !self.had_error {
            self.had_error = true;
            let ts = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("P021")
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
