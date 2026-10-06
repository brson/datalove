//! Tests for the `aot-compile --run` CLI command.
//!
//! These tests verify that AOT compilation and execution produces the same
//! output as the interpreter for simple scripts using `debuglog()`.

use std::path::Path;
use std::process::Command;

/// Run the datalove CLI with `aot-compile --run` on a script file.
fn run_aot(path: &Path) -> Result<String, String> {
    let binary_path = env!("CARGO_BIN_EXE_datalove");

    // Create a temp directory for the output executable.
    let temp_dir = rmx::tempfile::tempdir()
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let exe_path = temp_dir.path().join("test_exe");

    // Run the binary with aot-compile --run.
    let output = Command::new(&binary_path)
        .arg("aot-compile")
        .arg("--run")
        .arg("-o")
        .arg(&exe_path)
        .arg(path)
        .output()
        .map_err(|e| format!("Failed to execute binary: {}", e))?;

    // Capture both stdout and stderr.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // If the command failed, return stderr as the error.
    if !output.status.success() {
        return Err(format!("{}{}", stdout, stderr));
    }

    // Filter out the "Linked executable:" line from stdout.
    let filtered_stdout: String = stdout
        .lines()
        .filter(|line| !line.starts_with("Linked executable:"))
        .collect::<Vec<_>>()
        .join("\n");

    // Return stderr (debuglog output) with filtered stdout.
    // debuglog output goes to stderr.
    let mut result = String::new();
    if !filtered_stdout.is_empty() {
        result.push_str(&filtered_stdout);
        if !filtered_stdout.ends_with('\n') {
            result.push('\n');
        }
    }
    result.push_str(&stderr);
    Ok(result)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), run_aot)
        .fixture_subdir("aot_run")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
