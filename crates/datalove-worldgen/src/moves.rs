//! Functions that move, clone, lend and change heap values.
//!
//! Each seed writes a function over strings, a struct of two, an enum with a
//! string payload, a list it borrows and one it changes, and a map it sets
//! keys of, that moves, clones, lends and changes them and their fields
//! across nested `if`s, `match`es, `for`s over either list, counted `loop`s
//! and an option's binding. It
//! declares variables with no value for an `out` argument or a `set` to give
//! one, leaves by `ret`, `?`, `break` and `continue` at any depth, and
//! sometimes writes on past a way out, where nothing runs.
//!
//! The generator tracks what it moved and what holds nothing, so that a
//! script mostly does what the ownership pass lets through. At a chaos rate
//! an operand names any value of its kind instead, bare, for the pass to
//! refuse. Half the time a way out is preceded by a value given away, since
//! what a way out drops is decided from what was moved on the way to it.
//!
//! `ownership_fuzz_tests` in `datalove-cli` runs these through the binary,
//! and `moves_agree` in `datalove-formal-tests` holds the ownership pass to
//! the formal model's move analysis on them.

use rmx::prelude::*;
use bct::random::SeededRng;
use std::collections::BTreeSet;

const PRELUDE: &str = "\
type E = enum { atom N, term T string }
type P = {x: string, y: string}
fun take(x: string)
  debuglog x
end fun
fun look(ref x: string): int
  ret 1
end fun
fun change(mut x: string)
  set x = \"changed\"
end fun
fun fill(out x: string)
  set x = \"filled\"
end fun
fun both(x: string, y: string): string
  ret x
end fun
fun maybe(c: bool): ?string
  if c
    ret some \"m\"
  end if
  ret none
end fun
fun mk(c: bool): E
  if c
    ret term T \"t\"
  end if
  ret atom N
end fun
";

/// A kind of value the generator names.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Str,
    Struct,
    Enum,
}

/// What a block may name, and what of it the generator takes to hold
/// nothing.
#[derive(Clone)]
struct Scope {
    /// The owned values in scope, each with its kind.
    owned: Vec<(String, Kind)>,
    /// Those of `owned` that are variables, which `set` and an `out`
    /// argument may give a value.
    vars: Vec<String>,
    /// Those of `owned` a move may take: not one declared outside the
    /// innermost loop, whose move the loop's next round would repeat.
    movable: Vec<String>,
    /// Those of `owned` that hold nothing here: moved, or declared with no
    /// value and not given one yet.
    empty: BTreeSet<String>,
    /// Borrowed strings in scope, the elements a `for` walks.
    borrowed: Vec<String>,
    in_loop: bool,
    depth: usize,
}

impl Scope {
    /// Enter a nested block, `in_loop` if it is a loop's body.
    fn nested(&self, in_loop: bool) -> Scope {
        let mut inner = self.C();
        inner.depth += 1;
        if in_loop {
            inner.in_loop = true;
            inner.movable.clear();
        }
        inner
    }

    /// Take what a nested block left empty of this scope's values, where the
    /// block went on to its end: `None` where it left by a way out.
    fn join(&mut self, after: Option<BTreeSet<String>>) {
        if let Some(empty) = after {
            let owned: Vec<String> = self.owned.iter().map(|(n, _)| n.C()).collect();
            self.empty.extend(empty.into_iter().filter(|n| owned.contains(n)));
        }
    }

    /// Declare an owned value, holding one or not.
    fn declare(&mut self, name: &str, kind: Kind, var: bool, holds: bool) {
        self.owned.push((name.S(), kind));
        self.movable.push(name.S());
        if var {
            self.vars.push(name.S());
        }
        if !holds {
            self.empty.insert(name.S());
        }
    }

    /// The owned values of a kind that hold one.
    fn held(&self, kind: Kind) -> Vec<String> {
        self.owned.iter()
            .filter(|(n, k)| *k == kind && !self.empty.contains(n))
            .map(|(n, _)| n.C())
            .collect()
    }

    /// The variables of a kind.
    fn vars_of(&self, kind: Kind) -> Vec<String> {
        self.owned.iter()
            .filter(|(n, k)| *k == kind && self.vars.contains(n))
            .map(|(n, _)| n.C())
            .collect()
    }
}

struct Gen {
    rng: SeededRng,
    fresh: usize,
    /// How often an operand names any value of its kind, whatever it holds.
    chaos_rate: f64,
}

impl Gen {
    fn fresh(&mut self, prefix: &str) -> String {
        self.fresh += 1;
        fmt!("{prefix}{}", self.fresh)
    }

    fn pick(&mut self, names: &[String]) -> Option<String> {
        (!names.is_empty()).then(|| names[self.rng.random_range(0..names.len())].C())
    }

    fn chance(&mut self, p: f64) -> bool {
        self.rng.random_bool(p)
    }

    /// An operand, at the chaos rate, naming any value of its kind, bare,
    /// whatever it holds, for the ownership pass to refuse.
    ///
    /// Where the operand is `consumed`, naming an owned value bare moves it,
    /// which the pass lets through if it held one, so it holds nothing after.
    fn chaos(&mut self, scope: &mut Scope, kind: Kind, consumed: bool) -> Option<String> {
        if !self.chance(self.chaos_rate) {
            return None;
        }
        let mut all: Vec<String> = scope.owned.iter().filter(|(_, k)| *k == kind).map(|(n, _)| n.C()).collect();
        if kind == Kind::Str {
            all.extend(scope.borrowed.iter().cloned());
        }
        let name = self.pick(&all)?;
        if consumed && scope.owned.iter().any(|(n, _)| *n == name) {
            scope.empty.insert(name.C());
        }
        Some(name)
    }

    /// A value of `kind` held in scope, moved if `consumed` and a move may
    /// take it, cloned if consumed otherwise, and named as it is if not.
    fn named(&mut self, scope: &mut Scope, kind: Kind, consumed: bool) -> Option<String> {
        let mut held = scope.held(kind);
        if kind == Kind::Str {
            held.extend(scope.borrowed.iter().cloned());
        }
        let name = self.pick(&held)?;
        if !consumed {
            return Some(name);
        }
        if scope.movable.contains(&name) && !scope.empty.contains(&name) && self.chance(0.3) {
            scope.empty.insert(name.C());
            return Some(name);
        }
        Some(fmt!("{name}@"))
    }

    /// A string to read, `consumed` if where it goes takes it: a name in
    /// scope, a field of a struct that holds one, or a literal.
    fn string(&mut self, scope: &mut Scope, consumed: bool) -> String {
        if let Some(any) = self.chaos(scope, Kind::Str, consumed) {
            return any;
        }
        if self.chance(0.15) {
            if let Some(p) = self.pick(&scope.held(Kind::Struct)) {
                let field = ["x", "y"][self.rng.random_range(0..2)];
                return if consumed { fmt!("{p}.{field}@") } else { fmt!("{p}.{field}") };
            }
        }
        if self.chance(0.2) {
            return "\"lit\"".S();
        }
        self.named(scope, Kind::Str, consumed).unwrap_or_else(|| "\"lit\"".S())
    }

    /// A struct to read, as `string` reads a string, or a literal.
    fn structure(&mut self, scope: &mut Scope, consumed: bool) -> String {
        if let Some(any) = self.chaos(scope, Kind::Struct, consumed) {
            return any;
        }
        if !self.chance(0.3) {
            if let Some(name) = self.named(scope, Kind::Struct, consumed) {
                return name;
            }
        }
        let x = self.string(scope, true);
        let y = self.string(scope, true);
        fmt!("{{x = {x}, y = {y}}}")
    }

    /// An enum to match on or to keep: a fresh one, or a variable's, cloned.
    fn enumeration(&mut self, scope: &mut Scope, consumed: bool) -> String {
        if let Some(any) = self.chaos(scope, Kind::Enum, consumed) {
            return any;
        }
        match self.chance(0.5) {
            true => self.named(scope, Kind::Enum, consumed).unwrap_or_else(|| "mk(c)".S()),
            false => "mk(c)".S(),
        }
    }

    /// A string variable to give a value, which then holds one.
    fn target(&mut self, scope: &mut Scope) -> String {
        let t = self.pick(&scope.vars_of(Kind::Str)).X();
        scope.empty.remove(&t);
        t
    }

    /// A string variable or field to lend `mut`: one that holds a value, or
    /// at the chaos rate any string named, which may not be changed, or a
    /// literal, which is not a place.
    fn lent(&mut self, scope: &mut Scope) -> Option<String> {
        if let Some(any) = self.chaos(scope, Kind::Str, false) {
            return Some(if self.chance(0.2) { "\"lit\"".S() } else { any });
        }
        if self.chance(0.3) {
            let vars: Vec<String> = scope.held(Kind::Struct).into_iter().filter(|n| scope.vars.contains(n)).collect();
            if let Some(p) = self.pick(&vars) {
                return Some(fmt!("{p}.{}", ["x", "y"][self.rng.random_range(0..2)]));
            }
        }
        let held: Vec<String> = scope.held(Kind::Str).into_iter().filter(|n| scope.vars.contains(n)).collect();
        self.pick(&held)
    }

    /// Statements, and what the block left empty where it goes on to its
    /// end, or `None` where it leaves by a way out.
    ///
    /// Where every arm of a branch leaves, so does the block, and nothing
    /// follows the branch: the ownership pass walks what would follow with
    /// what the arms moved on their way out, which the generator does not
    /// track.
    fn block(&mut self, scope: &Scope, out: &mut String) -> Option<BTreeSet<String>> {
        let mut scope = scope.C();
        let indent = "  ".repeat(scope.depth + 1);
        let nested = scope.depth < 4;
        for _ in 0..self.rng.random_range(1..=4) {
            let line = match self.rng.random_range(0..if nested { 30 } else { 21 }) {
                0 | 1 => fmt!("debuglog {}", self.string(&mut scope, false)),
                2 | 3 => {
                    let v = self.fresh("v");
                    let line = fmt!("let {v} = {}", self.string(&mut scope, true));
                    scope.declare(&v, Kind::Str, false, true);
                    line
                }
                4 | 5 => {
                    let value = self.string(&mut scope, true);
                    fmt!("set {} = {value}", self.target(&mut scope))
                }
                6 => fmt!("call take({})", self.string(&mut scope, true)),
                7 => {
                    let n = self.fresh("n");
                    fmt!("let {n} = look(ref {})", self.string(&mut scope, false))
                }
                8 => match self.lent(&mut scope) {
                    Some(place) => fmt!("call change(mut {place})"),
                    None => continue,
                },
                9 => fmt!("set m = [{}]", self.string(&mut scope, true)),
                10 => "debuglog m".S(),
                11 => fmt!("debuglog {}", self.structure(&mut scope, false)),
                12 => {
                    let v = self.fresh("p");
                    let line = fmt!("let {v} = {}", self.structure(&mut scope, true));
                    scope.declare(&v, Kind::Struct, false, true);
                    line
                }
                13 => {
                    let value = self.structure(&mut scope, true);
                    let t = self.pick(&scope.vars_of(Kind::Struct)).X();
                    scope.empty.remove(&t);
                    fmt!("set {t} = {value}")
                }
                14 => match self.pick(&scope.held(Kind::Struct).into_iter().filter(|n| scope.vars.contains(n)).collect::<Vec<_>>()) {
                    Some(p) => {
                        let field = ["x", "y"][self.rng.random_range(0..2)];
                        match self.rng.random_range(0..2) {
                            0 => fmt!("set {p}.{field} = {}", self.string(&mut scope, true)),
                            _ => fmt!("call fill(out {p}.{field})"),
                        }
                    }
                    None => continue,
                },
                15 => {
                    // A variable that holds nothing until something gives
                    // it a value.
                    let o = self.fresh("u");
                    scope.declare(&o, Kind::Str, true, false);
                    fmt!("var {o}: string")
                }
                16 => fmt!("call fill(out {})", self.target(&mut scope)),
                // A `?`, sometimes after a value given away to the same call,
                // which a return from the `?` still owns, sometimes in a
                // `debuglog`, which only borrows what it logs, and sometimes
                // in a `set` of a map: in a key after the key's call took a
                // value, or in the value before a bare key is taken into the
                // map.
                17 => match self.rng.random_range(0..5) {
                    0 => {
                        let g = self.fresh("g");
                        scope.declare(&g, Kind::Str, false, true);
                        fmt!("let {g} = maybe(c)?")
                    }
                    1 => {
                        let first = self.string(&mut scope, true);
                        let g = self.fresh("g");
                        scope.declare(&g, Kind::Str, false, true);
                        fmt!("let {g} = both({first}, maybe(c)?)")
                    }
                    2 => fmt!("debuglog both({}, maybe(c)?)", self.string(&mut scope, true)),
                    3 => {
                        let first = self.string(&mut scope, true);
                        fmt!("set mp[both({first}, maybe(c)?)] = {}", self.string(&mut scope, true))
                    }
                    _ => {
                        let key = self.string(&mut scope, true);
                        fmt!("set mp[{key}] = both({}, maybe(c)?)", self.string(&mut scope, true))
                    }
                },
                18 => {
                    let value = self.enumeration(&mut scope, true);
                    let t = self.pick(&scope.vars_of(Kind::Enum)).X();
                    scope.empty.remove(&t);
                    fmt!("set {t} = {value}")
                }
                19 | 20 => {
                    // What a way out drops is decided from what was given
                    // away on the way to it, so half the time something is,
                    // just before.
                    let movable: Vec<String> = scope.held(Kind::Str).into_iter()
                        .filter(|n| scope.movable.contains(n))
                        .collect();
                    if self.chance(0.5) && let Some(v) = self.pick(&movable) {
                        scope.empty.insert(v.C());
                        let give = match self.rng.random_range(0..3) {
                            0 => fmt!("call take({v})"),
                            1 => {
                                let t = self.target(&mut scope);
                                fmt!("set {t} = {v}")
                            }
                            _ => fmt!("let {} = {v}", self.fresh("v")),
                        };
                        out.push_str(&fmt!("{indent}{give}\n"));
                    }
                    let exit = match self.rng.random_range(0..if scope.in_loop { 4 } else { 2 }) {
                        0 => fmt!("ret some {}", self.string(&mut scope, true)),
                        1 => "ret none".S(),
                        2 => "break".S(),
                        _ => "continue".S(),
                    };
                    out.push_str(&fmt!("{indent}{exit}\n"));
                    // Sometimes code nothing reaches, which the ownership
                    // pass still walks.
                    if self.chance(0.3) {
                        self.block(&scope, out);
                    }
                    return None;
                }
                21 | 22 => {
                    out.push_str(&fmt!("{indent}if c\n"));
                    let then = self.block(&scope.nested(false), out);
                    let otherwise = match self.chance(0.6) {
                        true => {
                            out.push_str(&fmt!("{indent}else\n"));
                            self.block(&scope.nested(false), out)
                        }
                        false => Some(BTreeSet::new()),
                    };
                    out.push_str(&fmt!("{indent}end if\n"));
                    if then.is_none() && otherwise.is_none() {
                        return None;
                    }
                    scope.join(then);
                    scope.join(otherwise);
                    continue;
                }
                23 => {
                    let v = self.fresh("o");
                    out.push_str(&fmt!("{indent}if maybe(c) |{v}|\n"));
                    let mut then = scope.nested(false);
                    then.declare(&v, Kind::Str, false, true);
                    let then = self.block(&then, out);
                    out.push_str(&fmt!("{indent}else\n"));
                    let otherwise = self.block(&scope.nested(false), out);
                    out.push_str(&fmt!("{indent}end if\n"));
                    if then.is_none() && otherwise.is_none() {
                        return None;
                    }
                    scope.join(then);
                    scope.join(otherwise);
                    continue;
                }
                24 | 25 => {
                    let scrutinee = self.enumeration(&mut scope, true);
                    out.push_str(&fmt!("{indent}match {scrutinee}\n"));
                    let t = self.fresh("t");
                    let mut term = scope.nested(false);
                    term.declare(&t, Kind::Str, false, true);
                    // The atom's arm first or last, and sometimes as the
                    // default.
                    let default = self.chance(0.3);
                    let atom_first = !default && self.chance(0.5);
                    let mut arms = Vec::new();
                    if atom_first {
                        out.push_str(&fmt!("{indent}case atom N\n"));
                        arms.push(self.block(&scope.nested(false), out));
                    }
                    out.push_str(&fmt!("{indent}case term T {t}\n"));
                    arms.push(self.block(&term, out));
                    if !atom_first {
                        out.push_str(&fmt!("{indent}case {}\n", if default { "default" } else { "atom N" }));
                        arms.push(self.block(&scope.nested(false), out));
                    }
                    out.push_str(&fmt!("{indent}end match\n"));
                    if arms.iter().all(Option::is_none) {
                        return None;
                    }
                    for arm in arms {
                        scope.join(arm);
                    }
                    continue;
                }
                26 | 27 => {
                    let w = self.fresh("w");
                    let list = if self.chance(0.7) { "l" } else { "m" };
                    out.push_str(&fmt!("{indent}for {w} in ref {list}\n"));
                    let mut body = scope.nested(true);
                    body.borrowed.push(w);
                    let left = self.block(&body, out);
                    out.push_str(&fmt!("{indent}end for\n"));
                    scope.join(left);
                    continue;
                }
                _ => {
                    // The count goes up first, so that a `continue` does not
                    // skip it.
                    let k = self.fresh("k");
                    out.push_str(&fmt!("{indent}var {k}: int = 0\n{indent}loop\n"));
                    out.push_str(&fmt!("{indent}  set {k} = {k} + 1\n{indent}  if {k} == 3\n{indent}    break\n{indent}  end if\n"));
                    let left = self.block(&scope.nested(true), out);
                    out.push_str(&fmt!("{indent}end loop\n"));
                    scope.join(left);
                    continue;
                }
            };
            out.push_str(&fmt!("{indent}{line}\n"));
        }
        Some(scope.empty)
    }
}

/// A script for a seed: the prelude, the function, and two calls of it, one
/// operand in `1 / chaos` naming whatever it likes.
///
/// Where the body goes on to its end, the function logs each of `b`, `s`,
/// `p` and `e` that still holds a value before it returns, so that what the
/// body left in them reaches the log the engines are held to.
pub fn moves_script(seed: u64, chaos: f64) -> String {
    let mut g = Gen { rng: SeededRng::derive(seed, "ownership-fuzz"), fresh: 0, chaos_rate: chaos };
    let mut scope = Scope {
        owned: Vec::new(),
        vars: Vec::new(),
        movable: Vec::new(),
        empty: BTreeSet::new(),
        borrowed: Vec::new(),
        in_loop: false,
        depth: 0,
    };
    scope.declare("a", Kind::Str, true, true);
    scope.declare("b", Kind::Str, true, true);
    scope.declare("s", Kind::Str, false, true);
    scope.declare("p", Kind::Struct, true, true);
    scope.declare("e", Kind::Enum, true, true);
    let mut body = String::new();
    let after = g.block(&scope, &mut body);
    let mut end = String::new();
    if let Some(empty) = &after {
        for name in ["b", "s", "p", "e"].into_iter().filter(|n| !empty.contains(*n)) {
            end.push_str(&fmt!("  debuglog {name}\n"));
        }
    }
    end.push_str(match &after {
        Some(empty) if empty.contains("a") => "  ret none\n",
        _ => "  ret some a\n",
    });
    fmt!("{PRELUDE}\
fun go(ref l: [string], mut m: [string], s: string, c: bool): ?string
  var a: string = \"a\"
  var b: string = \"b\"
  var p: P = {{x = \"px\", y = \"py\"}}
  var e: E = mk(c)
  var mp: %{{string -> string}} = %{{\"k\" -> \"v\"}}
{body}  debuglog mp
{end}end fun
let l: [string] = [\"x\", \"y\"]
var m: [string] = [\"z\"]
debuglog go(ref l, mut m, \"s\", true)
debuglog go(ref l, mut m, \"t\", false)
debuglog m
")
}

