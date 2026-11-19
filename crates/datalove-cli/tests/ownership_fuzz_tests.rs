//! Generated functions through the binary, which must never panic.
//!
//! Each seed writes a function over heap values -- strings, a list it
//! borrows and one it changes -- that moves, clones, lends and changes them
//! across nested `if`s, `for`s over either list, counted `loop`s and an
//! option's binding, leaves by `ret`, `break` and `continue` at any depth,
//! and sometimes writes on past a way out, where nothing runs. Most are
//! refused, for a value used after it moved among other things: that is
//! the ownership pass's to decide, and what is held here is only that
//! deciding it does not panic. A script the binary accepts it runs, twice
//! over the function, with the runtime's leak check on, and the run must
//! end cleanly: no panic, no leak, no error.
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
fun take(x: string)
  debuglog x
end fun
fun look(ref x: string): int
  ret 1
end fun
fun change(mut x: string)
  set x = \"changed\"
end fun
fun maybe(c: bool): ?string
  if c
    ret some \"m\"
  end if
  ret none
end fun
";

/// What a block may name, and what of it the generator takes to be moved.
#[derive(Clone)]
struct Scope {
    /// The owned values in scope.
    owned: Vec<String>,
    /// Those of `owned` a move may take: not one declared outside the
    /// innermost loop, whose move the loop's next round would repeat.
    movable: Vec<String>,
    /// Those of `owned` moved on the way here.
    moved: BTreeSet<String>,
    /// Borrowed values in scope, the elements a `for` walks.
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

    /// Take what a nested block moved of this scope's values, where the
    /// block went on to its end: `None` where it left by a way out.
    fn join(&mut self, after: Option<BTreeSet<String>>) {
        if let Some(moved) = after {
            self.moved.extend(moved.into_iter().filter(|n| self.owned.contains(n)));
        }
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

    /// A string to read, `consumed` if where it goes takes it: a name in
    /// scope, moved where the generator lets a move be, cloned or borrowed
    /// otherwise, or a literal. One in twenty is any name, bare, whatever it
    /// holds, for the ownership pass to refuse.
    fn operand(&mut self, scope: &mut Scope, consumed: bool) -> String {
        if self.rng.random_bool(0.05) {
            let all: Vec<String> = scope.owned.iter().chain(&scope.borrowed).cloned().collect();
            return self.pick(&all).X();
        }
        let held: Vec<String> = scope.owned.iter()
            .filter(|n| !scope.moved.contains(*n))
            .chain(&scope.borrowed)
            .cloned()
            .collect();
        let Some(name) = self.pick(&held).filter(|_| !self.rng.random_bool(0.2)) else {
            return "\"lit\"".S();
        };
        if !consumed {
            return name;
        }
        if scope.movable.contains(&name) && !scope.moved.contains(&name) && self.rng.random_bool(0.3) {
            scope.moved.insert(name.C());
            return name;
        }
        fmt!("{name}@")
    }

    /// A variable to set, which then holds a value again.
    fn target(&mut self, scope: &mut Scope) -> &'static str {
        let t = ["a", "b"][self.rng.random_range(0..2)];
        scope.moved.remove(t);
        t
    }

    /// Statements, and what the block moved where it goes on to its end, or
    /// `None` where it leaves by a way out.
    fn block(&mut self, scope: &Scope, out: &mut String) -> Option<BTreeSet<String>> {
        let mut scope = scope.C();
        let indent = "  ".repeat(scope.depth + 1);
        let nested = scope.depth < 4;
        for _ in 0..self.rng.random_range(1..=4) {
            let line = match self.rng.random_range(0..if nested { 22 } else { 14 }) {
                0 | 1 => fmt!("debuglog {}", self.operand(&mut scope, false)),
                2 | 3 => {
                    let v = self.fresh("v");
                    let line = fmt!("let {v} = {}", self.operand(&mut scope, true));
                    scope.owned.push(v.C());
                    scope.movable.push(v);
                    line
                }
                4 | 5 => {
                    let value = self.operand(&mut scope, true);
                    fmt!("set {} = {value}", self.target(&mut scope))
                }
                6 => fmt!("call take({})", self.operand(&mut scope, true)),
                7 => {
                    let n = self.fresh("n");
                    fmt!("let {n} = look(ref {})", self.operand(&mut scope, false))
                }
                8 => fmt!("call change(mut {})", self.target(&mut scope)),
                9 => fmt!("set m = [{}]", self.operand(&mut scope, true)),
                10 => "debuglog m".S(),
                11..=13 => {
                    let exit = match self.rng.random_range(0..if scope.in_loop { 3 } else { 1 }) {
                        0 => fmt!("ret {}", self.operand(&mut scope, true)),
                        1 => "break".S(),
                        _ => "continue".S(),
                    };
                    out.push_str(&fmt!("{indent}{exit}\n"));
                    // Sometimes code nothing reaches, which the ownership
                    // pass still walks.
                    if self.rng.random_bool(0.3) {
                        self.block(&scope, out);
                    }
                    return None;
                }
                14 | 15 => {
                    out.push_str(&fmt!("{indent}if c\n"));
                    let then = self.block(&scope.nested(false), out);
                    let otherwise = match self.rng.random_bool(0.6) {
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
                16 => {
                    let v = self.fresh("o");
                    out.push_str(&fmt!("{indent}if maybe(c) |{v}|\n"));
                    let mut then = scope.nested(false);
                    then.owned.push(v.C());
                    then.movable.push(v);
                    let then = self.block(&then, out);
                    out.push_str(&fmt!("{indent}else\n"));
                    let otherwise = self.block(&scope.nested(false), out);
                    out.push_str(&fmt!("{indent}end if\n"));
                    scope.join(then);
                    scope.join(otherwise);
                    continue;
                }
                17..=19 => {
                    let w = self.fresh("w");
                    let list = if self.rng.random_bool(0.7) { "l" } else { "m" };
                    out.push_str(&fmt!("{indent}for {w} in ref {list}\n"));
                    let mut body = scope.nested(true);
                    body.borrowed.push(w);
                    let moved = self.block(&body, out);
                    out.push_str(&fmt!("{indent}end for\n"));
                    scope.join(moved);
                    continue;
                }
                _ => {
                    // The count goes up first, so that a `continue` does not
                    // skip it.
                    let k = self.fresh("k");
                    out.push_str(&fmt!("{indent}var {k}: int = 0\n{indent}loop\n"));
                    out.push_str(&fmt!("{indent}  set {k} = {k} + 1\n{indent}  if {k} == 3\n{indent}    break\n{indent}  end if\n"));
                    let moved = self.block(&scope.nested(true), out);
                    out.push_str(&fmt!("{indent}end loop\n"));
                    scope.join(moved);
                    continue;
                }
            };
            out.push_str(&fmt!("{indent}{line}\n"));
        }
        Some(scope.moved)
    }
}

/// A script for a seed: the prelude, the function, and two calls of it.
fn script(seed: u64) -> String {
    let mut g = Gen { rng: SeededRng::derive(seed, "ownership-fuzz"), fresh: 0 };
    let names = vec!["a".S(), "b".S(), "s".S()];
    let scope = Scope {
        owned: names.C(),
        movable: names,
        moved: BTreeSet::new(),
        borrowed: Vec::new(),
        in_loop: false,
        depth: 0,
    };
    let mut body = String::new();
    let end = match g.block(&scope, &mut body) {
        Some(moved) if moved.contains("a") => "  ret \"end\"\n",
        _ => "  ret a\n",
    };
    fmt!("{PRELUDE}\
fun go(ref l: [string], mut m: [string], s: string, c: bool): string
  var a: string = \"a\"
  var b: string = \"b\"
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

fn check(text: &str) -> Outcome {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("fuzz.dfs");
    std::fs::write(&path, text).X();
    let mut child = Command::new(env!("CARGO_BIN_EXE_datalove"))
        .arg("script")
        .arg(&path)
        .env("DATALOVE_LEAK_CHECK", "panic")
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
            return Outcome::Failed("did not end".S());
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().X(), &mut stderr).X();
    let ansi = rmx::regex::Regex::new("\x1b\\[[0-9;]*m").X();
    let stderr = ansi.replace_all(&stderr, "").into_owned();
    if stderr.contains("panicked at") || stderr.contains("LEAK") {
        return Outcome::Failed(stderr);
    }
    let code = rmx::regex::Regex::new(r"(?m)^\[([A-Z][0-9]+)\] Error:").X();
    if let Some(c) = code.captures(&stderr) {
        return Outcome::Refused(c[1].S(), stderr.C());
    }
    match status.success() {
        true => Outcome::Ran,
        false => Outcome::Failed(stderr),
    }
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
