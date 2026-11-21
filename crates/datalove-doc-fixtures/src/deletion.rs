//! The grammar's notes of what a missing part is refused with, checked.
//!
//! A note `(* no X: CODE *)` on a rule says that where a node of the rule is
//! written without `X`, the parser refuses it with `CODE`: `X` is EBNF, a
//! terminal, a rule, or a sequence of them, written directly in the rule.
//! Each such note is checked on generated inputs: where a node of the rule
//! holds `X`, it is taken out, and if the grammar then stops where `X` was,
//! the parser's first error must be `CODE`, inside the node or on the token
//! after the gap. Where the
//! grammar reads on, what is left means something else, and the note does
//! not apply. Every note must be checked at least once.

use rmx::prelude::*;
use std::collections::BTreeMap;
use std::ops::Range;

use crate::ebnf::{Expr, Grammar, Node, Parse, Tree};
use crate::grammar_check::Diag;
use crate::token_grammar::{Kind, Recognizers, TokenGrammar, Tokens};

/// A note that a missing part is refused with a code.
#[derive(Clone, Debug)]
pub struct Deletion {
    pub rule: String,
    pub what: Expr,
    pub code: String,
}

impl Deletion {
    fn name(&self) -> String {
        fmt!("{}: no {}: {}", self.rule, self.what.show(), self.code)
    }
}

fn is_code(code: &str) -> bool {
    code.len() == 4 && code.starts_with(['D', 'P', 'F']) && code[1..].chars().all(|c| c.is_ascii_digit())
}

/// The codes a grammar's notes say the parser reports and reads past: a
/// clause `what: CODE, read anyway`.
pub fn read_anyway(g: &Grammar) -> Vec<String> {
    let clauses = g.rules.iter().flat_map(|r| &r.notes).flat_map(|n| n.split(';').map(str::trim));
    clauses
        .filter_map(|c| c.strip_suffix(", read anyway")?.rsplit_once(": ").map(|(_, code)| code.S()))
        .filter(|code| is_code(code))
        .collect()
}

/// The deletion notes of a grammar, each clause of a note read on its own:
/// `no X: CODE`, with `CODE` a diagnostic code.
pub fn deletions(g: &Grammar) -> Vec<Deletion> {
    let mut out = vec![];
    for rule in &g.rules {
        for note in &rule.notes {
            for clause in note.split(';').map(str::trim) {
                let Some(rest) = clause.strip_prefix("no ") else { continue };
                let Some((what, code)) = rest.rsplit_once(": ") else { continue };
                if is_code(code) {
                    out.push(Deletion { rule: rule.name.C(), what: Expr::parse(what), code: code.S() });
                }
            }
        }
    }
    out
}

/// The parts of a node written directly in its rule, in order: its
/// terminals and the nodes of the rules it uses, through the groups and
/// repetitions inside it, assertions left out.
fn parts(t: &Tree) -> Vec<&Tree> {
    let mut out = vec![];
    for c in &t.children {
        match c.node {
            Node::Rule(_) => out.push(c),
            Node::Term if !c.span.is_empty() => out.push(c),
            Node::Term => {}
            Node::Group | Node::Rep | Node::Iter => out.extend(parts(c)),
        }
    }
    out
}

/// Whether part `p` is `x`: a terminal by its text, a rule by its name, and
/// a rule read whole as an island by its being one.
fn is(g: &TokenGrammar, tokens: &Tokens, p: &Tree, x: &Expr) -> bool {
    match (x, &p.node) {
        (Expr::Lit(s), Node::Term) => p.span.len() == 1 && tokens.token_text(p.span.start) == Some(s.as_str()),
        (Expr::Ref(r), Node::Rule(n)) => r == n,
        (Expr::Ref(r), Node::Term) => Recognizers::is_island(g, r),
        _ => false,
    }
}

/// Where `what` is first written directly in node `t`, in tokens.
fn find(g: &TokenGrammar, tokens: &Tokens, t: &Tree, what: &Expr) -> Option<Range<usize>> {
    let seq: Vec<&Expr> = match what {
        Expr::Seq(xs) => xs.iter().collect(),
        x => vec![x],
    };
    let ps = parts(t);
    (0..ps.len()).find_map(|i| {
        let run = ps.get(i..i + seq.len())?;
        run.iter().zip(&seq).all(|(p, x)| is(g, tokens, p, x)).then(|| run[0].span.start..run[run.len() - 1].span.end)
    })
}

fn nodes<'t>(t: &'t Tree, out: &mut Vec<&'t Tree>) {
    if matches!(t.node, Node::Rule(_)) {
        out.push(t);
    }
    t.children.iter().for_each(|c| nodes(c, out));
}

/// The bytes tokens `span` cover, from the first's start to the last's end.
fn bytes(tokens: &Tokens, span: &Range<usize>) -> Range<usize> {
    tokens.toks[span.start].start..tokens.toks[span.end - 1].end
}

/// Checks a grammar's deletion notes on inputs, and says what it found.
pub struct Deletions<'a> {
    pub grammar: &'a TokenGrammar,
    pub recs: &'a Recognizers,
    /// The parser's diagnostics of a text as a start rule.
    pub parse: &'a dyn Fn(&str, &str) -> Result<Vec<Diag>, String>,
    notes: Vec<Deletion>,
    /// The codes the parser reports and reads past.
    read_anyway: Vec<String>,
    /// How many times each note was checked.
    checked: BTreeMap<String, usize>,
    /// What each note was wrong about first: the input, and what it got.
    wrong: BTreeMap<String, (String, String)>,
}

/// The most tokens of an input whose parse tree is built.
const MAX_TOKENS: usize = 60;

impl<'a> Deletions<'a> {
    pub fn new(
        grammar: &'a TokenGrammar,
        recs: &'a Recognizers,
        parse: &'a dyn Fn(&str, &str) -> Result<Vec<Diag>, String>,
    ) -> Deletions<'a> {
        let notes = deletions(&grammar.typed);
        assert!(!notes.is_empty(), "the grammar has no deletion notes");
        let checked = notes.iter().map(|d| (d.name(), 0)).collect();
        let read_anyway = read_anyway(&grammar.typed);
        Deletions { grammar, recs, parse, notes, read_anyway, checked, wrong: BTreeMap::new() }
    }

    /// Checks every note on every node of `text`, as `start`, it applies
    /// to, and says whether it could: `text` must be short, have one parse
    /// tree, and be taken by the parser as it is, so that what the parser
    /// refuses is the part taken out.
    pub fn check(&mut self, start: &str, text: &str) -> bool {
        let tokens = self.recs.tokens(text);
        if tokens.len() > MAX_TOKENS {
            return false;
        }
        let taken = (self.parse)(start, text).is_ok_and(|diags| diags.iter().all(|d| !d.error));
        if !taken {
            return false;
        }
        let Parse::Tree(tree) = self.recs.parse(start, &tokens) else { return false };
        let mut all = vec![];
        nodes(&tree, &mut all);
        for node in all {
            let Node::Rule(rule) = &node.node else { continue };
            for d in self.notes.iter().filter(|d| &d.rule == rule) {
                let Some(span) = find(self.grammar, &tokens, node, &d.what) else { continue };
                let cut = bytes(&tokens, &span);
                let left = fmt!("{}{}", &text[..cut.start], &text[cut.end..]);
                // The note is of `X` missing, which is where the grammar
                // stops: at the token after the cut. Where it reads on, what
                // is left means something else, as `for in ref xs` names its
                // element `in`.
                let after = tokens.toks.iter().find(|t| t.kind == Kind::Token && t.start >= cut.end);
                let at = after.map_or(left.len(), |t| t.start - cut.len());
                if self.recs.first_error(start, &left) != Some(at) {
                    continue;
                }
                // The error may be on the token after the gap, where the
                // grammar stops, though that is past the node.
                let within = bytes(&tokens, &node.span);
                let within = within.start..(within.end - cut.len()).max(at);
                let diags = (self.parse)(start, &left);
                let first = diags.as_ref().ok().and_then(|diags| {
                    diags.iter().filter(|d| d.error).min_by_key(|d| d.primary.as_ref().map(|p| p.start)).cloned()
                });
                // A complaint the parser reads past, as of a postfix written
                // apart, is not where it stopped: it read on over the gap.
                if first.as_ref().is_some_and(|e| self.read_anyway.contains(&e.code)) {
                    continue;
                }
                *self.checked.get_mut(&d.name()).X() += 1;
                let got = match diags.map(|_| first) {
                    Err(panic) => Some(fmt!("a panic: {panic}")),
                    Ok(first) => match first {
                        None => Some(S("no error")),
                        Some(e) if e.code != d.code => Some(fmt!("{} first", e.code)),
                        Some(e) => match &e.primary {
                            Some(p) if p.start < within.start || p.start > within.end => {
                                Some(fmt!("{} at byte {}, outside the {rule} at {within:?}", e.code, p.start))
                            }
                            _ => None,
                        },
                    },
                };
                if let Some(got) = got {
                    let shorter = |(t, _): &(String, String)| left.len() < t.len();
                    let entry = self.wrong.entry(d.name()).or_insert_with(|| (left.C(), got.C()));
                    if shorter(entry) {
                        *entry = (left, got);
                    }
                }
            }
        }
        true
    }

    /// Fails if a note was wrong, or never checked.
    pub fn finish(self, what: &str) {
        let unchecked: Vec<&String> = self.checked.iter().filter(|(_, n)| **n == 0).map(|(k, _)| k).collect();
        let wrong: Vec<String> =
            self.wrong.iter().map(|(note, (input, got))| fmt!("{note}\n    got {got}, from {input:?}")).collect();
        println!(
            "{what}: {} deletion notes, checked {} times",
            self.checked.len(),
            self.checked.values().sum::<usize>()
        );
        assert!(
            wrong.is_empty() && unchecked.is_empty(),
            "{what}: deletion notes wrong:\n{}\n\nnever checked: {unchecked:?}",
            wrong.join("\n")
        );
    }
}
