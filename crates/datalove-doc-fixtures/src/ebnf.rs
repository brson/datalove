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
use std::ops::Range;
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

    /// The terminals `e` matches, through the rules it names: the words an
    /// exception's `b` excepts. A rule this grammar lacks names nothing.
    pub fn words(&self, e: &Expr) -> BTreeSet<String> {
        let mut words = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut stack = vec![e.C()];
        while let Some(e) = stack.pop() {
            e.visit(&mut |x| match x {
                Expr::Lit(s) => {
                    words.insert(s.C());
                }
                Expr::Ref(r) => {
                    if seen.insert(r.C()) && let Some(rule) = self.get(r) {
                        stack.push(rule.body.C());
                    }
                }
                _ => {}
            });
        }
        words
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
    /// The expression as EBNF.
    pub fn show(&self) -> String {
        // Inside a sequence or an exception, an alternative needs brackets.
        let tight = |x: &Expr| match x {
            Expr::Alt(_) | Expr::Seq(_) => fmt!("( {} )", x.show()),
            x => x.show(),
        };
        match self {
            Expr::Alt(xs) => xs.iter().map(Expr::show).collect::<Vec<_>>().join(" | "),
            Expr::Seq(xs) => xs
                .iter()
                .map(|x| if matches!(x, Expr::Alt(_)) { tight(x) } else { x.show() })
                .collect::<Vec<_>>()
                .join(", "),
            Expr::Opt(x) => fmt!("[ {} ]", x.show()),
            Expr::Rep(x) => fmt!("{{ {} }}", x.show()),
            Expr::Except(a, b) => fmt!("{} - {}", tight(a), tight(b)),
            Expr::Glue(None) => "~".S(),
            Expr::Glue(Some(x)) => fmt!("~{}", tight(x)),
            Expr::Lit(t) => {
                let t = t.replace('\n', "\\n");
                if t.contains('"') { fmt!("'{t}'") } else { fmt!("\"{t}\"") }
            }
            Expr::Range(lo, hi) => fmt!("\"{lo}\" .. \"{hi}\""),
            Expr::Special(t) => fmt!("? {t} ?"),
            Expr::Ref(name) | Expr::Assert(name) => name.C(),
        }
    }

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
            // A `?` quoted in the text, as in `? no "?" after it ?`, does
            // not end it.
            let mut quote = None;
            let end = i + 1 + cs[i + 1..]
                .iter()
                .position(|&d| {
                    match quote {
                        Some(q) if d == q => quote = None,
                        None if d == '"' || d == '\'' => quote = Some(d),
                        _ => {}
                    }
                    quote.is_none() && d == '?'
                })
                .expect("unclosed special sequence");
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
    /// The grammar's rule names, by nonterminal.
    names: Vec<String>,
    /// What each nonterminal past the rules stands for.
    helpers: Vec<Helper>,
}

/// What a nonterminal the compiler made stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Helper {
    Alt,
    Opt,
    /// `n = | n, x`: its derivations are the iterations of `x`.
    Rep,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Item {
    prod: u32,
    dot: u32,
    origin: u32,
}

/// A set of small numbers, nonterminals here.
#[derive(Default)]
struct Bits(Vec<u64>);

impl Bits {
    /// Adds `n`, saying whether it was new.
    fn insert(&mut self, n: u32) -> bool {
        let (w, b) = ((n / 64) as usize, n % 64);
        if self.0.len() <= w {
            self.0.resize(w + 1, 0);
        }
        let new = self.0[w] & (1 << b) == 0;
        self.0[w] |= 1 << b;
        new
    }

    fn contains(&self, n: u32) -> bool {
        self.0.get((n / 64) as usize).is_some_and(|w| w & (1 << (n % 64)) != 0)
    }
}

#[derive(Default)]
struct ItemSet {
    items: Vec<Item>,
    seen: LookupSet<Item>,
    /// Items whose next symbol is a nonterminal, with it.
    waiting: Vec<(u32, Item)>,
    /// Nonterminals that have matched the empty string here.
    empty: Bits,
    /// Nonterminals predicted here, each once.
    predicted: Bits,
    /// Whether anything was predicted here: the input up to here is a prefix
    /// of some string, even where no item is kept.
    reached: bool,
}

impl ItemSet {
    fn add<I: ?Sized>(&mut self, item: Item, prods: &[(u32, Vec<Sym<I>>)]) {
        if self.seen.insert(item) {
            self.push(item, prods);
        }
    }

    /// Keeps an item known to be new here.
    fn push<I: ?Sized>(&mut self, item: Item, prods: &[(u32, Vec<Sym<I>>)]) {
        self.items.push(item);
        if let Some(Sym::Nt(n)) = prods[item.prod as usize].1.get(item.dot as usize) {
            self.waiting.push((*n, item));
        }
    }

    /// The items waiting here on nonterminal `n`.
    fn waiters(&self, n: u32) -> Vec<Item> {
        self.waiting.iter().filter(|(m, _)| *m == n).map(|(_, item)| *item).collect()
    }
}

struct Compiler<'a, I: ?Sized> {
    names: LookupMap<&'a str, u32>,
    prods: Vec<(u32, Vec<Sym<I>>)>,
    next_nt: u32,
    helpers: Vec<Helper>,
    term: &'a dyn Fn(&Expr) -> Term<I>,
}

impl<'a, I: ?Sized> Compiler<'a, I> {
    fn fresh(&mut self, helper: Helper) -> u32 {
        let n = self.next_nt;
        self.next_nt += 1;
        self.helpers.push(helper);
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
                let n = self.fresh(Helper::Alt);
                for x in xs {
                    let rhs = self.syms(x);
                    self.prods.push((n, rhs));
                }
                vec![Sym::Nt(n)]
            }
            Expr::Opt(x) => {
                let n = self.fresh(Helper::Opt);
                self.prods.push((n, vec![]));
                let rhs = self.syms(x);
                self.prods.push((n, rhs));
                vec![Sym::Nt(n)]
            }
            Expr::Rep(x) => {
                let n = self.fresh(Helper::Rep);
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
        let mut c = Compiler { names, prods: vec![], next_nt: g.rules.len() as u32, helpers: vec![], term };
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
        Recognizer { prods: c.prods, by_lhs, start, names: g.rules.iter().map(|r| r.name.C()).collect(), helpers: c.helpers }
    }

    /// Whether the whole of `input`, `n` long, is the start rule.
    pub fn accepts(&self, input: &I, n: usize) -> bool {
        self.prefixes(input, n).last() == Some(&n)
    }

    /// The lengths of the prefixes of `input`, `n` long, that are the start
    /// rule, shortest first.
    pub fn prefixes(&self, input: &I, n: usize) -> Vec<usize> {
        let sets = self.sets(input, n);
        (0..sets.len())
            .filter(|&i| {
                sets[i].items.iter().any(|item| {
                    let (lhs, rhs) = &self.prods[item.prod as usize];
                    *lhs == self.start && item.origin == 0 && item.dot as usize == rhs.len()
                })
            })
            .collect()
    }

    /// The longest prefix of `input`, `n` long, that is the start rule, and
    /// the rules that match the whole of it, the start rule among them.
    pub fn longest(&self, input: &I, n: usize) -> Option<(usize, BTreeSet<&str>)> {
        let sets = self.sets(input, n);
        let complete = |i: usize| {
            sets[i].items.iter().filter(|item| item.origin == 0 && item.dot as usize == self.prods[item.prod as usize].1.len())
        };
        let k = (0..sets.len()).rev().find(|&i| complete(i).any(|item| self.prods[item.prod as usize].0 == self.start))?;
        let names = complete(k).filter_map(|item| self.names.get(self.prods[item.prod as usize].0 as usize)).map(|s| s.as_str()).collect();
        Some((k, names))
    }

    /// How much of `input`, `n` long, begins some string of the start rule:
    /// the furthest position the recognizer reaches. Where it stops is the
    /// first place any parser of the grammar could see an error, `n` if none.
    pub fn viable_prefix(&self, input: &I, n: usize) -> usize {
        let sets = self.sets(input, n);
        (0..sets.len()).rev().find(|&i| sets[i].reached || !sets[i].items.is_empty()).X()
    }

    /// The item sets, up to the furthest any item reaches: past it nothing
    /// can be matched, so a long input with a short match costs little.
    ///
    /// Each nonterminal is predicted once in a set, so the items it predicts
    /// are new there without asking, and one beginning with a terminal is
    /// scanned at once rather than kept: those two were most of the cost.
    fn sets(&self, input: &I, n: usize) -> Vec<ItemSet> {
        let mut sets: Vec<ItemSet> = vec![ItemSet::default()];
        let mut lens = vec![];
        self.predict(&mut sets, 0, self.start, input, n, &mut lens);
        let mut i = 0;
        while i < sets.len() {
            let mut k = 0;
            while k < sets[i].items.len() {
                let item = sets[i].items[k];
                k += 1;
                let (lhs, rhs) = &self.prods[item.prod as usize];
                match rhs.get(item.dot as usize) {
                    None => {
                        let origin = item.origin as usize;
                        if origin == i {
                            sets[i].empty.insert(*lhs);
                        }
                        let waiters = sets[origin].waiters(*lhs);
                        for w in waiters {
                            sets[i].add(Item { dot: w.dot + 1, ..w }, &self.prods);
                        }
                    }
                    Some(Sym::Nt(m)) => {
                        self.predict(&mut sets, i, *m, input, n, &mut lens);
                        if sets[i].empty.contains(*m) {
                            sets[i].add(Item { dot: item.dot + 1, ..item }, &self.prods);
                        }
                    }
                    Some(Sym::T(term)) => self.scan(&mut sets, i, item, term, input, n, &mut lens),
                }
            }
            i += 1;
        }
        sets
    }

    fn predict(&self, sets: &mut Vec<ItemSet>, i: usize, m: u32, input: &I, n: usize, lens: &mut Vec<usize>) {
        sets[i].reached = true;
        if !sets[i].predicted.insert(m) {
            return;
        }
        for &p in &self.by_lhs[m as usize] {
            let item = Item { prod: p, dot: 0, origin: i as u32 };
            match self.prods[p as usize].1.first() {
                Some(Sym::T(term)) => self.scan(sets, i, item, term, input, n, lens),
                _ => sets[i].push(item, &self.prods),
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scan(&self, sets: &mut Vec<ItemSet>, i: usize, item: Item, term: &Term<I>, input: &I, n: usize, lens: &mut Vec<usize>) {
        lens.clear();
        term(input, i, lens);
        for k in 0..lens.len() {
            let to = i + lens[k];
            if to <= n {
                while sets.len() <= to {
                    sets.push(ItemSet::default());
                }
                sets[to].add(Item { dot: item.dot + 1, ..item }, &self.prods);
            }
        }
    }
}

// ============================================================================
// Parse trees
// ============================================================================

/// What a node of a parse tree is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// A rule of the grammar, by name.
    Rule(String),
    /// A terminal, special sequence or assertion.
    Term,
    /// An alternative or an option written inside a rule.
    Group,
    /// A repetition: its children are its iterations, in order.
    Rep,
    /// One iteration of a repetition.
    Iter,
}

/// A parse tree, its spans in input positions.
#[derive(Clone, Debug)]
pub struct Tree {
    pub node: Node,
    pub span: Range<usize>,
    pub children: Vec<Tree>,
}

/// What parsing an input found.
#[derive(Debug)]
pub enum Parse {
    /// The input is not the start rule.
    Rejected,
    /// The input is the start rule in more ways than one: the rule nearest
    /// the top that derives a span in two ways, and the span.
    Ambiguous(String, Range<usize>),
    Tree(Tree),
}

impl<I: ?Sized> Recognizer<I> {
    /// The parse tree of the whole of `input`, `n` long, if it has one and
    /// only one.
    ///
    /// The tree is read back out of the recognizer's item sets: what each
    /// nonterminal derives over each span is counted, to two, and where the
    /// count is one, the one derivation is the tree.
    pub fn parse(&self, input: &I, n: usize) -> Parse {
        let sets = self.sets(input, n);
        if sets.len() <= n {
            return Parse::Rejected;
        }
        let mut b = TreeBuilder { rec: self, sets: &sets, input, nts: LookupMap::new(), prefixes: LookupMap::new() };
        match b.nt_count(self.start, 0, n) {
            0 => Parse::Rejected,
            1 => Parse::Tree(b.build_nt(self.start, 0, n)),
            _ => b.ambiguity(self.start, &self.names[self.start as usize], 0, n),
        }
    }

    fn rule_name(&self, m: u32) -> Option<&str> {
        self.names.get(m as usize).map(|s| s.as_str())
    }
}

/// Counts and builds the derivations an item set records.
struct TreeBuilder<'r, I: ?Sized> {
    rec: &'r Recognizer<I>,
    sets: &'r [ItemSet],
    input: &'r I,
    /// How many ways a nonterminal derives a span, two meaning two or more,
    /// and [`COUNTING`] while it is being counted.
    nts: LookupMap<(u32, usize, usize), u8>,
    /// How many ways the first symbols of a production derive a span.
    prefixes: LookupMap<(u32, u32, usize, usize), u8>,
}

const COUNTING: u8 = u8::MAX;

impl<I: ?Sized> TreeBuilder<'_, I> {
    /// Whether the first `dot` symbols of production `p` were matched from
    /// `i` to `j`, as far as the item sets say.
    fn has(&self, p: u32, dot: u32, i: usize, j: usize) -> bool {
        if dot == 0 {
            return i == j;
        }
        j < self.sets.len() && self.sets[j].seen.contains(&Item { prod: p, dot, origin: i as u32 })
    }

    fn nt_count(&mut self, m: u32, i: usize, j: usize) -> u8 {
        if let Some(&c) = self.nts.get(&(m, i, j)) {
            assert!(c != COUNTING, "nonterminal {m} derives itself over {i}..{j}");
            return c;
        }
        self.nts.insert((m, i, j), COUNTING);
        let mut c = 0;
        for &p in &self.rec.by_lhs[m as usize] {
            let len = self.rec.prods[p as usize].1.len() as u32;
            if self.has(p, len, i, j) {
                c = (c + self.prefix_count(p, len, i, j)).min(2);
            }
        }
        self.nts.insert((m, i, j), c);
        c
    }

    /// How many ways symbol `dot - 1` of production `p` derives `t..j`.
    fn sym_count(&mut self, p: u32, dot: u32, t: usize, j: usize) -> u8 {
        match &self.rec.prods[p as usize].1[dot as usize - 1] {
            Sym::T(term) => {
                let mut lens = vec![];
                term(self.input, t, &mut lens);
                u8::from(lens.contains(&(j - t)))
            }
            Sym::Nt(m) => {
                let m = *m;
                self.nt_count(m, t, j)
            }
        }
    }

    /// The places the last of the first `dot` symbols of `p` may begin, in
    /// a derivation of `i..j`, with how many ways each.
    fn splits(&mut self, p: u32, dot: u32, i: usize, j: usize) -> Vec<(usize, u8, u8)> {
        let mut out = vec![];
        for t in i..=j {
            if !self.has(p, dot - 1, i, t) {
                continue;
            }
            let right = self.sym_count(p, dot, t, j);
            if right == 0 {
                continue;
            }
            let left = if dot == 1 { 1 } else { self.prefix_count(p, dot - 1, i, t) };
            if left > 0 {
                out.push((t, left, right));
            }
        }
        out
    }

    fn prefix_count(&mut self, p: u32, dot: u32, i: usize, j: usize) -> u8 {
        if dot == 0 {
            return u8::from(i == j);
        }
        if let Some(&c) = self.prefixes.get(&(p, dot, i, j)) {
            return c;
        }
        let c = self.splits(p, dot, i, j).iter().fold(0u8, |c, (_, l, r)| (c + l * r).min(2));
        self.prefixes.insert((p, dot, i, j), c);
        c
    }

    /// The productions of `m` that derive `i..j`, with how many ways each.
    fn live(&mut self, m: u32, i: usize, j: usize) -> Vec<(u32, u8)> {
        let mut out = vec![];
        for &p in &self.rec.by_lhs[m as usize] {
            let len = self.rec.prods[p as usize].1.len() as u32;
            if self.has(p, len, i, j) {
                let c = self.prefix_count(p, len, i, j);
                if c > 0 {
                    out.push((p, c));
                }
            }
        }
        out
    }

    fn build_nt(&mut self, m: u32, i: usize, j: usize) -> Tree {
        let [(p, 1)] = self.live(m, i, j)[..] else { bug!("built where there is not one derivation") };
        let mut children = vec![];
        self.build_prefix(p, self.rec.prods[p as usize].1.len() as u32, i, j, &mut children);
        let node = match self.rec.rule_name(m) {
            Some(name) => Node::Rule(name.S()),
            None => match self.rec.helpers[m as usize - self.rec.names.len()] {
                Helper::Alt | Helper::Opt => Node::Group,
                Helper::Rep => Node::Rep,
            },
        };
        if node == Node::Rep && !children.is_empty() {
            // `n = n, x`: the iterations so far, then this one.
            let before = children.remove(0);
            let at = before.span.end;
            let mut iterations = before.children;
            iterations.push(Tree { node: Node::Iter, span: at..j, children });
            children = iterations;
        }
        Tree { node, span: i..j, children }
    }

    fn build_prefix(&mut self, p: u32, dot: u32, i: usize, j: usize, out: &mut Vec<Tree>) {
        if dot == 0 {
            return;
        }
        let [(t, 1, 1)] = self.splits(p, dot, i, j)[..] else { bug!("built where there is not one derivation") };
        self.build_prefix(p, dot - 1, i, t, out);
        out.push(match &self.rec.prods[p as usize].1[dot as usize - 1] {
            Sym::T(_) => Tree { node: Node::Term, span: t..j, children: vec![] },
            Sym::Nt(m) => {
                let m = *m;
                self.build_nt(m, t, j)
            }
        });
    }

    /// Where a derivation of `m` over `i..j`, known to be ambiguous, is so:
    /// `rule` is the rule it is in.
    fn ambiguity(&mut self, m: u32, rule: &str, i: usize, j: usize) -> Parse {
        let rule = self.rec.rule_name(m).unwrap_or(rule).S();
        let live = self.live(m, i, j);
        let [(p, _)] = live[..] else { return Parse::Ambiguous(rule, i..j) };
        let (mut dot, mut j2) = (self.rec.prods[p as usize].1.len() as u32, j);
        while dot > 0 {
            let splits = self.splits(p, dot, i, j2);
            let [(t, left, right)] = splits[..] else { return Parse::Ambiguous(rule, i..j) };
            if right > 1 {
                let Sym::Nt(m2) = self.rec.prods[p as usize].1[dot as usize - 1] else { bug!("a terminal matches once") };
                return self.ambiguity(m2, &rule, t, j2);
            }
            assert!(left > 1, "an ambiguous derivation has an ambiguous part");
            dot -= 1;
            j2 = t;
        }
        bug!("an ambiguous derivation has an ambiguous part")
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
/// Half the time an alternative is drawn evenly and half the time it is the
/// one taken least so far, and options and repetitions are taken half the
/// time, or always where they never have been, until a rule has been written inside itself
/// `max_depth` times on the way to where it is; past that, each choice is the
/// one that finishes soonest. Only recursion counts: the chain of rules from
/// `expr` down to `primary` is long, and counting each of them left nothing
/// deep in an expression ever written. A special sequence's text comes from
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
    points: Points<'g>,
    rules: LookupMap<&'g str, &'g Rule>,
    /// How many times each point has been gone through, over every string
    /// generated so far.
    hits: std::cell::RefCell<Vec<u32>>,
}

/// The places in a grammar a generated string can go: each rule, each
/// alternative of a `|`, and each option or repetition taken.
struct Points<'g> {
    all: Vec<Point>,
    rules: LookupMap<&'g str, u32>,
    /// By the address of the alternative's expression.
    branches: LookupMap<usize, u32>,
    /// By the address of the option or repetition.
    taken: LookupMap<usize, u32>,
    /// The points beneath each alternative, option and repetition, its own
    /// among them, through the rules it uses: by its address.
    beneath: LookupMap<usize, Vec<u32>>,
}

/// A place in a grammar a generated string can go.
#[derive(Clone, Debug)]
pub struct Point {
    pub rule: String,
    /// What it is, as EBNF.
    pub what: String,
}

fn address(e: &Expr) -> usize {
    std::ptr::from_ref(e) as usize
}

impl<'g> Points<'g> {
    fn new(g: &'g Grammar) -> Points<'g> {
        let mut p = Points {
            all: vec![],
            rules: LookupMap::new(),
            branches: LookupMap::new(),
            taken: LookupMap::new(),
            beneath: LookupMap::new(),
        };
        for rule in &g.rules {
            let n = p.add(&rule.name, fmt!("{} is reached", rule.name));
            p.rules.insert(&rule.name, n);
            p.visit(&rule.name, &rule.body);
        }
        // What each rule reaches, itself included, then what lies beneath each
        // choice.
        let mut reach: LookupMap<&str, BTreeSet<u32>> = LookupMap::new();
        for rule in &g.rules {
            let mut seen = BTreeSet::new();
            let mut points = BTreeSet::new();
            let mut todo = vec![rule.name.as_str()];
            while let Some(name) = todo.pop() {
                if !seen.insert(name) {
                    continue;
                }
                let r = g.get(name).X();
                points.insert(p.rules[name]);
                p.inside(&r.body, &mut points);
                r.body.visit(&mut |e| {
                    if let Expr::Ref(n) = e {
                        todo.push(g.get(n).X().name.as_str());
                    }
                });
            }
            reach.insert(&rule.name, points);
        }
        for rule in &g.rules {
            rule.body.visit(&mut |e| {
                let choices: Vec<&Expr> = match e {
                    Expr::Alt(xs) => xs.iter().collect(),
                    Expr::Opt(_) | Expr::Rep(_) => vec![e],
                    _ => vec![],
                };
                for x in choices {
                    let mut points = BTreeSet::new();
                    points.extend(p.branches.get(&address(x)));
                    p.inside(x, &mut points);
                    x.visit(&mut |y| {
                        if let Expr::Ref(n) = y {
                            points.extend(&reach[n.as_str()]);
                        }
                    });
                    p.beneath.insert(address(x), points.into_iter().collect());
                }
            });
        }
        p
    }

    /// The points written inside `e`, as far as its rule goes.
    fn inside(&self, e: &Expr, out: &mut BTreeSet<u32>) {
        e.visit(&mut |x| {
            out.extend(self.taken.get(&address(x)));
            if let Expr::Alt(xs) = x {
                for y in xs {
                    out.extend(self.branches.get(&address(y)));
                }
            }
        });
    }

    fn add(&mut self, rule: &str, what: String) -> u32 {
        self.all.push(Point { rule: rule.S(), what });
        u32::try_from(self.all.len() - 1).X()
    }

    fn visit(&mut self, rule: &str, e: &Expr) {
        match e {
            Expr::Alt(xs) => {
                for x in xs {
                    let n = self.add(rule, fmt!("{rule}: the alternative {}", x.show()));
                    self.branches.insert(address(x), n);
                    self.visit(rule, x);
                }
            }
            Expr::Seq(xs) => xs.iter().for_each(|x| self.visit(rule, x)),
            Expr::Opt(x) | Expr::Rep(x) => {
                let n = self.add(rule, fmt!("{rule}: {} taken", e.show()));
                self.taken.insert(address(e), n);
                self.visit(rule, x);
            }
            Expr::Except(a, _) | Expr::Glue(Some(a)) => self.visit(rule, a),
            Expr::Glue(None) | Expr::Lit(_) | Expr::Range(..) | Expr::Special(_) | Expr::Ref(_) | Expr::Assert(_) => {}
        }
    }
}

/// How much of a grammar the strings generated from some start rules have
/// gone through, counted over the points those rules reach.
pub struct Coverage {
    points: Vec<Point>,
    /// Whether each point counts: whether the start rules reach its rule.
    counted: Vec<bool>,
    hits: Vec<u32>,
}

impl Coverage {
    pub fn new(generator: &Generator<'_>, starts: &[&str]) -> Coverage {
        let reached = generator.grammar.reachable(starts);
        let points = generator.points.all.C();
        let counted = points.iter().map(|p| reached.contains(&p.rule)).collect();
        let hits = vec![0; points.len()];
        Coverage { points, counted, hits }
    }

    /// Counts the points a generated string went through.
    pub fn add(&mut self, trace: &[u32]) {
        for &n in trace {
            self.hits[n as usize] += 1;
        }
    }

    /// The points no string went through.
    pub fn missed(&self) -> Vec<&Point> {
        (0..self.points.len()).filter(|&n| self.counted[n] && self.hits[n] == 0).map(|n| &self.points[n]).collect()
    }

    /// How many of the points counted were gone through.
    pub fn summary(&self) -> String {
        let total = self.counted.iter().filter(|c| **c).count();
        fmt!("{} of {total} points of the grammar reached", total - self.missed().len())
    }

    /// Fails, listing them, if any points were missed.
    pub fn assert_full(&self, what: &str) {
        let missed: Vec<String> = self.missed().iter().map(|p| p.what.C()).collect();
        assert!(
            missed.is_empty(),
            "{what}: {}; never generated:\n  {}",
            self.summary(),
            missed.join("\n  ")
        );
    }
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
        let rules = grammar.rules.iter().map(|r| (r.name.as_str(), r)).collect();
        let points = Points::new(grammar);
        let hits = std::cell::RefCell::new(vec![0; points.all.len()]);
        Generator { grammar, cost, sample, max_depth: 3, points, rules, hits }
    }

    /// The same generator, choosing freely only below `max_depth` levels of
    /// recursion.
    pub fn max_depth(self, max_depth: u32) -> Generator<'g> {
        Generator { max_depth, ..self }
    }

    pub fn generate(&self, start: &str, rng: &mut SeededRng) -> Vec<Piece> {
        self.generate_traced(start, rng).0
    }

    /// A string generated from `start`, and the points of the grammar it
    /// went through, for a [`Coverage`].
    pub fn generate_traced(&self, start: &str, rng: &mut SeededRng) -> (Vec<Piece>, Vec<u32>) {
        let start = Expr::Ref(start.S());
        let mut out = vec![];
        let mut trace = vec![];
        self.emit(&start, 0, rng, &mut out, &mut trace, &mut vec![]);
        (out, trace)
    }

    fn pass(&self, point: Option<&u32>, trace: &mut Vec<u32>) {
        if let Some(&n) = point {
            self.hits.borrow_mut()[n as usize] += 1;
            trace.push(n);
        }
    }

    /// The fewest times any point beneath a choice has been gone through.
    fn least_beneath(&self, e: &Expr) -> u32 {
        let hits = self.hits.borrow();
        self.points.beneath[&address(e)].iter().map(|&n| hits[n as usize]).min().unwrap_or(u32::MAX)
    }

    /// Whether to take an option or repetition: always the first time where
    /// something beneath it never has been, else half the time.
    fn take(&self, e: &Expr, first: bool, rng: &mut SeededRng) -> bool {
        (first && self.least_beneath(e) == 0) || rng.random_bool(0.5)
    }

    fn emit(&self, e: &Expr, depth: u32, rng: &mut SeededRng, out: &mut Vec<Piece>, trace: &mut Vec<u32>, stack: &mut Vec<&'g str>) {
        let deep = depth > self.max_depth;
        match e {
            Expr::Alt(xs) => {
                let x = if deep {
                    xs.iter().min_by_key(|x| expr_cost(x, &self.cost)).X()
                } else if rng.random_bool(0.5) {
                    // The alternative with the least gone through beneath
                    // it, so that what is rare, there or deep under it, is
                    // written as often as what is common.
                    let scores: Vec<u32> = xs.iter().map(|x| self.least_beneath(x)).collect();
                    let least = *scores.iter().min().X();
                    let rare: Vec<&Expr> = xs.iter().zip(&scores).filter(|(_, s)| **s == least).map(|(x, _)| x).collect();
                    rare[rng.random_range(0..rare.len())]
                } else {
                    &xs[rng.random_range(0..xs.len())]
                };
                self.pass(self.points.branches.get(&address(x)), trace);
                self.emit(x, depth, rng, out, trace, stack);
            }
            Expr::Seq(xs) => xs.iter().for_each(|x| self.emit(x, depth, rng, out, trace, stack)),
            Expr::Opt(x) => {
                if !deep && self.take(e, true, rng) {
                    self.pass(self.points.taken.get(&address(e)), trace);
                    self.emit(x, depth, rng, out, trace, stack);
                }
            }
            Expr::Rep(x) => {
                if !deep {
                    let mut first = true;
                    while self.take(e, first, rng) && out.len() < 4096 {
                        first = false;
                        self.pass(self.points.taken.get(&address(e)), trace);
                        self.emit(x, depth, rng, out, trace, stack);
                    }
                }
            }
            Expr::Except(a, b) => {
                // Draw `a` again while it is one of the words `b` names.
                let words = self.grammar.words(b);
                let (mark, traced) = (out.len(), trace.len());
                for _ in 0..20 {
                    out.truncate(mark);
                    trace.truncate(traced);
                    self.emit(a, depth, rng, out, trace, stack);
                    let text: String = out[mark..]
                        .iter()
                        .filter_map(|p| if let Piece::Text(t) = p { Some(t.as_str()) } else { None })
                        .collect();
                    if !words.contains(&text) {
                        break;
                    }
                }
            }
            Expr::Glue(x) => {
                out.push(Piece::Mark("~".S()));
                if let Some(x) = x {
                    self.emit(x, depth, rng, out, trace, stack);
                }
            }
            Expr::Lit(s) => out.push(Piece::Text(s.C())),
            Expr::Range(lo, hi) => {
                out.push(Piece::Text(char::from_u32(rng.random_range(*lo as u32..=*hi as u32)).X().S()));
            }
            Expr::Special(text) => out.push(Piece::Text((self.sample)(text, rng))),
            Expr::Assert(name) => out.push(Piece::Mark(name.C())),
            Expr::Ref(name) => {
                let rule = *self.rules.get(name.as_str()).X();
                self.pass(self.points.rules.get(rule.name.as_str()), trace);
                // Only recursion is deep: a rule already being written is
                // written inside itself again.
                let depth = depth + u32::from(stack.contains(&rule.name.as_str()));
                stack.push(&rule.name);
                self.emit(&rule.body, depth, rng, out, trace, stack);
                stack.pop();
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
        let cs: Vec<char> = "[1, 2 3]".chars().collect();
        assert_eq!(r.viable_prefix(&cs, cs.len()), 6);
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
