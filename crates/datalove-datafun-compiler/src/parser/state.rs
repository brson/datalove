//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    lexer::Sigil,
    bracer::TreeToken,
    text::InternedText,
};
use salsa::Accumulator;

use crate::ast;
use datalove_datalit::parser_util::{TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;

/// Parser state for datafun parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn crate::Db,
    pub(super) tokens: Vec<TreeToken<'db>>,
    pub(super) pos: usize,
    pub(super) had_error: bool,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens.
    pub(super) fn new(db: &'db dyn crate::Db, tokens: Vec<TreeToken<'db>>) -> Self {
        Parser {
            db,
            tokens,
            pos: 0,
            had_error: false,
        }
    }

    /// Emit both a diagnostic and create a StmtParseError node in one call.
    pub(super) fn emit_stmt_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Statement<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::Statement::ParseError(ast::StmtParseError::new(self.db, text, span, message_text))
    }

    /// Emit both a diagnostic and create an ExprFun with ParseError kind in one call.
    pub(super) fn emit_expr_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::ExprFun<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::ExprFun::new(
            self.db,
            ast::ExprFunKind::ParseError(ast::ExprFunParseError::new(self.db, text, span, message_text))
        )
    }

    /// Consume a specific sigil or panic.
    pub(super) fn need_sigil(&mut self, sigil: Sigil) {
        if !self.eat_sigil(sigil) {
            panic!("expected sigil {}", sigil.as_str());
        }
    }

    /// Check if looking at a colon type hint.
    pub(super) fn peek_colon_type_hint(&self) -> bool {
        self.peek_sigil(Sigil::Colon)
    }

    /// Create a sub-parser for processing branch content.
    pub(super) fn sub_parser(&self, tokens: Vec<TreeToken<'db>>) -> Parser<'db> {
        Parser {
            db: self.db,
            tokens,
            pos: 0,
            had_error: false,
        }
    }

    /// Get source Text for error reporting from the first token.
    pub(super) fn source_text(&self) -> bct::text::Text<'db> {
        if let Some(token) = self.tokens.first() {
            match token {
                TreeToken::Token(tok) => tok.text(self.db).text(self.db),
                TreeToken::Branch(_, iter) => {
                    // Try to find a token inside the branch.
                    for inner in iter.clone() {
                        if let Some(TreeToken::Token(tok)) = inner.without_space(self.db) {
                            return tok.text(self.db).text(self.db);
                        }
                    }
                    bct::text::Text::new(self.db, String::new())
                }
            }
        } else {
            bct::text::Text::new(self.db, String::new())
        }
    }

    /// Extract Text and ByteSpan from a token.
    pub(super) fn extract_text_span(&self, token: &TreeToken<'db>) -> (bct::text::Text<'db>, datalove_diagnostic::ByteSpan) {
        match token {
            TreeToken::Token(tok) => {
                let subtext = tok.text(self.db);
                (subtext.text(self.db), subtext.range(self.db))
            }
            TreeToken::Branch(_, _) => {
                (self.source_text(), 0..0)
            }
        }
    }

    /// Get Text and ByteSpan from current position for error reporting.
    pub(super) fn peek_text_span(&self) -> (bct::text::Text<'db>, datalove_diagnostic::ByteSpan) {
        if let Some(token) = self.peek() {
            self.extract_text_span(token)
        } else {
            (self.source_text(), 0..0)
        }
    }

    /// Create an expression and emit its span as accumulator.
    pub(super) fn create_expr(&mut self, kind: ast::ExprFunKind<'db>, text: bct::text::Text<'db>, span: datalove_diagnostic::ByteSpan) -> ast::ExprFun<'db> {
        use salsa::plumbing::AsId;
        let expr = ast::ExprFun::new(self.db, kind);
        crate::spans::DatafunSpanAccumulator {
            expr_id: expr.as_id(),
            text_id: text.as_id(),
            span,
        }.accumulate(self.db);
        expr
    }

    /// Emit error if tokens remain unconsumed after a successful parse.
    pub(super) fn error_if_not_exhausted(&mut self) {
        if self.pos < self.tokens.len() && !self.had_error {
            self.had_error = true;
            let (text, span) = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("P021")
                .primary_label(text, span, "unexpected token")
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
}
