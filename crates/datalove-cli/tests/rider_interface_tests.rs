//! Scripts beside a workspace whose rider interface is wrong.
//!
//! A workspace of its own, `fixtures/rider_interface/local/`: every package's
//! interface is checked whether or not a script requires it, so a broken one
//! beside the fixtures in `workspace_tests` would fail all of them.

use rmx::prelude::*;
use std::path::Path;
use std::process::Command;

/// Run the datalove CLI binary on a script file and capture its output.
fn run_script(path: &Path) -> Result<String, String> {
    let binary_path = env!("CARGO_BIN_EXE_datalove");

    // Run the binary with the script command.
    let output = Command::new(&binary_path)
        .arg("script")
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

    // Return the combined output.
    Ok(format!("{}{}", stdout, stderr))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), run_script)
        .fixture_subdir("rider_interface")
        .file_extension("dfs")
        .allow_errors(false)
        .run();
}
