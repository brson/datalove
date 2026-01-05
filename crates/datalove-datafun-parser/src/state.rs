//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    lexer::Sigil,
    bracer::TreeToken,
    text::InternedText,
};
use salsa::Accumulator;

use datalove_datafun_ast::ast;
use datalove_datafun_ast::spans::DatafunSpanAccumulator;
use datalove_datalit::parser_util::{TextSpan, TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;

use super::Db;

/// Parser state for datafun parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn Db,
    pub(super) tokens: Vec<TreeToken<'db>>,
    pub(super) pos: usize,
    pub(super) had_error: bool,
    /// Source text for error reporting when no current token.
    source_text: bct::text::Text<'db>,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens.
    pub(super) fn new(db: &'db dyn Db, tokens: Vec<TreeToken<'db>>) -> Self {
        let source_text = Self::extract_source_text(db, &tokens);
        Parser {
            db,
            tokens,
            pos: 0,
            had_error: false,
            source_text,
        }
    }

    /// Extract source Text from tokens, with fallback to empty text.
    fn extract_source_text(db: &'db dyn Db, tokens: &[TreeToken<'db>]) -> bct::text::Text<'db> {
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
        } else if self.pos > 0 {
            // At end of input, return end of last token.
            if let Some(token) = self.tokens.get(self.pos - 1) {
                self.extract_text_span(token).end()
            } else {
                0
            }
        } else {
            0
        }
    }

    /// Get the byte position at end of previous token (after consuming).
    pub(super) fn last_byte_end(&self) -> usize {
        if self.pos > 0 {
            if let Some(token) = self.tokens.get(self.pos - 1) {
                return self.extract_text_span(token).end();
            }
        }
        0
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
        if self.pos < self.tokens.len() && !self.had_error {
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
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<TreeToken<'db>> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn source_text(&self) -> bct::text::Text<'db> {
        self.source_text
    }
}
