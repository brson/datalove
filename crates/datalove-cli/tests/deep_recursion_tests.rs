//! Recursion deeper than the Rust stack allows, on the bytecode.
//!
//! The bytecode makes a call from one bytecode body to another without
//! recursing on the Rust stack: the callee's frame goes on the interpreter's
//! frame stack and the loop carries on in it. So recursion that stays in
//! bytecode goes as deep as the frame stack allows, far past the few thousand
//! levels the Rust stack gives the IR walker, and running out of frame stack
//! is an error rather than an abort.
//!
//! Run in a separate process, so that the engine is chosen for it alone and an
//! abort, were it one, would fail the test rather than end the suite.

use rmx::prelude::*;

use std::path::PathBuf;
use std::process::{Command, Output};

/// Run a script that recurses `depth` deep, on the bytecode.
fn recurse(depth: u32) -> Output {
    let dir = rmx::tempfile::tempdir().X();
    let script = dir.path().join("deep.dfs");
    std::fs::write(&script, format!("\
fun down(n: u32): u32
  if n == 0
    ret 0
  end if
  ret icall add_wrapping_u32(down(icall sub_wrapping_u32(n, : u32 / 1)), : u32 / 1)
end fun
debuglog(down({depth}))
")).X();
    Command::new(env!("CARGO_BIN_EXE_datalove"))
        .arg("script")
        .arg(&script)
        .env("DATALOVE_INTERP", "bc")
        .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .expect("failed to run datalove")
}

/// Two hundred thousand levels, where the Rust stack gave out at about nine
/// thousand.
#[test]
fn bytecode_recursion_is_not_bounded_by_the_rust_stack() {
    let output = recurse(200_000);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the script failed: {}", stderr);
    assert_eq!(stderr.trim(), "200000");
}

/// Ten million levels runs out of frame stack, which is an error the process
/// reports and exits from, not a signal.
#[test]
fn running_out_of_frame_stack_is_an_error() {
    let output = recurse(10_000_000);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.code().is_some(), "killed by a signal: {:?}\n{}", output.status, stderr);
    assert!(!output.status.success(), "ten million levels fit: {}", stderr);
    assert!(stderr.contains("StackOverflow"), "not a stack overflow: {}", stderr);
}
