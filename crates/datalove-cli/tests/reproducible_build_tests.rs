//! The same input compiles to the same bytes.
//!
//! The backends walk collections to decide what to declare and emit, and the
//! order they are walked in decides the identifiers Cranelift hands out and the
//! layout of the object file. Several of those collections were `HashMap` and
//! `HashSet`, whose iteration order depends on a seed drawn afresh in every
//! process, so compiling one file four times gave four different objects.
//!
//! The hash containers are `LookupMap` and `LookupSet` now, which make a walk
//! say whether it is unordered or sorted, but an unordered walk can still be
//! mislabeled. FxHash, which they use, has no seed of its own, so every run of
//! a build would walk the same way and hide it. So each run here is given its
//! own seed through `BCTS_LOOKUP_SEED`, in its own process, since the seed is
//! read once per process.

use rmx::prelude::*;
use rmx::rayon::prelude::*;

use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every fixture an AOT test compiles: the `aot_run` programs, and the
/// `backend` ones, which between them reach far more of the compiler.
fn fixtures() -> Vec<PathBuf> {
    let mut fixtures: Vec<PathBuf> = ["aot_run", "backend"].iter()
        .flat_map(|dir| std::fs::read_dir(manifest_dir().join("tests/fixtures").join(dir)).X())
        .map(|entry| entry.X().path())
        .filter(|path| path.extension().is_some_and(|e| e == "dfs"))
        .collect();
    fixtures.sort();
    fixtures
}

/// Compile `source` in a fresh process, hashing with `seed`, and return the
/// object bytes.
fn compile_once(source: &Path, seed: u32, out: &Path) -> Vec<u8> {
    let status = Command::new(env!("CARGO_BIN_EXE_datalove"))
        .env(bct::lookup::SEED_VAR, seed.S())
        .arg("aot-compile")
        .arg(source)
        .arg("-o")
        .arg(out)
        .current_dir(manifest_dir().join("../.."))
        .stdout(std::process::Stdio::null())
        .status()
        .expect("failed to run the compiler");
    assert!(status.success(), "compiling {} failed: {:?}", source.display(), status);
    std::fs::read(out).expect("no object file written")
}

/// Compiling the same source repeatedly, hashing differently each time, gives
/// byte-identical objects.
#[test]
fn aot_compilation_is_reproducible() {
    let dir = rmx::tempfile::tempdir().X();
    let fixtures = fixtures();
    assert!(fixtures.len() > 20, "the fixtures were not found");

    let differing: Vec<String> = fixtures.par_iter().enumerate()
        .filter_map(|(i, source)| {
            let seen: Vec<Vec<u8>> = (0..3)
                .map(|seed| compile_once(source, seed, &dir.path().join(format!("{i}_{seed}.o"))))
                .collect();
            seen.iter().any(|bytes| bytes != &seen[0])
                .then(|| source.file_name().X().to_string_lossy().into_owned())
        })
        .collect();

    assert!(
        differing.is_empty(),
        "compiled under three hash seeds, these gave different objects: {differing:?}; \
         something in the compiler walks a LookupMap or LookupSet in hash order",
    );
}
