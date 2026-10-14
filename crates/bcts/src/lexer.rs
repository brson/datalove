use rmx::prelude::*;

use rmx::std::ops::Range;
use rmx::std::{iter, mem};


use crate::input::Source;

/// One lexed token.
///
/// A plain value rather than a tracked struct. Tokens are read straight out of
/// the `Vec` in a [`Lexed`] and never looked up by identity, so the salsa id
/// and page slot each one used to carry paid for nothing.
///
/// Whitespace and comments are not tokens. Every reader skipped them, and the
/// one thing in them anything needed, whether a line ended there, is kept on
/// the token after them. Whether two tokens were written against each other
/// is in their spans.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct Token {
    /// Where the token is in its source, as `u32`s: halving a token is a
    /// third less memory for everything that reads one.
    start: u32,
    end: u32,
    pub kind: TokenKind,
    /// Whether whitespace between this token and the one before it held a
    /// newline. A newline inside a block comment does not count.
    pub newline_before: bool,
}

#[derive(Copy, Clone, Debug, Hash, salsa::SalsaValue)]
#[derive(Eq, PartialEq)]
pub enum TokenKind {
    Word,
    Sigil(Sigil),
    String,
    Error,
}

#[derive(Copy, Clone, Debug, Hash, salsa::SalsaValue)]
#[derive(Eq, PartialEq)]
#[derive(enum_iterator::Sequence)]
pub enum Sigil {
    // Three-character sigils (must come before two-character variants).
    PlusQuestionEquals,
    MinusQuestionEquals,
    StarQuestionEquals,
    SlashQuestionEquals,
    PlusExclamationEquals,
    MinusExclamationEquals,
    StarExclamationEquals,
    SlashExclamationEquals,

    // Earmuff braces (two-character, before single-char variants).
    ParenPipeOpen,    // "(|"
    ParenPipeClose,   // "|)"
    BracePipeOpen,    // "{|"
    BracePipeClose,   // "|}"
    BracketPipeOpen,  // "[|"
    BracketPipeClose, // "|]"
    AnglePipeOpen,    // "<|"
    AnglePipeClose,   // "|>"

    // Sigil-brace opens (two-character, before single-char variants).
    PercentBraceOpen, // "%{"
    HashBraceOpen,    // "#{"

    // Two-character sigils (must come before single-character variants).
    ColonDash,
    PlusQuestion,
    MinusQuestion,
    StarQuestion,
    SlashQuestion,
    PlusExclamation,
    MinusExclamation,
    StarExclamation,
    SlashExclamation,
    PlusEquals,
    MinusEquals,
    StarEquals,
    SlashEquals,
    DotDot,
    DotLess,
    DotGreater,
    LessEquals,
    GreaterEquals,
    EqualsEquals,
    ExclamationEquals,

    // Single-character sigils.
    Dot,
    Comma,
    Semicolon,
    Plus,
    Minus,
    Star,
    SlashForward,
    Equals,
    AngleOpen,
    AngleClose,
    Pipe,
    Question,
    Exclamation,
    Colon,
    Hash,
    At,
    Percent,
    ParenOpen,
    ParenClose,
    BraceOpen,
    BraceClose,
    BracketOpen,
    BracketClose,
    Dollar,
    Tilde,
}

/// Which sigils can begin with each byte, and whether any can.
///
/// The tokenizer asked two questions of the sigil set for every character it
/// looked at: "could a sigil start here" and, if so, "which one is it". Both were
/// a linear scan of all seventy-one variants calling `as_str` on each, and
/// `Sigil::as_str` was 7.5% of the time spent lexing. Both are an index now.
struct SigilTable {
    /// Whether any sigil begins with this byte.
    starts: [bool; 256],
    /// What an ASCII byte begins, so the tokenizer classifies one with a
    /// lookup rather than asking `char` about it; see `Class`.
    class: [Class; 256],
    /// The sigils beginning with this byte, and their spellings, in
    /// declaration order. The spelling is kept beside the sigil because asking
    /// `as_str` for it, once per candidate per sigil lexed, was still 1.4% of
    /// compiling a data file.
    ///
    /// Declaration order is load-bearing: `eat_sigil` takes the first match, and
    /// the longer sigils are declared before the shorter ones they begin with, so
    /// `+?=` is found before `+`. Grouping by first byte keeps that order among
    /// the candidates, which is the same order the full scan saw them in.
    by_first_byte: [Vec<(Sigil, &'static str)>; 256],
}

impl SigilTable {
    fn build() -> Self {
        let mut starts = [false; 256];
        let mut by_first_byte: [Vec<(Sigil, &'static str)>; 256] = std::array::from_fn(|_| Vec::new());
        for sigil in enum_iterator::all::<Sigil>() {
            let spelling = sigil.as_str();
            let first = spelling.as_bytes()[0];
            assert!(first.is_ascii(), "a sigil starting outside ASCII: {:?}", sigil);
            starts[first as usize] = true;
            by_first_byte[first as usize].push((sigil, spelling));
        }
        let class = std::array::from_fn(|byte| {
            let byte = byte as u8;
            let ch = byte as char;
            match () {
                _ if !byte.is_ascii() => Class::NonAscii,
                _ if byte == b'\n' => Class::Newline,
                _ if ch.is_whitespace() => Class::Space,
                _ if ch.is_ascii_alphanumeric() || byte == b'_' => Class::Word,
                _ if byte == b'"' => Class::Quote,
                _ if starts[byte as usize] => Class::Sigil,
                _ => Class::Other,
            }
        });
        Self { starts, class, by_first_byte }
    }

    /// The sigils that could begin with `byte`, longest first.
    fn starting_with(&self, byte: u8) -> &[(Sigil, &'static str)] {
        &self.by_first_byte[byte as usize]
    }
}

/// What a byte begins.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Class {
    Space,
    Newline,
    Word,
    Sigil,
    /// The `"` that opens a string.
    Quote,
    /// An ASCII byte that begins nothing, so an error token.
    Other,
    /// The first byte of a character outside ASCII, which is asked about
    /// whole.
    NonAscii,
}

/// The table, built once.
fn sigil_table() -> &'static SigilTable {
    static TABLE: std::sync::OnceLock<SigilTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(SigilTable::build)
}

/// A source's tokens.
#[salsa::tracked(heap_size = lexed_heap_size)]
pub struct Lexed<'db> {
    #[returns(copy)]
    pub source: Source,
    #[returns(ref)]
    pub tokens: Vec<Token>,
}

/// The tokens are values in a `Vec`, so they are heap, not fields.
///
/// Without this salsa's memory reporting would show the tokens costing nothing,
/// since it measures fields by their stack size and a `Vec` is three words
/// whatever it holds.
fn lexed_heap_size(fields: &(Source, Vec<Token>)) -> usize {
    fields.1.capacity() * mem::size_of::<Token>()
}

/// Lex a source: its words, sigils and strings, with whitespace and comments
/// left out and whether a line ended before each token kept on it.
#[salsa::tracked(returns(copy))]
pub fn lex<'db>(db: &'db dyn crate::Db, source: Source) -> Lexed<'db> {
    let text = source.as_str(db);
    // About one token for every four or five bytes of source, so a guess at
    // that saves the vector growing by doubling, which copied every token a
    // couple of times over.
    let mut tokens = Vec::with_capacity(text.len() / 4);
    let mut newline_before = false;
    let mut tokenizer = Tokenizer {
        text,
        range: 0..text.len(),
        newline_before: &mut newline_before,
        table: sigil_table(),
    };
    tokens.extend(iter::from_fn(|| tokenizer.next()));
    Lexed::new(db, source, tokens)
}

/// How far a comment starting at the front of `text` runs, if one does: `Ok`
/// for one that ends, `Err` for one that runs to the end unclosed. A line
/// comment stops short of its newline, which is whitespace like any other.
fn scan_comment(text: &str) -> Option<Result<usize, usize>> {
    let bytes = text.as_bytes();
    match bytes {
        [b'/', b'/', ..] => Some(Ok(rmx::memchr::memchr(b'\n', bytes).unwrap_or(text.len()))),
        [b'/', b'*', ..] => Some(scan_block_comment(text)),
        _ => None,
    }
}

/// A block comment, which nests: `/* /* */ */` is one comment.
fn scan_block_comment(text: &str) -> Result<usize, usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut at = 0;
    while at + 1 < bytes.len() {
        match (bytes[at], bytes[at + 1]) {
            (b'/', b'*') => {
                depth += 1;
                at += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return Ok(at);
                }
            }
            _ => at += 1,
        }
    }
    Err(text.len())
}

/// How far a string starting at the front of `text` runs: `Ok` through its
/// closing `"`, `Err` to the end where it has none. A `\` takes the byte
/// after it with it, so `\"` does not close the string.
fn scan_string(text: &str) -> Result<usize, usize> {
    let bytes = text.as_bytes();
    let mut at = 1;
    while at < bytes.len() {
        match bytes[at] {
            b'"' => return Ok(at + 1),
            b'\\' => at += 2,
            _ => at += 1,
        }
    }
    Err(text.len())
}


/// What is left of a source to lex.
///
/// `text` is the whole source rather than a handle to fetch it with: reading
/// it back out of salsa on every `peek` was two field reads per character,
/// and `peek` is what every other method is built on.
struct Tokenizer<'db, 'a> {
    text: &'db str,
    range: Range<usize>,
    newline_before: &'a mut bool,
    table: &'static SigilTable,
}

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
enum NextToken {
    Whitespace,
    Word,
    Sigil,
    Quote,
    Error,
}

impl<'db> Tokenizer<'db, '_> {
    fn next(&mut self) -> Option<Token> {
        let bytes = self.text.as_bytes();
        loop {
            if self.range.is_empty() {
                return None;
            }
            // An ASCII byte is classified by the table; anything else
            // goes the way of a whole character.
            let mut token = match self.table.class[bytes[self.range.start] as usize] {
                Class::Space => {
                    self.range.start += 1;
                    continue;
                }
                Class::Newline => {
                    self.range.start += 1;
                    *self.newline_before = true;
                    continue;
                }
                Class::Word => self.eat_word(),
                // A `/` that begins a comment is skipped like whitespace,
                // and one that is not closed is an error to the end.
                Class::Sigil if bytes[self.range.start] == b'/' => {
                    match scan_comment(&self.text[self.range.start..]) {
                        Some(Ok(length)) => {
                            self.range.start += length;
                            continue;
                        }
                        Some(Err(length)) => self.eat_bytes(length, TokenKind::Error),
                        None => self.eat_sigil(),
                    }
                }
                Class::Sigil => self.eat_sigil(),
                Class::Quote => match scan_string(&self.text[self.range.start..]) {
                    Ok(length) => self.eat_bytes(length, TokenKind::String),
                    Err(length) => self.eat_bytes(length, TokenKind::Error),
                },
                Class::Other => self.eat_error(),
                Class::NonAscii => match self.peek_token()? {
                    NextToken::Whitespace => {
                        if self.eat_whitespace() {
                            *self.newline_before = true;
                        }
                        continue;
                    }
                    NextToken::Word => self.eat_word(),
                    NextToken::Sigil => self.eat_sigil(),
                    // A character outside ASCII is never `"`.
                    NextToken::Quote => bug!(),
                    NextToken::Error => self.eat_error(),
                },
            };
            token.newline_before = mem::take(self.newline_before);
            return Some(token);
        }
    }

    fn peek_token(&self) -> Option<NextToken> {
        self.peek().map(Self::token_start)
    }

    fn token_start(ch: char) -> NextToken {
        match ch {
            _ if ch.is_whitespace() => NextToken::Whitespace,
            _ if Self::is_word_start(ch) => NextToken::Word,
            _ if Self::is_sigil_start(ch) => NextToken::Sigil,
            '"' => NextToken::Quote,
            _ => NextToken::Error,
        }
    }

    fn is_word_start(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    fn eat_word(&mut self) -> Token {
        debug_assert_eq!(self.peek_token(), Some(NextToken::Word));

        let start = self.range.start;
        let bytes = self.text.as_bytes();
        let mut at = start;
        while at < self.range.end && self.table.class[bytes[at] as usize] == Class::Word {
            at += 1;
        }
        // A letter or digit outside ASCII carries a word on.
        self.range.start = if at < self.range.end && !bytes[at].is_ascii() {
            self.scan(at, Self::is_word_start)
        } else {
            at
        };
        assert!(start < self.range.start);
        let span = start .. self.range.start;
        Token {
            start: u32::try_from(span.start).X(),
            end: u32::try_from(span.end).X(),
            kind: TokenKind::Word,
            newline_before: false,
        }
    }

    fn is_sigil_start(ch: char) -> bool {
        // No sigil starts with a character outside ASCII, so one index
        // answers this. It used to ask every sigil for its first character,
        // for every character the tokenizer classified.
        ch.is_ascii() && sigil_table().starts[ch as usize]
    }

    fn eat_sigil(&mut self) -> Token {
        debug_assert_eq!(self.peek_token(), Some(NextToken::Sigil));

        let text = &self.text[self.range.C()];

        // Only the sigils that could start here, rather than all of them.
        for &(sigil, sigil_str) in self.table.starting_with(text.as_bytes()[0]) {
            if text.starts_with(sigil_str) {
                let range_start = self.range.start;
                self.range.start = range_start.checked_add(sigil_str.len()).X();
                let span = range_start .. self.range.start;
                return Token {
                    start: u32::try_from(span.start).X(),
                    end: u32::try_from(span.end).X(),
                    kind: TokenKind::Sigil(sigil),
                    newline_before: false,
                }
            }
        }

        let ch = self.peek().X();
        self.eat_char(ch);
        self.eat_error_from(ch)
    }

    /// The next `length` bytes as one token of `kind`.
    fn eat_bytes(&mut self, length: usize, kind: TokenKind) -> Token {
        let start = self.range.start;
        self.range.start = start.checked_add(length).X();
        Token {
            start: u32::try_from(start).X(),
            end: u32::try_from(self.range.start).X(),
            kind,
            newline_before: false,
        }
    }

    fn eat_error(&mut self) -> Token {
        let ch = self.peek().X();
        self.eat_char(ch);
        self.eat_error_from(ch)
    }

    fn eat_error_from(&mut self, start_ch: char) -> Token {
        // The first error character has already been consumed by the
        // caller, so the token starts however wide that character was --
        // not one byte back. Backing up by one put the span inside a
        // multibyte character, and interning it sliced the source on a
        // boundary that is not one.
        let token_start = Self::token_start(start_ch);
        let start = self.range.start.checked_sub(start_ch.len_utf8()).X();
        while let Some(ch) = self.peek() {
            let next_token_start = Self::token_start(ch);
            let recover = match (token_start, next_token_start) {
                (NextToken::Whitespace | NextToken::Word | NextToken::Quote, _) => unreachable!(),
                // A string is a token of its own, so it ends the run.
                (_, NextToken::Quote) => true,
                (NextToken::Sigil, NextToken::Error) => false,
                (NextToken::Sigil, NextToken::Whitespace) => true,
                (NextToken::Sigil, NextToken::Word) => true,
                (NextToken::Sigil, NextToken::Sigil) => true,
                (NextToken::Error, NextToken::Error) => false,
                (NextToken::Error, NextToken::Whitespace) => true,
                (NextToken::Error, NextToken::Word) => true,
                (NextToken::Error, NextToken::Sigil) => true,
            };
            if !recover {
                self.eat_char(ch);
            } else {
                break;
            }
        }
        assert!(start < self.range.start);
        let span = start .. self.range.start;
        Token {
            start: u32::try_from(span.start).X(),
            end: u32::try_from(span.end).X(),
            kind: TokenKind::Error,
            newline_before: false,
        }
    }

    /// Skip a run of whitespace, saying whether it held a newline.
    fn eat_whitespace(&mut self) -> bool {
        debug_assert_eq!(self.peek_token(), Some(NextToken::Whitespace));

        let start = self.range.start;
        self.range.start = self.scan(start, char::is_whitespace);
        self.text.as_bytes()[start..self.range.start].contains(&b'\n')
    }

    /// Where the run of characters from `from` that `keep` accepts ends.
    ///
    /// A byte at a time, decoding a character only where one is not ASCII:
    /// a `peek` and an `eat_char` per character, with the bounds and
    /// overflow checks each makes, was most of what lexing cost.
    fn scan(&self, from: usize, keep: fn(char) -> bool) -> usize {
        let bytes = self.text.as_bytes();
        let mut at = from;
        while at < self.range.end {
            let byte = bytes[at];
            let ch = if byte.is_ascii() {
                byte as char
            } else {
                self.text[at..self.range.end].chars().next().X()
            };
            if !keep(ch) {
                break;
            }
            at += ch.len_utf8();
        }
        at
    }

    fn eat_char(&mut self, ch: char) {
        // Debug only: the caller has just peeked this character, and paying
        // for a second peek per character consumed doubled the lexer's work.
        debug_assert!(self.peek() == Some(ch));
        self.range.start = self.range.start.checked_add(ch.len_utf8()).X();
        assert!(self.range.start <= self.range.end);
    }

    fn peek(&self) -> Option<char> {
        if self.range.is_empty() {
            return None;
        }
        // Nearly everything is ASCII, which needs no decoding, and slicing
        // the text to decode it checks that both ends are boundaries.
        let byte = self.text.as_bytes()[self.range.start];
        if byte.is_ascii() {
            return Some(byte as char);
        }
        self.text[self.range.C()].chars().next()
    }
}

impl<'db> Lexed<'db> {
    #[cfg(test)]
    fn debug_str(&self, db: &'db dyn crate::Db) -> String {
        let text = self.source(db).as_str(db);
        #[allow(unstable_name_collisions)] // intersperse
        self.tokens(db).iter().map(|token| {
            if token.newline_before {
                fmt!("nl {}", token.debug_str(text))
            } else {
                token.debug_str(text).S()
            }
        }).intersperse(S(" ")).collect()
    }
}

impl Token {
    /// The token's span in its source.
    pub fn span(&self) -> Range<usize> {
        self.start as usize..self.end as usize
    }

    #[cfg(test)]
    pub fn debug_str<'a>(&self, text: &'a str) -> &'a str {
        match self.kind {
            TokenKind::Word | TokenKind::String => {
                self.text(text)
            }
            TokenKind::Sigil(s) => s.as_str(),
            TokenKind::Error => "err",
        }
    }

    pub fn is_close_sigil(&self) -> bool {
        match self.kind {
            TokenKind::Sigil(s) => s.is_close_sigil(),
            _ => false,
        }
    }

    /// The token's text, out of the text of the source it was lexed from.
    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        &text[self.span()]
    }

    pub fn word_str<'a>(&self, text: &'a str) -> Option<&'a str> {
        if self.kind != TokenKind::Word {
            return None;
        }
        Some(self.text(text))
    }
}

impl Sigil {
    pub fn as_str(&self) -> &'static str {
        match self {
            // Three-character sigils.
            Sigil::PlusQuestionEquals => "+?=",
            Sigil::MinusQuestionEquals => "-?=",
            Sigil::StarQuestionEquals => "*?=",
            Sigil::SlashQuestionEquals => "/?=",
            Sigil::PlusExclamationEquals => "+!=",
            Sigil::MinusExclamationEquals => "-!=",
            Sigil::StarExclamationEquals => "*!=",
            Sigil::SlashExclamationEquals => "/!=",

            // Earmuff braces.
            Sigil::ParenPipeOpen => "(|",
            Sigil::ParenPipeClose => "|)",
            Sigil::BracePipeOpen => "{|",
            Sigil::BracePipeClose => "|}",
            Sigil::BracketPipeOpen => "[|",
            Sigil::BracketPipeClose => "|]",
            Sigil::AnglePipeOpen => "<|",
            Sigil::AnglePipeClose => "|>",

            // Sigil-brace opens.
            Sigil::PercentBraceOpen => "%{",
            Sigil::HashBraceOpen => "#{",

            // Two-character sigils.
            Sigil::ColonDash => ":-",
            Sigil::PlusQuestion => "+?",
            Sigil::MinusQuestion => "-?",
            Sigil::StarQuestion => "*?",
            Sigil::SlashQuestion => "/?",
            Sigil::PlusExclamation => "+!",
            Sigil::MinusExclamation => "-!",
            Sigil::StarExclamation => "*!",
            Sigil::SlashExclamation => "/!",
            Sigil::PlusEquals => "+=",
            Sigil::MinusEquals => "-=",
            Sigil::StarEquals => "*=",
            Sigil::SlashEquals => "/=",
            Sigil::DotDot => "..",
            Sigil::DotLess => ".<",
            Sigil::DotGreater => ".>",
            Sigil::LessEquals => "<=",
            Sigil::GreaterEquals => ">=",
            Sigil::EqualsEquals => "==",
            Sigil::ExclamationEquals => "!=",

            // Single-character sigils.
            Sigil::Dot => ".",
            Sigil::Comma => ",",
            Sigil::Semicolon => ";",
            Sigil::Plus => "+",
            Sigil::Minus => "-",
            Sigil::Star => "*",
            Sigil::SlashForward => "/",
            Sigil::Equals => "=",
            Sigil::AngleOpen => "<",
            Sigil::AngleClose => ">",
            Sigil::Pipe => "|",
            Sigil::Question => "?",
            Sigil::Exclamation => "!",
            Sigil::Colon => ":",
            Sigil::Hash => "#",
            Sigil::At => "@",
            Sigil::Percent => "%",
            Sigil::ParenOpen => "(",
            Sigil::ParenClose => ")",
            Sigil::BraceOpen => "{",
            Sigil::BraceClose => "}",
            Sigil::BracketOpen => "[",
            Sigil::BracketClose => "]",
            Sigil::Dollar => "$",
            Sigil::Tilde => "~",
        }
    }

    pub fn close_sigil(&self) -> Sigil {
        match self {
            Sigil::ParenOpen => Sigil::ParenClose,
            Sigil::BraceOpen => Sigil::BraceClose,
            Sigil::BracketOpen => Sigil::BracketClose,
            Sigil::AngleOpen => Sigil::AngleClose,
            Sigil::ParenPipeOpen => Sigil::ParenPipeClose,
            Sigil::BracePipeOpen => Sigil::BracePipeClose,
            Sigil::BracketPipeOpen => Sigil::BracketPipeClose,
            Sigil::AnglePipeOpen => Sigil::AnglePipeClose,
            Sigil::PercentBraceOpen => Sigil::BraceClose,
            Sigil::HashBraceOpen => Sigil::BraceClose,
            _ => bug!(),
        }
    }

    /// Get the corresponding opening sigil for a closing sigil.
    pub fn open_sigil(&self) -> Sigil {
        match self {
            Sigil::ParenClose => Sigil::ParenOpen,
            Sigil::BraceClose => Sigil::BraceOpen,
            Sigil::BracketClose => Sigil::BracketOpen,
            Sigil::AngleClose => Sigil::AngleOpen,
            Sigil::ParenPipeClose => Sigil::ParenPipeOpen,
            Sigil::BracePipeClose => Sigil::BracePipeOpen,
            Sigil::BracketPipeClose => Sigil::BracketPipeOpen,
            Sigil::AnglePipeClose => Sigil::AnglePipeOpen,
            _ => bug!(),
        }
    }

    fn is_close_sigil(&self) -> bool {
        matches!(self,
            Sigil::ParenClose | Sigil::BraceClose | Sigil::BracketClose | Sigil::AngleClose |
            Sigil::ParenPipeClose | Sigil::BracePipeClose | Sigil::BracketPipeClose | Sigil::AnglePipeClose
        )
    }
}

#[test]
fn test_lex() {
    fn dbglex(s: &str) -> String {
        let ref db = crate::Database::default();
        let source = Source::new(db, S(s));
        lex(db, source).debug_str(db)
    }

    assert_eq!(
        dbglex(" "),
        "",
    );
    assert_eq!(
        dbglex("a"),
        "a",
    );
    assert_eq!(
        dbglex("a b"),
        "a b",
    );
    assert_eq!(
        dbglex("a:-b"),
        "a :- b",
    );
    assert_eq!(
        dbglex("a :- b \n c"),
        "a :- b nl c",
    );
    assert_eq!(
        dbglex("a//"),
        "a",
    );
    assert_eq!(
        dbglex("a//\n"),
        "a",
    );
    assert_eq!(
        dbglex("a//\nd"),
        "a nl d",
    );
    // A newline inside a block comment ends no line, and one before a
    // comment still counts for the token after it.
    assert_eq!(
        dbglex("a/*\n*/b"),
        "a b",
    );
    assert_eq!(
        dbglex("a\n/* x */ b"),
        "a nl b",
    );
    assert_eq!(
        dbglex("(){}){"),
        "( ) { } ) {",
    );
    assert_eq!(
        dbglex("a / b / c"),
        "a / b / c",
    );
    assert_eq!(
        dbglex("a/b/c"),
        "a / b / c",
    );
    assert_eq!(
        dbglex("a<b>c[d]e|f:g=h"),
        "a < b > c [ d ] e | f : g = h",
    );
    assert_eq!(
        dbglex("a?b!c"),
        "a ? b ! c",
    );
    assert_eq!(
        dbglex("?!"),
        "? !",
    );
    assert_eq!(
        dbglex("a#b"),
        "a # b",
    );

    // A bare `%` is a sigil, and the brace open is still the longer match.
    assert_eq!(
        dbglex("50%"),
        "50 %",
    );
    assert_eq!(
        dbglex("a % b"),
        "a % b",
    );
    assert_eq!(
        dbglex("%%"),
        "% %",
    );

    // A range is one sigil, and a float's point is still its own.
    assert_eq!(
        dbglex("0..8"),
        "0 .. 8",
    );
    assert_eq!(
        dbglex("0 .. 8"),
        "0 .. 8",
    );
    assert_eq!(
        dbglex("1.5"),
        "1 . 5",
    );
    assert_eq!(
        dbglex("..."),
        ".. .",
    );

    // Sigil-brace opens.
    assert_eq!(
        dbglex("%{a}"),
        "%{ a }",
    );
    assert_eq!(
        dbglex("#{a}"),
        "#{ a }",
    );
    assert_eq!(
        dbglex("%{}"),
        "%{ }",
    );
    assert_eq!(
        dbglex("#{}"),
        "#{ }",
    );

    // Basic arithmetic operators.
    assert_eq!(
        dbglex("a+b-c*d/e"),
        "a + b - c * d / e",
    );
    assert_eq!(
        dbglex("a + b - c * d / e"),
        "a + b - c * d / e",
    );

    // Question variants.
    assert_eq!(
        dbglex("a+?b-?c*?d/?e"),
        "a +? b -? c *? d /? e",
    );
    assert_eq!(
        dbglex("+? -? *? /?"),
        "+? -? *? /?",
    );

    // Exclamation variants.
    assert_eq!(
        dbglex("a+!b-!c*!d/!e"),
        "a +! b -! c *! d /! e",
    );
    assert_eq!(
        dbglex("+! -! *! /!"),
        "+! -! *! /!",
    );

    // A sigil followed by `%` is two, there being no percent variants. The
    // `%` before a brace is still the map literal's open.
    assert_eq!(
        dbglex("a+%b"),
        "a + % b",
    );

    // Assignment operators.
    assert_eq!(
        dbglex("a+=b-=c*=d/=e"),
        "a += b -= c *= d /= e",
    );
    assert_eq!(
        dbglex("+= -= *= /="),
        "+= -= *= /=",
    );

    // Question-equals variants.
    assert_eq!(
        dbglex("a+?=b-?=c*?=d/?=e"),
        "a +?= b -?= c *?= d /?= e",
    );
    assert_eq!(
        dbglex("+?= -?= *?= /?="),
        "+?= -?= *?= /?=",
    );

    // Exclamation-equals variants, which take precedence over `!=`.
    assert_eq!(
        dbglex("a+!=b-!=c*!=d/!=e"),
        "a +!= b -!= c *!= d /!= e",
    );
    assert_eq!(
        dbglex("+!= -!= *!= /!="),
        "+!= -!= *!= /!=",
    );
    assert_eq!(
        dbglex("a!=b"),
        "a != b",
    );

    // Comparison operators.
    assert_eq!(
        dbglex("a.<b.>c<=d>=e==f!=g"),
        "a .< b .> c <= d >= e == f != g",
    );
    assert_eq!(
        dbglex(".< .> <= >= == !="),
        ".< .> <= >= == !=",
    );

    // Mixed complex expressions.
    assert_eq!(
        dbglex("x+=1+?y"),
        "x += 1 +? y",
    );
    assert_eq!(
        dbglex("a+b+?c+!d+=e+?=f+!=g"),
        "a + b +? c +! d += e +?= f +!= g",
    );

    // Clone and widen postfix operators.
    assert_eq!(
        dbglex("a$b~c"),
        "a $ b ~ c",
    );
    assert_eq!(
        dbglex("x$~"),
        "x $ ~",
    );

    // Earmuff braces.
    assert_eq!(
        dbglex("(|a|)"),
        "(| a |)",
    );
    assert_eq!(
        dbglex("{|a|}"),
        "{| a |}",
    );
    assert_eq!(
        dbglex("[|a|]"),
        "[| a |]",
    );
    assert_eq!(
        dbglex("<|a|>"),
        "<| a |>",
    );
    assert_eq!(
        dbglex("(|[|a|]|)"),
        "(| [| a |] |)",
    );
    assert_eq!(
        dbglex("(||)"),
        "(| |)",
    );
}

/// A character that is neither a word, a sigil nor whitespace is an error
/// token, and it may be more than one byte wide.
///
/// Smart quotes and an em-dash pasted for a minus are the ones a person
/// actually produces, and the error path is what a live-reloading editor
/// relies on to report rather than die. Letters are not among them:
/// `is_word_start` is `is_alphanumeric`, so `café` is a word. A name is
/// ASCII, but that is for the parser to report, which can say so of the whole
/// name; see `parser_util::non_ascii_names`.
#[test]
fn test_lex_multibyte_error() {
    fn dbglex(s: &str) -> String {
        let ref db = crate::Database::default();
        let source = Source::new(db, S(s));
        lex(db, source).debug_str(db)
    }

    // At the start, at the end, and between two words.
    assert_eq!(dbglex("\u{a7}"), "err");
    assert_eq!(dbglex("a\u{a7}"), "a err");
    assert_eq!(dbglex("\u{a7}a"), "err a");
    assert_eq!(dbglex("a \u{a7} b"), "a err b");

    // Three bytes and four, since the width is what was got wrong.
    assert_eq!(dbglex("a \u{2014} b"), "a err b");
    assert_eq!(dbglex("a \u{1f600} b"), "a err b");

    // Adjacent error characters are one token, which is the arm that keeps
    // eating after the first.
    assert_eq!(dbglex("\u{201c}\u{201d}"), "err");
    assert_eq!(dbglex("a\u{201c}\u{201d}b"), "a err b");

    // And against a sigil on either side, since a sigil ends the run.
    assert_eq!(dbglex("{\u{a7}}"), "{ err }");
    assert_eq!(dbglex("=\u{2014}="), "= err =");

    // A multibyte letter is a word, not an error, and the run stops at the
    // one character that is neither.
    assert_eq!(dbglex("caf\u{e9}"), "caf\u{e9}");
    assert_eq!(dbglex("caf\u{e9}\u{2014}"), "caf\u{e9} err");
}

/// The span of an error token is the characters it covers, whole.
///
/// `debug_str` renders every error as `err`, so a span that began inside a
/// multibyte character would pass the test above and still be wrong. It was
/// wrong: the start was recovered by stepping back one *byte* from a character
/// that had been consumed by its width in bytes.
#[test]
fn test_lex_error_spans() {
    fn spans(s: &str) -> Vec<(Range<usize>, String)> {
        let ref db = crate::Database::default();
        let source = Source::new(db, S(s));
        lex(db, source)
            .tokens(db)
            .iter()
            .map(|t| (t.span(), t.debug_str(s).to_string()))
            .collect()
    }

    // `§` is bytes 1..3, so the error token is 1..3 and `b` starts at 3.
    assert_eq!(
        spans("a\u{a7}b"),
        vec![(0..1, "a".into()), (1..3, "err".into()), (3..4, "b".into())],
    );

    // An em-dash is three bytes.
    assert_eq!(
        spans("\u{2014}a"),
        vec![(0..3, "err".into()), (3..4, "a".into())],
    );

    // Two of them, run together into one token.
    assert_eq!(
        spans("\u{201c}\u{201d}"),
        vec![(0..6, "err".into())],
    );

    // An ASCII error character is still one byte, which is the case the old
    // arithmetic was written for.
    assert_eq!(
        spans("a\\b"),
        vec![(0..1, "a".into()), (1..2, "err".into()), (2..3, "b".into())],
    );

    // Every span indexes the source, which is the property the panic broke.
    for src in ["a\u{a7}b", "\u{2014}a", "\u{201c}\u{201d}", "a\\b", "\u{1f600}"] {
        for (span, _) in spans(src) {
            assert!(src.is_char_boundary(span.start), "{src:?} {span:?}");
            assert!(src.is_char_boundary(span.end), "{src:?} {span:?}");
        }
    }
}

/// Comments and strings, found where they begin wherever that is.
///
/// Each case is the text in pieces: `C` a comment, which makes no token; `S`
/// a string and `E` an error, each of which is exactly one token; and `T`
/// anything else, lexed however it lexes.
#[test]
fn test_lex_comments_and_strings() {
    #[derive(Copy, Clone)]
    enum F<'s> {
        T(&'s str),
        C(&'s str),
        S(&'s str),
        E(&'s str),
    }

    fn run(frags: &[F<'_>]) {
        let db = &crate::Database::default();
        let mut text = String::new();
        let mut comments = vec![];
        let mut expected = vec![];
        for frag in frags {
            let start = text.len();
            match *frag {
                F::T(s) => text.push_str(s),
                F::C(s) => {
                    text.push_str(s);
                    comments.push(start..text.len());
                }
                F::S(s) => {
                    text.push_str(s);
                    expected.push((TokenKind::String, start..text.len()));
                }
                F::E(s) => {
                    text.push_str(s);
                    expected.push((TokenKind::Error, start..text.len()));
                }
            }
        }
        let source = Source::new(db, text.C());
        let tokens = lex(db, source).tokens(db).C();
        let found: Vec<_> = tokens.iter()
            .filter(|t| matches!(t.kind, TokenKind::String | TokenKind::Error))
            .map(|t| (t.kind, t.span()))
            .collect();
        assert_eq!(found, expected, "strings and errors in {text:?}");
        for token in &tokens {
            let span = token.span();
            assert!(
                comments.iter().all(|c| span.end <= c.start || c.end <= span.start),
                "a token inside a comment in {text:?}",
            );
        }
    }

    use F::*;
    run(&[T("ab"), T("bdd"), C("//")]);
    run(&[T("ab"), T("bdd"), C("//"), T("\n")]);
    run(&[T("ab"), C("//a"), T("\nbdd"), C("//b"), T("\n"), C("//b")]);
    run(&[T("ab"), T("bdd"), S("\"x\"")]);
    run(&[T("ab"), T("bdd"), S("\"x\""), S("\"x\"")]);
    run(&[T("a"), T("b"), T("c")]);
    run(&[T("ab"), E("\"x")]);
    run(&[E("\"x . //")]);
    run(&[C("// \" . \""), T("\n")]);
    run(&[S("\"// . \"")]);
    run(&[T("/ a")]);
    run(&[E("/* a")]);
    run(&[C("/* */")]);
    run(&[C("/*/**/*/")]);
    run(&[E("/*/**/ab")]);

    // Escapes: a backslash takes the byte after it, so `\"` does not close.
    run(&[S("\"foo\\\"bar\"")]);
    run(&[S("\"foo\\\\\"")]);
    run(&[S("\"foo\\\\\\\"bar\"")]);
    run(&[S("\"\\\\\\\\\"")]);
    run(&[E("\"foo\\\"bar")]);
    run(&[E("\"foo\\\\")]);
    run(&[T("x"), S("\"a\\\"b\""), T("y")]);

    // A string or a comment ends a run of error characters, and a comment
    // begins wherever a `/` would begin a token.
    run(&[E("\u{2603}"), S("\"x\"")]);
    run(&[T("a *"), C("//c"), T("\nb")]);
}
