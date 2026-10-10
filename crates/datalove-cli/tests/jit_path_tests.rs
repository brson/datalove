//! That the JIT's fast paths are the ones taken, counted rather than timed.
//!
//! A call from the bytecode into compiled code is made from a planned call
//! site, and a loop in a function called once is entered by OSR. If either
//! quietly falls back to the slow path -- every call offered to the dispatcher,
//! a loop left to the interpreter -- the program still gives the right answer
//! and only gets slower, which no other test would notice. These read
//! `--jit-stats` and fail instead.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jit_paths/calls.dfs")
}

/// Run `script --no-sys` with `args` before the fixture, giving stderr.
fn run(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_datalove"))
        .arg("script")
        .arg("--no-sys")
        .args(args)
        .arg(fixture())
        .output()
        .expect("the binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "exited with {}:\n{stderr}", output.status);
    stderr
}

/// The numbers in the line of `--jit-stats` starting with `prefix`, in order.
fn numbers(stats: &str, prefix: &str) -> Vec<u64> {
    let line = stats.lines().find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no `{prefix}` line in:\n{stats}"));
    line[prefix.len()..]
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("digits"))
        .collect()
}

/// What the program prints, the same under the JIT as interpreted.
fn check_output(stats: &str) {
    let want = run(&[]);
    let got: String = stats.lines()
        .filter(|l| !l.starts_with("jit:") && !l.starts_with("time:") && !l.trim().is_empty()
            && !l.starts_with(' ') && !l.starts_with("interpreted"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(got.trim(), want.trim(), "the JIT's output differs from the interpreter's");
}

/// With OSR out of the way, `step` is called 5000 times from an interpreted
/// loop: past the threshold of 100, every call is entered from a planned
/// call site.
#[test]
fn calls_into_compiled_code_are_planned() {
    let stats = run(&["--jit", "--jit-osr-threshold", "4000000000", "--jit-stats"]);
    check_output(&stats);
    let [interpreted, entered, planned, _exited] =
        numbers(&stats, "jit: calls through the dispatcher:")[..] else {
        panic!("unexpected calls line in:\n{stats}")
    };
    assert!(entered >= 4800, "{entered} calls entered compiled code, wanted most of 5000:\n{stats}");
    assert!(planned + 10 >= entered,
        "{planned} of {entered} entries were planned; the rest went through the dispatcher:\n{stats}");
    assert!(interpreted <= 200, "{interpreted} calls interpreted past the threshold:\n{stats}");
}

/// Under the defaults, `drive`'s loop runs 5000 times in its one call, which
/// is past the OSR threshold of 1000: it is entered in compiled code.
#[test]
fn a_hot_loop_is_entered_by_osr() {
    let stats = run(&["--jit", "--jit-stats"]);
    check_output(&stats);
    let [compiled, _refused, entered] = numbers(&stats, "jit: loops:")[..] else {
        panic!("unexpected loops line in:\n{stats}")
    };
    assert!(compiled >= 1 && entered >= 1, "no loop was entered by OSR:\n{stats}");
}

/// `triangle`'s inner loop needs what its outer loop defines, so its header is
/// refused and the outer one entered instead, which a low threshold reaches.
#[test]
fn an_inner_loop_refused_is_entered_from_outside() {
    let stats = run(&["--jit", "--jit-osr-threshold", "50", "--jit-stats"]);
    check_output(&stats);
    let [compiled, refused, entered] = numbers(&stats, "jit: loops:")[..] else {
        panic!("unexpected loops line in:\n{stats}")
    };
    assert!(refused >= 1, "the inner loop was not refused:\n{stats}");
    assert!(compiled >= 2 && entered >= 2, "the outer loop was not entered:\n{stats}");
}
