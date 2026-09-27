//! What `aot-compile` refuses, and what it says.
//!
//! `error_tests` covers the same ground for `script`. Both are needed because
//! the two commands check the same things in their own code: `aot-compile`
//! once skipped the ownership check entirely, got as far as asking for the IR,
//! found none, and reported "IR unit not available after lowering" -- naming
//! the step that produced nothing rather than the analysis that refused to
//! produce it. A refusal reaching one command and not the other is exactly
//! what this is here to notice.
//!
//! Compilation stops before linking, so nothing here needs a C compiler.

use rmx::prelude::*;
use std::path::Path;
use std::process::Command;

fn run_aot_compile(path: &Path) -> Result<String, String> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = std::path::PathBuf::from(manifest_dir);
    let workspace_dir = manifest_path.parent().unwrap().parent().unwrap();

    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let binary_path = workspace_dir.join("target").join(profile).join("datalove");
    if !binary_path.exists() {
        return Err(format!(
            "Binary not found at {}. Run 'cargo build -p datalove-cli' first.",
            binary_path.display()
        ));
    }

    // The object goes somewhere nothing else is using, since a fixture that
    // unexpectedly compiles would otherwise write into the tree.
    let dir = rmx::tempfile::tempdir().map_err(|e| format!("temp dir: {}", e))?;
    let object = dir.path().join("out.o");

    let output = Command::new(&binary_path)
        .arg("aot-compile")
        .arg("-o")
        .arg(&object)
        .arg(path)
        .output()
        .map_err(|e| format!("Failed to execute binary: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok(strip_colors(&format!("{}{}", stdout, stderr)))
}

/// Take out the terminal color sequences a rendered diagnostic carries.
///
/// The renderer colors what it writes to stderr whether or not a terminal is
/// reading it, and the expected output is text.
fn strip_colors(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // A CSI sequence: `ESC [`, parameters, then a final letter.
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), run_aot_compile)
        .fixture_subdir("aot_error")
        .file_extension("dfs")
        .allow_errors(false)
        .run();
}
