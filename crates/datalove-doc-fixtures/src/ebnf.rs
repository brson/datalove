//! An EBNF grammar read out of a document, with a recognizer and a generator.
//!
//! The grammar is ISO 14977 EBNF as the `botdocs/*-ebnf.md` documents write
//! it: `,` concatenates, `|` separates alternatives, `[ x ]` is optional,
//! `{ x }` repeats, `( x )` groups, `"x"` and `'x'` are terminals (with no
//! escapes, but `"\n"` is a newline), `"a" .. "z"` is a range of characters,
//! `? x ?` is a special sequence and `(* x *)` is a comment. Terminals are characters: the grammar describes
//! text, not tokens, so whitespace and comments are rules like any other.
//!
//! What a special sequence means is English, so the caller gives each one a
//! [`CharTest`]. The caller may also rewrite the grammar before compiling it,
//! adding [`Expr::Assert`]s, which match no character but test the position
//! they stand at.

use rmx::prelude::*;
use bct::lookup::{LookupMap, LookupSet};
use bct::random::SeededRng;
use std::collections::BTreeSet;

/// A test on the text at a position.
///
/// For a special sequence, it is whether the character at the position
/// matches, and the position is always before the end. For an assertion, it
/// is whether the position itself is acceptable, and it may be the end.
pub type CharTest = fn(&[char], usize) -> bool;

/// A grammar: its rules, in the order they are written.
#[derive(Clone, Debug)]
pub struct Grammar {
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub body: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Alt(Vec<Expr>),
    Seq(Vec<Expr>),
    Opt(Box<Expr>),
    Rep(Box<Expr>),
    Lit(String),
    Range(char, char),
    Special(String),
    Ref(String),
    /// A zero-width test, never written in a document.
    Assert(String),
}

impl Grammar {
    /// Reads the one ```` ```ebnf ```` block in a markdown document.
    pub fn from_markdown(doc: &str) -> Grammar {
        let blocks: Vec<&str> = doc.split("```ebnf\n").skip(1).collect();
        assert_eq!(blocks.len(), 1, "expected exactly one ebnf block");
        let block = blocks[0].split("```").next().X();
        Grammar::parse(block)
    }

    /// Parses EBNF text.
    pub fn parse(text: &str) -> Grammar {
        let tokens = tokenize(text);
        let mut p = Reader { tokens, pos: 0 };
        let mut rules = vec![];
        while p.pos < p.tokens.len() {
            let name = match p.next() {
                Tok::Ident(name) => name,
                t => panic!("expected a rule name, found {t:?}"),
            };
            p.expect(Tok::Punct('='));
            let body = p.alt();
            p.expect(Tok::Punct(';'));
            rules.push(Rule { name, body });
        }
        let g = Grammar { rules };
        g.check_refs();
        g
    }

    pub fn get(&self, name: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.name == name)
    }

    /// The names of the rules reachable from `roots`, the roots included.
    pub fn reachable(&self, roots: &[&str]) -> BTreeSet<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = roots.iter().map(|r| r.S()).collect();
        while let Some(name) = stack.pop() {
            if !seen.insert(name.C()) {
                continue;
            }
            let rule = self.get(&name).unwrap_or_else(|| panic!("no rule {name}"));
            rule.body.visit(&mut |e| {
                if let Expr::Ref(r) = e {
                    stack.push(r.C());
                }
            });
        }
        seen
    }

    fn check_refs(&self) {
        let names: BTreeSet<&str> = self.rules.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), self.rules.len(), "a rule is defined twice");
        for rule in &self.rules {
            rule.body.visit(&mut |e| {
                if let Expr::Ref(r) = e {
                    assert!(names.contains(r.as_str()), "rule {} uses undefined {r}", rule.name);
                }
            });
        }
    }
}

impl Expr {
    /// Calls `f` on this expression and every one inside it.
    pub fn visit(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Alt(xs) | Expr::Seq(xs) => xs.iter().for_each(|x| x.visit(f)),
            Expr::Opt(x) | Expr::Rep(x) => x.visit(f),
            Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Ref(_) | Expr::Assert(_) => {}
        }
    }
}

// ============================================================================
// Reading
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Lit(String),
    Special(String),
    Punct(char),
    DotDot,
}

fn tokenize(text: &str) -> Vec<Tok> {
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '(' && cs.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            loop {
                if cs[i] == '(' && cs.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if cs[i] == '*' && cs.get(i + 1) == Some(&')') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
                assert!(i < cs.len(), "unclosed comment");
            }
        } else if c == '"' || c == '\'' {
            let end = i + 1 + cs[i + 1..].iter().position(|&d| d == c).expect("unclosed terminal");
            let lit: String = cs[i + 1..end].iter().collect();
            out.push(Tok::Lit(if lit == "\\n" { "\n".S() } else { lit }));
            i = end + 1;
        } else if c == '?' {
            let end = i + 1 + cs[i + 1..].iter().position(|&d| d == '?').expect("unclosed special sequence");
            out.push(Tok::Special(cs[i + 1..end].iter().collect::<String>().trim().S()));
            i = end + 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(cs[start..i].iter().collect()));
        } else if c == '.' && cs.get(i + 1) == Some(&'.') {
            out.push(Tok::DotDot);
            i += 2;
        } else if "=,|;[]{}()".contains(c) {
            out.push(Tok::Punct(c));
            i += 1;
        } else {
            panic!("unexpected {c:?} in ebnf");
        }
    }
    out
}

struct Reader {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Reader {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Tok {
        let t = self.tokens.get(self.pos).expect("unexpected end of ebnf").C();
        self.pos += 1;
        t
    }

    fn expect(&mut self, want: Tok) {
        let got = self.next();
        assert_eq!(got, want, "at ebnf token {}", self.pos - 1);
    }

    fn alt(&mut self) -> Expr {
        let mut xs = vec![self.seq()];
        while self.peek() == Some(&Tok::Punct('|')) {
            self.pos += 1;
            xs.push(self.seq());
        }
        if xs.len() == 1 { xs.pop().X() } else { Expr::Alt(xs) }
    }

    fn seq(&mut self) -> Expr {
        let mut xs = vec![self.term()];
        while self.peek() == Some(&Tok::Punct(',')) {
            self.pos += 1;
            xs.push(self.term());
        }
        if xs.len() == 1 { xs.pop().X() } else { Expr::Seq(xs) }
    }

    fn term(&mut self) -> Expr {
        match self.next() {
            Tok::Ident(name) => Expr::Ref(name),
            Tok::Special(text) => Expr::Special(text),
            Tok::Lit(s) => {
                if self.peek() == Some(&Tok::DotDot) {
                    self.pos += 1;
                    let Tok::Lit(hi) = self.next() else { panic!("a range needs a terminal after ..") };
                    let one = |s: &str| {
                        let mut cs = s.chars();
                        let c = cs.next().X();
                        assert!(cs.next().is_none(), "a range is between single characters");
                        c
                    };
                    Expr::Range(one(&s), one(&hi))
                } else {
                    Expr::Lit(s)
                }
            }
            Tok::Punct('[') => {
                let x = self.alt();
                self.expect(Tok::Punct(']'));
                Expr::Opt(Box::new(x))
            }
            Tok::Punct('{') => {
                let x = self.alt();
                self.expect(Tok::Punct('}'));
                Expr::Rep(Box::new(x))
            }
            Tok::Punct('(') => {
                let x = self.alt();
                self.expect(Tok::Punct(')'));
                x
            }
            t => panic!("unexpected {t:?} in ebnf"),
        }
    }
}

// ============================================================================
// Recognizing
// ============================================================================

#[derive(Clone, Copy, Debug)]
enum Sym {
    Nt(u32),
    Char(char),
    Range(char, char),
    Class(CharTest),
    Assert(CharTest),
}

/// An Earley recognizer for one start rule of a grammar.
pub struct Recognizer {
    /// Each production's left-hand side and right-hand side.
    prods: Vec<(u32, Vec<Sym>)>,
    by_lhs: Vec<Vec<u32>>,
    start: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Item {
    prod: u32,
    dot: u32,
    origin: u32,
}

#[derive(Default)]
struct ItemSet {
    items: Vec<Item>,
    seen: LookupSet<Item>,
    /// Items whose next symbol is the nonterminal, by nonterminal.
    waiting: LookupMap<u32, Vec<Item>>,
    /// Nonterminals that have matched the empty string here.
    empty: LookupSet<u32>,
}

impl ItemSet {
    fn add(&mut self, item: Item, prods: &[(u32, Vec<Sym>)]) {
        if self.seen.insert(item) {
            self.items.push(item);
            if let Some(Sym::Nt(n)) = prods[item.prod as usize].1.get(item.dot as usize) {
                self.waiting.entry(*n).or_default().push(item);
            }
        }
    }
}

struct Compiler<'a> {
    names: LookupMap<&'a str, u32>,
    prods: Vec<(u32, Vec<Sym>)>,
    next_nt: u32,
    special: &'a dyn Fn(&str) -> CharTest,
}

impl<'a> Compiler<'a> {
    fn fresh(&mut self) -> u32 {
        let n = self.next_nt;
        self.next_nt += 1;
        n
    }

    fn syms(&mut self, e: &Expr) -> Vec<Sym> {
        match e {
            Expr::Seq(xs) => xs.iter().flat_map(|x| self.syms(x)).collect(),
            Expr::Lit(s) => s.chars().map(Sym::Char).collect(),
            Expr::Range(lo, hi) => vec![Sym::Range(*lo, *hi)],
            Expr::Special(text) => vec![Sym::Class((self.special)(text))],
            Expr::Assert(name) => vec![Sym::Assert((self.special)(name))],
            Expr::Ref(name) => vec![Sym::Nt(self.names[name.as_str()])],
            Expr::Alt(xs) => {
                let n = self.fresh();
                for x in xs {
                    let rhs = self.syms(x);
                    self.prods.push((n, rhs));
                }
                vec![Sym::Nt(n)]
            }
            Expr::Opt(x) => {
                let n = self.fresh();
                self.prods.push((n, vec![]));
                let rhs = self.syms(x);
                self.prods.push((n, rhs));
                vec![Sym::Nt(n)]
            }
            Expr::Rep(x) => {
                let n = self.fresh();
                self.prods.push((n, vec![]));
                let mut rhs = vec![Sym::Nt(n)];
                rhs.extend(self.syms(x));
                self.prods.push((n, rhs));
                vec![Sym::Nt(n)]
            }
        }
    }
}

impl Recognizer {
    /// Compiles the grammar, giving each special sequence and assertion its
    /// test with `special`, which panics on one it does not know.
    pub fn new(g: &Grammar, start: &str, special: &dyn Fn(&str) -> CharTest) -> Recognizer {
        let names: LookupMap<&str, u32> =
            g.rules.iter().enumerate().map(|(i, r)| (r.name.as_str(), i as u32)).collect();
        let mut c = Compiler { names, prods: vec![], next_nt: g.rules.len() as u32, special };
        for (i, rule) in g.rules.iter().enumerate() {
            let alts: Vec<&Expr> = match &rule.body {
                Expr::Alt(xs) => xs.iter().collect(),
                x => vec![x],
            };
            for x in alts {
                let rhs = c.syms(x);
                c.prods.push((i as u32, rhs));
            }
        }
        let mut by_lhs = vec![vec![]; c.next_nt as usize];
        for (i, (lhs, _)) in c.prods.iter().enumerate() {
            by_lhs[*lhs as usize].push(i as u32);
        }
        let start = *c.names.get(start).unwrap_or_else(|| panic!("no rule {start}"));
        Recognizer { prods: c.prods, by_lhs, start }
    }

    /// Whether the whole of `text` is the start rule.
    pub fn accepts(&self, text: &str) -> bool {
        let input: Vec<char> = text.chars().collect();
        let n = input.len();
        let mut sets: Vec<ItemSet> = (0..=n).map(|_| ItemSet::default()).collect();
        for &p in &self.by_lhs[self.start as usize] {
            sets[0].add(Item { prod: p, dot: 0, origin: 0 }, &self.prods);
        }
        for i in 0..=n {
            let mut k = 0;
            while k < sets[i].items.len() {
                let item = sets[i].items[k];
                k += 1;
                let (lhs, rhs) = &self.prods[item.prod as usize];
                let advanced = Item { dot: item.dot + 1, ..item };
                match rhs.get(item.dot as usize) {
                    None => {
                        let origin = item.origin as usize;
                        if origin == i {
                            sets[i].empty.insert(*lhs);
                        }
                        let waiters = sets[origin].waiting.get(lhs).cloned().unwrap_or_default();
                        for w in waiters {
                            sets[i].add(Item { dot: w.dot + 1, ..w }, &self.prods);
                        }
                    }
                    Some(Sym::Nt(m)) => {
                        for &p in &self.by_lhs[*m as usize] {
                            sets[i].add(Item { prod: p, dot: 0, origin: i as u32 }, &self.prods);
                        }
                        if sets[i].empty.contains(m) {
                            sets[i].add(advanced, &self.prods);
                        }
                    }
                    Some(Sym::Assert(test)) => {
                        if test(&input, i) {
                            sets[i].add(advanced, &self.prods);
                        }
                    }
                    Some(sym) => {
                        if i < n {
                            let ok = match sym {
                                Sym::Char(c) => input[i] == *c,
                                Sym::Range(lo, hi) => (*lo..=*hi).contains(&input[i]),
                                Sym::Class(test) => test(&input, i),
                                Sym::Nt(_) | Sym::Assert(_) => unreachable!(),
                            };
                            if ok {
                                sets[i + 1].add(advanced, &self.prods);
                            }
                        }
                    }
                }
            }
        }
        sets[n].items.iter().any(|item| {
            let (lhs, rhs) = &self.prods[item.prod as usize];
            *lhs == self.start && item.origin == 0 && item.dot as usize == rhs.len()
        })
    }
}

// ============================================================================
// Generating
// ============================================================================

/// Generates random strings of a grammar.
///
/// Below `max_depth` rule references, every alternative is equally likely
/// and options and repetitions are taken half the time; past it, each choice
/// is the one that finishes soonest.
///
/// A special sequence's character comes from `sample`. An assertion cannot be
/// tested where it is reached, since what follows is not yet written, so once
/// the string is done, `repair` is given each assertion's name and the text
/// before and after it, and returns what to insert there to make it hold,
/// often nothing. What is generated is still a candidate, to be checked with a
/// [`Recognizer`].
pub struct Generator<'g> {
    grammar: &'g Grammar,
    /// The fewest rule references each rule needs to finish.
    cost: LookupMap<&'g str, u32>,
    sample: &'g dyn Fn(&str, &mut SeededRng) -> char,
    repair: &'g dyn Fn(&str, &str, &str) -> &'static str,
    max_depth: u32,
}

/// A string being generated, with the assertions reached in it.
struct Draft<'e> {
    text: String,
    asserts: Vec<(usize, &'e str)>,
}

const INF: u32 = u32::MAX / 4;

impl<'g> Generator<'g> {
    pub fn new(
        grammar: &'g Grammar,
        sample: &'g dyn Fn(&str, &mut SeededRng) -> char,
        repair: &'g dyn Fn(&str, &str, &str) -> &'static str,
    ) -> Generator<'g> {
        let mut cost: LookupMap<&str, u32> = grammar.rules.iter().map(|r| (r.name.as_str(), INF)).collect();
        loop {
            let mut changed = false;
            for rule in &grammar.rules {
                let c = expr_cost(&rule.body, &cost).saturating_add(1).min(INF);
                if c < cost[rule.name.as_str()] {
                    cost.insert(&rule.name, c);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for (name, c) in cost.iter_unordered() {
            assert!(*c < INF, "rule {name} never finishes");
        }
        Generator { grammar, cost, sample, repair, max_depth: 12 }
    }

    pub fn generate(&self, start: &str, rng: &mut SeededRng) -> String {
        let start = Expr::Ref(start.S());
        let mut draft = Draft { text: String::new(), asserts: vec![] };
        self.emit(&start, 0, rng, &mut draft);
        let mut text = draft.text;
        for &(at, name) in draft.asserts.iter().rev() {
            let insert = (self.repair)(name, &text[..at], &text[at..]);
            text.insert_str(at, insert);
        }
        text
    }

    fn emit<'e>(&'e self, e: &'e Expr, depth: u32, rng: &mut SeededRng, out: &mut Draft<'e>) {
        let deep = depth > self.max_depth;
        match e {
            Expr::Alt(xs) => {
                let x = if deep {
                    xs.iter().min_by_key(|x| expr_cost(x, &self.cost)).X()
                } else {
                    &xs[rng.random_range(0..xs.len())]
                };
                self.emit(x, depth, rng, out);
            }
            Expr::Seq(xs) => xs.iter().for_each(|x| self.emit(x, depth, rng, out)),
            Expr::Opt(x) => {
                if !deep && rng.random_bool(0.5) {
                    self.emit(x, depth, rng, out);
                }
            }
            Expr::Rep(x) => {
                if !deep {
                    while rng.random_bool(0.5) && out.text.len() < 4096 {
                        self.emit(x, depth, rng, out);
                    }
                }
            }
            Expr::Lit(s) => out.text.push_str(s),
            Expr::Range(lo, hi) => {
                out.text.push(char::from_u32(rng.random_range(*lo as u32..=*hi as u32)).X());
            }
            Expr::Special(text) => out.text.push((self.sample)(text, rng)),
            Expr::Assert(name) => out.asserts.push((out.text.len(), name)),
            Expr::Ref(name) => {
                let rule = self.grammar.get(name).X();
                self.emit(&rule.body, depth + 1, rng, out);
            }
        }
    }
}

fn expr_cost(e: &Expr, cost: &LookupMap<&str, u32>) -> u32 {
    match e {
        Expr::Alt(xs) => xs.iter().map(|x| expr_cost(x, cost)).min().X(),
        Expr::Seq(xs) => xs.iter().map(|x| expr_cost(x, cost)).fold(0, |a, b| a.saturating_add(b).min(INF)),
        Expr::Opt(_) | Expr::Rep(_) => 0,
        Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Assert(_) => 0,
        Expr::Ref(name) => cost[name.as_str()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any(_: &[char], _: usize) -> bool {
        true
    }

    fn special(_: &str) -> CharTest {
        any
    }

    #[test]
    fn reads_and_recognizes() {
        let g = Grammar::parse(
            r#"
            list = "[", ws, [ item, { ws, ",", ws, item } ], ws, "]" ;  (* a list *)
            item = digit, { digit } | list ;
            digit = "0" .. "9" ;
            ws = { " " } ;
            "#,
        );
        let r = Recognizer::new(&g, "list", &special);
        assert!(r.accepts("[]"));
        assert!(r.accepts("[ 1, [22 , []] ]"));
        assert!(!r.accepts("[1,]"));
        assert!(!r.accepts("[1 2]"));
        assert!(!r.accepts("[[]"));
    }

    #[test]
    fn generates_what_it_recognizes() {
        let g = Grammar::parse(
            r#"
            e = t, { "+", t } ;
            t = "x" | "(", e, ")" | ? any ? ;
            "#,
        );
        let r = Recognizer::new(&g, "e", &special);
        let sample = |_: &str, _: &mut SeededRng| 'y';
        let repair = |_: &str, _: &str, _: &str| "";
        let generator = Generator::new(&g, &sample, &repair);
        let mut rng = SeededRng::new(1);
        for _ in 0..200 {
            let s = generator.generate("e", &mut rng);
            assert!(r.accepts(&s), "{s}");
        }
    }
}
