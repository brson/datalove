//! The datalit grammar in `botdocs/datalit-ebnf.md`, made executable.
//!
//! The grammar is over characters. It is taken as written but for the rules
//! the document states in English, which are added here: words are whole (an
//! assertion around each word the grammar writes), comments are whole (an
//! assertion after each `/` the grammar writes, and after a line comment), a
//! type name is no word the type grammar uses (an assertion before it), and
//! inside a table, outside nested brackets, `ws` takes no newline (a copy of
//! every rule a table row reaches, named `rule@row`). The meaning of each
//! special sequence is here too, and a new one fails until it is given one.
//!
//! `tests/datalit_grammar_tests.rs` checks it against the datalit parser, and
//! the datafun grammar's test uses its type rules and lexical rules.

use rmx::prelude::*;
use bct::random::SeededRng;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::ebnf::{Expr, Grammar, Rule, Term};
use crate::grammar_check::{Diag, catch_panic, diags, workspace_dir};

pub const DOC: &str = "botdocs/datalit-ebnf.md";

pub fn doc_text() -> String {
    std::fs::read_to_string(workspace_dir().join(DOC)).X()
}

/// A test on the characters at a position: for a special sequence, whether
/// the character there matches; for an assertion, whether the position
/// itself is acceptable, which may be the end.
pub type CharTest = fn(&[char], usize) -> bool;

/// The terminal over characters for a literal, range, special sequence or
/// assertion of [`grammar`].
pub fn term(e: &Expr) -> Term<[char]> {
    crate::ebnf::char_term(e, special)
}

/// What the datalit parser reports of `text`, or its panic.
pub fn parse_diags(text: &str) -> Result<Vec<Diag>, String> {
    catch_panic(|| {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let found = datalove_datalit::parser::parse_integration_test::accumulated::<
            datalove_diagnostic::ParseDiagnostic,
        >(&db, source);
        diags(&db, &found)
    })
}

/// The codes the datalit parser refuses `text` with, or its panic.
pub fn parse_codes(text: &str) -> Result<Vec<String>, String> {
    parse_diags(text).map(|ds| ds.into_iter().map(|d| d.code).collect())
}

/// The assertion added around each word: no word character on both sides.
const GLUE: &str = "word boundary";

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn starts(s: &[char], i: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(k, p)| s.get(i + k) == Some(&p))
}

fn ws_char(s: &[char], i: usize) -> bool {
    s[i].is_whitespace() && s[i] != '\n'
}

fn string_char(s: &[char], i: usize) -> bool {
    s[i] != '"' && s[i] != '\\'
}

fn line_char(s: &[char], i: usize) -> bool {
    s[i] != '\n'
}

fn block_char(s: &[char], i: usize) -> bool {
    !(starts(s, i, "/*") || starts(s, i, "*/"))
}

fn word_boundary(s: &[char], i: usize) -> bool {
    !(i > 0 && i < s.len() && is_word(s[i - 1]) && is_word(s[i]))
}

/// The assertion after a line comment: it runs to the end of its line.
const LINE_END: &str = "line end";

fn line_end(s: &[char], i: usize) -> bool {
    i == s.len() || s[i] == '\n'
}

/// The assertion after a `/` token: no `/` or `*` glued to it, which would
/// make the two a comment opener.
const NO_COMMENT: &str = "no comment opener";

fn no_comment(s: &[char], i: usize) -> bool {
    !matches!(s.get(i), Some('/' | '*'))
}

/// The assertion before a type name: it is not a word the type grammar uses.
const NOT_TYPE_WORD: &str = "not a type word";

/// The words the type grammar uses, read from it by [`grammar`].
static TYPE_WORDS: OnceLock<BTreeSet<String>> = OnceLock::new();

fn not_type_word(s: &[char], i: usize) -> bool {
    let word: String = s[i..].iter().take_while(|c| is_word(**c)).collect();
    !TYPE_WORDS.get().expect("the type words are read with the grammar").contains(&word)
}

/// What each special sequence in the grammar means.
pub fn special(text: &str) -> CharTest {
    match text {
        r#"any Unicode whitespace character but "\n""# => ws_char,
        r#"any character except '"' and '\', newlines included"# => string_char,
        "any character except newline" => line_char,
        r#"any character, not starting "/*" or "*/""# => block_char,
        GLUE => word_boundary,
        LINE_END => line_end,
        NOT_TYPE_WORD => not_type_word,
        NO_COMMENT => no_comment,
        t => panic!("no meaning is given here for the special sequence `? {t} ?`"),
    }
}

/// A character for a special sequence, when generating.
pub fn sample(text: &str, rng: &mut SeededRng) -> String {
    let pool: &[char] = match text {
        r#"any Unicode whitespace character but "\n""# => &[' ', ' ', ' ', '\t', '\r', '\u{3000}'],
        r#"any character except '"' and '\', newlines included"# => &['a', 'Z', ' ', '\n', '{', '|', ';', 'é', '/', '*'],
        "any character except newline" => &['a', ' ', '/', '*', ';', '}', 'é'],
        r#"any character, not starting "/*" or "*/""# => &['a', ' ', '\n', '*', '/', ';', '{'],
        t => panic!("no sample is given here for the special sequence `? {t} ?`"),
    };
    pool[rng.random_range(0..pool.len())].S()
}

/// What to insert where an assertion is reached, when generating.
pub fn repair(name: &str, before: &str, after: &str) -> &'static str {
    match name {
        GLUE => {
            let glued = before.ends_with(is_word) && after.starts_with(is_word);
            if glued { " " } else { "" }
        }
        LINE_END => {
            if after.is_empty() || after.starts_with('\n') { "" } else { "\n" }
        }
        NOT_TYPE_WORD => "",
        NO_COMMENT => {
            if after.starts_with(['/', '*']) { " " } else { "" }
        }
        t => panic!("no repair is given here for the assertion {t}"),
    }
}

/// The rules that are each one word, or whitespace, and whose insides are not
/// words of their own.
const LEXEMES: &[&str] = &["ident", "numeric_lit", "string_lit", "digit_run", "rank", "ws"];

fn wordish(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(is_word)
}

/// Keeps the tokens written outside the lexical rules whole: a word boundary
/// around each keyword and each word-sized rule, and no comment opener made
/// from a `/` and what follows it.
fn whole_tokens(g: Grammar) -> Grammar {
    let lexical = g.reachable(LEXEMES);
    fn wrap(e: &Expr) -> Expr {
        let boundary = || Expr::Assert(GLUE.S());
        match e {
            Expr::Lit(s) if wordish(s) => Expr::Seq(vec![boundary(), e.C(), boundary()]),
            Expr::Lit(s) if s.ends_with('/') => Expr::Seq(vec![e.C(), Expr::Assert(NO_COMMENT.S())]),
            Expr::Ref(r) if LEXEMES.contains(&r.as_str()) && r != "ws" => {
                Expr::Seq(vec![boundary(), e.C(), boundary()])
            }
            Expr::Alt(xs) => Expr::Alt(xs.iter().map(wrap).collect()),
            Expr::Seq(xs) => Expr::Seq(xs.iter().map(wrap).collect()),
            Expr::Opt(x) => Expr::Opt(Box::new(wrap(x))),
            Expr::Rep(x) => Expr::Rep(Box::new(wrap(x))),
            _ => e.C(),
        }
    }
    let rules = g
        .rules
        .iter()
        .map(|r| {
            if lexical.contains(&r.name) {
                r.C()
            } else {
                Rule { name: r.name.C(), body: wrap(&r.body) }
            }
        })
        .collect();
    Grammar { rules }
}

const OPENERS: &[&str] = &["(", "[", "{", "%{", "#{", "{|", "[|"];
const CLOSERS: &[&str] = &[")", "]", "}", "|}", "|]"];
const TABLE_OPEN: &str = "{|";
/// The rule whose `{|` opens rows; a table type's does not.
const TABLE_RULE: &str = "table_expr";

/// Makes a table's rows take no newline in their `ws`.
///
/// Every rule reached between a `table_expr`'s `{|` and `|}`, outside other
/// brackets, gets a `@row` copy in which `ws` is `ws@row`, which is `ws`
/// without its newline.
struct Rows<'g> {
    g: &'g Grammar,
    lexical: BTreeSet<String>,
    made: BTreeSet<String>,
    out: Vec<Rule>,
}

impl<'g> Rows<'g> {
    /// Rewrites `e`, written in `rule`, as it is in a row when `row`.
    fn walk(&mut self, e: &Expr, row: bool, rule: &str) -> Expr {
        match e {
            Expr::Seq(xs) => {
                let mut stack: Vec<bool> = vec![];
                let mut out = vec![];
                for x in xs {
                    match x {
                        Expr::Lit(s) if CLOSERS.contains(&s.as_str()) && !stack.is_empty() => {
                            stack.pop();
                            out.push(x.C());
                        }
                        Expr::Lit(s) if OPENERS.contains(&s.as_str()) => {
                            stack.push(s == TABLE_OPEN && rule == TABLE_RULE);
                            out.push(x.C());
                        }
                        _ => {
                            let ctx = *stack.last().unwrap_or(&row);
                            out.push(self.walk(x, ctx, rule));
                        }
                    }
                }
                Expr::Seq(out)
            }
            Expr::Alt(xs) => Expr::Alt(xs.iter().map(|x| self.walk(x, row, rule)).collect()),
            Expr::Opt(x) => Expr::Opt(Box::new(self.walk(x, row, rule))),
            Expr::Rep(x) => Expr::Rep(Box::new(self.walk(x, row, rule))),
            Expr::Ref(r) if row && r == "ws" => Expr::Ref("ws@row".S()),
            Expr::Ref(r) if row && !self.lexical.contains(r) => {
                let name = fmt!("{r}@row");
                if self.made.insert(name.C()) {
                    let body = self.g.get(r).X().body.C();
                    let body = self.walk(&body, true, r);
                    self.out.push(Rule { name: name.C(), body });
                }
                Expr::Ref(name)
            }
            _ => e.C(),
        }
    }
}

fn table_rows(g: Grammar) -> Grammar {
    let ws = &g.get("ws").expect("the grammar has ws").body;
    let want = Expr::Rep(Box::new(Expr::Alt(vec![
        Expr::Ref("whitespace".S()),
        Expr::Ref("newline".S()),
        Expr::Ref("comment".S()),
    ])));
    assert_eq!(ws, &want, "ws@row is written here for ws = {{ whitespace | newline | comment }}");
    let mut rows = Rows {
        g: &g,
        lexical: g.reachable(LEXEMES),
        made: BTreeSet::new(),
        out: vec![],
    };
    let mut rules: Vec<Rule> =
        g.rules.iter().map(|r| Rule { name: r.name.C(), body: rows.walk(&r.body, false, &r.name) }).collect();
    rules.append(&mut rows.out);
    rules.push(Rule {
        name: "ws@row".S(),
        body: Expr::Rep(Box::new(Expr::Alt(vec![Expr::Ref("whitespace".S()), Expr::Ref("comment".S())]))),
    });
    Grammar { rules }
}

/// The keywords written in the rules a type reaches, outside the lexical ones.
fn type_words(g: &Grammar) -> BTreeSet<String> {
    let lexical = g.reachable(LEXEMES);
    let mut words = BTreeSet::new();
    for name in g.reachable(&["type"]) {
        if !lexical.contains(&name) {
            g.get(&name).X().body.visit(&mut |e| {
                if let Expr::Lit(s) = e && wordish(s) {
                    words.insert(s.C());
                }
            });
        }
    }
    words
}

/// Adds the rules about names and comments the document states in English:
/// a line comment runs to the end of its line, and a type name is no word the
/// type grammar uses.
fn names_and_comments(mut g: Grammar) -> Grammar {
    for rule in &mut g.rules {
        match rule.name.as_str() {
            "comment" => {
                let line = Expr::Ref("line_comment".S());
                let Expr::Alt(xs) = &mut rule.body else { panic!("comment is an alternation") };
                let x = xs.iter_mut().find(|x| **x == line).expect("comment has line_comment");
                *x = Expr::Seq(vec![line, Expr::Assert(LINE_END.S())]);
            }
            "type_name" => {
                rule.body = Expr::Seq(vec![Expr::Assert(NOT_TYPE_WORD.S()), rule.body.C()]);
            }
            _ => {}
        }
    }
    g
}

/// The grammar of [`DOC`], with the rules it states in English added.
pub fn grammar() -> Grammar {
    let g = Grammar::from_markdown(&doc_text());
    let words = type_words(&g);
    assert!(words.contains("bool") && words.contains("rec"), "type words: {words:?}");
    TYPE_WORDS.get_or_init(|| words);
    table_rows(whole_tokens(names_and_comments(g)))
}

