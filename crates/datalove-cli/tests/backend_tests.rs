//! Every backend, through the binary, on the library the binary carries.
//!
//! `std_all_tests` compares the three backends too, but it builds its own
//! rider component and registers the natives itself. What it cannot see is
//! the driver: the embedded component an installed binary links against, and
//! the registration each of `script`, `script --jit` and `aot-compile` does
//! for itself. A native reaching the interpreter and not the jit is invisible
//! to it, and was: `script --jit` aborted on any script that called one.
//!
//! The recorded output is what all three agree on, so a disagreement is a
//! failure rather than a blessed difference.

use rmx::prelude::*;
use std::path::Path;
use std::process::Command;

fn binary() -> Result<std::path::PathBuf, String> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir.parent().unwrap().parent().unwrap();
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let path = workspace_dir.join("target").join(profile).join("datalove");

    if !path.exists() {
        return Err(format!(
            "Binary not found at {}. Run 'cargo build -p datalove-cli' first.",
            path.display()
        ));
    }
    Ok(path)
}

fn run(binary: &Path, args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("failed to execute binary: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(format!("exited with {}\n{}{}", output.status, stdout, stderr));
    }
    Ok(format!("{}{}", stdout, stderr))
}

fn run_every_backend(path: &Path) -> Result<String, String> {
    let binary = binary()?;
    let script: &std::ffi::OsStr = path.as_ref();

    let interp = run(&binary, &["script".as_ref(), script])
        .map_err(|e| format!("interpreter: {}", e))?;
    let jit = run(&binary, &["script".as_ref(), "--jit".as_ref(), script])
        .map_err(|e| format!("jit: {}", e))?;

    // The compiled program is written beside nothing anyone else is using,
    // since two of these running at once would otherwise link over each
    // other's output.
    let dir = rmx::tempfile::tempdir()
        .map_err(|e| format!("temp dir: {}", e))?;
    let exe = dir.path().join("compiled");
    let aot = run(&binary, &[
        "aot-compile".as_ref(), "--run".as_ref(), "-o".as_ref(), exe.as_ref(), script,
    ]).map_err(|e| format!("aot: {}", e))?;

    // The AOT driver says where it put the executable before running it,
    // which is a path that differs every time and says nothing about the
    // program.
    let aot: String = aot.lines()
        .filter(|line| !line.starts_with("Linked executable:"))
        .map(|line| format!("{line}\n"))
        .collect();

    if interp != jit {
        return Err(format!("interpreter and jit disagree:\n{interp}---\n{jit}"));
    }
    if interp != aot {
        return Err(format!("interpreter and aot disagree:\n{interp}---\n{aot}"));
    }
    Ok(interp)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), run_every_backend)
        .fixture_subdir("backend")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
