//! Rider crate building via cargo.
//!
//! Builds rider Cargo crates found alongside packages, producing shared libraries
//! that can be loaded at runtime for native function dispatch.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};

/// Result of a successful rider crate build.
pub struct RiderBuildResult {
    /// Name of the rider (matches the package name).
    pub rider_name: String,
    /// Path to the built shared library.
    pub lib_path: PathBuf,
}

/// Error from a failed rider crate build.
#[derive(Debug)]
pub struct RiderBuildError {
    pub rider_name: String,
    pub message: String,
    pub stderr: String,
}

impl std::fmt::Display for RiderBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rider '{}' build failed: {}\n{}", self.rider_name, self.message, self.stderr)
    }
}

impl std::error::Error for RiderBuildError {}

/// Build a rider crate, producing a shared library.
///
/// Runs `cargo build --lib` in the rider crate directory. The crate's `Cargo.toml`
/// must specify `crate-type = ["cdylib"]`.
pub fn build_rider_crate(
    crate_dir: &Path,
    rider_name: &str,
) -> Result<RiderBuildResult, RiderBuildError> {
    let output = std::process::Command::new("cargo")
        .arg("build")
        .arg("--lib")
        .arg("--release")
        .current_dir(crate_dir)
        .output()
        .map_err(|e| RiderBuildError {
            rider_name: rider_name.to_string(),
            message: format!("failed to invoke cargo: {}", e),
            stderr: String::new(),
        })?;

    if !output.status.success() {
        return Err(RiderBuildError {
            rider_name: rider_name.to_string(),
            message: format!("cargo build exited with {}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    // Find the built shared library in target/release/.
    let lib_path = find_cdylib(crate_dir, rider_name)?;

    Ok(RiderBuildResult {
        rider_name: rider_name.to_string(),
        lib_path,
    })
}

/// Find the cdylib output for a rider crate.
fn find_cdylib(crate_dir: &Path, rider_name: &str) -> Result<PathBuf, RiderBuildError> {
    let target_dir = crate_dir.join("target").join("release");

    // Rider crate name in Cargo.toml is conventionally `datalove_rider_{name}`,
    // but we search for any .so/.dylib matching the rider name pattern.
    // The cdylib output follows the crate name from Cargo.toml.

    // Read the crate name from Cargo.toml.
    let cargo_toml_path = crate_dir.join("Cargo.toml");
    let cargo_toml = std::fs::read_to_string(&cargo_toml_path)
        .map_err(|e| RiderBuildError {
            rider_name: rider_name.to_string(),
            message: format!("failed to read Cargo.toml: {}", e),
            stderr: String::new(),
        })?;

    // Extract crate name (simple parse: look for name = "...").
    let crate_name = extract_crate_name(&cargo_toml)
        .unwrap_or_else(|| rider_name.to_string());

    // The cdylib filename follows platform conventions.
    let lib_name = crate_name.replace('-', "_");
    let candidates = [
        target_dir.join(format!("lib{}.so", lib_name)),       // Linux
        target_dir.join(format!("lib{}.dylib", lib_name)),     // macOS
        target_dir.join(format!("{}.dll", lib_name)),           // Windows
    ];

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }

    Err(RiderBuildError {
        rider_name: rider_name.to_string(),
        message: format!(
            "built library not found in {}; expected lib{}.so or lib{}.dylib",
            target_dir.display(), lib_name, lib_name,
        ),
        stderr: String::new(),
    })
}

/// Extract the crate name from a Cargo.toml string.
fn extract_crate_name(cargo_toml: &str) -> Option<String> {
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") {
            if let Some(value) = trimmed.split('=').nth(1) {
                let name = value.trim().trim_matches('"').trim_matches('\'');
                return Some(name.to_string());
            }
        }
    }
    None
}
