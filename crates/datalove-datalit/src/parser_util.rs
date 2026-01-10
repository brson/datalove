//! Shared parser utilities for token-based parsing.
//!
//! Provides traits and helpers that can be shared between datalit and datafun parsers.

use bct::{
    bracer::{BracerIter, TreeToken},
    lexer::{Sigil, TokenKind},
    text::InternedText,
};

use rmx::prelude::*;
use salsa::Database as Db;

// Re-export TextSpan from bct for convenience.
pub use bct::text::TextSpan;

/// Token stream for parser operations.
///
/// Provides basic peek/next operations over a sequence of tokens.
pub trait TokenStream<'db> {
    /// Get a reference to the database.
    fn db(&self) -> &'db dyn Db;

    /// Peek at the current token without consuming.
    fn peek(&self) -> Option<&TreeToken<'db>>;

    /// Consume and return the current token.
    fn next(&mut self) -> Option<TreeToken<'db>>;

    /// Get the source Text for error reporting when no current token.
    fn source_text(&self) -> bct::text::Text<'db>;
}

/// Extension trait providing shared parsing methods.
///
/// Blanket implemented for all types implementing `TokenStream`.
pub trait TokenStreamExt<'db>: TokenStream<'db> {
    /// Check if the current token is a specific sigil.
    fn peek_sigil(&self, sigil: Sigil) -> bool {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind(self.db()), TokenKind::Sigil(s) if s == sigil)
            }
            Some(TreeToken::Branch { sigil: s, .. }) => *s == sigil,
            None => false,
        }
    }

    /// Try to consume a sigil. Returns true if successful.
    fn eat_sigil(&mut self, sigil: Sigil) -> bool {
        if self.peek_sigil(sigil) {
            self.next();
            true
        } else {
            false
        }
    }

    /// Peek at the current token if it's a word, returning the word string.
    fn peek_word(&self) -> Option<&'db str> {
        match self.peek() {
            Some(TreeToken::Token(token)) => token.word_str(self.db()),
            _ => None,
        }
    }

    /// Try to consume a specific word. Returns true if successful.
    fn eat_word(&mut self, word: &str) -> bool {
        if self.peek_word() == Some(word) {
            self.next();
            true
        } else {
            false
        }
    }

    /// Consume a branch if it matches the given sigil, returning its iterator.
    fn eat_branch(&mut self, sigil: Sigil) -> Option<BracerIter<'db>> {
        if self.peek_sigil(sigil) {
            match self.next() {
                Some(TreeToken::Branch { inner, .. }) => Some(inner),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Try to consume any word and return it as InternedText.
    fn eat_name(&mut self) -> Option<InternedText<'db>> {
        if self.peek_word().is_some() {
            match self.next() {
                Some(TreeToken::Token(token)) => {
                    token.word_str(self.db()).map(|word| InternedText::new(self.db(), word.S()))
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Peek returning an owned token (cloned).
    fn peek_owned(&self) -> Option<TreeToken<'db>> {
        self.peek().cloned()
    }

    /// Extract TextSpan from a token, using source_text for the text.
    fn extract_text_span(&self, token: &TreeToken<'db>) -> TextSpan<'db> {
        token.text_span(self.db(), self.source_text()).X()
    }

    /// Get Text and ByteSpan from current position for error reporting.
    ///
    /// Falls back to source_text with empty span if at end of input.
    fn peek_text_span(&self) -> TextSpan<'db> {
        match self.peek() {
            Some(token) => self.extract_text_span(token),
            None => TextSpan::new(self.source_text(), 0..0),
        }
    }

    /// Parse comma-separated items using the provided parsing function.
    ///
    /// Handles trailing commas and empty input. Returns items in order.
    fn parse_comma_separated<T>(&mut self, mut parse_item: impl FnMut(&mut Self) -> T) -> Vec<T>
    where
        Self: Sized,
    {
        let mut items = vec![];
        if self.peek().is_none() {
            return items;
        }
        loop {
            items.push(parse_item(self));
            if !self.eat_sigil(Sigil::Comma) {
                break;
            }
            // Handle trailing comma.
            if self.peek().is_none() {
                break;
            }
        }
        items
    }
}

// Blanket implementation.
impl<'db, T: TokenStream<'db>> TokenStreamExt<'db> for T {}

/// Check if a word is a numeric literal (decimal or hex).
pub fn is_numeric_literal(word: &str) -> bool {
    if word.starts_with("0x") || word.starts_with("0X") {
        // Hex literal: 0x followed by hex digits.
        word.len() > 2 && word[2..].chars().all(|c| c.is_ascii_hexdigit())
    } else {
        // Decimal literal: all digits.
        word.chars().all(|c| c.is_ascii_digit())
    }
}
