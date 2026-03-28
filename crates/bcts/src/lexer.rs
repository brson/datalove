use rmx::prelude::*;

use rmx::itertools::Itertools;
use rmx::std::ops::Range;
use rmx::std::{iter, mem};
use rmx::std::collections::BTreeMap;

use crate::input::Source;
use crate::text::{Text, InternedText};
use crate::chunk::{Chunk, RangeKind};
use crate::source_map::{
    basic_source_map,
};

#[salsa::tracked]
pub struct ChunkLex<'db> {
    pub chunk: Chunk<'db>,
    #[returns(ref)]
    pub tokens: Vec<Token<'db>>,
}

#[salsa::tracked]
#[derive(Debug)]
pub struct Token<'db> {
    pub text: InternedText<'db>,
    pub span: Range<usize>,
    pub kind: TokenKind,
}

#[derive(Copy, Clone, Debug, Hash, salsa::Update)]
#[derive(Eq, PartialEq)]
pub enum TokenKind {
    Word,
    Sigil(Sigil),
    String,
    Whitespace,
    Comment,
    Error,
}

#[derive(Copy, Clone, Debug, Hash, salsa::Update)]
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

    // Unicode bracket pairs (single unicode char, 3 bytes UTF-8).
    MapOpen,       // "⦇"
    MapClose,      // "⦈"
    SetOpen,       // "⦃"
    SetClose,      // "⦄"
    TableOpen,     // "⟦"
    TableClose,    // "⟧"
    TensorOpen,    // "⟪"
    TensorClose,   // "⟫"

    // Unicode sigils (single unicode char, 3 bytes UTF-8).
    MapsTo,        // "↦"
    LessOrEqual,   // "≤"
    GreaterOrEqual, // "≥"
    Identical,     // "≡"
    NotIdentical,  // "≢"

    // Single-character sigils.
    Dot,
    Comma,
    Semicolon,
    Plus,
    Minus,
    Star,
    SlashForward,
    Equals,
    Less,          // "<"
    Greater,       // ">"
    Pipe,
    Question,
    Exclamation,
    Colon,
    Hash,
    At,
    ParenOpen,
    ParenClose,
    BraceOpen,
    BraceClose,
    BracketOpen,
    BracketClose,
    Dollar,
    Tilde,
}

#[salsa::tracked]
pub fn lex_chunk<'db>(
    db: &'db dyn crate::Db,
    chunk: Chunk<'db>,
) -> ChunkLex<'db> {
    let mut tokens = Vec::new();
    let chunk_text = chunk.text(db);
    let chunk_str = chunk_text.as_str(db);
    let intern = |range: Range<usize>| InternedText::new(db, S(&chunk_str[range]));

    for range in chunk.ranges(db) {
        match range {
            (range, RangeKind::Comment) => {
                tokens.push(Token::new(
                    db,
                    intern(range.C()),
                    range,
                    TokenKind::Comment,
                ));
            }
            (range, RangeKind::String) => {
                tokens.push(Token::new(
                    db,
                    intern(range.C()),
                    range,
                    TokenKind::String,
                ));
            }
            (range, RangeKind::Error) => {
                tokens.push(Token::new(
                    db,
                    intern(range.C()),
                    range,
                    TokenKind::Error,
                ));
            }
            (range, RangeKind::Unknown) => {
                let mut tokenizer = Tokenizer {
                    db,
                    chunk,
                    range,
                    chunk_text: chunk_text.C(),
                };

                tokens.extend(
                    iter::from_fn(|| tokenizer.next())
                );
            }
        }
    }

    return ChunkLex::new(db, chunk, tokens);

    struct Tokenizer<'db> {
        db: &'db dyn crate::Db,
        chunk: Chunk<'db>,
        chunk_text: Text<'db>,
        range: Range<usize>,
    }

    #[derive(Eq, PartialEq, Debug, Copy, Clone)]
    enum NextToken {
        Whitespace,
        Word,
        Sigil,
        Error,
    }

    impl<'db> Tokenizer<'db> {
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

        fn intern(&self, range: Range<usize>) -> InternedText<'db> {
            InternedText::new(self.db, S(&self.chunk_text.as_str(self.db)[range]))
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
            Token::new(
                self.db,
                self.intern(span.C()),
                span,
                TokenKind::Word,
            )
        }

        fn is_sigil_start(ch: char) -> bool {
            enum_iterator::all::<Sigil>().map(|s| s.start_char()).any(|c| c == ch)
        }

        fn eat_sigil(&mut self) -> Token<'db> {
            assert_eq!(self.peek_token(), Some(NextToken::Sigil));

            let all_sigils = enum_iterator::all::<Sigil>();
            let text = &self.chunk.text(self.db).as_str(self.db)[self.range.C()];

            for sigil in all_sigils {
                let sigil_str = sigil.as_str();
                if text.starts_with(sigil_str) {
                    let range_start = self.range.start;
                    self.range.start = range_start.checked_add(sigil_str.len()).X();
                    let span = range_start .. self.range.start;
                    return Token::new(
                        self.db,
                        self.intern(span.C()),
                        span,
                        TokenKind::Sigil(sigil),
                    )
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
            // The first error character has already been consumed by the caller.
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
            Token::new(
                self.db,
                self.intern(span.C()),
                span,
                TokenKind::Error,
            )
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
            Token::new(
                self.db,
                self.intern(span.C()),
                span,
                TokenKind::Whitespace,
            )
        }

        fn eat_char(&mut self, ch: char) {
            assert!(self.peek() == Some(ch));
            self.range.start = self.range.start.checked_add(ch.len_utf8()).X();
            assert!(self.range.start <= self.range.end);
        }

        fn peek(&self) -> Option<char> {
            self.chunk.text(self.db).as_str(self.db)[self.range.C()].chars().next()
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
    pub fn without_space(self, db: &'db dyn crate::Db) -> Option<Self> {
        match self.kind(db) {
            TokenKind::Whitespace => None,
            TokenKind::Comment => None,
            _ => Some(self),
        }
    }

    #[cfg(test)]
    pub fn debug_str(&self, db: &'db dyn crate::Db) -> &'db str {
        match self.kind(db) {
            TokenKind::Word | TokenKind::String => {
                self.text(db).as_str(db)
            }
            TokenKind::Sigil(s) => s.as_str(),
            TokenKind::Whitespace => "ws",
            TokenKind::Comment => "cmt",
            TokenKind::Error => "err",
        }
    }

    pub fn is_close_sigil(&self, db: &'db dyn crate::Db) -> bool {
        match self.kind(db) {
            TokenKind::Sigil(s) => s.is_close_sigil(),
            _ => false,
        }
    }

    pub fn word_str(&self, db: &'db dyn crate::Db) -> Option<&'db str> {
        if self.kind(db) != TokenKind::Word {
            return None;
        }
        Some(self.text(db).as_str(db))
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

            // Unicode bracket pairs.
            Sigil::MapOpen => "\u{2987}",
            Sigil::MapClose => "\u{2988}",
            Sigil::SetOpen => "\u{2983}",
            Sigil::SetClose => "\u{2984}",
            Sigil::TableOpen => "\u{27E6}",
            Sigil::TableClose => "\u{27E7}",
            Sigil::TensorOpen => "\u{27EA}",
            Sigil::TensorClose => "\u{27EB}",

            // Unicode sigils.
            Sigil::MapsTo => "\u{21A6}",
            Sigil::LessOrEqual => "\u{2264}",
            Sigil::GreaterOrEqual => "\u{2265}",
            Sigil::Identical => "\u{2261}",
            Sigil::NotIdentical => "\u{2262}",

            // Single-character sigils.
            Sigil::Dot => ".",
            Sigil::Comma => ",",
            Sigil::Semicolon => ";",
            Sigil::Plus => "+",
            Sigil::Minus => "-",
            Sigil::Star => "*",
            Sigil::SlashForward => "/",
            Sigil::Equals => "=",
            Sigil::Less => "<",
            Sigil::Greater => ">",
            Sigil::Pipe => "|",
            Sigil::Question => "?",
            Sigil::Exclamation => "!",
            Sigil::Colon => ":",
            Sigil::Hash => "#",
            Sigil::At => "@",
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

    fn start_char(&self) -> char {
        self.as_str().chars().next().X()
    }

    pub fn close_sigil(&self) -> Sigil {
        match self {
            Sigil::ParenOpen => Sigil::ParenClose,
            Sigil::BraceOpen => Sigil::BraceClose,
            Sigil::BracketOpen => Sigil::BracketClose,
            Sigil::MapOpen => Sigil::MapClose,
            Sigil::SetOpen => Sigil::SetClose,
            Sigil::TableOpen => Sigil::TableClose,
            Sigil::TensorOpen => Sigil::TensorClose,
            _ => bug!(),
        }
    }

    /// Get the corresponding opening sigil for a closing sigil.
    pub fn open_sigil(&self) -> Sigil {
        match self {
            Sigil::ParenClose => Sigil::ParenOpen,
            Sigil::BraceClose => Sigil::BraceOpen,
            Sigil::BracketClose => Sigil::BracketOpen,
            Sigil::MapClose => Sigil::MapOpen,
            Sigil::SetClose => Sigil::SetOpen,
            Sigil::TableClose => Sigil::TableOpen,
            Sigil::TensorClose => Sigil::TensorOpen,
            _ => bug!(),
        }
    }

    fn is_close_sigil(&self) -> bool {
        matches!(self,
            Sigil::ParenClose | Sigil::BraceClose | Sigil::BracketClose |
            Sigil::MapClose | Sigil::SetClose | Sigil::TableClose | Sigil::TensorClose
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
    // Unicode bracket pairs.
    assert_eq!(dbglex("\u{2987}a\u{2988}"), "\u{2987} a \u{2988}");
    assert_eq!(dbglex("\u{2983}a\u{2984}"), "\u{2983} a \u{2984}");
    assert_eq!(dbglex("\u{27E6}a\u{27E7}"), "\u{27E6} a \u{27E7}");
    assert_eq!(dbglex("\u{27EA}a\u{27EB}"), "\u{27EA} a \u{27EB}");
    // Unicode sigils.
    assert_eq!(dbglex("a \u{21A6} b"), "a ws \u{21A6} ws b");
    assert_eq!(dbglex("a \u{2264} b"), "a ws \u{2264} ws b");
    assert_eq!(dbglex("a \u{2261} b"), "a ws \u{2261} ws b");
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

    // Map and set brackets.
    assert_eq!(dbglex("\u{2987}a\u{2988}"), "\u{2987} a \u{2988}");
    assert_eq!(dbglex("\u{2983}a\u{2984}"), "\u{2983} a \u{2984}");

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

    // Comparison operators (unicode).
    assert_eq!(
        dbglex("a<b>c\u{2264}d\u{2265}e\u{2261}f\u{2262}g"),
        "a < b > c \u{2264} d \u{2265} e \u{2261} f \u{2262} g",
    );
    assert_eq!(
        dbglex("< > \u{2264} \u{2265} \u{2261} \u{2262}"),
        "< ws > ws \u{2264} ws \u{2265} ws \u{2261} ws \u{2262}",
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

    // Unicode bracket pairs (table, tensor).
    assert_eq!(dbglex("\u{27E6}a\u{27E7}"), "\u{27E6} a \u{27E7}");
    assert_eq!(dbglex("\u{27EA}a\u{27EB}"), "\u{27EA} a \u{27EB}");
    assert_eq!(dbglex("\u{2987}\u{27EA}a\u{27EB}\u{2988}"), "\u{2987} \u{27EA} a \u{27EB} \u{2988}");
}


