//! A grammar over tokens, the datalit and datafun grammars made executable.
//!
//! Both grammars are written over the tokens of `botdocs/lexical-grammar.md`,
//! and their recognizers run over the tokens the real lexer makes of an
//! input, with two kinds added from the newlines it records: an `EOL` before
//! a token outside brackets, in datafun, and a table's `newline` before a
//! token in a `{|` `|}` pair, outside brackets nested in it. A terminal
//! matches a token by its text, and `~` asserts that a token is written
//! against the one before it.
//!
//! Two kinds of rule match a run of tokens instead:
//!
//! - the rules under `(* Written in characters`, each the text of one glued
//!   run, read by those rules in characters;
//! - `type`, a run with its brackets matched, read by the type rules over its
//!   tokens with the `newline`s in its brackets dropped: a type reads them as
//!   whitespace.

use rmx::prelude::*;
use bct::random::SeededRng;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use bct::lexer::{Sigil, TokenKind};
use bct::lookup::LookupMap;

use crate::ebnf::{Expr, Generator, Grammar, Piece, Recognizer, Term, join_chars};

/// The special sequences over tokens.
pub const EOL: &str = "a newline outside brackets";
pub const ROW_NEWLINE: &str = "a newline in a table, outside brackets nested in it";
pub const NAME: &str = "an ASCII word not beginning with a digit";
pub const NUMBER_END: &str = r#"no "." or word glued after it"#;

/// The comment that heads the rules written in characters.
pub const CHARS_MARKER: &str = "(* Written in characters";

/// The rule read as a type: over its tokens, with newlines dropped.
pub const TYPE: &str = "type";

const ISLAND: &str = "island ";

// ============================================================================
// Tokens
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Token,
    Eol,
    RowNewline,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub kind: Kind,
    pub word: bool,
    pub start: usize,
    pub end: usize,
}

/// An input as the recognizer reads it: its tokens and its text, and the
/// runs of tokens each island has matched, as they are asked.
pub struct Tokens {
    pub toks: Vec<Tok>,
    pub text: Rc<str>,
    /// Where in the text the tokens end: its end, or the end of the last
    /// token of a run taken from it.
    end: usize,
    islands: RefCell<LookupMap<(usize, usize), Vec<usize>>>,
    /// For each island tried from a token, where the run it was tried over
    /// ends: the token past it.
    runs: RefCell<LookupMap<(usize, usize), usize>>,
}

impl Tokens {
    /// The tokens of `text`, with `EOL`s if `eols`.
    pub fn new(text: &str, eols: bool) -> Tokens {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let lexed = bct::lexer::lex(&db, source);
        let mut toks = vec![];
        let mut open: Vec<Sigil> = vec![];
        for t in lexed.tokens(&db) {
            let at = t.span().start;
            if t.newline_before {
                match open.last() {
                    None if eols => toks.push(Tok { kind: Kind::Eol, word: false, start: at, end: at }),
                    Some(Sigil::BracePipeOpen) => toks.push(Tok { kind: Kind::RowNewline, word: false, start: at, end: at }),
                    _ => {}
                }
            }
            if let TokenKind::Sigil(s) = t.kind {
                if is_open(s) {
                    open.push(s);
                } else if t.is_close_sigil() {
                    open.pop();
                }
            }
            toks.push(Tok { kind: Kind::Token, word: t.kind == TokenKind::Word, start: at, end: t.span().end });
        }
        Tokens::of(toks, text.into(), text.len())
    }

    fn of(toks: Vec<Tok>, text: Rc<str>, end: usize) -> Tokens {
        Tokens { toks, text, end, islands: RefCell::new(LookupMap::new()), runs: RefCell::new(LookupMap::new()) }
    }

    pub fn len(&self) -> usize {
        self.toks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    fn token_text(&self, i: usize) -> Option<&str> {
        let t = self.toks.get(i)?;
        (t.kind == Kind::Token).then(|| &self.text[t.start..t.end])
    }

    /// Whether token `i` is written against the token before it.
    fn glued(&self, i: usize) -> bool {
        i > 0
            && i < self.toks.len()
            && self.toks[i].kind == Kind::Token
            && self.toks[i - 1].kind == Kind::Token
            && self.toks[i - 1].end == self.toks[i].start
    }

    /// The tokens from `i` to `j` with the table newlines among them
    /// dropped, and for each of them the index it had here.
    fn without_newlines(&self, i: usize, j: usize) -> (Tokens, Vec<usize>) {
        let kept: Vec<usize> = (i..j).filter(|&k| self.toks[k].kind == Kind::Token).collect();
        let toks: Vec<Tok> = kept.iter().map(|&k| self.toks[k].C()).collect();
        let end = toks.last().map_or(self.byte(i), |t| t.end);
        (Tokens::of(toks, self.text.C(), end), kept)
    }

    /// Where token `i` begins in the text, or where the tokens end past the last.
    fn byte(&self, i: usize) -> usize {
        self.toks.get(i).map_or(self.end, |t| t.start)
    }
}

fn is_open(s: Sigil) -> bool {
    matches!(
        s,
        Sigil::ParenOpen
            | Sigil::BraceOpen
            | Sigil::BracketOpen
            | Sigil::AngleOpen
            | Sigil::ParenPipeOpen
            | Sigil::BracePipeOpen
            | Sigil::BracketPipeOpen
            | Sigil::AnglePipeOpen
            | Sigil::PercentBraceOpen
            | Sigil::HashBraceOpen
    )
}

const OPENS: &[&str] = &["(", "[", "{", "<", "(|", "{|", "[|", "<|", "%{", "#{"];
const CLOSES: &[&str] = &[")", "]", "}", ">", "|)", "|}", "|]", "|>"];

// ============================================================================
// The grammar
// ============================================================================

/// What the special sequences of the character rules mean, at a character.
pub type CharTest = fn(&[char], usize) -> bool;

/// A grammar over tokens with the rules it reads in characters.
pub struct TokenGrammar {
    /// The rules over tokens, each character rule they use replaced by its
    /// island, but `type` written out: what generating expands.
    pub typed: Grammar,
    /// `typed` with `type` an island too, if the grammar has one: what
    /// recognizing reads.
    pub tokens: Grammar,
    /// The rules written in characters.
    pub chars: Grammar,
    /// The character rules the token rules use, by island number.
    islands: Vec<String>,
    special: fn(&str) -> CharTest,
}

/// The names of the rules a grammar block writes after [`CHARS_MARKER`].
pub fn char_rule_names(doc: &str, name: &str) -> Vec<String> {
    let block = doc.split("```ebnf\n").nth(1).X().split("```").next().X();
    let at = block.find(CHARS_MARKER).unwrap_or_else(|| panic!("{name} has no `{CHARS_MARKER}`"));
    Grammar::parse(&block[at..]).rules.into_iter().map(|r| r.name).collect()
}

impl TokenGrammar {
    /// Splits `g` into its rules over tokens and those in characters, which
    /// are `chars`; `special` gives the meaning of a character rule's
    /// special sequences.
    pub fn new(g: &Grammar, chars: &[String], special: fn(&str) -> CharTest) -> TokenGrammar {
        let char_rules = Grammar { rules: g.rules.iter().filter(|r| chars.contains(&r.name)).cloned().collect() };
        let mut token_rules: Vec<_> = g.rules.iter().filter(|r| !chars.contains(&r.name)).cloned().collect();
        let mut islands = vec![];
        for rule in &token_rules {
            rule.body.visit(&mut |e| {
                if let Expr::Ref(r) = e && chars.contains(r) && !islands.contains(r) {
                    islands.push(r.C());
                }
            });
        }
        for name in &islands {
            token_rules.push(crate::ebnf::Rule { name: name.C(), body: Expr::Special(fmt!("{ISLAND}{name}")) });
        }
        let typed = Grammar { rules: token_rules };
        let mut tokens = typed.C();
        if let Some(rule) = tokens.get_mut(TYPE) {
            rule.body = Expr::Special(fmt!("{ISLAND}{TYPE}"));
        }
        TokenGrammar { typed, tokens, chars: char_rules, islands, special }
    }

    /// The words an exception's `b` names, through the rules of both kinds.
    fn words(&self, b: &Expr) -> BTreeSet<String> {
        let mut words = self.typed.words(b);
        words.extend(self.chars.words(b));
        words
    }
}

// ============================================================================
// Recognizing
// ============================================================================

/// The islands of a grammar: its character rules, by island number, then
/// its type, if it has one.
struct Islands {
    chars: Rc<Vec<Recognizer<[char]>>>,
    ty: Option<Rc<Recognizer<Tokens>>>,
}

/// Recognizers for some start rules of a grammar over tokens.
pub struct Recognizers {
    recs: Vec<(String, Recognizer<Tokens>)>,
    islands: Rc<Islands>,
    eols: bool,
}

impl Recognizers {
    /// Recognizers for `starts`, over tokens with `EOL`s if `eols`.
    pub fn new(g: &TokenGrammar, starts: &[&str], eols: bool) -> Recognizers {
        let chars: Rc<Vec<_>> = Rc::new(
            g.islands.iter().map(|name| Recognizer::new(&g.chars, name, &|e: &Expr| crate::ebnf::char_term(e, g.special))).collect(),
        );
        // The type's own recognizer reads no type island: its rules are written out.
        let inner = Rc::new(Islands { chars: chars.C(), ty: None });
        let ty = g.typed.get(TYPE).map(|_| Rc::new(Recognizer::new(&g.typed, TYPE, &|e: &Expr| token_term(g, &inner, e))));
        let islands = Rc::new(Islands { chars, ty });
        let recs = starts.iter().map(|s| (s.S(), Recognizer::new(&g.tokens, s, &|e: &Expr| token_term(g, &islands, e)))).collect();
        Recognizers { recs, islands, eols }
    }

    fn rec(&self, start: &str) -> &Recognizer<Tokens> {
        &self.recs.iter().find(|(s, _)| s == start).unwrap_or_else(|| panic!("no recognizer for {start}")).1
    }

    pub fn tokens(&self, text: &str) -> Tokens {
        Tokens::new(text, self.eols)
    }

    pub fn derives(&self, start: &str, text: &str) -> bool {
        let tokens = self.tokens(text);
        self.rec(start).accepts(&tokens, tokens.len())
    }

    /// The byte offset where the grammar first cannot go on, or `None` if it
    /// derives the text.
    ///
    /// Over tokens that is the first token it cannot take. But an island is
    /// matched whole or not at all, so where one was tried and failed, at or
    /// before that token, it is read for how far into it it got, and the
    /// furthest any got is where the grammar stops.
    pub fn first_error(&self, start: &str, text: &str) -> Option<usize> {
        let tokens = self.tokens(text);
        let rec = self.rec(start);
        if rec.accepts(&tokens, tokens.len()) {
            return None;
        }
        Some(self.reach(rec, &tokens))
    }

    /// How far into `tokens`, in bytes, `rec` gets.
    fn reach(&self, rec: &Recognizer<Tokens>, tokens: &Tokens) -> usize {
        let i = rec.viable_prefix(tokens, tokens.len());
        let mut at = tokens.byte(i);
        let runs: Vec<((usize, usize), usize)> = tokens.runs.borrow().iter_unordered().map(|(k, v)| (*k, *v)).collect();
        let islands = &self.islands;
        for ((k, j), end) in runs {
            if j > i || end <= j {
                continue;
            }
            let from = tokens.toks[j].start;
            let reached = if let Some(island) = islands.chars.get(k) {
                let cs: Vec<char> = tokens.text[from..tokens.toks[end - 1].end].chars().collect();
                let n = island.viable_prefix(&cs, cs.len());
                from + cs[..n].iter().map(|c| c.len_utf8()).sum::<usize>()
            } else {
                let (sub, _) = tokens.without_newlines(j, end);
                self.reach(islands.ty.as_ref().X(), &sub)
            };
            at = at.max(reached);
        }
        at
    }
}

/// The terminal over tokens for `e`.
fn token_term(g: &TokenGrammar, islands: &Rc<Islands>, e: &Expr) -> Term<Tokens> {
    match e {
        Expr::Lit(s) => {
            let s = s.C();
            Rc::new(move |input: &Tokens, i, out: &mut Vec<usize>| {
                if input.token_text(i) == Some(s.as_str()) {
                    out.push(1);
                }
            })
        }
        Expr::Glue(None) => Rc::new(|input: &Tokens, i, out: &mut Vec<usize>| {
            if input.glued(i) {
                out.push(0);
            }
        }),
        Expr::Except(_, b) => {
            let words = g.words(b);
            Rc::new(move |input: &Tokens, i, out: &mut Vec<usize>| {
                if !input.token_text(i).is_some_and(|t| words.contains(t)) {
                    out.push(0);
                }
            })
        }
        Expr::Special(text) if text == EOL => pseudo(Kind::Eol),
        Expr::Special(text) if text == ROW_NEWLINE => pseudo(Kind::RowNewline),
        Expr::Special(text) if text == NAME => Rc::new(|input: &Tokens, i, out: &mut Vec<usize>| {
            let is_name = input.token_text(i).is_some_and(|t| {
                input.toks[i].word
                    && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !t.starts_with(|c: char| c.is_ascii_digit())
            });
            if is_name {
                out.push(1);
            }
        }),
        Expr::Special(text) if text == NUMBER_END => Rc::new(|input: &Tokens, i, out: &mut Vec<usize>| {
            let continues = input.glued(i) && (input.toks[i].word || input.token_text(i) == Some("."));
            if !continues {
                out.push(0);
            }
        }),
        Expr::Special(text) if text.starts_with(ISLAND) => {
            let name = &text[ISLAND.len()..];
            let k = if name == TYPE {
                g.islands.len()
            } else {
                g.islands.iter().position(|n| n == name).X()
            };
            let islands = islands.C();
            Rc::new(move |input: &Tokens, i, out: &mut Vec<usize>| {
                out.extend(island_matches(input, &islands, k, i));
            })
        }
        e => panic!("no terminal over tokens for {e:?}"),
    }
}

fn pseudo(kind: Kind) -> Term<Tokens> {
    Rc::new(move |input: &Tokens, i, out: &mut Vec<usize>| {
        if input.toks.get(i).is_some_and(|t| t.kind == kind) {
            out.push(1);
        }
    })
}

/// How many tokens from `i` each match of island `k` takes.
fn island_matches(input: &Tokens, islands: &Islands, k: usize, i: usize) -> Vec<usize> {
    if let Some(found) = input.islands.borrow().get(&(k, i)) {
        return found.C();
    }
    let (found, end) = match islands.chars.get(k) {
        Some(rec) => chars_matches(input, rec, i),
        None => type_matches(input, islands.ty.as_ref().X(), i),
    };
    input.islands.borrow_mut().insert((k, i), found.C());
    input.runs.borrow_mut().insert((k, i), end);
    found
}

/// The longest run a character rule is tried over is one glued run of tokens.
fn chars_matches(input: &Tokens, rec: &Recognizer<[char]>, i: usize) -> (Vec<usize>, usize) {
    if input.token_text(i).is_none() {
        return (vec![], i);
    }
    let mut j = i + 1;
    while input.glued(j) {
        j += 1;
    }
    let from = input.toks[i].start;
    let cs: Vec<char> = input.text[from..input.toks[j - 1].end].chars().collect();
    // The character count at the end of each token of the run.
    let mut ends = vec![];
    let mut count = 0;
    for t in &input.toks[i..j] {
        count += input.text[t.start..t.end].chars().count();
        ends.push(count);
    }
    let found = rec
        .prefixes(&cs, cs.len())
        .into_iter()
        .filter_map(|n| ends.iter().position(|&e| e == n).map(|k| k + 1))
        .collect();
    (found, j)
}

/// A type is tried over a run with its brackets matched, and the newlines
/// inside them, but not the tokens past the bracket it is in.
fn type_matches(input: &Tokens, rec: &Recognizer<Tokens>, i: usize) -> (Vec<usize>, usize) {
    let mut depth = 0i32;
    let mut j = i;
    while j < input.len() && j - i < 4096 {
        match input.toks[j].kind {
            Kind::RowNewline if depth > 0 => {
                j += 1;
                continue;
            }
            Kind::Token => {}
            _ => break,
        }
        let t = input.token_text(j).X();
        if OPENS.contains(&t) {
            depth += 1;
        } else if CLOSES.contains(&t) {
            depth -= 1;
            if depth < 0 {
                break;
            }
        }
        j += 1;
    }
    let (sub, kept) = input.without_newlines(i, j);
    let found = rec.prefixes(&sub, sub.len()).into_iter().filter(|&n| n > 0).map(|n| kept[n - 1] + 1 - i).collect();
    (found, j)
}

// ============================================================================
// Generating
// ============================================================================

const NAMES: &[&str] = &["x", "y", "f", "go", "a1", "T", "_z", "xs", "none", "int", "rec", "ref"];

/// A string for a special sequence or island, when generating.
pub fn sample(g: &TokenGrammar, char_sample: &dyn Fn(&str, &mut SeededRng) -> String, text: &str, rng: &mut SeededRng) -> String {
    match text {
        EOL => "\n".S(),
        ROW_NEWLINE => if rng.random_bool(0.5) { "\n" } else { ";" }.S(),
        NAME => NAMES[rng.random_range(0..NAMES.len())].S(),
        NUMBER_END => String::new(),
        t if t.starts_with(ISLAND) => {
            let generator = Generator::new(&g.chars, char_sample);
            join_chars(&generator.generate(&t[ISLAND.len()..], rng), &|name, _, _| {
                panic!("no assertion {name} is in the character rules")
            })
        }
        t => panic!("no sample is given here for the special sequence `? {t} ?`"),
    }
}

/// Generated tokens as text: glued after a `~`, a space where the two
/// would lex as something else written together, and elsewhere a space or
/// not, at random. A newline is a token on its own.
pub fn join_tokens(pieces: &[Piece], rng: &mut SeededRng) -> String {
    let db = datalove_datalit::Database::default();
    let lex = |t: &str| -> Vec<(usize, usize)> {
        let source = bct::input::Source::new(&db, t.S());
        bct::lexer::lex(&db, source).tokens(&db).iter().map(|t| (t.span().start, t.span().end)).collect()
    };
    let merges = |a: &str, b: &str| {
        let mut apart = lex(a);
        apart.extend(lex(b).into_iter().map(|(s, e)| (s + a.len(), e + a.len())));
        lex(&fmt!("{a}{b}")) != apart
    };
    let mut text = String::new();
    let mut last = String::new();
    let mut glue = true;
    for piece in pieces {
        match piece {
            Piece::Mark(m) if m == "~" => glue = true,
            Piece::Mark(m) => panic!("no assertion {m} is in the rules over tokens"),
            Piece::Text(t) if t.is_empty() => {}
            Piece::Text(t) => {
                let newline = t == "\n" || t == ";" || text.ends_with('\n') || text.is_empty();
                if !glue && !newline && (merges(&last, t) || rng.random_bool(0.5)) {
                    text.push(' ');
                }
                text.push_str(t);
                last = t.C();
                glue = false;
            }
        }
    }
    text
}
