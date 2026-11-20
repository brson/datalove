//! Generated functions through the binary, which must never panic.
//!
//! Each seed writes a function over heap values that moves, clones, lends
//! and changes them across nested branches, loops and ways out
//! (`datalove_worldgen::moves_script`), one operand in twenty naming
//! whatever it likes, for the ownership pass to refuse. What is held here is that the binary
//! never panics deciding it. A script the binary accepts it runs, with the
//! runtime's leak check on, on the bytecode, on the IR walker, which asserts
//! that every drop finds something to drop, and under the JIT, whose frees
//! the allocator checks; each run must end cleanly: no panic, no leak, no
//! error. What a script logs, which is every value `take` is given and what
//! the function returns, goes to stderr, and the IR walker and the JIT must
//! log what the bytecode logged, so that an engine that moves or changes
//! the wrong value is caught even when it ends cleanly.
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
use bct::random::env_seeds;
use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one script may take, checked and run.
const LIMIT: Duration = Duration::from_secs(30);

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
            Some((true, again)) if !again.contains("panicked at") && !again.contains("LEAK") => {
                if again != stderr {
                    return Outcome::Failed(fmt!(
                        "on {engine}, a log unlike the bytecode's:\n{again}\nthe bytecode's:\n{stderr}"
                    ));
                }
            }
            Some((_, again)) => return Outcome::Failed(fmt!("on {engine}:\n{again}")),
        }
    }
    Outcome::Ran
}

#[test]
fn ownership_never_panics() {
    let seeds = env_seeds("OWNERSHIP_FUZZ", 0, 2000);
    let results: Vec<(u64, String, Outcome)> = seeds.iter().collect::<Vec<_>>().par_iter()
        .map(|&seed| {
            let text = datalove_worldgen::moves_script(seed, 0.05);
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
