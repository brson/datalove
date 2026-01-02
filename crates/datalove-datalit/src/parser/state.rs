//! Parser state and helper methods.

use rmx::prelude::*;

use bct::{
    bracer::TreeToken,
    text::InternedText,
};

use crate::ast;
use crate::parser_util::TokenStream;
use datalove_diagnostic::DiagnosticBuilder;

/// Parser state for datalit parsing.
pub(super) struct Parser<'db> {
    pub(super) db: &'db dyn crate::Db,
    pub(super) tokens: Vec<TreeToken<'db>>,
    pub(super) pos: usize,
    pub(super) source_text: Option<bct::text::Text<'db>>,
    pub(super) expr_spans: Vec<ast::ParseSpanEntry>,
    pub(super) had_error: bool,
}

impl<'db> Parser<'db> {
    /// Create a new parser with the given tokens.
    pub(super) fn new(
        db: &'db dyn crate::Db,
        tokens: Vec<TreeToken<'db>>,
        source_text: Option<bct::text::Text<'db>>,
    ) -> Self {
        Parser {
            db,
            tokens,
            pos: 0,
            source_text,
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

    /// Peek returning an owned token (cloned) for patterns that need to capture branch content.
    pub(super) fn peek_owned(&self) -> Option<TreeToken<'db>> {
        self.tokens.get(self.pos).cloned()
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

    /// Get source Text for error reporting.
    ///
    /// Try source_text field first, otherwise extract from first token.
    pub(super) fn source_text(&self) -> bct::text::Text<'db> {
        if let Some(text) = self.source_text {
            return text;
        }
        // Try to get from the first token.
        if let Some(token) = self.tokens.first() {
            match token {
                TreeToken::Token(tok) => {
                    let subtext = tok.text(self.db);
                    return subtext.text(self.db);
                }
                TreeToken::Branch(_, iter) => {
                    // Try to find a Token inside the branch.
                    for inner_token in iter.clone() {
                        if let Some(TreeToken::Token(tok)) = inner_token.without_space(self.db) {
                            let subtext = tok.text(self.db);
                            return subtext.text(self.db);
                        }
                    }
                }
            }
        }
        // Last resort: create an empty text as a fallback.
        // This can happen when parsing tokens without source_text (e.g., from datafun parser).
        bct::text::Text::new(self.db, String::new())
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

    /// Create a sub-parser for processing branch content.
    pub(super) fn sub_parser(&self, tokens: Vec<TreeToken<'db>>) -> Parser<'db> {
        Parser::new(self.db, tokens, self.source_text)
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
