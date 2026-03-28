//! Native component building via cargo.
//!
//! Synthesizes a single Rust crate that depends on all discovered rider crates,
//! producing one shared library (for interpreter dlopen) and one static library
//! (for AOT linking). This avoids duplicate symbols from each rider crate
//! independently bundling the runtime.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};

const NATIVE_COMPONENT_CRATE_NAME: &str = "datalove-native-component";

/// Result of building the unified native component library.
pub struct NativeComponentBuild {
    /// Path to the built shared library (.so/.dylib/.dll) for interpreter use.
    pub cdylib_path: PathBuf,
    /// Path to the built static library (.a/.lib) for AOT linking.
    pub staticlib_path: PathBuf,
}

/// Error from a failed native component build.
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

/// Build a unified native component library containing the runtime and all riders.
///
/// Synthesizes a single Rust crate that depends on `datalove-rt` and all rider
/// crates as rlib dependencies, producing one cdylib and one staticlib. The
/// synthesized crate is generated in `target/datalove-native-component/` under
/// the workspace root.
///
/// Can be called with an empty `riders` slice to produce a runtime-only component.
pub fn build_native_component(
    riders: &[(String, PathBuf)],
) -> Result<NativeComponentBuild, RiderBuildError> {
    // Resolve rider crate names from their Cargo.toml files.
    let mut rider_crates = Vec::new();
    for (rider_name, crate_dir) in riders {
        let cargo_toml_path = crate_dir.join("Cargo.toml");
        let cargo_toml = std::fs::read_to_string(&cargo_toml_path)
            .map_err(|e| RiderBuildError {
                rider_name: rider_name.clone(),
                message: format!("failed to read Cargo.toml: {}", e),
                stderr: String::new(),
            })?;
        let crate_name = extract_crate_name(&cargo_toml)
            .unwrap_or_else(|| rider_name.clone());
        let abs_dir = crate_dir.canonicalize()
            .map_err(|e| RiderBuildError {
                rider_name: rider_name.clone(),
                message: format!("failed to canonicalize rider path: {}", e),
                stderr: String::new(),
            })?;
        rider_crates.push((rider_name.clone(), crate_name, abs_dir));
    }

    let synth_dir = synth_crate_dir();
    let src_dir = synth_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .map_err(|e| build_err("native-component", format!("failed to create synth dir: {}", e)))?;

    // The runtime crate is always included so the native component provides
    // datalove_rt for AOT linking, eliminating the need for a separate
    // libdatalove_rt.a.
    let workspace_root = workspace_root_dir();
    let rt_dir = workspace_root.join("crates").join("datalove-rt");
    let rt_abs = rt_dir.canonicalize()
        .map_err(|e| build_err("native-component", format!("failed to canonicalize datalove-rt path: {}", e)))?;

    // Generate Cargo.toml.
    let mut cargo_toml = String::new();
    cargo_toml.push_str(&format!(
        "[package]\nname = \"{NATIVE_COMPONENT_CRATE_NAME}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lib]\ncrate-type = [\"cdylib\", \"staticlib\"]\n\n\
         # Empty workspace table prevents cargo from treating this as part of\n\
         # the parent workspace.\n\
         [workspace]\n\n\
         [dependencies]\n\
         datalove-rt = {{ path = \"{}\" }}\n",
        rt_abs.display(),
    ));
    for (_rider_name, crate_name, abs_dir) in &rider_crates {
        cargo_toml.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            crate_name,
            abs_dir.display(),
        ));
    }
    std::fs::write(synth_dir.join("Cargo.toml"), &cargo_toml)
        .map_err(|e| build_err("native-component", format!("failed to write Cargo.toml: {}", e)))?;

    // Generate lib.rs that pulls in the runtime and all rider crates.
    // The extern crate declarations ensure #[no_mangle] symbols are included.
    let mut lib_rs = String::from("extern crate datalove_rt;\n");
    for (_rider_name, crate_name, _abs_dir) in &rider_crates {
        let ident = crate_name.replace('-', "_");
        lib_rs.push_str(&format!("extern crate {};\n", ident));
    }
    std::fs::write(src_dir.join("lib.rs"), &lib_rs)
        .map_err(|e| build_err("native-component", format!("failed to write lib.rs: {}", e)))?;

    // Build the synthesized crate.
    let output = std::process::Command::new("cargo")
        .arg("build")
        .arg("--lib")
        .arg("--release")
        .current_dir(&synth_dir)
        .output()
        .map_err(|e| build_err("native-component", format!("failed to invoke cargo: {}", e)))?;

    if !output.status.success() {
        return Err(RiderBuildError {
            rider_name: "native-component".to_string(),
            message: format!("cargo build exited with {}", output.status),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let lib_name = NATIVE_COMPONENT_CRATE_NAME.replace('-', "_");
    let target_dir = synth_dir.join("target").join("release");
    let cdylib_path = find_cdylib(&target_dir, &lib_name)?;
    let staticlib_path = find_staticlib(&target_dir, &lib_name)?;

    Ok(NativeComponentBuild {
        cdylib_path,
        staticlib_path,
    })
}

/// Workspace root directory (repo root).
fn workspace_root_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/datalove-datafun -> crates -> repo root
    manifest_dir.parent().unwrap().parent().unwrap().to_path_buf()
}

/// Directory for the synthesized native component crate.
fn synth_crate_dir() -> PathBuf {
    workspace_root_dir().join("target").join("datalove-native-component")
}

fn build_err(rider_name: &str, message: String) -> RiderBuildError {
    RiderBuildError {
        rider_name: rider_name.to_string(),
        message,
        stderr: String::new(),
    }
}

/// Find the cdylib output in a target directory.
fn find_cdylib(target_dir: &Path, lib_name: &str) -> Result<PathBuf, RiderBuildError> {
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

    Err(build_err("native-component", format!(
        "shared library not found in {}; expected lib{}.so or lib{}.dylib",
        target_dir.display(), lib_name, lib_name,
    )))
}

/// Find the staticlib output in a target directory.
fn find_staticlib(target_dir: &Path, lib_name: &str) -> Result<PathBuf, RiderBuildError> {
    let candidates = [
        target_dir.join(format!("lib{}.a", lib_name)),         // Linux/macOS
        target_dir.join(format!("{}.lib", lib_name)),           // Windows
    ];

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }

    Err(build_err("native-component", format!(
        "static library not found in {}; expected lib{}.a",
        target_dir.display(), lib_name,
    )))
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
