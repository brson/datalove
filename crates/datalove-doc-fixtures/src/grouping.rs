//! How an expression groups, as a grammar says and as a parser does.
//!
//! The grammar groups by its parse tree. A node of an expression rule is one
//! expression, and in a rule `a, { op, b }` each iteration takes everything
//! before it in the rule as its left operand: `a - b - c` is `(a - b) - c`.
//! A parser groups by the spans of the expressions it builds. The two must
//! make the same spans of two tokens or more: one token is one expression
//! however it is read, and its span says nothing of grouping.
//!
//! The expression rules are the top one and every rule it is by itself,
//! through any number of rules: `expr` is `or_expr` with no `or`, and so on
//! down to `primary`. A [`GROUPING`] node of one element and no comma is its
//! element, as parentheses group and make nothing.

use rmx::prelude::*;
use std::collections::BTreeSet;
use std::ops::Range;

use crate::ebnf::{Expr, Grammar, Node, Tree};
use crate::token_grammar::{Kind, Tokens};

/// A span, from its start to its end.
pub type Span = (usize, usize);

/// The rule of parentheses, which group where they hold one element and no
/// comma.
pub const GROUPING: &str = "paren";

/// The rules that derive nothing, as far as rules go: a special sequence is
/// taken to match something.
fn nullable_rules(g: &Grammar) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    loop {
        let before = out.len();
        for rule in &g.rules {
            if nullable(&rule.body, &out) {
                out.insert(rule.name.C());
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

fn nullable(e: &Expr, rules: &BTreeSet<String>) -> bool {
    match e {
        Expr::Seq(xs) => xs.iter().all(|x| nullable(x, rules)),
        Expr::Alt(xs) => xs.iter().any(|x| nullable(x, rules)),
        Expr::Opt(_) | Expr::Rep(_) | Expr::Assert(_) | Expr::Glue(None) => true,
        Expr::Glue(Some(x)) | Expr::Except(x, _) => nullable(x, rules),
        Expr::Ref(name) => rules.contains(name),
        Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) => false,
    }
}

/// The rules `e` can be by itself, with nothing else matched beside it.
fn units(e: &Expr, nullable_rules: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    match e {
        Expr::Ref(name) => {
            out.insert(name.C());
        }
        Expr::Alt(xs) => xs.iter().for_each(|x| units(x, nullable_rules, out)),
        Expr::Seq(xs) => {
            let solid: Vec<&Expr> = xs.iter().filter(|x| !nullable(x, nullable_rules)).collect();
            if let [x] = solid[..] {
                units(x, nullable_rules, out);
            }
        }
        Expr::Opt(x) | Expr::Rep(x) | Expr::Glue(Some(x)) | Expr::Except(x, _) => units(x, nullable_rules, out),
        Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Assert(_) | Expr::Glue(None) => {}
    }
}

/// `top` and the rules it is by itself, through any number of rules.
pub fn expression_rules(g: &Grammar, top: &str) -> BTreeSet<String> {
    let empty = nullable_rules(g);
    let mut out = BTreeSet::from([top.S()]);
    let mut todo = vec![top.S()];
    while let Some(name) = todo.pop() {
        let Some(rule) = g.get(&name) else { continue };
        let mut found = BTreeSet::new();
        units(&rule.body, &empty, &mut found);
        for r in found {
            if out.insert(r.C()) {
                todo.push(r);
            }
        }
    }
    out
}

/// The nodes under `t` that are rules, through the groups and repetitions
/// written inside it.
fn rule_children(t: &Tree) -> Vec<&Tree> {
    let mut out = vec![];
    for c in &t.children {
        match c.node {
            Node::Rule(_) => out.push(c),
            Node::Group | Node::Rep | Node::Iter => out.extend(rule_children(c)),
            Node::Term => {}
        }
    }
    out
}

fn terms(t: &Tree) -> usize {
    t.children
        .iter()
        .map(|c| match c.node {
            Node::Term => 1,
            Node::Group | Node::Rep | Node::Iter => terms(c),
            Node::Rule(_) => 0,
        })
        .sum()
}

/// Whether `t` is, by itself through any number of rules, parentheses
/// that group.
fn groups(t: &Tree) -> bool {
    let mut t = t;
    loop {
        if t.node == Node::Rule(GROUPING.S()) {
            return terms(t) == 2 && rule_children(t).len() == 1;
        }
        match rule_children(t)[..] {
            [c] if c.span == t.span => t = c,
            _ => return false,
        }
    }
}

/// The spans, in tokens, of the expressions of a parse tree.
pub fn tree_spans(t: &Tree, exprs: &BTreeSet<String>, out: &mut BTreeSet<Span>) {
    if let Node::Rule(name) = &t.node
        && exprs.contains(name)
    {
        if !groups(t) {
            out.insert((t.span.start, t.span.end));
        }
        for c in &t.children {
            if c.node == Node::Rep {
                for iteration in &c.children {
                    out.insert((t.span.start, iteration.span.end));
                }
            }
        }
    }
    for c in &t.children {
        tree_spans(c, exprs, out);
    }
}

/// A span of tokens in bytes, from the first real token to the last, and
/// how many real tokens it holds.
pub fn token_bytes(tokens: &Tokens, span: Span) -> (Span, usize) {
    let real: Vec<_> = tokens.toks[span.0..span.1].iter().filter(|t| t.kind == Kind::Token).collect();
    match (real.first(), real.last()) {
        (Some(a), Some(b)) => ((a.start, b.end), real.len()),
        _ => ((0, 0), 0),
    }
}

/// How many real tokens lie within `bytes`.
pub fn tokens_within(tokens: &Tokens, bytes: Span) -> usize {
    tokens.toks.iter().filter(|t| t.kind == Kind::Token && bytes.0 <= t.start && t.end <= bytes.1).count()
}

/// The grammar's grouping of a parse tree: the byte spans of its
/// expressions of two tokens or more.
pub fn grammar_grouping(t: &Tree, tokens: &Tokens, exprs: &BTreeSet<String>) -> BTreeSet<Span> {
    let mut spans = BTreeSet::new();
    tree_spans(t, exprs, &mut spans);
    spans.iter().map(|&s| token_bytes(tokens, s)).filter(|(_, n)| *n >= 2).map(|(b, _)| b).collect()
}

/// A parser's grouping: the byte spans of its expressions of two tokens or
/// more.
pub fn parser_grouping(spans: &[Range<usize>], tokens: &Tokens) -> BTreeSet<Span> {
    spans.iter().map(|s| (s.start, s.end)).filter(|&s| tokens_within(tokens, s) >= 2).collect()
}

/// `text` with each span in brackets, `{` and `}`, to show a grouping.
pub fn render(text: &str, spans: &BTreeSet<Span>) -> String {
    let mut opens = vec![0; text.len() + 1];
    let mut closes = vec![0; text.len() + 1];
    for &(start, end) in spans {
        opens[start] += 1;
        closes[end] += 1;
    }
    let mut out = String::new();
    for (i, c) in text.char_indices() {
        out.push_str(&"}".repeat(closes[i]));
        out.push_str(&"{".repeat(opens[i]));
        out.push(c);
    }
    out.push_str(&"}".repeat(closes[text.len()]));
    out
}
