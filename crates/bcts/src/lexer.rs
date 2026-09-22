use rmx::prelude::*;

use rmx::std::ops::Range;
use rmx::std::{iter, mem};

use crate::text::InternedText;
use crate::chunk::{Chunk, RangeKind};

#[cfg(test)]
use crate::input::Source;
#[cfg(test)]
use crate::source_map::basic_source_map;

#[salsa::tracked(heap_size = chunk_lex_heap_size)]
pub struct ChunkLex<'db> {
    #[returns(copy)]
    pub chunk: Chunk<'db>,
    #[returns(ref)]
    pub tokens: Vec<Token<'db>>,
}

/// The tokens are values in a `Vec`, so they are heap, not fields.
///
/// Without this salsa's memory reporting would show the tokens costing nothing,
/// since it measures fields by their stack size and a `Vec` is three words
/// whatever it holds.
fn chunk_lex_heap_size<'db>(fields: &(Chunk<'db>, Vec<Token<'db>>)) -> usize {
    fields.1.capacity() * mem::size_of::<Token<'db>>()
}

/// One lexed token.
///
/// A plain value rather than a tracked struct. Tokens are read straight out of
/// the `Vec` in a [`ChunkLex`] and never looked up by identity, so the salsa id
/// and page slot each one used to carry paid for nothing.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct Token<'db> {
    pub text: InternedText<'db>,
    pub span: Range<usize>,
    pub kind: TokenKind,
}

#[derive(Copy, Clone, Debug, Hash, salsa::SalsaValue)]
#[derive(Eq, PartialEq)]
pub enum TokenKind {
    Word,
    Sigil(Sigil),
    String,
    Whitespace,
    Comment,
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
    PlusBarEquals,
    MinusBarEquals,
    StarBarEquals,
    SlashBarEquals,

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
    PlusBar,
    MinusBar,
    StarBar,
    SlashBar,
    PlusExclamation,
    MinusExclamation,
    StarExclamation,
    SlashExclamation,
    PlusPercent,
    MinusPercent,
    StarPercent,
    SlashPercent,
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
    /// The sigils beginning with this byte, in declaration order.
    ///
    /// Declaration order is load-bearing: `eat_sigil` takes the first match, and
    /// the longer sigils are declared before the shorter ones they begin with, so
    /// `+?=` is found before `+`. Grouping by first byte keeps that order among
    /// the candidates, which is the same order the full scan saw them in.
    by_first_byte: [Vec<Sigil>; 256],
}

impl SigilTable {
    fn build() -> Self {
        let mut starts = [false; 256];
        let mut by_first_byte: [Vec<Sigil>; 256] = std::array::from_fn(|_| Vec::new());
        for sigil in enum_iterator::all::<Sigil>() {
            let first = sigil.as_str().as_bytes()[0];
            assert!(first.is_ascii(), "a sigil starting outside ASCII: {:?}", sigil);
            starts[first as usize] = true;
            by_first_byte[first as usize].push(sigil);
        }
        Self { starts, by_first_byte }
    }

    /// The sigils that could begin with `byte`, longest first.
    fn starting_with(&self, byte: u8) -> &[Sigil] {
        &self.by_first_byte[byte as usize]
    }
}

/// The table, built once.
fn sigil_table() -> &'static SigilTable {
    static TABLE: std::sync::OnceLock<SigilTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(SigilTable::build)
}

/// Interning a chunk's token texts, remembering what it has already interned.
///
/// Most of a chunk's tokens are whitespace and punctuation, and between them they
/// hold a few dozen distinct strings: of the standard library's 61,952 tokens,
/// 14,597 are sigils drawn from at most seventy-one spellings and 23,623 are
/// whitespace runs drawn from the handful a formatter emits. Salsa deduplicates
/// them, but only after hashing each occurrence and probing a sharded concurrent
/// map, so asking it 38,220 times to be told one of eighty answers was most of the
/// interning cost.
///
/// A sigil needs no hashing at all, the token kind already saying which one it is.
/// Everything else is looked up by the slice it came from, which costs a hash but
/// not salsa's map, and reaches salsa once per distinct string per chunk.
struct Interner<'db> {
    db: &'db dyn crate::Db,
    text: &'db str,
    seen: rustc_hash::FxHashMap<&'db str, InternedText<'db>>,
    /// Indexed by `Sigil as usize`, which is its declaration order.
    sigils: Vec<Option<InternedText<'db>>>,
}

impl<'db> Interner<'db> {
    fn new(db: &'db dyn crate::Db, text: &'db str) -> Self {
        Self {
            db,
            text,
            seen: rustc_hash::FxHashMap::default(),
            sigils: vec![None; enum_iterator::cardinality::<Sigil>()],
        }
    }

    /// The interned text of `range`.
    fn intern(&mut self, range: Range<usize>) -> InternedText<'db> {
        let slice = &self.text[range];
        if let Some(interned) = self.seen.get(slice) {
            return *interned;
        }
        let interned = InternedText::new(self.db, slice);
        self.seen.insert(slice, interned);
        interned
    }

    /// The interned text of a sigil, which is the sigil's own spelling.
    fn intern_sigil(&mut self, sigil: Sigil) -> InternedText<'db> {
        let slot = sigil as usize;
        if let Some(interned) = self.sigils[slot] {
            return interned;
        }
        let interned = InternedText::new(self.db, sigil.as_str());
        self.sigils[slot] = Some(interned);
        interned
    }
}

#[salsa::tracked(returns(copy))]
pub fn lex_chunk<'db>(
    db: &'db dyn crate::Db,
    chunk: Chunk<'db>,
) -> ChunkLex<'db> {
    let mut tokens = Vec::new();
    let chunk_text = chunk.text(db);
    let chunk_str = chunk_text.as_str(db);
    let mut interner = Interner::new(db, chunk_str);

    for range in chunk.ranges(db) {
        match range {
            (range, RangeKind::Comment) => {
                tokens.push(Token {
                    text: interner.intern(range.C()),
                    span: range,
                    kind: TokenKind::Comment,
                });
            }
            (range, RangeKind::String) => {
                tokens.push(Token {
                    text: interner.intern(range.C()),
                    span: range,
                    kind: TokenKind::String,
                });
            }
            (range, RangeKind::Error) => {
                tokens.push(Token {
                    text: interner.intern(range.C()),
                    span: range,
                    kind: TokenKind::Error,
                });
            }
            (range, RangeKind::Unknown) => {
                let mut tokenizer = Tokenizer {
                    text: chunk_str,
                    range,
                    interner: &mut interner,
                };

                tokens.extend(
                    iter::from_fn(|| tokenizer.next())
                );
            }
        }
    }

    return ChunkLex::new(db, chunk, tokens);

    /// What is left of one unknown range, and where in the chunk it started.
    ///
    /// `text` is the whole chunk rather than a handle to fetch it with: reading
    /// it back out of salsa on every `peek` was two field reads per character,
    /// and `peek` is what every other method is built on.
    struct Tokenizer<'db, 'a> {
        text: &'db str,
        range: Range<usize>,
        interner: &'a mut Interner<'db>,
    }

    #[derive(Eq, PartialEq, Debug, Copy, Clone)]
    enum NextToken {
        Whitespace,
        Word,
        Sigil,
        Error,
    }

    impl<'db> Tokenizer<'db, '_> {
        fn next(&mut self) -> Option<Token<'db>> {
            match self.peek_token() {
                None => None,
                Some(NextToken::Whitespace) => Some(self.eat_whitespace()),
                Some(NextToken::Word) => Some(self.eat_word()),
                Some(NextToken::Sigil) => Some(self.eat_sigil()),
                Some(NextToken::Error) => Some(self.eat_error()),
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
                _ => NextToken::Error,
            }
        }

        fn is_word_start(ch: char) -> bool {
            ch.is_alphanumeric() || ch == '_'
        }

        fn intern(&mut self, range: Range<usize>) -> InternedText<'db> {
            self.interner.intern(range)
        }

        fn eat_word(&mut self) -> Token<'db> {
            assert_eq!(self.peek_token(), Some(NextToken::Word));

            let is_word_char = Self::is_word_start;

            let start = self.range.start;
            while let Some(ch) = self.peek() {
                if is_word_char(ch) {
                    self.eat_char(ch);
                } else {
                    break;
                }
            }
            assert!(start < self.range.start);
            let span = start .. self.range.start;
            Token {
                text: self.intern(span.C()),
                span,
                kind: TokenKind::Word,
            }
        }

        fn is_sigil_start(ch: char) -> bool {
            // No sigil starts with a character outside ASCII, so one index
            // answers this. It used to ask every sigil for its first character,
            // for every character the tokenizer classified.
            ch.is_ascii() && sigil_table().starts[ch as usize]
        }

        fn eat_sigil(&mut self) -> Token<'db> {
            assert_eq!(self.peek_token(), Some(NextToken::Sigil));

            let text = &self.text[self.range.C()];

            // Only the sigils that could start here, rather than all of them.
            for &sigil in sigil_table().starting_with(text.as_bytes()[0]) {
                let sigil_str = sigil.as_str();
                if text.starts_with(sigil_str) {
                    let range_start = self.range.start;
                    self.range.start = range_start.checked_add(sigil_str.len()).X();
                    let span = range_start .. self.range.start;
                    return Token {
                        text: self.interner.intern_sigil(sigil),
                        span,
                        kind: TokenKind::Sigil(sigil),
                    }
                }
            }

            let ch = self.peek().X();
            self.eat_char(ch);
            self.eat_error_from(ch)
        }

        fn eat_error(&mut self) -> Token<'db> {
            let ch = self.peek().X();
            self.eat_char(ch);
            self.eat_error_from(ch)
        }

        fn eat_error_from(&mut self, start_ch: char) -> Token<'db> {
            // The first error character has already been consumed by the
            // caller, so the token starts however wide that character was --
            // not one byte back. Backing up by one put the span inside a
            // multibyte character, and interning it sliced the chunk on a
            // boundary that is not one.
            let token_start = Self::token_start(start_ch);
            let start = self.range.start.checked_sub(start_ch.len_utf8()).X();
            while let Some(ch) = self.peek() {
                let next_token_start = Self::token_start(ch);
                let recover = match (token_start, next_token_start) {
                    (NextToken::Whitespace, _) => unreachable!(),
                    (NextToken::Word, _) => unreachable!(),
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
                text: self.intern(span.C()),
                span,
                kind: TokenKind::Error,
            }
        }

        fn eat_whitespace(&mut self) -> Token<'db> {
            assert_eq!(self.peek_token(), Some(NextToken::Whitespace));

            let start = self.range.start;
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() {
                    self.eat_char(ch);
                } else {
                    break;
                }
            }
            assert!(start < self.range.start);
            let span = start .. self.range.start;
            Token {
                text: self.intern(span.C()),
                span,
                kind: TokenKind::Whitespace,
            }
        }

        fn eat_char(&mut self, ch: char) {
            // Debug only: the caller has just peeked this character, and paying
            // for a second peek per character consumed doubled the lexer's work.
            debug_assert!(self.peek() == Some(ch));
            self.range.start = self.range.start.checked_add(ch.len_utf8()).X();
            assert!(self.range.start <= self.range.end);
        }

        fn peek(&self) -> Option<char> {
            self.text[self.range.C()].chars().next()
        }
    }
}

impl<'db> ChunkLex<'db> {
    #[cfg(test)]
    fn debug_str(&self, db: &'db dyn crate::Db) -> String {
        #[allow(unstable_name_collisions)] // intersperse
        self.tokens(db).iter().map(|token| {
            token.debug_str(db)
        }).intersperse(" ").collect()
    }
}

impl<'db> Token<'db> {
    /// The token's span in its chunk.
    ///
    /// `Range` is not `Copy`, so the field is cloned rather than borrowed; a
    /// span is two words and every caller wants it by value.
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub fn without_space(self) -> Option<Self> {
        match self.kind {
            TokenKind::Whitespace => None,
            TokenKind::Comment => None,
            _ => Some(self),
        }
    }

    #[cfg(test)]
    pub fn debug_str(&self, db: &'db dyn crate::Db) -> &'db str {
        match self.kind {
            TokenKind::Word | TokenKind::String => {
                self.text.as_str(db)
            }
            TokenKind::Sigil(s) => s.as_str(),
            TokenKind::Whitespace => "ws",
            TokenKind::Comment => "cmt",
            TokenKind::Error => "err",
        }
    }

    pub fn is_close_sigil(&self) -> bool {
        match self.kind {
            TokenKind::Sigil(s) => s.is_close_sigil(),
            _ => false,
        }
    }

    pub fn word_str(&self, db: &'db dyn crate::Db) -> Option<&'db str> {
        if self.kind != TokenKind::Word {
            return None;
        }
        Some(self.text.as_str(db))
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
            Sigil::PlusBarEquals => "+|=",
            Sigil::MinusBarEquals => "-|=",
            Sigil::StarBarEquals => "*|=",
            Sigil::SlashBarEquals => "/|=",

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
            Sigil::PlusBar => "+|",
            Sigil::MinusBar => "-|",
            Sigil::StarBar => "*|",
            Sigil::SlashBar => "/|",
            Sigil::PlusExclamation => "+!",
            Sigil::MinusExclamation => "-!",
            Sigil::StarExclamation => "*!",
            Sigil::SlashExclamation => "/!",
            Sigil::PlusPercent => "+%",
            Sigil::MinusPercent => "-%",
            Sigil::StarPercent => "*%",
            Sigil::SlashPercent => "/%",
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
fn test_lex_chunk() {
    fn dbglex(s: &str) -> String {
        let ref db = crate::Database::default();
        let source = Source::new(db, S(s));
        let chunk = basic_source_map(db, source);
        let chunk_lex = lex_chunk(db, chunk);
        chunk_lex.debug_str(db)
    }

    assert_eq!(
        dbglex(" "),
        "ws",
    );
    assert_eq!(
        dbglex("a"),
        "a",
    );
    assert_eq!(
        dbglex("a b"),
        "a ws b",
    );
    assert_eq!(
        dbglex("a:-b"),
        "a :- b",
    );
    assert_eq!(
        dbglex("a :- b \n c"),
        "a ws :- ws b ws c",
    );
    assert_eq!(
        dbglex("a//"),
        "a cmt",
    );
    assert_eq!(
        dbglex("a//\n"),
        "a cmt ws",
    );
    assert_eq!(
        dbglex("a//\nd"),
        "a cmt ws d",
    );
    assert_eq!(
        dbglex("(){}){"),
        "( ) { } ) {",
    );
    assert_eq!(
        dbglex("a / b / c"),
        "a ws / ws b ws / ws c",
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
        "a ws % ws b",
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
        "0 ws .. ws 8",
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
        "a ws + ws b ws - ws c ws * ws d ws / ws e",
    );

    // Question variants.
    assert_eq!(
        dbglex("a+?b-?c*?d/?e"),
        "a +? b -? c *? d /? e",
    );
    assert_eq!(
        dbglex("+? -? *? /?"),
        "+? ws -? ws *? ws /?",
    );

    // Bar variants.
    assert_eq!(
        dbglex("a+|b-|c*|d/|e"),
        "a +| b -| c *| d /| e",
    );
    assert_eq!(
        dbglex("+| -| *| /|"),
        "+| ws -| ws *| ws /|",
    );

    // Exclamation variants.
    assert_eq!(
        dbglex("a+!b-!c*!d/!e"),
        "a +! b -! c *! d /! e",
    );
    assert_eq!(
        dbglex("+! -! *! /!"),
        "+! ws -! ws *! ws /!",
    );

    // Percent variants.
    assert_eq!(
        dbglex("a+%b-%c*%d/%e"),
        "a +% b -% c *% d /% e",
    );
    assert_eq!(
        dbglex("+% -% *% /%"),
        "+% ws -% ws *% ws /%",
    );

    // Assignment operators.
    assert_eq!(
        dbglex("a+=b-=c*=d/=e"),
        "a += b -= c *= d /= e",
    );
    assert_eq!(
        dbglex("+= -= *= /="),
        "+= ws -= ws *= ws /=",
    );

    // Question-equals variants.
    assert_eq!(
        dbglex("a+?=b-?=c*?=d/?=e"),
        "a +?= b -?= c *?= d /?= e",
    );
    assert_eq!(
        dbglex("+?= -?= *?= /?="),
        "+?= ws -?= ws *?= ws /?=",
    );

    // Bar-equals variants.
    assert_eq!(
        dbglex("a+|=b-|=c*|=d/|=e"),
        "a +|= b -|= c *|= d /|= e",
    );
    assert_eq!(
        dbglex("+|= -|= *|= /|="),
        "+|= ws -|= ws *|= ws /|=",
    );

    // Comparison operators.
    assert_eq!(
        dbglex("a.<b.>c<=d>=e==f!=g"),
        "a .< b .> c <= d >= e == f != g",
    );
    assert_eq!(
        dbglex(".< .> <= >= == !="),
        ".< ws .> ws <= ws >= ws == ws !=",
    );

    // Mixed complex expressions.
    assert_eq!(
        dbglex("x+=1+?y"),
        "x += 1 +? y",
    );
    assert_eq!(
        dbglex("a+b+?c+|d+=e+?=f+|=g"),
        "a + b +? c +| d += e +?= f +|= g",
    );
    assert_eq!(
        dbglex("a+b+?c+|d+!e"),
        "a + b +? c +| d +! e",
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
/// `is_word_start` is `is_alphanumeric`, so `café` is a word.
#[test]
fn test_lex_multibyte_error() {
    fn dbglex(s: &str) -> String {
        let ref db = crate::Database::default();
        let source = Source::new(db, S(s));
        let chunk = basic_source_map(db, source);
        lex_chunk(db, chunk).debug_str(db)
    }

    // At the start, at the end, and between two words.
    assert_eq!(dbglex("\u{a7}"), "err");
    assert_eq!(dbglex("a\u{a7}"), "a err");
    assert_eq!(dbglex("\u{a7}a"), "err a");
    assert_eq!(dbglex("a \u{a7} b"), "a ws err ws b");

    // Three bytes and four, since the width is what was got wrong.
    assert_eq!(dbglex("a \u{2014} b"), "a ws err ws b");
    assert_eq!(dbglex("a \u{1f600} b"), "a ws err ws b");

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
        let chunk = basic_source_map(db, source);
        lex_chunk(db, chunk)
            .tokens(db)
            .iter()
            .map(|t| (t.span(), t.debug_str(db).to_string()))
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
