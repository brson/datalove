//! Generated functions through the binary, which must never panic.
//!
//! Each seed writes a function over heap values -- strings, a struct of
//! two, an enum with a string payload, a list it borrows and one it changes
//! -- that moves, clones, lends and changes them, and their fields, across
//! nested `if`s, `match`es, `for`s over either list, counted `loop`s and an
//! option's binding. It declares variables with no value for an `out`
//! argument or a `set` to give one, leaves by `ret`, `?`, `break` and
//! `continue` at any depth, and sometimes writes on past a way out, where
//! nothing runs. The generator tracks what it moved and what holds nothing,
//! so that most scripts run; one operand in twenty names whatever it likes,
//! for the ownership pass to refuse. What is held here is that the binary
//! never panics deciding it. A script the binary accepts it runs, with the
//! runtime's leak check on, on the bytecode, on the IR walker, which asserts
//! that every drop finds something to drop, and under the JIT, whose frees
//! the allocator checks; each run must end cleanly: no panic, no leak, no
//! error.
//!
//! The shapes come from a panic in the ownership pass that a dead `break`
//! after a `ret` in a loop under a `ref` `for` caused (backend fixture
//! `49_dead_break_after_return`), which the generators that hold the Rust
//! to the formal model met only by chance.
//!
//! Environment: `OWNERSHIP_FUZZ_SEED`, the first seed, or `random`;
//! `OWNERSHIP_FUZZ_COUNT`, how many; `OWNERSHIP_FUZZ_SHOW`, a code, to
//! print the first script refused with it; and `OWNERSHIP_FUZZ_WRITE`, a
//! directory to write each failing script to.

use rmx::prelude::*;
use rmx::rayon::prelude::*;
use bct::random::{SeededRng, env_seeds};
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one script may take, checked and run.
const LIMIT: Duration = Duration::from_secs(30);

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

    /// One in twenty operands names any value of its kind, bare, whatever
    /// it holds, for the ownership pass to refuse.
    fn chaos(&mut self, scope: &Scope, kind: Kind) -> Option<String> {
        if !self.chance(0.05) {
            return None;
        }
        let mut all: Vec<String> = scope.owned.iter().filter(|(_, k)| *k == kind).map(|(n, _)| n.C()).collect();
        if kind == Kind::Str {
            all.extend(scope.borrowed.iter().cloned());
        }
        self.pick(&all)
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
        if let Some(any) = self.chaos(scope, Kind::Str) {
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
        if let Some(any) = self.chaos(scope, Kind::Struct) {
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
        if let Some(any) = self.chaos(scope, Kind::Enum) {
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

    /// A string variable or field to lend `mut`: one that holds a value.
    fn lent(&mut self, scope: &mut Scope) -> Option<String> {
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
                17 => {
                    let g = self.fresh("g");
                    scope.declare(&g, Kind::Str, false, true);
                    fmt!("let {g} = maybe(c)?")
                }
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

/// A script for a seed: the prelude, the function, and two calls of it.
fn script(seed: u64) -> String {
    let mut g = Gen { rng: SeededRng::derive(seed, "ownership-fuzz"), fresh: 0 };
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
    let end = match g.block(&scope, &mut body) {
        Some(empty) if empty.contains("a") => "  ret none\n",
        _ => "  ret some a\n",
    };
    fmt!("{PRELUDE}\
fun go(ref l: [string], mut m: [string], s: string, c: bool): ?string
  var a: string = \"a\"
  var b: string = \"b\"
  var p: P = {{x = \"px\", y = \"py\"}}
  var e: E = mk(c)
{body}{end}end fun
let l: [string] = [\"x\", \"y\"]
var m: [string] = [\"z\"]
debuglog go(ref l, mut m, \"s\", true)
debuglog go(ref l, mut m, \"t\", false)
debuglog m
")
}

/// What the binary made of a script.
enum Outcome {
    /// Refused, with the first diagnostic's code, and what the binary said.
    Refused(String, String),
    /// Accepted and run to the end.
    Ran,
    /// Anything else: a panic, a leak, a run that failed, or one that never
    /// ended.
    Failed(String),
}

/// How an accepted script is run again, each way holding its drops to a
/// check of its own. Every engine refuses to destroy what the runtime
/// destroyed already, which the leak check leaves poisoned; the IR walker
/// also asserts that what a drop drops holds something, which sees a second
/// drop of a value whose destruction went another way, and compiled code
/// frees what it frees, where the allocator's record sees a second free.
const AGAIN: &[(&str, &[&str], &[(&str, &str)])] = &[
    ("the IR walker", &[], &[("DATALOVE_INTERP", "ir")]),
    ("the JIT", &["--jit", "--jit-threshold", "0"], &[]),
];

/// Run the binary on a script, with the leak check on: what it wrote to
/// stderr, or `None` if it did not end in time.
fn run(path: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> Option<(bool, String)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_datalove"))
        .arg("script")
        .args(args)
        .arg(path)
        .env("DATALOVE_LEAK_CHECK", "panic")
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn().X();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().X() {
            break status;
        }
        if start.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().X(), &mut stderr).X();
    let ansi = rmx::regex::Regex::new("\x1b\\[[0-9;]*m").X();
    Some((status.success(), ansi.replace_all(&stderr, "").into_owned()))
}

fn check(text: &str) -> Outcome {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("fuzz.dfs");
    std::fs::write(&path, text).X();
    let Some((success, stderr)) = run(&path, &[], &[]) else {
        return Outcome::Failed("did not end".S());
    };
    if stderr.contains("panicked at") || stderr.contains("LEAK") {
        return Outcome::Failed(stderr);
    }
    let code = rmx::regex::Regex::new(r"(?m)^\[([A-Z][0-9]+)\] Error:").X();
    if let Some(c) = code.captures(&stderr) {
        return Outcome::Refused(c[1].S(), stderr.C());
    }
    if !success {
        return Outcome::Failed(stderr);
    }
    for (engine, args, env) in AGAIN {
        match run(&path, args, env) {
            None => return Outcome::Failed(fmt!("on {engine}, did not end")),
            Some((true, stderr)) if !stderr.contains("panicked at") && !stderr.contains("LEAK") => {}
            Some((_, stderr)) => return Outcome::Failed(fmt!("on {engine}:\n{stderr}")),
        }
    }
    Outcome::Ran
}

#[test]
fn ownership_never_panics() {
    let seeds = env_seeds("OWNERSHIP_FUZZ", 0, 2000);
    let results: Vec<(u64, String, Outcome)> = seeds.iter().collect::<Vec<_>>().par_iter()
        .map(|&seed| {
            let text = script(seed);
            let outcome = check(&text);
            (seed, text, outcome)
        })
        .collect();
    let mut outcomes = BTreeMap::<String, usize>::new();
    let mut failures = Vec::new();
    for (seed, text, outcome) in &results {
        match outcome {
            Outcome::Refused(code, _) => *outcomes.entry(fmt!("refused {code}")).or_default() += 1,
            Outcome::Ran => *outcomes.entry("ran".S()).or_default() += 1,
            Outcome::Failed(why) => {
                if let Ok(dir) = std::env::var("OWNERSHIP_FUZZ_WRITE") {
                    std::fs::write(std::path::Path::new(&dir).join(fmt!("seed_{seed}.dfs")), text).X();
                }
                failures.push(fmt!("seed {seed}:\n{text}\n{why}"));
            }
        }
    }
    println!(
        "{} scripts over seeds {}..{}: {} failed; {outcomes:?}",
        results.len(), seeds.start, seeds.start.wrapping_add(seeds.count), failures.len(),
    );
    if let Ok(show) = std::env::var("OWNERSHIP_FUZZ_SHOW") {
        let refused = results.iter().find_map(|(seed, text, o)| match o {
            Outcome::Refused(c, why) if *c == show => Some((seed, text, why)),
            _ => None,
        });
        if let Some((seed, text, why)) = refused {
            println!("seed {seed}, refused {show}:\n{text}\n{why}");
        }
    }
    for f in failures.iter().take(5) {
        println!("{f}");
    }
    assert!(failures.is_empty(), "the binary panicked, leaked or failed on {} scripts", failures.len());
}
