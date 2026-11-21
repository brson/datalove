//! The lexical grammar in `botdocs/lexical-grammar.md`, made executable.
//!
//! The grammar is over characters, and lexes by its one rule outside the
//! EBNF: from where the last lexeme ended, the longest match of `lexeme`.
//! What each special sequence means is here, and a new one fails until it is
//! given a meaning. `tests/lexical_grammar_tests.rs` checks the lexer against it.

use rmx::prelude::*;
use bct::random::SeededRng;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::OnceLock;

use crate::ebnf::{Expr, Grammar, Recognizer, Term};
use crate::grammar_check::workspace_dir;

pub const DOC: &str = "botdocs/lexical-grammar.md";

pub fn doc_text() -> String {
    std::fs::read_to_string(workspace_dir().join(DOC)).X()
}

const WORD_CHAR: &str = "a Unicode alphanumeric character";
const STRING_CHAR: &str = r#"any character except '"' and '\'"#;
const ANY: &str = "any character";
const WHITESPACE: &str = r#"a Unicode whitespace character but "\n""#;
const LINE_CHAR: &str = r#"any character except "\n""#;
const COMMENT_CHAR: &str = r#"any character, not starting "/*" or "*/""#;
const STRAY: &str = "a character that begins no other lexeme";
const END: &str = "the end of the source";

/// The special sequences that match no character but test a position.
const ZERO_WIDTH: &[&str] = &[END];

fn starts(s: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| s.get(i + k) == Some(&p))
}

/// The first characters of the sigils, read from the grammar's `sigil` rule.
static SIGIL_STARTS: OnceLock<BTreeSet<char>> = OnceLock::new();

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn stray(c: char) -> bool {
    let sigil = SIGIL_STARTS.get().expect("the sigils are read with the grammar").contains(&c);
    !(c.is_whitespace() || c == '"' || is_word_char(c) || sigil)
}

/// What each special sequence means, at a position.
fn special(text: &str) -> fn(&[char], usize) -> bool {
    match text {
        WORD_CHAR => |s, i| s[i].is_alphanumeric(),
        STRING_CHAR => |s, i| s[i] != '"' && s[i] != '\\',
        ANY => |_, _| true,
        WHITESPACE => |s, i| s[i].is_whitespace() && s[i] != '\n',
        LINE_CHAR => |s, i| s[i] != '\n',
        COMMENT_CHAR => |s, i| !(starts(s, i, "/*") || starts(s, i, "*/")),
        STRAY => |s, i| stray(s[i]),
        END => |s, i| i == s.len(),
        t => panic!("no meaning is given here for the special sequence `? {t} ?`"),
    }
}

/// The terminal over characters for a literal, special sequence or assertion.
pub fn term(e: &Expr) -> Term<[char]> {
    crate::ebnf::char_term(e, special)
}

/// A character for a special sequence, when generating.
pub fn sample(text: &str, rng: &mut SeededRng) -> String {
    let pool: &[char] = match text {
        WORD_CHAR => &['a', 'Z', '0', '9', 'é', 'ß', '١'],
        STRING_CHAR => &['a', ' ', '\n', ';', '/', '*', 'é', '|'],
        ANY => &['a', '"', '\\', 'n', ' ', 'é'],
        WHITESPACE => &[' ', ' ', '\t', '\r', '\u{3000}', '\u{85}'],
        LINE_CHAR => &['a', ' ', '/', '*', ';', '"', 'é'],
        COMMENT_CHAR => &['a', ' ', '\n', ';', '"', '*', '/', 'é'],
        STRAY => &['&', '^', '\'', '`', '\\', '\u{0}', '→', '€'],
        t => panic!("no sample is given here for the special sequence `? {t} ?`"),
    };
    pool[rng.random_range(0..pool.len())].S()
}

/// The lexical grammar, its zero-width special sequences made assertions.
pub fn grammar() -> Grammar {
    let mut g = Grammar::from_markdown(&doc_text());
    for rule in &mut g.rules {
        rule.body = rule.body.map(&mut |e| match e {
            Expr::Special(t) if ZERO_WIDTH.contains(&t.as_str()) => Expr::Assert(t),
            e => e,
        });
    }
    SIGIL_STARTS.get_or_init(|| sigils(&g).iter().map(|s| s.chars().next().X()).collect());
    g
}

/// The sigils the grammar lists.
pub fn sigils(g: &Grammar) -> BTreeSet<String> {
    g.words(&g.get("sigil").expect("the grammar has sigil").body)
}

/// What a lexeme is: one of the alternatives of `lexeme` and `token`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Word,
    Sigil,
    String,
    Error,
    Whitespace,
    Newline,
    Comment,
}

const KINDS: &[(Kind, &str)] = &[
    (Kind::Word, "word"),
    (Kind::Sigil, "sigil"),
    (Kind::String, "string"),
    (Kind::Error, "error"),
    (Kind::Whitespace, "whitespace"),
    (Kind::Newline, "newline"),
    (Kind::Comment, "comment"),
];

/// A token as the checks compare them: its kind, its span in bytes, and
/// whether a newline lexeme came before it since the token before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub span: Range<usize>,
    pub newline_before: bool,
}

/// A lexer made of the grammar.
pub struct Lexer {
    lexeme: Recognizer<[char]>,
}

impl Lexer {
    pub fn new(g: &Grammar) -> Lexer {
        Lexer { lexeme: Recognizer::new(g, "lexeme", &term) }
    }

    /// The tokens of `text`, or what stopped the lexing: a place no lexeme
    /// begins, or a lexeme of more than one kind.
    pub fn tokens(&self, text: &str) -> Result<Vec<Token>, String> {
        let cs: Vec<char> = text.chars().collect();
        let mut out = vec![];
        let mut newline = false;
        let (mut i, mut byte) = (0, 0);
        while i < cs.len() {
            let rest = &cs[i..];
            let Some((n, rules)) = self.lexeme.longest(rest, rest.len()).filter(|(n, _)| *n > 0) else {
                return Err(fmt!("no lexeme begins at byte {byte}"));
            };
            let kinds: Vec<Kind> =
                KINDS.iter().filter(|(_, rule)| rules.contains(rule)).map(|(k, _)| *k).collect();
            let [kind] = kinds[..] else {
                return Err(fmt!("the lexeme at byte {byte} is {kinds:?}"));
            };
            let len: usize = rest[..n].iter().map(|c| c.len_utf8()).sum();
            match kind {
                Kind::Whitespace | Kind::Comment => {}
                Kind::Newline => newline = true,
                kind => {
                    out.push(Token { kind, span: byte..byte + len, newline_before: newline });
                    newline = false;
                }
            }
            i += n;
            byte += len;
        }
        Ok(out)
    }
}

/// The tokens the lexer in `bcts` makes of `text`.
pub fn lexer_tokens(text: &str) -> Vec<Token> {
    use bct::lexer::TokenKind;
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, text.S());
    let lexed = bct::lexer::lex(&db, source);
    lexed
        .tokens(&db)
        .iter()
        .map(|t| Token {
            kind: match t.kind {
                TokenKind::Word => Kind::Word,
                TokenKind::Sigil(_) => Kind::Sigil,
                TokenKind::String => Kind::String,
                TokenKind::Error => Kind::Error,
            },
            span: t.span(),
            newline_before: t.newline_before,
        })
        .collect()
}
