//! The datalit grammar in `botdocs/datalit-ebnf.md`, checked against the parser.
//!
//! The grammar's one `ebnf` block is read and compiled to a recognizer, and
//! every input is given to both it and `datalove_datalit::parser::parse`:
//!
//! - what the parser accepts, the grammar must derive;
//! - what the grammar derives, the parser must accept, or refuse only with
//!   the codes the document lists as refused outside the grammar.
//!
//! The inputs are the document's own examples, every `.dlt` file under
//! `crates/`, strings generated from the grammar, and all of those mutated a
//! character at a time. A disagreement is shrunk to the shortest input that
//! still disagrees the same way.
//!
//! The grammar is taken as written but for the rules the document states in
//! English, which are added here: words are whole (an assertion around each
//! word the grammar writes), comments are whole (an assertion after each `/`
//! the grammar writes, and after a line comment), a type name is no word the
//! type grammar uses (an assertion before it), and inside a table, outside
//! nested brackets, `ws` takes no newline (a copy of every rule a table row
//! reaches, named `rule@row`). The meaning of each special sequence is here
//! too, and a new one fails the test until it is given one.
//!
//! A string is generated from each seed of `DATALOVE_GRAMMAR_SEED` and
//! `DATALOVE_GRAMMAR_COUNT` (see `bct::random::env_seeds`).

use rmx::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::ebnf::{CharTest, Expr, Generator, Grammar, Recognizer, Rule};

const DOC: &str = "botdocs/datalit-ebnf.md";

/// The sentence of the document that lists the codes refused outside the grammar.
const OUTSIDE_MARKER: &str = "then only with these codes:";

fn workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn doc_text() -> String {
    std::fs::read_to_string(workspace_dir().join(DOC)).X()
}

// ============================================================================
// The grammar, made executable
// ============================================================================

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
fn special(text: &str) -> CharTest {
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
fn sample(text: &str, rng: &mut SeededRng) -> char {
    let pool: &[char] = match text {
        r#"any Unicode whitespace character but "\n""# => &[' ', ' ', ' ', '\t', '\r', '\u{3000}'],
        r#"any character except '"' and '\', newlines included"# => &['a', 'Z', ' ', '\n', '{', '|', ';', 'é', '/', '*'],
        "any character except newline" => &['a', ' ', '/', '*', ';', '}', 'é'],
        r#"any character, not starting "/*" or "*/""# => &['a', ' ', '\n', '*', '/', ';', '{'],
        t => panic!("no sample is given here for the special sequence `? {t} ?`"),
    };
    pool[rng.random_range(0..pool.len())]
}

/// What to insert where an assertion is reached, when generating.
fn repair(name: &str, before: &str, after: &str) -> &'static str {
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

fn grammar() -> Grammar {
    let g = Grammar::from_markdown(&doc_text());
    let words = type_words(&g);
    assert!(words.contains("bool") && words.contains("rec"), "type words: {words:?}");
    TYPE_WORDS.get_or_init(|| words);
    table_rows(whole_tokens(names_and_comments(g)))
}

/// The codes the document says the parser may refuse a derived string with.
fn outside_codes() -> BTreeSet<String> {
    let doc = doc_text();
    let at = doc.find(OUTSIDE_MARKER).unwrap_or_else(|| panic!("{DOC} has no `{OUTSIDE_MARKER}`"));
    let rest = &doc[at + OUTSIDE_MARKER.len()..];
    let sentence = &rest[..rest.find('.').X()];
    let codes: BTreeSet<String> = sentence.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| is_code(w)).map(|w| w.S()).collect();
    assert!(!codes.is_empty(), "no codes after `{OUTSIDE_MARKER}`");
    codes
}

fn is_code(w: &str) -> bool {
    w.len() == 4 && w.starts_with(['D', 'T']) && w[1..].chars().all(|c| c.is_ascii_digit())
}

// ============================================================================
// Checking one input
// ============================================================================

/// The codes the parser refuses `text` with, or the message it panicked with.
fn parse_codes(text: &str) -> Result<Vec<String>, String> {
    let text = text.S();
    std::panic::catch_unwind(move || {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text);
        let _ = datalove_datalit::parser::parse_integration_test(&db, source);
        let diags = datalove_datalit::parser::parse_integration_test::accumulated::<
            datalove_diagnostic::ParseDiagnostic,
        >(&db, source);
        diags
            .iter()
            .map(|d| d.to_diagnostic(&db).code.map(|c| c.as_str(&db).S()).unwrap_or_else(|| "no code".S()))
            .collect()
    })
    .map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.S()))
            .unwrap_or_else(|| "a panic".S())
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Disagreement {
    /// The parser accepts what the grammar does not derive.
    TooNarrow,
    /// The grammar derives what the parser refuses, with these codes.
    TooWide(Vec<String>),
    ParserPanics(String),
}

impl Disagreement {
    fn same_kind(&self, other: &Disagreement) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    fn describe(&self) -> String {
        match self {
            Disagreement::TooNarrow => "the parser accepts, the grammar does not derive".S(),
            Disagreement::TooWide(codes) => fmt!("the grammar derives, the parser refuses: {}", codes.join(" ")),
            Disagreement::ParserPanics(msg) => fmt!("the parser panics: {msg}"),
        }
    }
}

/// How many disagreements are shrunk before the rest are reported as found.
const SHRINK_LIMIT: usize = 30;

struct Checker {
    rec: Recognizer,
    outside: BTreeSet<String>,
    /// Each disagreement, shrunk, with an input it was found from.
    found: BTreeMap<(String, Disagreement), String>,
    checked: usize,
    disagreed: usize,
}

impl Checker {
    fn new() -> Checker {
        Checker {
            rec: Recognizer::new(&grammar(), "datalit", &special),
            outside: outside_codes(),
            found: BTreeMap::new(),
            checked: 0,
            disagreed: 0,
        }
    }

    fn judge(&self, text: &str) -> Option<Disagreement> {
        let codes = match parse_codes(text) {
            Ok(codes) => codes,
            Err(msg) => return Some(Disagreement::ParserPanics(msg)),
        };
        let derived = self.rec.accepts(text);
        if codes.is_empty() && !derived {
            return Some(Disagreement::TooNarrow);
        }
        if derived && codes.iter().any(|c| !self.outside.contains(c)) {
            let mut codes = codes;
            codes.sort();
            codes.dedup();
            return Some(Disagreement::TooWide(codes));
        }
        None
    }

    fn check(&mut self, text: &str) {
        self.checked += 1;
        if let Some(d) = self.judge(text) {
            self.disagreed += 1;
            // Shrinking is slow, and past a few dozen cases more teach little.
            let small = if self.found.len() < SHRINK_LIMIT { self.shrink(text, &d) } else { text.S() };
            let d = self.judge(&small).unwrap_or(d);
            self.found.entry((small, d)).or_insert_with(|| text.S());
        }
    }

    /// The shortest input, by deleting characters, that disagrees as `d` does.
    fn shrink(&self, text: &str, d: &Disagreement) -> String {
        let mut cs: Vec<char> = text.chars().collect();
        let mut chunk = (cs.len() / 2).max(1);
        loop {
            let mut progress = false;
            let mut i = 0;
            while i < cs.len() {
                let mut t = cs.C();
                t.drain(i..(i + chunk).min(cs.len()));
                let s: String = t.iter().collect();
                if self.judge(&s).is_some_and(|e| e.same_kind(d)) {
                    cs = t;
                    progress = true;
                } else {
                    i += chunk;
                }
            }
            if !progress {
                if chunk == 1 {
                    break;
                }
                chunk = (chunk / 2).max(1);
            }
        }
        cs.into_iter().collect()
    }

    fn finish(self, what: &str) {
        if self.found.is_empty() {
            println!("{what}: {} inputs, grammar and parser agree", self.checked);
            return;
        }
        let mut report = fmt!(
            "{what}: {} of {} inputs disagree, in {} cases, the first {SHRINK_LIMIT} shrunk\n",
            self.disagreed,
            self.checked,
            self.found.len(),
        );
        for ((small, d), from) in &self.found {
            report.push_str(&fmt!("\n{small:?}\n    {}\n    from {from:?}\n", d.describe()));
        }
        panic!("{report}");
    }
}

// ============================================================================
// Inputs
// ============================================================================

fn dlt_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).X() {
        let path = entry.X().path();
        if path.is_dir() {
            if path.file_name().X() != "target" {
                dlt_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "dlt") {
            out.push(path);
        }
    }
}

fn corpus() -> Vec<String> {
    let mut paths = vec![];
    dlt_files(&workspace_dir().join("crates"), &mut paths);
    paths.sort();
    assert!(paths.len() > 100, "found only {} .dlt files", paths.len());
    paths.iter().map(|p| std::fs::read_to_string(p).X()).collect()
}

const MUTATION_CHARS: &[char] = &[
    ' ', '\n', '\t', ';', ',', '-', '|', ':', '/', '*', '{', '}', '[', ']', '(', ')', '#', '%', '"', '\\', 'x',
    'a', 'e', '0', '1', '.', '_', '>', '?', '!', '=', 'é',
];

/// `text` with one character deleted, inserted, replaced or doubled.
fn mutate(text: &str, rng: &mut SeededRng) -> String {
    let mut cs: Vec<char> = text.chars().collect();
    let i = rng.random_range(0..=cs.len());
    let c = MUTATION_CHARS[rng.random_range(0..MUTATION_CHARS.len())];
    match rng.random_range(0..4) {
        0 if i < cs.len() => {
            cs.remove(i);
        }
        1 if i < cs.len() => cs[i] = c,
        2 if i < cs.len() => cs.insert(i, cs[i]),
        _ => cs.insert(i, c),
    }
    cs.into_iter().collect()
}

/// One claim the document makes about an example.
#[derive(Debug)]
enum Claim {
    /// The parser refuses it with this code.
    Refused(String),
    /// It parses, perhaps to be refused later by the type checker.
    Parses,
}

/// The examples in the document's prose: "`x` is D020", "`x` and `y` are
/// D020", "`x` and `y` parse" and "`x` is T032", which parses.
fn claims(doc: &str) -> Vec<(String, Claim)> {
    let prose: String = {
        let mut out = String::new();
        let mut fenced = false;
        for line in doc.lines() {
            if line.starts_with("```") {
                fenced = !fenced;
            } else if !fenced {
                out.push_str(line);
                out.push(' ');
            }
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let mut spans: Vec<(usize, usize)> = vec![];
    let mut rest = 0;
    while let Some(open) = prose[rest..].find('`') {
        let open = rest + open;
        let Some(close) = prose[open + 1..].find('`') else { break };
        let close = open + 1 + close;
        spans.push((open, close + 1));
        rest = close + 1;
    }
    let code = |s: &str| s.split(|c: char| !c.is_ascii_alphanumeric()).next().filter(|w| is_code(w)).map(|w| w.S());
    let mut out = vec![];
    let mut group: Vec<String> = vec![];
    for (k, &(start, end)) in spans.iter().enumerate() {
        let joined_before = k > 0 && [", ", " and ", ", and "].contains(&&prose[spans[k - 1].1..start]);
        if !joined_before {
            group.clear();
        }
        let text = prose[start + 1..end - 1].S();
        group.push(text.C());
        let after = &prose[end..];
        if let Some(c) = after.strip_prefix(" is ").and_then(code) {
            out.push((text, claim_of(c)));
        } else if let Some(c) = after.strip_prefix(" are ").and_then(code) {
            out.extend(group.iter().map(|t| (t.C(), claim_of(c.C()))));
        } else if [" parse", " parses"].iter().any(|p| {
            after.strip_prefix(p).is_some_and(|r| r.is_empty() || r.starts_with(['.', ',', ';', ':', ')']))
        }) {
            out.extend(group.iter().map(|t| (t.C(), Claim::Parses)));
        }
    }
    out
}

fn claim_of(code: String) -> Claim {
    if code.starts_with('T') { Claim::Parses } else { Claim::Refused(code) }
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn doc_examples() {
    let claims = claims(&doc_text());
    assert!(claims.len() > 10, "found only {} claims in {DOC}", claims.len());
    let mut checker = Checker::new();
    let mut wrong = vec![];
    for (text, claim) in &claims {
        let codes = parse_codes(text).unwrap_or_else(|msg| vec![fmt!("panic: {msg}")]);
        let holds = match claim {
            Claim::Refused(c) => codes.contains(c),
            Claim::Parses => codes.is_empty(),
        };
        if !holds {
            wrong.push(fmt!("`{text}`: the document says {claim:?}, the parser says {codes:?}"));
        }
        checker.check(text);
    }
    assert!(wrong.is_empty(), "{DOC} is wrong about:\n{}", wrong.join("\n"));
    checker.finish("the document's examples");
}

#[test]
fn corpus_agrees() {
    let mut checker = Checker::new();
    for text in corpus() {
        let mut rng = SeededRng::new(seed_of(&text));
        checker.check(&text);
        for _ in 0..2 {
            checker.check(&mutate(&text, &mut rng));
        }
    }
    checker.finish("the .dlt files and their mutants");
}

#[test]
fn generated_agree() {
    let g = grammar();
    let generator = Generator::new(&g, &sample, &repair);
    let mut checker = Checker::new();
    let mut dropped = 0;
    let seeds = env_seeds("DATALOVE_GRAMMAR", 0, 1500);
    for seed in seeds.iter() {
        let text = generator.generate("datalit", &mut SeededRng::new(seed));
        // A repair can fail: a newline after a line comment in a table row
        // ends the row, and a name can be a type word.
        if !checker.rec.accepts(&text) {
            dropped += 1;
            continue;
        }
        checker.check(&text);
        checker.check(&mutate(&text, &mut SeededRng::derive(seed, "mutate")));
    }
    let n = seeds.count;
    assert!(dropped < n / 2, "{dropped} of {n} generated strings were not derived");
    checker.finish("generated strings and their mutants");
}
