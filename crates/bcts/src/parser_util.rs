//! Shared parser utilities for token-based parsing.
//!
//! Provides traits and helpers that can be shared between parsers using the bcts lexer.

use crate::{
    bracer::{BracerIter, TreeToken},
    lexer::{Sigil, TokenKind},
    text::{InternedText, TextSpan},
};

use rmx::std::ops::Range;

use rmx::prelude::*;

/// Token stream for parser operations.
///
/// Provides basic peek/next operations over a sequence of tokens.
pub trait TokenStream<'db> {
    /// Get a reference to the database.
    fn db(&self) -> &'db dyn crate::Db;

    /// Peek at the current token without consuming.
    fn peek(&self) -> Option<&TreeToken<'db>>;

    /// Peek at the token after the current one.
    fn peek_next(&self) -> Option<&TreeToken<'db>>;

    /// Consume and return the current token.
    fn next(&mut self) -> Option<TreeToken<'db>>;

    /// The end of the last token consumed, absent at the start of a run.
    ///
    /// Whitespace never reaches a parser, so this is the only evidence that
    /// the token at the cursor was written against the one before it.
    fn prev_end(&self) -> Option<usize>;

    /// Get the source Text for error reporting when no current token.
    fn source_text(&self) -> crate::text::Text<'db>;

    /// The text of the chunk the tokens were lexed from, which their spans
    /// index.
    fn text(&self) -> &'db str;

    /// Intern some of that text.
    ///
    /// A stream that sees the same names many times can remember what it has
    /// interned; salsa hashes and probes its map for each one it is asked.
    fn intern(&mut self, text: &'db str) -> InternedText<'db> {
        InternedText::new(self.db(), text)
    }
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
            Some(TreeToken::Token(token)) => token.word_str(self.text()),
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
                Some(TreeToken::Branch { inner, .. }) => Some(*inner),
                _ => None,
            }
        } else {
            None
        }
    }

    /// Try to consume a name and return it as InternedText.
    ///
    /// A name is a word that begins with a letter or `_`. One that begins
    /// with a digit is a number, or a number with letters on it, and is
    /// refused here rather than taken for a field called `1x`.
    fn eat_name(&mut self) -> Option<InternedText<'db>> {
        if !is_identifier(self.peek_word()?) {
            return None;
        }
        match self.next() {
            Some(TreeToken::Token(token)) => {
                let text = token.text(self.text());
                Some(self.intern(text))
            }
            // `peek_word` just said the token at the cursor is a word.
            _ => bug!(),
        }
    }

    /// Peek returning an owned token (cloned).
    fn peek_owned(&self) -> Option<TreeToken<'db>> {
        self.peek().cloned()
    }

    /// Whether the token at the cursor was written against the one before it.
    ///
    /// False at the start of a run, where there is nothing for it to be
    /// written against. A comment between two tokens leaves a gap, so it
    /// separates them as a space would.
    fn glued_left(&self) -> bool {
        match (self.prev_end(), self.peek()) {
            (Some(end), Some(token)) => end == token.span().start,
            _ => false,
        }
    }

    /// Whether the token at the cursor was written against the one after it.
    fn glued_right(&self) -> bool {
        match (self.peek(), self.peek_next()) {
            (Some(token), Some(next)) => token.span().end == next.span().start,
            _ => false,
        }
    }

    /// Whether the operator at the cursor is spaced as an infix one.
    ///
    /// An operator written against both its neighbours, or apart from both,
    /// goes between them. One written against only the token after it is a
    /// prefix operator, and one written against only the token before it is a
    /// postfix operator. The caller asks this only where an operand precedes,
    /// since without one there is nothing for an infix operator to join.
    fn is_infix_spacing(&self) -> bool {
        self.glued_left() == self.glued_right()
    }

    /// Extract TextSpan from a token, using source_text for the text.
    fn extract_text_span(&self, token: &TreeToken<'db>) -> TextSpan<'db> {
        token.text_span(self.source_text()).X()
    }

    /// Extract the TextSpan covering a nonempty run of tokens.
    fn extract_group_text_span(&self, tokens: &[TreeToken<'db>]) -> TextSpan<'db> {
        let start = tokens.first().X().span().start;
        let end = tokens.last().X().span().end;
        TextSpan::new(self.source_text(), start..end)
    }

    /// Get Text and ByteSpan from current position for error reporting.
    ///
    /// At the end of input this is an empty span just after the last token
    /// consumed, which is where the missing token would have gone. A run is
    /// usually a single line or bracket, so without that the error would point
    /// at the start of the file.
    fn peek_text_span(&self) -> TextSpan<'db> {
        match self.peek() {
            Some(token) => self.extract_text_span(token),
            None => {
                let end = self.prev_end().unwrap_or(0);
                TextSpan::new(self.source_text(), end..end)
            }
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

/// Whether a word begins a number.
///
/// A name begins with a letter or an underscore, so a word beginning with a
/// digit is an attempt at a number and could be nothing else. That is what
/// lets `1u8` be reported as a number carrying letters rather than as an
/// unknown name.
pub fn is_number_word(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_digit())
}

/// A numeric literal's radix, which its prefix decides.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Radix {
    Dec,
    Hex,
}

/// What was wrong with a numeric literal.
///
/// The tokens are consumed whichever of these happened, so that a parser
/// makes one complaint about the number rather than meeting its pieces again
/// as something else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumberError {
    /// Digits that are not a run, as in `1_`.
    Digits,
    /// A float with a space written into it, as in `1 . 5`.
    FloatSpaced,
    /// A `.` against the digits with no fraction after it.
    FractionMissing,
    /// An exponent with a space written into it, as in `2.5e - 10`.
    ExponentSpaced,
    /// An exponent marker with no digits after it.
    ExponentMissing,
}

/// One numeric literal, read as far as it was written without spaces.
///
/// The pieces are kept as they were written, separators and all, since what
/// a reader of the source typed is what a diagnostic has to quote and what
/// the value is read from later.
#[derive(Clone, Debug)]
pub struct Number {
    pub negative: bool,
    pub radix: Radix,
    /// Whether a fraction or an exponent makes this a float.
    pub float: bool,
    /// The leading word, its radix prefix included and any suffix taken off.
    pub digits: String,
    /// The word after the `.`, which carries an in-word exponent with it.
    pub fraction: Option<String>,
    /// An exponent's sign and digits, where they were tokens of their own.
    pub exponent_tail: Option<String>,
    /// Letters written onto the digits, meaning whatever the caller says.
    pub suffix: Option<String>,
    pub span: Range<usize>,
    pub error: Option<NumberError>,
}

impl Number {
    /// What to say about a number that was not written as one.
    ///
    /// The message and the label it goes under. Built here rather than at
    /// each parser so that the two languages say the same thing: a reader
    /// moving between them is reading one literal syntax and should not have
    /// to learn its complaints twice.
    pub fn complaint(&self) -> Option<(String, String)> {
        let error = self.error.C()?;
        let message = match error {
            NumberError::Digits => S("a separator goes between two digits"),
            NumberError::FloatSpaced => S("a float is written without spaces in it"),
            NumberError::FractionMissing => S("expected the digits of a float"),
            NumberError::ExponentSpaced => S("an exponent is written without spaces in it"),
            NumberError::ExponentMissing => S("expected the digits of an exponent"),
        };
        let label = match error {
            NumberError::Digits => S("not a run of digits"),
            NumberError::FloatSpaced | NumberError::ExponentSpaced => {
                fmt!("write it as `{}`", self.text())
            }
            NumberError::FractionMissing => S("expected the digits of a float"),
            NumberError::ExponentMissing => S("expected the digits of an exponent"),
        };
        Some((message, label))
    }

    /// The literal as it was written, without its suffix.
    pub fn text(&self) -> String {
        let sign = if self.negative { "-" } else { "" };
        let digits = &self.digits;
        let fraction = match &self.fraction {
            Some(fraction) => fmt!(".{fraction}"),
            None => String::new(),
        };
        let exponent = match &self.exponent_tail {
            Some(tail) => tail.as_str(),
            None => "",
        };
        fmt!("{sign}{digits}{fraction}{exponent}")
    }
}

/// Read one numeric literal, as far as it was written without spaces.
///
/// `None` where the cursor is not on a number, having consumed nothing. A
/// leading `-` is part of the number only where it was written against the
/// digits; whether it is ever offered one is the grammar's business, since a
/// language with a prefix operator claims it before reaching here.
pub fn eat_number<'db, S: TokenStreamExt<'db>>(stream: &mut S) -> Option<Number> {
    let text = stream.text();

    // A hex literal is a bit pattern and takes no sign, so a `-` against one
    // is left for the caller: an operator where the language has one.
    let negative = stream.peek_sigil(Sigil::Minus)
        && stream.glued_right()
        && stream.peek_next()
            .and_then(|token| number_word(token, text))
            .is_some_and(|word| !is_hex_word(word));

    // Decided before anything is consumed, so that a `-` that turns out not
    // to be a sign is left where the caller can read it as an operator.
    let word_token = if negative { stream.peek_next() } else { stream.peek() }?.clone();
    let word = number_word(&word_token, text)?.S();
    let start = stream.peek()?.span().start;

    if negative {
        stream.next();
    }
    stream.next();

    let mut number = Number {
        negative,
        radix: Radix::Dec,
        float: false,
        digits: word.C(),
        fraction: None,
        exponent_tail: None,
        suffix: None,
        span: start..word_token.span().end,
        error: None,
    };

    read_number_tail(stream, &word, &mut number);
    number.span.end = stream.prev_end().unwrap_or(number.span.end);

    Some(number)
}

/// Read what a number's leading word carries and what follows it.
fn read_number_tail<'db, S: TokenStreamExt<'db>>(
    stream: &mut S,
    word: &str,
    number: &mut Number,
) {
    if word.starts_with("0x") || word.starts_with("0X") {
        let (digits, suffix) = split_suffix(&word[2..], |c| c.is_ascii_hexdigit());
        number.radix = Radix::Hex;
        number.digits = fmt!("{}{}", &word[..2], digits);
        number.suffix = suffix;
        if !is_digit_run(digits, |c| c.is_ascii_hexdigit()) {
            number.error = Some(NumberError::Digits);
        }
        // A hex literal has no fraction, so `0x1.2` is a hex and then a dot.
        return;
    }

    // An exponent on the leading digits means there is no fraction.
    match float_exponent(word) {
        Some(FloatExponent::Complete { .. }) => {
            number.float = true;
            return;
        }
        Some(FloatExponent::Pending { .. }) => {
            number.float = true;
            let tail = eat_exponent_tail(stream);
            number.exponent_tail = tail.text;
            number.error = tail.error;
            return;
        }
        None => {}
    }

    let (digits, suffix) = split_suffix(word, |c| c.is_ascii_digit());
    number.digits = digits.S();
    number.suffix = suffix;
    if !is_decimal_run(digits) {
        number.error = Some(NumberError::Digits);
        return;
    }
    // Letters end the number, so `1px.5` is `1px` and then a dot.
    if number.suffix.is_some() {
        return;
    }

    read_fraction(stream, number);
}

/// Read the `.` and the fraction after a number's digits, if they are there.
fn read_fraction<'db, S: TokenStreamExt<'db>>(stream: &mut S, number: &mut Number) {
    if !stream.peek_sigil(Sigil::Dot) {
        return;
    }
    let dot_glued_left = stream.glued_left();
    let dot_glued_right = stream.glued_right();

    // A dot written against the digits commits to a float, whatever the word
    // after it says. One with a space before it does so only where digits
    // follow, since `1 .foo` is a field read written apart from its base and
    // `1 . 5` is a float written apart from itself.
    let next_is_word = matches!(
        stream.peek_next(),
        Some(TreeToken::Token(token)) if token.kind == TokenKind::Word
    );
    let next_is_number = stream
        .peek_next()
        .and_then(|token| number_word(token, stream.text()))
        .is_some();

    if !next_is_word {
        if dot_glued_left && stream.peek_next().is_none() {
            stream.next();
            number.float = true;
            number.error = Some(NumberError::FractionMissing);
        }
        return;
    }
    if !dot_glued_left && !next_is_number {
        return;
    }

    stream.next();
    let fraction = stream.peek_word().X().S();
    stream.next();
    number.float = true;

    if !(dot_glued_left && dot_glued_right) {
        number.error = Some(NumberError::FloatSpaced);
    }

    match float_exponent(&fraction) {
        Some(FloatExponent::Complete { .. }) => {
            number.fraction = Some(fraction);
        }
        Some(FloatExponent::Pending { .. }) => {
            number.fraction = Some(fraction);
            let tail = eat_exponent_tail(stream);
            number.exponent_tail = tail.text;
            number.error = number.error.C().or(tail.error);
        }
        None => {
            let (digits, suffix) = split_suffix(&fraction, |c| c.is_ascii_digit());
            if digits.is_empty() || !is_decimal_run(digits) {
                number.error = number.error.C().or(Some(NumberError::FractionMissing));
                return;
            }
            number.fraction = Some(digits.S());
            number.suffix = suffix;
        }
    }
}

/// A word's leading run of digits and the letters written onto it.
fn split_suffix(word: &str, is_digit: impl Fn(char) -> bool) -> (&str, Option<String>) {
    let end = word
        .find(|c: char| !is_digit(c) && c != '_')
        .unwrap_or(word.len());
    let (digits, suffix) = word.split_at(end);
    (digits, (!suffix.is_empty()).then(|| suffix.S()))
}

/// The text of a word that begins with a digit, which is a number attempt.
fn number_word<'a>(token: &TreeToken<'_>, chunk_text: &'a str) -> Option<&'a str> {
    let TreeToken::Token(token) = token else {
        return None;
    };
    let word = token.word_str(chunk_text)?;
    is_number_word(word).then_some(word)
}

/// Whether a word is an identifier: one that begins with a letter or `_`.
///
/// A word is letters, digits and underscores, so this is the same as saying
/// it does not begin with a digit, of any script.
pub fn is_identifier(word: &str) -> bool {
    word.starts_with(|c: char| c.is_alphabetic() || c == '_')
}

/// Whether a word is written as a hex literal.
pub fn is_hex_word(word: &str) -> bool {
    word.starts_with("0x") || word.starts_with("0X")
}

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
/// reading a float. A word ending in `e` where a number belongs is not a
/// name, so what follows is either the exponent or a mistake, and the pieces
/// of a mistake are consumed too rather than left to mean something else.
fn eat_exponent_tail<'db, S: TokenStreamExt<'db>>(stream: &mut S) -> ExponentTail {
    let mut spaced = !stream.glued_left();

    // The sign is kept as written. It carries no meaning a reader of the
    // text needs, but rewriting someone's source is not this function's
    // business.
    let sign = if stream.peek_sigil(Sigil::Minus) {
        stream.next();
        "-"
    } else if stream.peek_sigil(Sigil::Plus) {
        stream.next();
        "+"
    } else {
        ""
    };

    if !sign.is_empty() && !stream.glued_left() {
        spaced = true;
    }

    let Some(digits) = stream.peek_word() else {
        return ExponentTail { text: None, error: Some(NumberError::ExponentMissing) };
    };
    if !is_decimal_run(digits) {
        return ExponentTail { text: None, error: Some(NumberError::ExponentMissing) };
    }
    let digits = digits.S();
    stream.next();

    // The text is kept even where the spacing was wrong, since the complaint
    // shows the reader the number they meant to write.
    ExponentTail {
        text: Some(fmt!("{sign}{digits}")),
        error: spaced.then_some(NumberError::ExponentSpaced),
    }
}

/// An exponent read off the stream, and what was wrong with how it was written.
struct ExponentTail {
    text: Option<String>,
    error: Option<NumberError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bracer::bracer;
    use crate::input::Source;
    use crate::lexer::lex_chunk;
    use crate::source_map::basic_source_map;
    use crate::text::Text;

    /// A token stream over a whole source, for reading numbers off it.
    struct TestStream<'db> {
        db: &'db dyn crate::Db,
        tokens: Vec<TreeToken<'db>>,
        pos: usize,
        text: Text<'db>,
    }

    impl<'db> TestStream<'db> {
        fn new(db: &'db crate::Database, source_text: &str) -> Self {
            let source = Source::new(db, source_text.S());
            let chunk = basic_source_map(db, source);
            let tokens = bracer(db, lex_chunk(db, chunk))
                .iter(db)
                .collect();
            TestStream { db, tokens, pos: 0, text: chunk.text(db) }
        }
    }

    impl<'db> TokenStream<'db> for TestStream<'db> {
        fn db(&self) -> &'db dyn crate::Db {
            self.db
        }

        fn peek(&self) -> Option<&TreeToken<'db>> {
            self.tokens.get(self.pos)
        }

        fn peek_next(&self) -> Option<&TreeToken<'db>> {
            self.tokens.get(self.pos.checked_add(1).X())
        }

        fn next(&mut self) -> Option<TreeToken<'db>> {
            let token = self.tokens.get(self.pos).cloned();
            if token.is_some() {
                self.pos = self.pos.checked_add(1).X();
            }
            token
        }

        fn prev_end(&self) -> Option<usize> {
            self.tokens.get(self.pos.checked_sub(1)?).map(|token| token.span().end)
        }

        fn source_text(&self) -> Text<'db> {
            self.text
        }

        fn text(&self) -> &'db str {
            self.text.as_str(self.db)
        }
    }

    /// A number read off the front of the text, as `text/error` or `-` for
    /// nothing that was a number at all.
    fn read(db: &crate::Database, source_text: &str) -> String {
        let mut stream = TestStream::new(db, source_text);
        let Some(number) = eat_number(&mut stream) else {
            return S("-");
        };
        let kind = match (number.radix, number.float) {
            (Radix::Hex, _) => "hex",
            (Radix::Dec, true) => "float",
            (Radix::Dec, false) => "int",
        };
        let suffix = match &number.suffix {
            Some(suffix) => fmt!("+{suffix}"),
            None => String::new(),
        };
        let error = match &number.error {
            Some(error) => fmt!("/{error:?}"),
            None => String::new(),
        };
        fmt!("{kind} {}{suffix}{error}", number.text())
    }

    #[test]
    fn test_eat_number() {
        let ref db = crate::Database::default();

        // The pieces of a number, written together.
        assert_eq!(read(db, "1"), "int 1");
        assert_eq!(read(db, "1_000"), "int 1_000");
        assert_eq!(read(db, "1.5"), "float 1.5");
        assert_eq!(read(db, "-1.5"), "float -1.5");
        assert_eq!(read(db, "0x1e5"), "hex 0x1e5");
        assert_eq!(read(db, "1.0e300"), "float 1.0e300");
        assert_eq!(read(db, "2.5e-10"), "float 2.5e-10");
        assert_eq!(read(db, "2.5e+10"), "float 2.5e+10");
        assert_eq!(read(db, "-1e-7"), "float -1e-7");

        // A space in the middle of one is reported, and the number it was
        // meant to be is what the complaint quotes.
        assert_eq!(read(db, "1 . 5"), "float 1.5/FloatSpaced");
        assert_eq!(read(db, "1. 5"), "float 1.5/FloatSpaced");
        assert_eq!(read(db, "1 .5"), "float 1.5/FloatSpaced");
        assert_eq!(read(db, "2.5e - 10"), "float 2.5e-10/ExponentSpaced");
        assert_eq!(read(db, "2.5e"), "float 2.5e/ExponentMissing");
        assert_eq!(read(db, "1."), "float 1/FractionMissing");
        assert_eq!(read(db, "1_"), "int 1_/Digits");

        // A sign belongs to the number only where it was written against it.
        assert_eq!(read(db, "- 5"), "-");
        assert_eq!(read(db, "-x"), "-");
        // A hex literal takes no sign, so the `-` is left for the caller.
        assert_eq!(read(db, "-0x10"), "-");

        // Letters on the end are reported, not interpreted.
        assert_eq!(read(db, "1u8"), "int 1+u8");
        assert_eq!(read(db, "50pct"), "int 50+pct");
        assert_eq!(read(db, "1.5px"), "float 1.5+px");

        // A dot that no fraction follows belongs to whoever wants it.
        assert_eq!(read(db, "1.foo"), "float 1/FractionMissing");
        assert_eq!(read(db, "1 .foo"), "int 1");
        assert_eq!(read(db, "x"), "-");
    }

    #[test]
    fn test_gluing() {
        let ref db = crate::Database::default();
        let glued = |text: &str| {
            let mut stream = TestStream::new(db, text);
            stream.next();
            (stream.glued_left(), stream.glued_right(), stream.is_infix_spacing())
        };

        // An operator against both its neighbours or against neither goes
        // between them; one against a single side does not.
        assert_eq!(glued("a-b"), (true, true, true));
        assert_eq!(glued("a - b"), (false, false, true));
        assert_eq!(glued("a -b"), (false, true, false));
        assert_eq!(glued("a- b"), (true, false, false));
        // A comment between two tokens separates them as a space would.
        assert_eq!(glued("a/*c*/-b"), (false, true, false));
    }
}
