//! Checking a grammar document against its parser, shared by the grammar tests.
//!
//! A test reads the grammar out of its document with [`crate::ebnf`] and
//! makes a recognizer of it, then gives inputs to both that and the parser
//! through a [`Judge`]. What the parser accepts, the grammar must derive;
//! what the grammar derives, the parser must accept, or refuse only with the
//! codes the document lists as refused outside the grammar, which
//! [`outside_codes`] reads. [`Findings`] collects the disagreements, each
//! shrunk to the shortest input that still disagrees the same way.

use rmx::prelude::*;
use bct::random::SeededRng;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::{Path, PathBuf};

use bct::diagnostic::{LabelStyle, Severity};

pub fn workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The sentence of a grammar document that lists the codes refused outside
/// the grammar, up to its period.
pub const OUTSIDE_MARKER: &str = "then only with these codes:";

/// The codes a document says its parser may refuse a derived string with.
pub fn outside_codes(doc: &str, name: &str) -> BTreeSet<String> {
    let at = doc.find(OUTSIDE_MARKER).unwrap_or_else(|| panic!("{name} has no `{OUTSIDE_MARKER}`"));
    let rest = &doc[at + OUTSIDE_MARKER.len()..];
    let sentence = &rest[..rest.find('.').X()];
    let codes: BTreeSet<String> =
        sentence.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| is_code(w)).map(|w| w.S()).collect();
    assert!(!codes.is_empty(), "no codes after `{OUTSIDE_MARKER}` in {name}");
    codes
}

/// Whether a word is a diagnostic code, a capital letter and three digits.
pub fn is_code(w: &str) -> bool {
    w.len() == 4 && w.starts_with(|c: char| c.is_ascii_uppercase()) && w[1..].chars().all(|c| c.is_ascii_digit())
}

/// One diagnostic a parser reported, as the checks read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    /// The code, or `"no code"`.
    pub code: String,
    pub error: bool,
    pub message: String,
    /// The primary label's span, if there is one.
    pub primary: Option<Range<usize>>,
    /// Every label's span, the primary one included.
    pub spans: Vec<Range<usize>>,
}

/// The parse diagnostics a database accumulated, as [`Diag`]s.
pub fn diags(db: &dyn salsa::Database, found: &[&datalove_diagnostic::ParseDiagnostic]) -> Vec<Diag> {
    found
        .iter()
        .map(|d| {
            let d = d.to_diagnostic(db);
            let primary = d.labels.iter().find(|l| l.style == LabelStyle::Primary).map(|l| l.span.C());
            Diag {
                code: d.code.map(|c| c.as_str(db).S()).unwrap_or_else(|| "no code".S()),
                error: d.severity == Severity::Error,
                message: d.message.as_str(db).S(),
                primary,
                spans: d.labels.iter().map(|l| l.span.C()).collect(),
            }
        })
        .collect()
}

/// What `f` returns, or the message it panicked with.
pub fn catch_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.S()))
            .unwrap_or_else(|| "a panic".S())
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Disagreement {
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

/// One grammar's start rule and its parser, to give inputs to.
pub struct Judge<'a> {
    /// Whether the grammar derives the text.
    pub derives: &'a dyn Fn(&str) -> bool,
    /// The codes the parser refuses the text with, or its panic.
    pub parse: &'a dyn Fn(&str) -> Result<Vec<String>, String>,
    pub outside: &'a BTreeSet<String>,
}

impl Judge<'_> {
    pub fn judge(&self, text: &str) -> Option<Disagreement> {
        let codes = match (self.parse)(text) {
            Ok(codes) => codes,
            Err(msg) => return Some(Disagreement::ParserPanics(msg)),
        };
        let derived = (self.derives)(text);
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

    /// The shortest input, by deleting characters, that disagrees as `d` does.
    fn shrink(&self, text: &str, d: &Disagreement) -> String {
        shrink(text, &|s| self.judge(s).is_some_and(|e| e.same_kind(d)))
    }
}

/// The shortest text, by deleting characters from `text`, that `keep` holds of.
pub fn shrink(text: &str, keep: &dyn Fn(&str) -> bool) -> String {
    let mut cs: Vec<char> = text.chars().collect();
    let mut chunk = (cs.len() / 2).max(1);
    loop {
        let mut progress = false;
        let mut i = 0;
        while i < cs.len() {
            let mut t = cs.C();
            t.drain(i..(i + chunk).min(cs.len()));
            let s: String = t.iter().collect();
            if keep(&s) {
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

/// How many disagreements are shrunk before the rest are reported as found.
const SHRINK_LIMIT: usize = 30;

/// The disagreements found, each shrunk, by start rule.
#[derive(Default)]
pub struct Findings {
    /// By start rule, shrunk input and disagreement, an input it was found from.
    found: BTreeMap<(String, String, Disagreement), String>,
    checked: usize,
    disagreed: usize,
}

impl Findings {
    /// Gives `text` to `judge`, whose start rule is `start`.
    pub fn check(&mut self, judge: &Judge, start: &str, text: &str) {
        self.checked += 1;
        if let Some(d) = judge.judge(text) {
            self.disagreed += 1;
            // Shrinking is slow, and past a few dozen cases more teach little.
            let small = if self.found.len() < SHRINK_LIMIT { judge.shrink(text, &d) } else { text.S() };
            let d = judge.judge(&small).unwrap_or(d);
            self.found.entry((start.S(), small, d)).or_insert_with(|| text.S());
        }
    }

    /// Panics with every disagreement, if there are any.
    pub fn finish(self, what: &str) {
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
        for ((start, small, d), from) in &self.found {
            report.push_str(&fmt!("\n{start}: {small:?}\n    {}\n    from {from:?}\n", d.describe()));
        }
        panic!("{report}");
    }
}

/// The files under `dir`, but not under a `target`, with one of `extensions`, sorted.
pub fn files(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    fn walk(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).X() {
            let path = entry.X().path();
            if path.is_dir() {
                if path.file_name().X() != "target" {
                    walk(&path, extensions, out);
                }
            } else if path.extension().is_some_and(|e| extensions.iter().any(|x| e == *x)) {
                out.push(path);
            }
        }
    }
    let mut out = vec![];
    walk(dir, extensions, &mut out);
    out.sort();
    out
}

const MUTATION_CHARS: &[char] = &[
    ' ', '\n', '\t', ';', ',', '-', '|', ':', '/', '*', '{', '}', '[', ']', '(', ')', '#', '%', '"', '\\', 'x',
    'a', 'e', '0', '1', '.', '_', '<', '>', '?', '!', '=', '@', 'é',
];

/// `text` with one character deleted, inserted, replaced or doubled.
pub fn mutate(text: &str, rng: &mut SeededRng) -> String {
    mutate_at(text, rng).text
}

/// One character of a text changed.
pub struct Mutation {
    pub text: String,
    /// The byte offset of the change in `text`.
    pub at: usize,
    /// The characters it removed and added.
    pub touched: Vec<char>,
}

impl Mutation {
    /// Whether the change made or unmade a line break, a newline or a `;`,
    /// which can break two statements at once.
    pub fn touches_lines(&self) -> bool {
        self.touched.iter().any(|c| matches!(c, '\n' | ';'))
    }
}

/// `text` with one character deleted, inserted, replaced or doubled.
pub fn mutate_at(text: &str, rng: &mut SeededRng) -> Mutation {
    let mut cs: Vec<char> = text.chars().collect();
    let i = rng.random_range(0..=cs.len());
    let c = MUTATION_CHARS[rng.random_range(0..MUTATION_CHARS.len())];
    let touched = match rng.random_range(0..4) {
        0 if i < cs.len() => vec![cs.remove(i)],
        1 if i < cs.len() => vec![rmx::std::mem::replace(&mut cs[i], c), c],
        2 if i < cs.len() => {
            cs.insert(i, cs[i]);
            vec![cs[i]]
        }
        _ => {
            cs.insert(i, c);
            vec![c]
        }
    };
    let at = cs[..i.min(cs.len())].iter().map(|c| c.len_utf8()).sum();
    Mutation { text: cs.into_iter().collect(), at, touched }
}

/// One claim a document makes about an example.
#[derive(Debug)]
pub enum Claim {
    /// The parser refuses it with this code.
    Refused(String),
    /// It parses, perhaps to be refused later by the type checker.
    Parses,
}

/// The examples in a document's prose: "`x` is D020", "`x` and `y` are
/// D020", "`x` and `y` parse" and "`x` is T032", which parses.
pub fn claims(doc: &str) -> Vec<(String, Claim)> {
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
