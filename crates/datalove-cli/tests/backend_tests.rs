//! Every backend, through the binary, on the library the binary carries.
//!
//! `std_all_tests` compares the three backends too, but it builds its own
//! rider component and registers the natives itself. What it cannot see is
//! the driver: the embedded component an installed binary links against, and
//! the registration each of `script`, `script --jit` and `aot-compile` does
//! for itself. A native reaching the interpreter and not the jit is invisible
//! to it, and was: `script --jit` aborted on any script that called one.
//!
//! The recorded output is what all four agree on, so a disagreement is a
//! failure rather than a blessed difference.
//!
//! Four, because there are two ways to compile ahead of time and they share
//! no code: one emits an object through cranelift and one emits C. Only the
//! first was run here, and the C backend carried its own copies of two out
//! parameter bugs the whole time the cranelift one was being fixed.

use rmx::prelude::*;
use std::path::Path;
use std::process::Command;

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

/// Compile the script ahead of time and run what comes out.
///
/// `extra` is what picks the backend: nothing for the cranelift object, `--c`
/// for the one that emits C. Each gets its own directory, since two of these
/// running at once would otherwise link over each other's output.
fn run_aot(binary: &Path, script: &std::ffi::OsStr, extra: &[&str]) -> Result<String, String> {
    let dir = rmx::tempfile::tempdir()
        .map_err(|e| format!("temp dir: {}", e))?;
    let exe = dir.path().join("compiled");

    let mut args: Vec<&std::ffi::OsStr> = vec!["aot-compile".as_ref(), "--run".as_ref()];
    args.extend(extra.iter().map(|a| std::ffi::OsStr::new(a)));
    args.extend(["-o".as_ref(), exe.as_ref(), script]);
    let output = run(binary, &args)?;

    // The AOT driver says where it put the executable before running it,
    // which is a path that differs every time and says nothing about the
    // program.
    Ok(output.lines()
        .filter(|line| !line.starts_with("Linked executable:"))
        .map(|line| format!("{line}\n"))
        .collect())
}

fn run_every_backend(path: &Path) -> Result<String, String> {
    let binary = Path::new(env!("CARGO_BIN_EXE_datalove"));
    let script: &std::ffi::OsStr = path.as_ref();

    let interp = run(&binary, &["script".as_ref(), script])
        .map_err(|e| format!("interpreter: {}", e))?;
    let jit = run(&binary, &["script".as_ref(), "--jit".as_ref(), script])
        .map_err(|e| format!("jit: {}", e))?;
    let aot = run_aot(&binary, script, &[])
        .map_err(|e| format!("aot: {}", e))?;
    let c_aot = run_aot(&binary, script, &["--c"])
        .map_err(|e| format!("c aot: {}", e))?;

    if interp != jit {
        return Err(format!("interpreter and jit disagree:\n{interp}---\n{jit}"));
    }
    if interp != aot {
        return Err(format!("interpreter and aot disagree:\n{interp}---\n{aot}"));
    }
    if interp != c_aot {
        return Err(format!("interpreter and c aot disagree:\n{interp}---\n{c_aot}"));
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
