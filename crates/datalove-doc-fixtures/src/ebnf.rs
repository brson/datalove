//! An EBNF grammar read out of a document, with a recognizer and a generator.
//!
//! The grammar is ISO 14977 EBNF as the `botdocs/*-ebnf.md` documents write
//! it: `,` concatenates, `|` separates alternatives, `[ x ]` is optional,
//! `{ x }` repeats, `( x )` groups, `a - b` is `a` but not `b`, `"x"` and
//! `'x'` are terminals (with no escapes, but `"\n"` is a newline),
//! `"a" .. "z"` is a range of characters, `? x ?` is a special sequence and
//! `(* x *)` is a comment. Two more notations are the datafun grammar's: `~x`
//! is `x` glued to what comes before it, and a rule may take parameters,
//! `end_line(k) = "end", k ;`, which are filled in where it is used.
//!
//! What the terminals are, characters or tokens, is the caller's to say: the
//! recognizer runs over any input, and asks the caller for a [`Term`] for
//! each terminal, special sequence, `~` and exception. The caller may also
//! rewrite the grammar before compiling it, adding [`Expr::Assert`]s, which
//! match nothing but test the position they stand at.

use rmx::prelude::*;
use bct::lookup::{LookupMap, LookupSet};
use bct::random::SeededRng;
use std::collections::BTreeSet;
use std::rc::Rc;

/// A terminal: pushes the length of each match of it at a position in an
/// input, `0` for an assertion that holds there.
pub type Term<I> = Rc<dyn Fn(&I, usize, &mut Vec<usize>)>;

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
    /// `a - b`: what `a` matches that `b` does not.
    Except(Box<Expr>, Box<Expr>),
    /// `~x`, or a bare `~`.
    Glue(Option<Box<Expr>>),
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
        let mut templates = vec![];
        while p.pos < p.tokens.len() {
            let name = match p.next() {
                Tok::Ident(name) => name,
                t => panic!("expected a rule name, found {t:?}"),
            };
            let mut params = vec![];
            if p.peek() == Some(&Tok::Punct('(')) {
                p.pos += 1;
                loop {
                    match p.next() {
                        Tok::Ident(param) => params.push(param),
                        t => panic!("expected a parameter of {name}, found {t:?}"),
                    }
                    match p.next() {
                        Tok::Punct(',') => {}
                        Tok::Punct(')') => break,
                        t => panic!("expected , or ) in the parameters of {name}, found {t:?}"),
                    }
                }
            }
            p.expect(Tok::Punct('='));
            let body = p.alt();
            p.expect(Tok::Punct(';'));
            if params.is_empty() {
                rules.push(Rule { name, body });
            } else {
                templates.push(Template { name, params, body });
            }
        }
        let mut g = Grammar { rules };
        g.instantiate(&templates);
        g.check_refs();
        g
    }

    pub fn get(&self, name: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Rule> {
        self.rules.iter_mut().find(|r| r.name == name)
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

    /// Replaces each use of a rule with parameters by a rule of its own.
    ///
    /// A use is read as `Ref("name(args)")` with its arguments as written, so
    /// `end_line("fun")` becomes the rule `end_line("fun")`.
    fn instantiate(&mut self, templates: &[Template]) {
        let mut made: BTreeSet<String> = BTreeSet::new();
        let mut uses: Vec<String> = vec![];
        for rule in &self.rules {
            rule.body.visit(&mut |e| {
                if let Expr::Ref(r) = e && r.contains('(') {
                    uses.push(r.C());
                }
            });
        }
        for call in uses {
            if !made.insert(call.C()) {
                continue;
            }
            let (name, args) = call.split_once('(').X();
            let template = templates.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no rule {name}"));
            let mut reader = Reader { tokens: tokenize(args.strip_suffix(')').X()), pos: 0 };
            let args = reader.alt();
            assert_eq!(reader.pos, reader.tokens.len(), "the arguments of {call}");
            let args = match args {
                Expr::Seq(xs) => xs,
                x => vec![x],
            };
            assert_eq!(args.len(), template.params.len(), "{call} takes {} arguments", template.params.len());
            let body = substitute(&template.body, &template.params, &args);
            self.rules.push(Rule { name: call, body });
        }
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

struct Template {
    name: String,
    params: Vec<String>,
    body: Expr,
}

fn substitute(e: &Expr, params: &[String], args: &[Expr]) -> Expr {
    let sub = |x: &Expr| substitute(x, params, args);
    match e {
        Expr::Ref(r) => match params.iter().position(|p| p == r) {
            Some(k) => args[k].C(),
            None => e.C(),
        },
        Expr::Alt(xs) => Expr::Alt(xs.iter().map(sub).collect()),
        Expr::Seq(xs) => Expr::Seq(xs.iter().map(sub).collect()),
        Expr::Opt(x) => Expr::Opt(Box::new(sub(x))),
        Expr::Rep(x) => Expr::Rep(Box::new(sub(x))),
        Expr::Except(a, b) => Expr::Except(Box::new(sub(a)), Box::new(sub(b))),
        Expr::Glue(x) => Expr::Glue(x.as_ref().map(|x| Box::new(sub(x)))),
        Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Assert(_) => e.C(),
    }
}

impl Expr {
    /// Calls `f` on this expression and every one inside it.
    pub fn visit(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Alt(xs) | Expr::Seq(xs) => xs.iter().for_each(|x| x.visit(f)),
            Expr::Opt(x) | Expr::Rep(x) | Expr::Glue(Some(x)) => x.visit(f),
            Expr::Except(a, b) => {
                a.visit(f);
                b.visit(f);
            }
            Expr::Glue(None) | Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Ref(_) | Expr::Assert(_) => {}
        }
    }

    /// This expression with `f` applied to every one inside it, innermost
    /// first, then to itself.
    pub fn map(&self, f: &mut impl FnMut(Expr) -> Expr) -> Expr {
        let inner = match self {
            Expr::Alt(xs) => Expr::Alt(xs.iter().map(|x| x.map(f)).collect()),
            Expr::Seq(xs) => Expr::Seq(xs.iter().map(|x| x.map(f)).collect()),
            Expr::Opt(x) => Expr::Opt(Box::new(x.map(f))),
            Expr::Rep(x) => Expr::Rep(Box::new(x.map(f))),
            Expr::Except(a, b) => Expr::Except(Box::new(a.map(f)), Box::new(b.map(f))),
            Expr::Glue(x) => Expr::Glue(x.as_ref().map(|x| Box::new(x.map(f)))),
            Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Ref(_) | Expr::Assert(_) => self.C(),
        };
        f(inner)
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
            let text: String = cs[i + 1..end].iter().collect();
            out.push(Tok::Special(text.split_whitespace().collect::<Vec<_>>().join(" ")));
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
        } else if "=,|;[]{}()~-".contains(c) {
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
        let mut xs = vec![self.except()];
        while self.peek() == Some(&Tok::Punct(',')) {
            self.pos += 1;
            xs.push(self.except());
        }
        if xs.len() == 1 { xs.pop().X() } else { Expr::Seq(xs) }
    }

    fn except(&mut self) -> Expr {
        let x = self.term();
        if self.peek() == Some(&Tok::Punct('-')) {
            self.pos += 1;
            let y = self.term();
            Expr::Except(Box::new(x), Box::new(y))
        } else {
            x
        }
    }

    fn starts_term(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Ident(_) | Tok::Lit(_) | Tok::Special(_) | Tok::Punct('[' | '{' | '(' | '~'))
        )
    }

    fn term(&mut self) -> Expr {
        match self.next() {
            Tok::Ident(name) => {
                if self.peek() == Some(&Tok::Punct('(')) {
                    // A use of a rule with parameters, kept as written.
                    let mut depth = 0;
                    let mut args = String::new();
                    loop {
                        let t = self.next();
                        match &t {
                            Tok::Punct('(') => depth += 1,
                            Tok::Punct(')') => depth -= 1,
                            _ => {}
                        }
                        args.push_str(&match &t {
                            Tok::Lit(s) => fmt!("{s:?}"),
                            Tok::Ident(s) => s.C(),
                            Tok::Special(s) => fmt!("? {s} ?"),
                            Tok::Punct(c) => c.S(),
                            Tok::DotDot => "..".S(),
                        });
                        if depth == 0 {
                            break;
                        }
                    }
                    Expr::Ref(fmt!("{name}{args}"))
                } else {
                    Expr::Ref(name)
                }
            }
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
            Tok::Punct('~') => {
                if self.starts_term() {
                    Expr::Glue(Some(Box::new(self.term())))
                } else {
                    Expr::Glue(None)
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

enum Sym<I: ?Sized> {
    Nt(u32),
    T(Term<I>),
}

/// An Earley recognizer for one start rule of a grammar, over inputs of type `I`.
pub struct Recognizer<I: ?Sized> {
    /// Each production's left-hand side and right-hand side.
    prods: Vec<(u32, Vec<Sym<I>>)>,
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
    fn add<I: ?Sized>(&mut self, item: Item, prods: &[(u32, Vec<Sym<I>>)]) {
        if self.seen.insert(item) {
            self.items.push(item);
            if let Some(Sym::Nt(n)) = prods[item.prod as usize].1.get(item.dot as usize) {
                self.waiting.entry(*n).or_default().push(item);
            }
        }
    }
}

struct Compiler<'a, I: ?Sized> {
    names: LookupMap<&'a str, u32>,
    prods: Vec<(u32, Vec<Sym<I>>)>,
    next_nt: u32,
    term: &'a dyn Fn(&Expr) -> Term<I>,
}

impl<'a, I: ?Sized> Compiler<'a, I> {
    fn fresh(&mut self) -> u32 {
        let n = self.next_nt;
        self.next_nt += 1;
        n
    }

    fn syms(&mut self, e: &Expr) -> Vec<Sym<I>> {
        match e {
            Expr::Seq(xs) => xs.iter().flat_map(|x| self.syms(x)).collect(),
            Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Assert(_) => vec![Sym::T((self.term)(e))],
            Expr::Glue(x) => {
                let mut out = vec![Sym::T((self.term)(&Expr::Glue(None)))];
                if let Some(x) = x {
                    out.extend(self.syms(x));
                }
                out
            }
            Expr::Except(a, _) => {
                let mut out = vec![Sym::T((self.term)(e))];
                out.extend(self.syms(a));
                out
            }
            Expr::Ref(name) => vec![Sym::Nt(*self.names.get(name.as_str()).unwrap_or_else(|| panic!("no rule {name}")))],
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

impl<I: ?Sized> Recognizer<I> {
    /// Compiles the grammar, asking `term` for each terminal, special
    /// sequence and assertion, for a bare `~`, and for each exception `a - b`,
    /// which is an assertion that what follows is not `b`, before `a`.
    pub fn new(g: &Grammar, start: &str, term: &dyn Fn(&Expr) -> Term<I>) -> Recognizer<I> {
        let names: LookupMap<&str, u32> =
            g.rules.iter().enumerate().map(|(i, r)| (r.name.as_str(), i as u32)).collect();
        let mut c = Compiler { names, prods: vec![], next_nt: g.rules.len() as u32, term };
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

    /// Whether the whole of `input`, `n` long, is the start rule.
    pub fn accepts(&self, input: &I, n: usize) -> bool {
        let mut sets: Vec<ItemSet> = (0..=n).map(|_| ItemSet::default()).collect();
        for &p in &self.by_lhs[self.start as usize] {
            sets[0].add(Item { prod: p, dot: 0, origin: 0 }, &self.prods);
        }
        let mut lens = vec![];
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
                    Some(Sym::T(term)) => {
                        lens.clear();
                        term(input, i, &mut lens);
                        for &len in &lens {
                            if i + len <= n {
                                sets[i + len].add(advanced, &self.prods);
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

/// A piece of a generated string.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// A terminal's text, or a special sequence's sample.
    Text(String),
    /// Where an assertion was reached, by name, or a `~`, as `"~"`.
    Mark(String),
}

/// Generates random strings of a grammar, as pieces.
///
/// Below `max_depth` rule references, every alternative is equally likely
/// and options and repetitions are taken half the time; past it, each choice
/// is the one that finishes soonest. A special sequence's text comes from
/// `sample`. An exception's `a` is generated whatever `b` is, and an
/// assertion or `~` is left as a mark, since what follows it is not yet
/// written: what the pieces make is a candidate, to be put together by the
/// caller and checked with a [`Recognizer`].
pub struct Generator<'g> {
    grammar: &'g Grammar,
    /// The fewest rule references each rule needs to finish.
    cost: LookupMap<&'g str, u32>,
    sample: &'g dyn Fn(&str, &mut SeededRng) -> String,
    max_depth: u32,
}

const INF: u32 = u32::MAX / 4;

impl<'g> Generator<'g> {
    pub fn new(grammar: &'g Grammar, sample: &'g dyn Fn(&str, &mut SeededRng) -> String) -> Generator<'g> {
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
        Generator { grammar, cost, sample, max_depth: 12 }
    }

    pub fn generate(&self, start: &str, rng: &mut SeededRng) -> Vec<Piece> {
        let start = Expr::Ref(start.S());
        let mut out = vec![];
        self.emit(&start, 0, rng, &mut out);
        out
    }

    fn emit(&self, e: &Expr, depth: u32, rng: &mut SeededRng, out: &mut Vec<Piece>) {
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
                    while rng.random_bool(0.5) && out.len() < 4096 {
                        self.emit(x, depth, rng, out);
                    }
                }
            }
            Expr::Except(a, _) => self.emit(a, depth, rng, out),
            Expr::Glue(x) => {
                out.push(Piece::Mark("~".S()));
                if let Some(x) = x {
                    self.emit(x, depth, rng, out);
                }
            }
            Expr::Lit(s) => out.push(Piece::Text(s.C())),
            Expr::Range(lo, hi) => {
                out.push(Piece::Text(char::from_u32(rng.random_range(*lo as u32..=*hi as u32)).X().S()));
            }
            Expr::Special(text) => out.push(Piece::Text((self.sample)(text, rng))),
            Expr::Assert(name) => out.push(Piece::Mark(name.C())),
            Expr::Ref(name) => {
                let rule = self.grammar.get(name).X();
                self.emit(&rule.body, depth + 1, rng, out);
            }
        }
    }
}

/// Puts the pieces of a grammar over characters together: the text as it
/// is, and at each mark what `repair` returns for it, given the text before
/// and after, to make the assertion hold, often nothing.
pub fn join_chars(pieces: &[Piece], repair: &dyn Fn(&str, &str, &str) -> &'static str) -> String {
    let mut text = String::new();
    let mut marks = vec![];
    for piece in pieces {
        match piece {
            Piece::Text(s) => text.push_str(s),
            Piece::Mark(name) => marks.push((text.len(), name)),
        }
    }
    for &(at, name) in marks.iter().rev() {
        let insert = repair(name, &text[..at], &text[at..]);
        text.insert_str(at, insert);
    }
    text
}

fn expr_cost(e: &Expr, cost: &LookupMap<&str, u32>) -> u32 {
    match e {
        Expr::Alt(xs) => xs.iter().map(|x| expr_cost(x, cost)).min().X(),
        Expr::Seq(xs) => xs.iter().map(|x| expr_cost(x, cost)).fold(0, |a, b| a.saturating_add(b).min(INF)),
        Expr::Opt(_) | Expr::Rep(_) => 0,
        Expr::Except(a, _) => expr_cost(a, cost),
        Expr::Glue(x) => x.as_ref().map_or(0, |x| expr_cost(x, cost)),
        Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Assert(_) => 0,
        Expr::Ref(name) => cost[name.as_str()],
    }
}

/// The terminal for a literal, range or one-character special sequence over
/// characters, with `special` giving each special sequence's test.
pub fn char_term(e: &Expr, special: fn(&str) -> fn(&[char], usize) -> bool) -> Term<[char]> {
    match e {
        Expr::Lit(s) => {
            let cs: Vec<char> = s.chars().collect();
            Rc::new(move |input: &[char], i, out: &mut Vec<usize>| {
                if input.len() >= i + cs.len() && input[i..i + cs.len()] == cs[..] {
                    out.push(cs.len());
                }
            })
        }
        &Expr::Range(lo, hi) => Rc::new(move |input: &[char], i, out: &mut Vec<usize>| {
            if i < input.len() && (lo..=hi).contains(&input[i]) {
                out.push(1);
            }
        }),
        Expr::Special(text) => {
            let test = special(text);
            Rc::new(move |input: &[char], i, out: &mut Vec<usize>| {
                if i < input.len() && test(input, i) {
                    out.push(1);
                }
            })
        }
        Expr::Assert(name) => {
            let test = special(name);
            Rc::new(move |input: &[char], i, out: &mut Vec<usize>| {
                if test(input, i) {
                    out.push(0);
                }
            })
        }
        e => panic!("no terminal over characters for {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn any(_: &[char], _: usize) -> bool {
        true
    }

    fn special(_: &str) -> fn(&[char], usize) -> bool {
        any
    }

    fn term(e: &Expr) -> Term<[char]> {
        char_term(e, special)
    }

    fn accepts(r: &Recognizer<[char]>, s: &str) -> bool {
        let cs: Vec<char> = s.chars().collect();
        r.accepts(&cs, cs.len())
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
        let r = Recognizer::new(&g, "list", &term);
        assert!(accepts(&r, "[]"));
        assert!(accepts(&r, "[ 1, [22 , []] ]"));
        assert!(!accepts(&r, "[1,]"));
        assert!(!accepts(&r, "[1 2]"));
        assert!(!accepts(&r, "[[]"));
    }

    #[test]
    fn fills_in_parameters_and_reads_exceptions() {
        let g = Grammar::parse(
            r#"
            s = wrap("a"), wrap(b), ~, "c" - "d" ;
            wrap(x) = "<", x, ">" ;
            b = "b" ;
            "#,
        );
        assert!(g.get(r#"wrap("a")"#).is_some() && g.get("wrap(b)").is_some());
        let s = &g.get("s").X().body;
        let Expr::Seq(xs) = s else { panic!("{s:?}") };
        assert_eq!(xs[2], Expr::Glue(None));
        assert_eq!(xs[3], Expr::Except(Box::new(Expr::Lit("c".S())), Box::new(Expr::Lit("d".S()))));
    }

    #[test]
    fn generates_what_it_recognizes() {
        let g = Grammar::parse(
            r#"
            e = t, { "+", t } ;
            t = "x" | "(", e, ")" | ? any ? ;
            "#,
        );
        let r = Recognizer::new(&g, "e", &term);
        let sample = |_: &str, _: &mut SeededRng| "y".S();
        let generator = Generator::new(&g, &sample);
        let mut rng = SeededRng::new(1);
        for _ in 0..200 {
            let s = join_chars(&generator.generate("e", &mut rng), &|_, _, _| "");
            assert!(accepts(&r, &s), "{s}");
        }
    }
}
