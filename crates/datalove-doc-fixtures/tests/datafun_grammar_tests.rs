//! The datafun grammar in `botdocs/datafun-ebnf.md`, checked against the parser.
//!
//! The grammar is over tokens, so its recognizer runs over the tokens the
//! real lexer makes of an input, with two kinds added from the newlines the
//! lexer records: an `EOL` before a token outside brackets, and a table's
//! `newline` before a token in a table literal, outside brackets nested in
//! it. A terminal matches a token by its text, and `~` asserts that a token
//! is written against the one before it.
//!
//! Three kinds of rule match a run of tokens by their text instead, read in
//! characters:
//!
//! - `type`, by the datalit grammar's `type` rule, as the document says the
//!   parser reads it, made executable by `datalove_doc_fixtures::datalit_grammar`;
//! - the rules under `(* Written in characters *)`, each over one glued run;
//! - the rules that are `? as in the datalit grammar ?`, by those rules of it,
//!   also over one glued run.
//!
//! Each input is given to the recognizer and to the parser, as a script, a
//! module or a rider interface:
//!
//! - what the parser accepts, the grammar must derive;
//! - what the grammar derives, the parser must accept, or refuse only with
//!   the codes the document lists as refused outside the grammar.
//!
//! The inputs are every `.dfs`, `.dfm` and `.dli` file under `crates/` and
//! `sys/`, the module and script sections of every worldfile there, strings
//! generated from the grammar, and all of those mutated a character at a
//! time. A string is generated from each seed of `DATALOVE_DF_GRAMMAR_SEED`
//! and `DATALOVE_DF_GRAMMAR_COUNT` (see `bct::random::env_seeds`).
//!
//! The document's prose examples are not checked here, as datalit's are:
//! most are fragments of a statement, not whole scripts.

use rmx::prelude::*;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use bct::lexer::{Sigil, TokenKind};
use bct::lookup::LookupMap;
use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::datalit_grammar;
use datalove_doc_fixtures::ebnf::{Expr, Generator, Grammar, Piece, Recognizer, Term, join_chars};
use datalove_doc_fixtures::grammar_check::{
    Findings, Judge, catch_panic, files, mutate, outside_codes, workspace_dir,
};

const DOC: &str = "botdocs/datafun-ebnf.md";

fn doc_text() -> String {
    std::fs::read_to_string(workspace_dir().join(DOC)).X()
}

/// The special sequences over tokens.
const EOL: &str = "a newline outside brackets";
const ROW_NEWLINE: &str = "a newline in a table literal, outside brackets nested in it";
const NAME: &str = "an ASCII word not beginning with a digit";
const FROM_DATALIT: &str = "as in the datalit grammar";

/// The rules for numbers.
const NUMBERS: &[&str] = &["dec_number", "hex_number"];

/// The comment that heads the rules written in characters.
const CHARS_MARKER: &str = "(* Written in characters";

// ============================================================================
// Tokens
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Token,
    Eol,
    RowNewline,
}

#[derive(Clone, Debug)]
struct Tok {
    kind: Kind,
    word: bool,
    start: usize,
    end: usize,
}

/// An input as the recognizer reads it: its tokens and its text, and the
/// runs of tokens each character-level rule has matched, as they are asked.
struct Tokens {
    toks: Vec<Tok>,
    text: String,
    islands: RefCell<LookupMap<(usize, usize), Vec<usize>>>,
}

impl Tokens {
    fn new(text: &str) -> Tokens {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let lexed = bct::lexer::lex(&db, source);
        let mut toks = vec![];
        let mut open: Vec<Sigil> = vec![];
        for t in lexed.tokens(&db) {
            if t.newline_before {
                match open.last() {
                    None => toks.push(Tok { kind: Kind::Eol, word: false, start: t.span().start, end: t.span().start }),
                    Some(Sigil::BracePipeOpen) => {
                        toks.push(Tok { kind: Kind::RowNewline, word: false, start: t.span().start, end: t.span().start })
                    }
                    Some(_) => {}
                }
            }
            if let TokenKind::Sigil(s) = t.kind {
                if is_open(s) {
                    open.push(s);
                } else if t.is_close_sigil() {
                    open.pop();
                }
            }
            toks.push(Tok { kind: Kind::Token, word: t.kind == TokenKind::Word, start: t.span().start, end: t.span().end });
        }
        Tokens { toks, text: text.S(), islands: RefCell::new(LookupMap::new()) }
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

// ============================================================================
// The grammar, made executable
// ============================================================================

/// A rule matched in characters over a run of tokens.
struct Island {
    rec: Recognizer<[char]>,
    /// Whether the run is one glued run, or any run with its brackets
    /// matched, as a type's is.
    glued: bool,
    /// Whether the run is a number, which a `.` or word written against its
    /// end would be part of.
    number: bool,
}

/// The two grammars: datafun's over tokens, and the character-level rules
/// its islands are read with, datalit's prepared grammar with datafun's own
/// character-level rules added.
struct Grammars {
    tokens: Grammar,
    chars: Grammar,
    /// The rules of `tokens` read in characters, by name.
    island_names: Vec<String>,
}

fn grammars() -> Grammars {
    let doc = doc_text();
    let mut g = Grammar::from_markdown(&doc);
    let block = doc.split("```ebnf\n").nth(1).X().split("```").next().X();
    let after = &block[block.find(CHARS_MARKER).unwrap_or_else(|| panic!("{DOC} has no `{CHARS_MARKER}`"))..];
    let written_in_chars: Vec<String> =
        g.rules.iter().filter(|r| after.contains(&fmt!("\n{} ", r.name))).map(|r| r.name.C()).collect();
    assert!(written_in_chars.contains(&"dec_number".S()), "the character rules: {written_in_chars:?}");

    let mut chars = datalit_grammar::grammar();
    for name in &written_in_chars {
        let rule = g.get(name).X();
        if rule.body == Expr::Special(FROM_DATALIT.S()) {
            assert!(chars.get(name).is_some(), "{name} is not a rule of the datalit grammar");
        } else {
            chars.rules.push(rule.C());
        }
    }
    let mut island_names = written_in_chars.C();
    island_names.push("type".S());
    // The datalit grammar compiles every rule it reaches, so make sure the
    // added ones are complete there.
    let _ = chars.reachable(&written_in_chars.iter().map(|s| s.as_str()).collect::<Vec<_>>());

    for rule in &mut g.rules {
        if island_names.contains(&rule.name) {
            rule.body = Expr::Special(fmt!("island {}", rule.name));
        }
    }
    Grammars { tokens: g, chars, island_names }
}

/// The words an exception's `b` names: its terminals, and those of the rules
/// it names, in the character grammar.
fn excepted_words(b: &Expr, chars: &Grammar) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    b.visit(&mut |e| match e {
        Expr::Lit(s) => {
            words.insert(s.C());
        }
        Expr::Ref(r) => {
            let rule = chars.get(r).unwrap_or_else(|| panic!("the exception names {r}, which is no datalit rule"));
            rule.body.visit(&mut |e| {
                if let Expr::Lit(s) = e {
                    words.insert(s.C());
                }
            });
        }
        _ => {}
    });
    words
}

/// The recognizer of one start rule, over tokens.
fn recognizer(gs: &Grammars, start: &str) -> Recognizer<Tokens> {
    let islands: Rc<Vec<Island>> = Rc::new(
        gs.island_names
            .iter()
            .map(|name| Island {
                rec: Recognizer::new(&gs.chars, name, &datalit_grammar::term),
                glued: name != "type",
                number: NUMBERS.contains(&name.as_str()),
            })
            .collect(),
    );
    let chars = &gs.chars;
    let names = gs.island_names.C();
    let term = move |e: &Expr| -> Term<Tokens> {
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
                let words = excepted_words(b, chars);
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
            Expr::Special(text) if text.starts_with("island ") => {
                let name = &text["island ".len()..];
                let k = names.iter().position(|n| n == name).X();
                let islands = islands.C();
                Rc::new(move |input: &Tokens, i, out: &mut Vec<usize>| {
                    out.extend(island_matches(input, &islands[k], k, i));
                })
            }
            e => panic!("no terminal over tokens for {e:?}"),
        }
    };
    Recognizer::new(&gs.tokens, start, &term)
}

fn pseudo(kind: Kind) -> Term<Tokens> {
    Rc::new(move |input: &Tokens, i, out: &mut Vec<usize>| {
        if input.toks.get(i).is_some_and(|t| t.kind == kind) {
            out.push(1);
        }
    })
}

/// The lengths of the runs of tokens from `i` whose text island `k` matches.
fn island_matches(input: &Tokens, island: &Island, k: usize, i: usize) -> Vec<usize> {
    if let Some(found) = input.islands.borrow().get(&(k, i)) {
        return found.C();
    }
    let mut found = vec![];
    let mut depth = 0i32;
    let mut j = i;
    while j < input.toks.len() && input.toks[j].kind == Kind::Token && j - i < 4096 {
        if island.glued && j > i && !input.glued(j) {
            break;
        }
        let t = &input.text[input.toks[j].start..input.toks[j].end];
        if t.len() <= 2 && ["(", "[", "{", "<", "(|", "{|", "[|", "<|", "%{", "#{"].contains(&t) {
            depth += 1;
        } else if [")", "]", "}", ">", "|)", "|}", "|]", "|>"].contains(&t) {
            depth -= 1;
            if depth < 0 {
                break;
            }
        }
        j += 1;
        let continues_number = island.number
            && input.glued(j)
            && input.token_text(j).is_some_and(|t| t == "." || input.toks[j].word);
        if depth == 0 && !continues_number {
            let text: Vec<char> = input.text[input.toks[i].start..input.toks[j - 1].end].chars().collect();
            if island.rec.accepts(&text, text.len()) {
                found.push(j - i);
            }
        }
    }
    input.islands.borrow_mut().insert((k, i), found.C());
    found
}

// ============================================================================
// The parser
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Start {
    Script,
    Module,
    Interface,
}

impl Start {
    fn rule(self) -> &'static str {
        match self {
            Start::Script => "script",
            Start::Module => "module",
            Start::Interface => "interface",
        }
    }
}

/// The codes the parser refuses `text` with as a `start`, or its panic.
fn parse_codes(start: Start, text: &str) -> Result<Vec<String>, String> {
    use datalove_datafun_parser as p;
    use datalove_diagnostic::ParseDiagnostic;
    catch_panic(|| {
        let db = datalove_datalit::Database::default();
        let source = bct::input::Source::new(&db, text.S());
        let diags = match start {
            Start::Script => p::parse::accumulated::<ParseDiagnostic>(&db, source),
            Start::Module => p::parse_module_for_test::accumulated::<ParseDiagnostic>(&db, source),
            Start::Interface => p::parse_rider_interface::accumulated::<ParseDiagnostic>(&db, source),
        };
        diags
            .iter()
            .map(|d| d.to_diagnostic(&db).code.map(|c| c.as_str(&db).S()).unwrap_or_else(|| "no code".S()))
            .collect()
    })
}

struct Checker {
    recs: Vec<(Start, Recognizer<Tokens>)>,
    outside: BTreeSet<String>,
}

impl Checker {
    fn new(gs: &Grammars) -> Checker {
        let recs = [Start::Script, Start::Module, Start::Interface]
            .into_iter()
            .map(|s| (s, recognizer(gs, s.rule())))
            .collect();
        Checker { recs, outside: outside_codes(&doc_text(), DOC) }
    }

    fn rec(&self, start: Start) -> &Recognizer<Tokens> {
        &self.recs.iter().find(|(s, _)| *s == start).X().1
    }

    fn derives(&self, start: Start, text: &str) -> bool {
        let tokens = Tokens::new(text);
        self.rec(start).accepts(&tokens, tokens.toks.len())
    }

    fn check(&self, findings: &mut Findings, start: Start, text: &str) {
        let derives = |t: &str| self.derives(start, t);
        let parse = |t: &str| parse_codes(start, t);
        let judge = Judge { derives: &derives, parse: &parse, outside: &self.outside };
        findings.check(&judge, start.rule(), text);
    }
}

// ============================================================================
// Inputs
// ============================================================================

/// Every datafun source under `crates/` and `sys/`, and what it is parsed as.
fn corpus() -> Vec<(Start, String)> {
    let root = workspace_dir();
    let mut out = vec![];
    for dir in ["crates", "sys"] {
        for path in files(&root.join(dir), &["dfs", "dfm", "dli", "world"]) {
            let text = std::fs::read_to_string(&path).X();
            match path.extension().X().to_str().X() {
                "dfs" => out.push((Start::Script, text)),
                "dfm" => out.push((Start::Module, text)),
                "dli" => out.push((Start::Interface, text)),
                _ => {
                    for section in datalove_script_steps::sections(&text) {
                        let kind = section.header.split_whitespace().next().unwrap_or("");
                        match kind {
                            "module" | "module-edit" | "module-add" => out.push((Start::Module, section.content)),
                            "scriptunit-fragment" | "scriptunit-edit" | "scriptunit-insert" => {
                                out.push((Start::Script, section.content))
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    out
}

/// A string for a special sequence or island, when generating.
fn sample(gs: &Grammars, text: &str, rng: &mut SeededRng) -> String {
    const NAMES: &[&str] = &["x", "y", "f", "go", "a1", "T", "_z", "xs"];
    match text {
        EOL | ROW_NEWLINE => "\n".S(),
        NAME => NAMES[rng.random_range(0..NAMES.len())].S(),
        t if t.starts_with("island ") => {
            let name = &t["island ".len()..];
            let generator = Generator::new(&gs.chars, &datalit_grammar::sample);
            join_chars(&generator.generate(name, rng), &datalit_grammar::repair)
        }
        t => panic!("no sample is given here for the special sequence `? {t} ?`"),
    }
}

/// Generated tokens as text: a space between two, none after a `~`, and a
/// newline that is a token on its own.
fn join_tokens(pieces: &[Piece]) -> String {
    let mut text = String::new();
    let mut glue = true;
    for piece in pieces {
        match piece {
            Piece::Mark(m) if m == "~" => glue = true,
            Piece::Mark(m) => panic!("no assertion {m} is added to the datafun grammar"),
            Piece::Text(t) => {
                if !glue && t != "\n" && !text.ends_with('\n') {
                    text.push(' ');
                }
                text.push_str(t);
                glue = false;
            }
        }
    }
    text
}

// ============================================================================
// Tests
// ============================================================================

#[test]
fn corpus_agrees() {
    let gs = grammars();
    let checker = Checker::new(&gs);
    let mut findings = Findings::default();
    let corpus = corpus();
    assert!(corpus.len() > 500, "found only {} datafun sources", corpus.len());
    for (start, text) in corpus {
        let mut rng = SeededRng::new(seed_of(&text));
        checker.check(&mut findings, start, &text);
        checker.check(&mut findings, start, &mutate(&text, &mut rng));
    }
    findings.finish("the datafun sources and their mutants");
}

#[test]
fn generated_agree() {
    let gs = grammars();
    let sampler = |text: &str, rng: &mut SeededRng| sample(&gs, text, rng);
    let generator = Generator::new(&gs.tokens, &sampler);
    let checker = Checker::new(&gs);
    let mut findings = Findings::default();
    let mut dropped = 0;
    let seeds = env_seeds("DATALOVE_DF_GRAMMAR", 0, 600);
    for seed in seeds.iter() {
        let start = [Start::Script, Start::Module, Start::Interface][(seed % 3) as usize];
        let text = join_tokens(&generator.generate(start.rule(), &mut SeededRng::new(seed)));
        // An exception's words and a space that ends a glued run are not
        // repaired when generating, so some strings are not derived.
        if !checker.derives(start, &text) {
            dropped += 1;
            continue;
        }
        checker.check(&mut findings, start, &text);
        checker.check(&mut findings, start, &mutate(&text, &mut SeededRng::derive(seed, "mutate")));
    }
    let n = seeds.count;
    assert!(dropped < n / 2, "{dropped} of {n} generated strings were not derived");
    findings.finish("generated strings and their mutants");
}
