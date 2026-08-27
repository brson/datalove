//! The same input compiles to the same bytes.
//!
//! The backends walk collections to decide what to declare and emit, and the
//! order they are walked in decides the identifiers Cranelift hands out and the
//! layout of the object file. Several of those collections were `HashMap` and
//! `HashSet`, whose iteration order depends on a seed drawn afresh in every
//! process, so compiling one file four times gave four different objects.
//!
//! This has to run the compiler in separate processes to see it: within one
//! process the seed is fixed, so a hash map iterates the same way every time
//! and the bug is invisible.

use rmx::prelude::*;

use std::path::PathBuf;
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Compile `fixture` in a fresh process and return the object bytes.
fn compile_once(fixture: &str, out: &std::path::Path) -> Vec<u8> {
    let source = manifest_dir().join("tests/fixtures/aot_run").join(fixture);
    let status = Command::new(env!("CARGO_BIN_EXE_datalove"))
        .arg("aot-compile")
        .arg(&source)
        .arg("-o")
        .arg(out)
        .current_dir(manifest_dir().join("../.."))
        .status()
        .expect("failed to run the compiler");
    assert!(status.success(), "compiling {} failed: {:?}", fixture, status);
    std::fs::read(out).expect("no object file written")
}

/// Compiling the same source repeatedly gives byte-identical objects.
#[test]
fn aot_compilation_is_reproducible() {
    let dir = rmx::tempfile::tempdir().X();

    for fixture in ["04_arithmetic.dfs", "01_debuglog_i32.dfs"] {
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for run in 0..3 {
            let out = dir.path().join(format!("{}_{}.o", fixture, run));
            seen.push(compile_once(fixture, &out));
        }

        assert!(
            seen.iter().all(|bytes| bytes == &seen[0]),
            "compiling {} three times gave {} distinct objects of sizes {:?}; \
             something in the backend is walking a hash collection",
            fixture,
            {
                let mut uniq = seen.C();
                uniq.sort();
                uniq.dedup();
                uniq.len()
            },
            seen.iter().map(|b| b.len()).collect::<Vec<_>>(),
        );
    }
}
