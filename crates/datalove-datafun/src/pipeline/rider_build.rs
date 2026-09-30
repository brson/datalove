//! Native component building via cargo.
//!
//! Synthesizes a single Rust crate that depends on all discovered rider crates,
//! producing one shared library (for interpreter dlopen) and one static library
//! (for AOT linking). This avoids duplicate symbols from each rider crate
//! independently bundling the runtime.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const NATIVE_COMPONENT_CRATE_NAME: &str = "datalove-native-component";

/// Counter for unique temp file names in `write_atomic`.
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Identifies a native component build: where it is built and what goes in it.
type ComponentKey = (PathBuf, Vec<(String, PathBuf)>);

/// Components already built by this process, keyed by work dir and rider set.
static COMPONENT_CACHE: LazyLock<Mutex<HashMap<ComponentKey, NativeComponentBuild>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Result of building the unified native component library.
#[derive(Clone)]
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
/// synthesized crate is generated in `native-component/` under `work_dir`, which
/// is the workspace's writable working area (see
/// [`WorkspaceDescriptor::work_dir`](super::WorkspaceDescriptor::work_dir)).
///
/// Can be called with an empty `riders` slice to produce a runtime-only component.
///
/// The result is memoized per process for a given work dir and rider set, so
/// concurrent callers compiling the same workspace build once and share the
/// artifacts. A caller that edits rider sources within one process will keep
/// seeing the first build.
pub fn build_native_component(
    work_dir: &Path,
    riders: &[(String, PathBuf)],
) -> Result<NativeComponentBuild, RiderBuildError> {
    let key: ComponentKey = (work_dir.to_path_buf(), riders.to_vec());

    // Held across the build so that concurrent callers wait for the first
    // build rather than each running cargo against the same directory.
    let mut cache = COMPONENT_CACHE.lock()
        .expect("native component cache poisoned");
    if let Some(build) = cache.get(&key) {
        return Ok(build.clone());
    }

    let build = build_native_component_uncached(work_dir, riders)?;
    cache.insert(key, build.clone());
    Ok(build)
}

fn build_native_component_uncached(
    work_dir: &Path,
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

    let synth_dir = synth_crate_dir(work_dir);
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

    // Propagate index-64 feature when this crate was compiled with it.
    #[cfg(feature = "index-64")]
    let rt_features = ", features = [\"index-64\"]";
    #[cfg(not(feature = "index-64"))]
    let rt_features = "";

    // Generate Cargo.toml.
    let mut cargo_toml = String::new();
    cargo_toml.push_str(&format!(
        "[package]\nname = \"{NATIVE_COMPONENT_CRATE_NAME}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lib]\ncrate-type = [\"cdylib\", \"staticlib\"]\n\n\
         # Empty workspace table prevents cargo from treating this as part of\n\
         # the parent workspace.\n\
         [workspace]\n\n\
         [dependencies]\n\
         datalove-rt = {{ path = \"{}\"{} }}\n",
        rt_abs.display(),
        rt_features,
    ));
    for (_rider_name, crate_name, abs_dir) in &rider_crates {
        cargo_toml.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            crate_name,
            abs_dir.display(),
        ));
    }
    write_if_changed(&synth_dir.join("Cargo.toml"), &cargo_toml)
        .map_err(|e| build_err("native-component", format!("failed to write Cargo.toml: {}", e)))?;

    // Start from the workspace's own resolution.
    //
    // This crate's dependencies are a subset of the workspace's, but it
    // resolves them by itself, and left to do that it takes the newest of
    // everything rather than the versions the workspace is known to build
    // with. Cargo adapts the lock to what is actually needed here; what it
    // does not do is go looking for newer.
    let synth_lock = synth_dir.join("Cargo.lock");
    if !synth_lock.exists() {
        let workspace_lock = workspace_root_dir().join("Cargo.lock");
        if workspace_lock.is_file() {
            let _ = std::fs::copy(&workspace_lock, &synth_lock);
        }
    }

    // Generate lib.rs that pulls in the runtime and all rider crates.
    // The extern crate declarations ensure #[no_mangle] symbols are included.
    let mut lib_rs = String::from("extern crate datalove_rt;\n");
    for (_rider_name, crate_name, _abs_dir) in &rider_crates {
        let ident = crate_name.replace('-', "_");
        lib_rs.push_str(&format!("extern crate {};\n", ident));
    }
    write_if_changed(&src_dir.join("lib.rs"), &lib_rs)
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

/// The work dir for callers that have no workspace to take one from.
///
/// A program that calls no rider still links the runtime, which it gets from a
/// component built with an empty rider set. The linking helpers here and in
/// [`c_aot`](super::c_aot) are handed an object file rather than a workspace,
/// so they share this directory. What lands in it is fully determined by the
/// empty rider set, so sharing costs one build for the whole tree.
pub fn default_work_dir() -> PathBuf {
    workspace_root_dir().join("target").join("datalove-work").join("default")
}

/// Directory for the synthesized native component crate within a work dir.
fn synth_crate_dir(work_dir: &Path) -> PathBuf {
    work_dir.join("native-component")
}

/// Write a file only if its contents would change.
///
/// Every caller regenerates these files, and their contents are fully
/// determined by the rider set, so almost every write is a no-op in content but
/// not in metadata: `write_atomic` renames a just-created temp file over the
/// destination, which moves its mtime forward. Cargo fingerprints local sources
/// by mtime, so it recompiles and relinks, and relinking briefly unlinks the
/// output archive - while other threads are handing that path to the linker.
///
/// Skipping the write leaves the fingerprint intact, so cargo does no work and
/// never touches the archive. Contents that genuinely differ still go through
/// `write_atomic`.
fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == contents {
            return Ok(());
        }
    }
    write_atomic(path, contents)
}

/// Write a file by renaming a fully-written temp file over the destination.
///
/// Concurrent builders sharing a work dir may be running `cargo` against these
/// files. A plain write truncates first, and another builder's cargo may parse
/// the file inside that window. Rename is atomic, so readers always see one
/// complete version or the other.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file_name = path.file_name().unwrap().to_string_lossy();
    // Leading dot keeps the transient file out of cargo's way.
    let tmp = path.with_file_name(format!(".{}.tmp{}-{}", file_name, std::process::id(), n));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
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
