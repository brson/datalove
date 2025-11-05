use rmx::prelude::*;
use std::path::Path;
use std::process::Command;

/// Run the datalove CLI binary on a script file and capture its output.
fn run_script(path: &Path) -> Result<String, String> {
    // Get the path to the compiled binary.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = std::path::PathBuf::from(manifest_dir);
    let workspace_dir = manifest_path
        .parent().unwrap()
        .parent().unwrap();

    let target_dir = workspace_dir.join("target");
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let binary_path = target_dir.join(profile).join("datalove");

    // Ensure the binary exists.
    if !binary_path.exists() {
        return Err(format!(
            "Binary not found at {}. Run 'cargo build -p datalove-cli' first.",
            binary_path.display()
        ));
    }

    // Run the binary with the script command.
    let output = Command::new(&binary_path)
        .arg("script")
        .arg(path)
        .output()
        .map_err(|e| format!("Failed to execute binary: {}", e))?;

    // Capture both stdout and stderr.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // For type error tests, we expect the command to fail and return stderr as the error.
    if !output.status.success() {
        return Err(format!("{}{}", stdout, stderr));
    }

    // If it succeeded unexpectedly, return the combined output.
    Ok(format!("{}{}", stdout, stderr))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), run_script)
        .fixture_subdir("type_error")
        .file_extension("dfs")
        .allow_errors(false)
        .run();
}
