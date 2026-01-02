//! Shared parser utilities for token-based parsing.
//!
//! Provides traits and helpers that can be shared between datalit and datafun parsers.

use bct::{
    bracer::TreeToken,
    lexer::{Sigil, TokenKind},
    text::InternedText,
};

use rmx::prelude::*;
use salsa::Database as Db;

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
            Some(TreeToken::Branch(s, _)) => *s == sigil,
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
    fn try_eat_word(&mut self, word: &str) -> bool {
        if self.peek_word() == Some(word) {
            self.next();
            true
        } else {
            false
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

    /// Consume a word and return it as InternedText, panicking if not a word.
    fn need_name(&mut self) -> InternedText<'db> {
        match self.eat_name() {
            Some(name) => name,
            None => panic!("expected name"),
        }
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
