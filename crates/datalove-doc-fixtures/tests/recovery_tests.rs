//! How the datalit and datafun parsers recover from errors, held and measured.
//!
//! Every source of the grammar tests' corpora is changed a character at a
//! time, and what the parser reports of each change is read against the
//! grammar's recognizer, which says where the grammar first cannot go on.
//!
//! - `diagnostics_are_well_formed`: every diagnostic has a code, is an error,
//!   has a primary label, and its spans lie in the source; none is reported
//!   twice, and neither parser panics.
//! - `recovery_contract`: the Recovery sections of `botdocs/datalit-ebnf.md`
//!   and `botdocs/datafun-ebnf.md`. One error for each statement; the first
//!   where the grammar stops; and a change to a source that parses makes
//!   errors in one statement only, but for one that moves a statement
//!   boundary. `DATALOVE_RECOVERY_SEED` and `DATALOVE_RECOVERY_COUNT` give the
//!   changes made of each source (see `bct::random::env_seeds`).
//! - `report`, run by hand with `--ignored --nocapture`, prints how many
//!   errors changes make and with what codes, where first errors fall, errors
//!   away from the change, and literals datafun's parser reads with other
//!   codes than datalit's, with examples.

use rmx::prelude::*;
use std::collections::BTreeMap;

use bct::random::{SeededRng, env_seeds, seed_of};
use datalove_doc_fixtures::datafun_grammar::{self as df, Start};
use datalove_doc_fixtures::datalit_grammar as dl;
use datalove_doc_fixtures::grammar_check::{Diag, Mutation, files, mutate_at, outside_codes, shrink, workspace_dir};

/// How many changes are made to each clean source.
const MUTANTS: usize = 3;

/// The codes for unmatched brackets, which are found before parsing and
/// reported where the bracket is, wherever the change was.
const BRACKET_CODES: &[&str] = &["D054", "D055"];

/// The codes reported where a bracket or block opens, not where it should
/// have closed: unmatched brackets, and a block with no `end` line.
const OPENER_CODES: &[&str] = &["D054", "D055", "P034"];

/// One language's parser and grammar, as the checks use them.
struct Lang<'a> {
    name: &'static str,
    parse: &'a dyn Fn(&str) -> Result<Vec<Diag>, String>,
    /// The byte offset where the grammar first cannot go on, `None` if it derives the text.
    first_error: &'a dyn Fn(&str) -> Option<usize>,
    outside: Vec<String>,
}

/// What one change made, read for the report.
struct Report {
    /// Examples by kind and detail: the change, shrunk, and where it came from.
    examples: BTreeMap<(String, String), (usize, String)>,
    /// How many errors each change made, by count.
    cascade: BTreeMap<usize, usize>,
    /// The codes of each change that made more than one error, as a set.
    combos: BTreeMap<String, (usize, String)>,
    /// The change that made the most errors.
    worst: (usize, String),
    /// How the parser's first error stood to the grammar's, by relation.
    first: BTreeMap<String, usize>,
    changes: usize,
}

impl Report {
    fn new() -> Report {
        Report {
            examples: BTreeMap::new(),
            cascade: BTreeMap::new(),
            worst: (0, String::new()),
            combos: BTreeMap::new(),
            first: BTreeMap::new(),
            changes: 0,
        }
    }

    fn note(&mut self, kind: &str, detail: &str, text: &str, keep: &dyn Fn(&str) -> bool) {
        let key = (kind.S(), detail.S());
        let entry = self.examples.entry(key).or_insert((0, String::new()));
        entry.0 += 1;
        if entry.0 == 1 {
            entry.1 = shrink(text, keep);
        }
    }

    /// Notes an example as it is, marking the change with `⟨⟩`, for a
    /// property shrinking would lose: which character was changed.
    fn note_unshrunk(&mut self, kind: &str, detail: &str, text: &str, at: usize) {
        let entry = self.examples.entry((kind.S(), detail.S())).or_insert((0, String::new()));
        entry.0 += 1;
        if entry.0 == 1 || text.len() < entry.1.len() {
            let at = (0..=at.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).X();
            entry.1 = fmt!("{}⟨⟩{}", &text[..at], &text[at..]);
        }
    }

    fn print(&self, title: &str) {
        println!("\n==== {title}: {} changes\n", self.changes);
        println!("errors per change:");
        for (n, count) in &self.cascade {
            println!("  {n:>3}: {count}");
        }
        if self.worst.0 > 0 {
            let parse_shown: String = self.worst.1.chars().take(400).collect();
            println!("  the most: {} from {parse_shown:?}", self.worst.0);
        }
        let mut combos: Vec<_> = self.combos.iter().collect();
        combos.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
        println!("\nthe codes of changes with more than one error:");
        for (codes, (n, example)) in combos.iter().take(25) {
            let example: String = example.chars().take(160).collect();
            println!("  {n:>5}  {codes}\n         e.g. {example:?}");
        }
        println!("\nthe parser's first error, against the grammar's:");
        for (relation, count) in &self.first {
            println!("  {relation}: {count}");
        }
        let mut kind = "";
        for ((k, detail), (count, example)) in &self.examples {
            if k != kind {
                println!("\n{k}:");
                kind = k;
            }
            println!("  {count:>5}  {detail}\n         e.g. {example:?}");
        }
    }
}

/// The spans of the lexer's tokens of `text`.
fn token_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, text.S());
    bct::lexer::lex(&db, source).tokens(&db).iter().map(|t| t.span()).collect()
}

fn line_of(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].matches('\n').count()
}

/// The span of the innermost brackets around `at`, by the lexer's tokens,
/// or the whole text.
fn enclosing(text: &str, at: usize) -> std::ops::Range<usize> {
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, text.S());
    let lexed = bct::lexer::lex(&db, source);
    let mut open: Vec<usize> = vec![];
    let mut best = 0..text.len();
    for t in lexed.tokens(&db) {
        let s = t.text(text);
        if ["(", "[", "{", "<", "(|", "{|", "[|", "<|", "%{", "#{"].contains(&s) {
            open.push(t.span().start);
        } else if t.is_close_sigil() {
            if let Some(start) = open.pop() {
                let span = start..t.span().end;
                if span.contains(&at) && span.len() < best.len() {
                    best = span;
                }
            }
        }
    }
    best
}

fn is_ill_formed(d: &Diag, len: usize) -> Option<&'static str> {
    if d.code == "no code" {
        Some("no code")
    } else if !d.error {
        Some("not an error")
    } else if d.primary.is_none() {
        Some("no primary label")
    } else if d.spans.iter().any(|s| s.start > s.end || s.end > len) {
        Some("a span outside the source")
    } else {
        None
    }
}

/// How a primary span `p` stands to the grammar's first error `g`.
///
/// A span that reaches `g`, or ends with no token between it and `g`, only
/// whitespace, line breaks and comments, is at it: it is the construct the
/// grammar stops in.
///
/// `g` is taken at the start of the token it falls in: the datalit grammar
/// reads characters, and can stop partway into a token, as at the `-` of a
/// `->`, which it reads as the sign of an exponent.
fn relation(text: &str, p: &std::ops::Range<usize>, g: usize) -> &'static str {
    let spans = token_spans(text);
    let g = spans.iter().find(|s| s.start < g && g < s.end).map_or(g, |s| s.start);
    let gap = |a: usize, b: usize| !spans.iter().any(|s| a <= s.start && s.start < b);
    if p.start <= g && g <= p.end || (p.start <= g && gap(p.end, g)) {
        "at it"
    } else if p.is_empty() && p.start > g && gap(g, p.start) {
        "at it"
    } else if p.end <= g {
        if line_of(text, p.start) == line_of(text, g) { "before it, on its line" } else { "before it, on an earlier line" }
    } else if line_of(text, p.start) == line_of(text, g) {
        "after it, on its line"
    } else {
        "after it, on a later line"
    }
}

/// The parser's earliest error, but for codes reported where a bracket or
/// block opens and those refused outside the grammar.
fn earliest(diags: &[Diag], outside: &[String]) -> Option<(std::ops::Range<usize>, String)> {
    diags
        .iter()
        .filter(|d| d.error && !OPENER_CODES.contains(&d.code.as_str()) && !outside.contains(&d.code))
        .filter_map(|d| d.primary.C().map(|p| (p, d.code.C())))
        .min_by_key(|(p, _)| p.start)
}

/// Reads one change, `text`, made at `at` to a source the parser accepts.
fn read_change(lang: &Lang, report: &mut Report, text: &str, at: usize) {
    report.changes += 1;
    let diags = match (lang.parse)(text) {
        Ok(d) => d,
        Err(msg) => {
            let parse = lang.parse;
            report.note("panic", &msg, text, &|t| parse(t).is_err());
            return;
        }
    };
    let errors = diags.iter().filter(|d| d.error).count();
    *report.cascade.entry(errors).or_default() += 1;
    if errors > 1 {
        let mut codes: Vec<&str> = diags.iter().filter(|d| d.error).map(|d| d.code.as_str()).collect();
        codes.sort();
        let e = report.combos.entry(codes.join(" ")).or_insert((0, String::new()));
        e.0 += 1;
        if e.1.is_empty() || text.len() < e.1.len() {
            let at = (0..=at.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).X();
            e.1 = fmt!("{}⟨⟩{}", &text[..at], &text[at..]);
        }
    }
    if errors > report.worst.0 {
        report.worst = (errors, text.S());
    }

    // Form.
    for d in &diags {
        if let Some(what) = is_ill_formed(d, text.len()) {
            let parse = lang.parse;
            let code = d.code.C();
            report.note("form", &fmt!("{what}: {code} {:?}", d.message), text, &|t| {
                parse(t).is_ok_and(|ds| ds.iter().any(|e| e.code == code && is_ill_formed(e, t.len()) == Some(what)))
            });
        }
    }
    let mut seen = vec![];
    for d in &diags {
        let key = (d.code.C(), d.primary.C());
        if seen.contains(&key) {
            let parse = lang.parse;
            let code = d.code.C();
            report.note("form", &fmt!("reported twice: {code}"), text, &|t| {
                parse(t).is_ok_and(|ds| {
                    let keys: Vec<_> = ds.iter().filter(|e| e.code == code).map(|e| e.primary.C()).collect();
                    (1..keys.len()).any(|k| keys[..k].contains(&keys[k]))
                })
            });
        }
        seen.push(key);
    }

    // Cascade.
    if errors >= 4 {
        let parse = lang.parse;
        report.note("cascade", "four or more errors from one change", text, &|t| {
            parse(t).is_ok_and(|ds| ds.iter().filter(|d| d.error).count() >= 4)
        });
    }

    // Locality.
    let region = enclosing(text, at);
    let line = line_of(text, at);
    for d in &diags {
        let Some(p) = &d.primary else { continue };
        if BRACKET_CODES.contains(&d.code.as_str()) {
            continue;
        }
        let remote = line_of(text, p.start) != line && !(region.start <= p.start && p.end <= region.end);
        if remote {
            let detail = fmt!("{} reported away from the change", d.code);
            report.note_unshrunk("locality", &detail, text, at);
        }
    }

    // First error.
    let grammar = (lang.first_error)(text);
    let first = earliest(&diags, &lang.outside);
    let rel = match (grammar, &first) {
        (None, _) => "the grammar derives the change",
        (Some(_), None) => "only opener or outside codes",
        (Some(g), _) if diags.iter().any(|d| d.error && d.primary.as_ref().is_some_and(|p| relation(text, p, g) == "at it")) => {
            "at it"
        }
        (Some(g), Some((p, _))) => relation(text, p, g),
    };
    *report.first.entry(rel.S()).or_default() += 1;
    if rel.starts_with("before") || rel.starts_with("after") {
        let (_, code) = first.X();
        let first_error = lang.first_error;
        let parse = lang.parse;
        let outside = lang.outside.C();
        let code_kept = code.C();
        report.note("first error", &fmt!("{code} {rel}"), text, &move |t| {
            let Some(g) = first_error(t) else { return false };
            let Ok(ds) = parse(t) else { return false };
            let any_at = ds.iter().any(|d| d.error && d.primary.as_ref().is_some_and(|p| relation(t, p, g) == "at it"));
            !any_at && earliest(&ds, &outside).is_some_and(|(p, c)| c == code_kept && relation(t, &p, g) == rel)
        });
    }
}

/// A datalit code and the datafun code the same fault gets, where the two
/// grammars differ, as `botdocs/datafun-ebnf.md` lists them.
const SHARED_DIFFERENCES: &[(&str, &[&str])] = &[("D018", &["P080", "P081"]), ("D020", &["P080", "P081"]), ("D059", &["P080", "P081"])];

fn same_codes(dl_codes: &[String], df_codes: &[String]) -> bool {
    let mapped = |c: &String| -> Vec<String> {
        match SHARED_DIFFERENCES.iter().find(|(d, _)| d == c) {
            Some((_, ps)) => ps.iter().map(|p| p.S()).collect(),
            None => vec![c.C()],
        }
    };
    let dl_set: std::collections::BTreeSet<String> = dl_codes.iter().cloned().collect();
    let df_set: std::collections::BTreeSet<String> = df_codes.iter().cloned().collect();
    dl_set.iter().all(|c| mapped(c).iter().any(|m| df_set.contains(m)))
        && df_set.iter().all(|c| dl_set.iter().any(|d| mapped(d).contains(c)))
}

/// Every diagnostic either parser makes of a changed source is well formed,
/// and neither parser panics.
#[test]
fn diagnostics_are_well_formed() {
    let mut wrong = vec![];
    let mut check = |what: &str, text: &str, found: Result<Vec<Diag>, String>| {
        let diags = match found {
            Ok(d) => d,
            Err(msg) => {
                wrong.push(fmt!("{what} panics: {msg}\n    {text:?}"));
                return;
            }
        };
        for (k, d) in diags.iter().enumerate() {
            if let Some(how) = is_ill_formed(d, text.len()) {
                wrong.push(fmt!("{what}: {} {:?}: {how}\n    {text:?}", d.code, d.message));
            }
            if diags[..k].iter().any(|e| e.code == d.code && e.primary == d.primary) {
                wrong.push(fmt!("{what}: {} reported twice at {:?}\n    {text:?}", d.code, d.primary));
            }
        }
    };
    for path in files(&workspace_dir().join("crates"), &["dlt"]) {
        let source = std::fs::read_to_string(path).X();
        let mut rng = SeededRng::new(seed_of(&source));
        for _ in 0..MUTANTS {
            let text = mutate_at(&source, &mut rng).text;
            check("datalit", &text, dl::parse_diags(&text));
            check("datafun expression", &text, df::parse_expr_diags(&text));
        }
    }
    for (start, source) in df::corpus() {
        let mut rng = SeededRng::new(seed_of(&source));
        for _ in 0..MUTANTS {
            let text = mutate_at(&source, &mut rng).text;
            check(start.rule(), &text, df::parse_diags(start, &text));
        }
    }
    assert!(wrong.is_empty(), "{} ill-formed diagnostics:\n{}", wrong.len(), wrong.join("\n"));
}

/// Where each statement of `text` begins, as the datafun parser divides it.
fn statement_starts(text: &str) -> Vec<usize> {
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, text.S());
    let bracer = bct::bracer::bracer(&db, bct::lexer::lex(&db, source));
    datalove_datalit::parser_util::statement_starts(&db, bracer)
}

/// Whether a change moved a statement boundary: made or unmade a line break,
/// matched a bracket that spans lines differently, or exposed a `;` in a
/// comment, any of which moves lines from one statement to another and can
/// break two at once.
fn reshapes_statements(source: &str, m: &Mutation) -> bool {
    let delta = m.text.len() as isize - source.len() as isize;
    let shift = |pos: usize| if pos >= m.at { pos.saturating_add_signed(delta) } else { pos };
    let before: Vec<usize> = statement_starts(source).into_iter().map(shift).collect();
    before != statement_starts(&m.text)
}

/// What one language's recovery is held to, broken: by what was broken, how
/// many changes broke it, and the shortest change that did, marked.
#[derive(Default)]
struct Broken(BTreeMap<String, (usize, String)>);

impl Broken {
    fn note(&mut self, what: String, text: &str, at: usize) {
        let entry = self.0.entry(what).or_insert((0, String::new()));
        entry.0 += 1;
        if entry.1.is_empty() || text.len() < entry.1.len() {
            let at = (0..=at.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).X();
            entry.1 = fmt!("{}⟨⟩{}", &text[..at], &text[at..]);
        }
    }

    /// An example, cut to the lines around its change.
    fn window(example: &str) -> String {
        let at = example.find("⟨⟩").unwrap_or(0);
        let lines: Vec<&str> = example.split('\n').collect();
        let line = example[..at].matches('\n').count();
        let from = line.saturating_sub(6);
        let to = (line + 7).min(lines.len());
        lines[from..to].join("\n")
    }
}

/// Holds one change to the recovery contract.
///
/// - No statement has more than one error.
/// - The first error is where the grammar first cannot go on, on that token
///   or a construct ending there, but for the errors reported where something
///   opens and the codes refused outside the grammar.
/// - The errors are all in one statement, unless the change reshaped the
///   statements (see `reshapes_statements`), which can break two at once.
///
/// The last two are of a change to a source the parser accepts, `clean`: in a
/// source with errors of its own, a change can only add to them.
fn hold(lang: &Lang, starts: &dyn Fn(&str) -> Vec<usize>, broken: &mut Broken, source: &str, m: &Mutation, clean: bool) {
    let text = m.text.as_str();
    let Ok(diags) = (lang.parse)(text) else {
        broken.note("a panic".S(), text, m.at);
        return;
    };
    let errors: Vec<&Diag> = diags.iter().filter(|d| d.error).collect();
    let starts = starts(text);
    let statement = |d: &Diag| starts.partition_point(|&s| s <= d.primary.as_ref().map_or(0, |p| p.start)).saturating_sub(1);
    let mut by_statement: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for d in &errors {
        by_statement.entry(statement(d)).or_default().push(&d.code);
    }
    for codes in by_statement.values() {
        if codes.len() > 1 {
            broken.note(fmt!("{}: two errors in one statement: {}", lang.name, codes.join(" ")), text, m.at);
        }
    }
    if !clean {
        return;
    }
    if by_statement.len() > 1 && !reshapes_statements(source, m) {
        let codes: Vec<&str> = by_statement.values().flatten().copied().collect();
        broken.note(fmt!("{}: errors in {} statements: {}", lang.name, by_statement.len(), codes.join(" ")), text, m.at);
    }
    if let Some(g) = (lang.first_error)(text) {
        let any_at = errors.iter().any(|d| d.primary.as_ref().is_some_and(|p| relation(text, p, g) == "at it"));
        // A rule outside the grammar broken first is the first error.
        let outside_first = errors
            .iter()
            .any(|d| lang.outside.contains(&d.code) && d.primary.as_ref().is_some_and(|p| p.start <= g));
        if !any_at && !outside_first && let Some((p, code)) = earliest(&diags, &lang.outside) {
            broken.note(fmt!("{}: first error {code} {}", lang.name, relation(text, &p, g)), text, m.at);
        }
    }
}

/// Both parsers recover as `botdocs/plan-parse-recovery.md` says: one error
/// for each statement, the first where the grammar stops, and one statement's
/// worth for one change.
#[test]
fn recovery_contract() {
    let mut broken = Broken::default();
    // Each seed makes one change to every source.
    let seeds = env_seeds("DATALOVE_RECOVERY", 0, MUTANTS as u64);

    let recs = dl::recognizers(&dl::grammar());
    let first_error = |t: &str| recs.first_error(dl::START, t);
    let lang = Lang {
        name: "datalit",
        parse: &dl::parse_diags,
        first_error: &first_error,
        outside: outside_codes(&dl::doc_text(), dl::DOC).into_iter().collect(),
    };
    // A literal is one statement.
    let one_statement = |_: &str| vec![];
    for path in files(&workspace_dir().join("crates"), &["dlt"]) {
        let source = std::fs::read_to_string(path).X();
        let clean = dl::parse_diags(&source).is_ok_and(|d| d.is_empty());
        for seed in seeds.iter() {
            let m = mutate_at(&source, &mut SeededRng::derive(seed_of(&source), seed));
            hold(&lang, &one_statement, &mut broken, &source, &m, clean);
        }
    }

    let recs = df::recognizers(&df::grammar());
    for start in [Start::Script, Start::Module, Start::Interface] {
        let parse = move |t: &str| df::parse_diags(start, t);
        let first_error = |t: &str| recs.first_error(start.rule(), t);
        let lang = Lang {
            name: start.rule(),
            parse: &parse,
            first_error: &first_error,
            outside: outside_codes(&df::doc_text(), df::DOC).into_iter().collect(),
        };
        for (s, source) in df::corpus() {
            if s != start {
                continue;
            }
            let clean = df::parse_diags(s, &source).is_ok_and(|d| d.is_empty());
            for seed in seeds.iter() {
                let m = mutate_at(&source, &mut SeededRng::derive(seed_of(&source), seed));
                hold(&lang, &statement_starts, &mut broken, &source, &m, clean);
            }
        }
    }

    let mut report = String::new();
    for (what, (count, example)) in &broken.0 {
        report.push_str(&fmt!("\n{count:>5}  {what}\n       e.g. {:?}\n", Broken::window(example)));
    }
    assert!(broken.0.is_empty(), "the recovery contract is broken:\n{report}");
}

#[test]
#[ignore = "a report, run by hand"]
fn report() {
    // Datalit.
    let recs = dl::recognizers(&dl::grammar());
    let first_error = |t: &str| recs.first_error(dl::START, t);
    let lang = Lang {
        name: "datalit",
        parse: &dl::parse_diags,
        first_error: &first_error,
        outside: outside_codes(&dl::doc_text(), dl::DOC).into_iter().collect(),
    };
    let mut report = Report::new();
    let mut shared = Report::new();
    for path in files(&workspace_dir().join("crates"), &["dlt"]) {
        let source = std::fs::read_to_string(path).X();
        if !dl::parse_diags(&source).is_ok_and(|d| d.is_empty()) {
            continue;
        }
        let mut rng = SeededRng::new(seed_of(&source));
        for _ in 0..MUTANTS {
            let m = mutate_at(&source, &mut rng);
            let (text, at) = (m.text, m.at);
            read_change(&lang, &mut report, &text, at);

            // Shared syntax.
            shared.changes += 1;
            let codes = |t: &str| -> Option<(Vec<String>, Vec<String>)> {
                let a = dl::parse_diags(t).ok()?.into_iter().map(|d| d.code).collect();
                let b = df::parse_expr_diags(t).ok()?.into_iter().map(|d| d.code).collect();
                Some((a, b))
            };
            if let Some((a, b)) = codes(&text) && !b.is_empty() && !same_codes(&a, &b) {
                let detail = fmt!("datalit {a:?}, datafun {b:?}");
                shared.note("shared syntax", &detail, &text, &|t| codes(t).is_some_and(|(x, y)| (x, y) == (a.C(), b.C())));
            }
        }
    }
    report.print(lang.name);
    shared.print("datalit literals read by datafun's parser");

    // Datafun.
    let recs = df::recognizers(&df::grammar());
    for start in [Start::Script, Start::Module, Start::Interface] {
        let parse = move |t: &str| df::parse_diags(start, t);
        let first_error = |t: &str| recs.first_error(start.rule(), t);
        let lang = Lang {
            name: start.rule(),
            parse: &parse,
            first_error: &first_error,
            outside: outside_codes(&df::doc_text(), df::DOC).into_iter().collect(),
        };
        let mut report = Report::new();
        for (s, source) in df::corpus() {
            if s != start || !df::parse_diags(s, &source).is_ok_and(|d| d.is_empty()) {
                continue;
            }
            let mut rng = SeededRng::new(seed_of(&source));
            for _ in 0..MUTANTS {
                let m = mutate_at(&source, &mut rng);
                read_change(&lang, &mut report, &m.text, m.at);
            }
        }
        report.print(&fmt!("datafun {}", lang.name));
    }
}
