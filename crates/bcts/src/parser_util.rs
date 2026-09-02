//! Shared parser utilities for token-based parsing.
//!
//! Provides traits and helpers that can be shared between parsers using the bcts lexer.

use crate::{
    bracer::{BracerIter, TreeToken},
    lexer::{Sigil, TokenKind},
    text::{InternedText, TextSpan},
};

use rmx::prelude::*;

/// Token stream for parser operations.
///
/// Provides basic peek/next operations over a sequence of tokens.
pub trait TokenStream<'db> {
    /// Get a reference to the database.
    fn db(&self) -> &'db dyn crate::Db;

    /// Peek at the current token without consuming.
    fn peek(&self) -> Option<&TreeToken<'db>>;

    /// Consume and return the current token.
    fn next(&mut self) -> Option<TreeToken<'db>>;

    /// Get the source Text for error reporting when no current token.
    fn source_text(&self) -> crate::text::Text<'db>;
}

/// Extension trait providing shared parsing methods.
///
/// Blanket implemented for all types implementing `TokenStream`.
pub trait TokenStreamExt<'db>: TokenStream<'db> {
    /// Check if the current token is a specific sigil.
    fn peek_sigil(&self, sigil: Sigil) -> bool {
        match self.peek() {
            Some(TreeToken::Token(token)) => {
                matches!(token.kind, TokenKind::Sigil(s) if s == sigil)
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
        token.text_span(self.source_text()).X()
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

    /// Parse comma-separated items, reporting whether any comma was seen.
    ///
    /// Returns `(items, had_comma)` where `had_comma` is true if at least
    /// one comma was consumed. This distinguishes `(x)` from `(x,)`.
    fn parse_comma_separated_with_trailing<T>(
        &mut self,
        mut parse_item: impl FnMut(&mut Self) -> T,
    ) -> (Vec<T>, bool)
    where
        Self: Sized,
    {
        let mut items = vec![];
        let mut had_comma = false;
        if self.peek().is_none() {
            return (items, had_comma);
        }
        loop {
            items.push(parse_item(self));
            if !self.eat_sigil(Sigil::Comma) {
                break;
            }
            had_comma = true;
            // Handle trailing comma.
            if self.peek().is_none() {
                break;
            }
        }
        (items, had_comma)
    }
}

// Blanket implementation.
impl<'db, T: TokenStream<'db>> TokenStreamExt<'db> for T {}

/// Check if a word is a numeric literal (decimal or hex).
pub fn is_numeric_literal(word: &str) -> bool {
    if word.starts_with("0x") || word.starts_with("0X") {
        is_digit_run(&word[2..], |c| c.is_ascii_hexdigit())
    } else {
        is_digit_run(word, |c| c.is_ascii_digit())
    }
}

/// Check a run of digits, which may be grouped by underscores.
///
/// An underscore separates digits for a reader and means nothing to the
/// value, so it goes between them: a run begins and ends with a digit. That
/// also leaves `_1` the name it looks like.
pub fn is_digit_run(text: &str, is_digit: impl Fn(char) -> bool) -> bool {
    let mut chars = text.chars();
    let (Some(first), Some(last)) = (chars.next(), text.chars().next_back()) else {
        return false;
    };
    is_digit(first) && is_digit(last) && chars.all(|c| is_digit(c) || c == '_')
}

/// Check a run of decimal digits, which may be grouped by underscores.
pub fn is_decimal_run(text: &str) -> bool {
    is_digit_run(text, |c| c.is_ascii_digit())
}

/// A numeric literal's text with its digit separators removed.
///
/// Underscores mean nothing to the value, so they come off before anything
/// reads the text as a number. Text carrying none is passed through
/// untouched, which is nearly all of it.
pub fn strip_separators(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains('_') {
        std::borrow::Cow::Owned(text.chars().filter(|c| *c != '_').collect())
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// A word carrying the digits of a float and an exponent.
///
/// A word runs to the first character that is neither alphanumeric nor an
/// underscore, so an exponent arrives in one piece or two: `2.5e10` puts
/// `5e10` in a single word, while `2.5e-10` breaks after the marker into
/// `5e`, `-` and `10`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatExponent<'a> {
    /// The whole exponent is in this word, as in the `5e10` of `2.5e10`.
    Complete { digits: &'a str, exponent: &'a str },
    /// The exponent's sign and digits are the tokens after this word, as in
    /// the `5e` of `2.5e-10`.
    Pending { digits: &'a str },
}

impl<'a> FloatExponent<'a> {
    /// The digits before the exponent marker.
    pub fn digits(&self) -> &'a str {
        match self {
            FloatExponent::Complete { digits, .. } => digits,
            FloatExponent::Pending { digits } => digits,
        }
    }
}

/// Read a word as decimal digits followed by an exponent, if it is one.
///
/// `None` for a word with no exponent marker, which is a plain integer or
/// something that is not a number at all, and for one whose marker is not
/// where an exponent could be.
pub fn float_exponent(word: &str) -> Option<FloatExponent<'_>> {
    // A hex literal carries its own `e`s, which are digits rather than a
    // marker.
    if word.starts_with("0x") || word.starts_with("0X") {
        return None;
    }

    let marker = word.find(['e', 'E'])?;
    let (digits, rest) = word.split_at(marker);
    let exponent = &rest[1..];

    if !is_decimal_run(digits) {
        return None;
    }
    // A second marker is not an exponent, it is a name.
    if exponent.contains(['e', 'E']) {
        return None;
    }

    if exponent.is_empty() {
        Some(FloatExponent::Pending { digits })
    } else if is_decimal_run(exponent) {
        Some(FloatExponent::Complete { digits, exponent })
    } else {
        None
    }
}

/// The sign and digits of an exponent whose word ended at its marker.
///
/// Consumes them, so it is for a caller that has already committed to
/// reading a float. `None` when what follows is not an exponent, which
/// leaves the literal malformed rather than meaning something else: a word
/// ending in `e` where a number belongs is not a name.
pub fn eat_exponent_tail<'db, S: TokenStreamExt<'db>>(stream: &mut S) -> Option<String> {
    // The sign is kept as written. It carries no meaning a reader of the
    // text needs, but rewriting someone's source is not this function's
    // business.
    let sign = if stream.eat_sigil(Sigil::Minus) {
        "-"
    } else if stream.eat_sigil(Sigil::Plus) {
        "+"
    } else {
        ""
    };

    let digits = stream.peek_word()?;
    if !is_decimal_run(digits) {
        return None;
    }
    stream.next();

    Some(format!("{sign}{digits}"))
}
