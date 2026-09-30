//! Works out where this datalove's own sources came from, and bakes it in.
//!
//! The compiler needs the runtime and the riders as cargo dependencies, and
//! how to name them depends entirely on this: a build from a checkout points
//! at that checkout, and a build from a registry names versions. Nothing at
//! run time can tell the two apart, so it is decided here, while the answer
//! is still in front of us.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));

    let generated = match checkout_root(&manifest_dir) {
        Some(root) => {
            let sha = match head_revision(&root) {
                Some(sha) => format!("Some({sha:?})"),
                None => "None".to_string(),
            };
            format!(
                "pub const BUILD_INFO: BuildInfo = BuildInfo::Local {{\n\
                 \x20   git_sha: {sha},\n\
                 \x20   checkout: {:?},\n\
                 }};\n",
                root.display().to_string(),
            )
        }
        None => format!(
            "pub const BUILD_INFO: BuildInfo = BuildInfo::Prod {{\n\
             \x20   version: {:?},\n\
             }};\n",
            std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION"),
        ),
    };

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
        .join("build_info.rs");
    std::fs::write(&out, generated)
        .unwrap_or_else(|e| panic!("unable to write {}: {e}", out.display()));
}

/// The datalove checkout this crate is inside, if it is inside one.
///
/// Asking git whether we are in a repository would answer a different
/// question: a published crate unpacked inside somebody else's repository, or
/// vendored into one, is in a checkout but not in *this* one. So the test is
/// for the tree's own shape, which no other repository has.
///
/// It does not need git to be installed, or to have ever been run. A tarball
/// of the tree is still the tree.
fn checkout_root(from: &Path) -> Option<PathBuf> {
    for dir in from.ancestors() {
        let looks_right = dir.join("sys").join("std").is_dir()
            && dir.join("crates").join("datalove-buildinfo").join("Cargo.toml").is_file();
        if looks_right {
            println!("cargo:rerun-if-changed={}", dir.join("Cargo.toml").display());
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// What git says is checked out, if it can be asked.
///
/// Recorded so that a binary whose checkout has been moved or deleted can one
/// day fetch the sources it was built from by revision. Nothing reads it yet.
///
/// Deliberately not a `rerun-if-changed` on `.git/HEAD`: that would rebuild
/// this crate, and relink everything above it, on every commit. The cost is
/// that the revision is as of whenever this crate last compiled, which is
/// worth knowing before anything relies on it.
fn head_revision(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C").arg(root)
        .arg("rev-parse").arg("HEAD")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}
