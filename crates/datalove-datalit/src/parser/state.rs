//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    bracer::TreeToken,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::{TokenStream, TokenStreamExt};
use datalove_diagnostic::DiagnosticBuilder;

/// Parser state for datalit parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn crate::Db,
    pub(super) tokens: Vec<TreeToken<'db>>,
    pub(super) pos: usize,
    pub(super) expr_spans: Vec<ast::ParseSpanEntry>,
    pub(super) had_error: bool,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens.
    pub(super) fn new(db: &'db dyn crate::Db, tokens: Vec<TreeToken<'db>>) -> Self {
        Parser {
            db,
            tokens,
            pos: 0,
            expr_spans: Vec::new(),
            had_error: false,
        }
    }

    /// Get current position.
    pub(super) fn pos(&self) -> usize {
        self.pos
    }

    /// Take the accumulated expression spans.
    pub(super) fn take_expr_spans(&mut self) -> Vec<ast::ParseSpanEntry> {
        std::mem::take(&mut self.expr_spans)
    }

    /// Emit both a diagnostic and create an ExprParseError node in one call.
    pub(super) fn emit_expr_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::Expr<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::Expr::ParseError(ast::ExprParseError::new(self.db, text, span, message_text))
    }

    /// Emit both a diagnostic and create a TypeHintParseError node in one call.
    pub(super) fn emit_type_hint_error(
        &mut self,
        text: bct::text::Text<'db>,
        span: datalove_diagnostic::ByteSpan,
        message: &str,
        code: &str,
        label: &str,
    ) -> ast::TypeHint<'db> {
        self.had_error = true;
        let message_text = InternedText::new(self.db, message.S());
        DiagnosticBuilder::error(self.db, message)
            .code(code)
            .primary_label(text, span.clone(), label)
            .emit_parse();
        ast::TypeHint::ParseError(ast::TypeHintParseError::new(self.db, text, span, message_text))
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
        if self.pos < self.tokens.len() && !self.had_error {
            self.had_error = true;
            let (text, span) = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after expression")
                .code("D021")
                .primary_label(text, span, "unexpected token")
                .emit_parse();
        }
    }

    /// Emit error if tokens remain unconsumed after a successful type hint parse.
    ///
    /// Only emits if the parser succeeded (no prior errors). This catches
    /// both parser bugs and user syntax errors.
    pub(super) fn error_if_not_exhausted_type_hint(&mut self) {
        if self.pos < self.tokens.len() && !self.had_error {
            self.had_error = true;
            let (text, span) = self.peek_text_span();
            DiagnosticBuilder::error(self.db, "unexpected token after type")
                .code("D022")
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

    fn source_text(&self) -> bct::text::Text<'db> {
        if let Some(token) = self.tokens.first() {
            match token {
                TreeToken::Token(tok) => {
                    return tok.text(self.db).text(self.db);
                }
                TreeToken::Branch(_, iter) => {
                    // Try to find a Token inside the branch.
                    for inner_token in iter.clone() {
                        if let Some(TreeToken::Token(tok)) = inner_token.without_space(self.db) {
                            return tok.text(self.db).text(self.db);
                        }
                    }
                }
            }
        }
        // Empty token list - create empty text as fallback.
        bct::text::Text::new(self.db, String::new())
    }
}
