//! Native component building via cargo.
//!
//! A rider is a Rust crate of native functions that a datalove program calls.
//! Getting those functions to a program means compiling the riders a module
//! graph uses into one library, which is what this does: it synthesizes a
//! crate depending on them and runs cargo over it. One crate rather than one
//! per rider, so that whatever they share is shared once.
//!
//! There are two shapes of that library, and they differ in whether the
//! runtime is inside:
//!
//! - A **dylib**, which the interpreter and the JIT dlopen. The process doing
//!   that already contains a runtime, so this must not contain another; its
//!   `dtlv_rti_*` symbols are left undefined and resolve against the host.
//!   That is why riders depend on `datalove-rti`, which declares those
//!   functions without defining them.
//! - A **staticlib**, which an AOT-compiled program links. That program is a
//!   separate executable with no host to resolve against, so the runtime has
//!   to be in it.
//!
//! The host has to export the runtime symbols for the dylib to find them.
//! See `.cargo/config.toml`.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const NATIVE_COMPONENT_CRATE_NAME: &str = "datalove-native-component";

/// Counter for unique temp file names in `write_atomic`.
static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Which library is wanted, and so whether the runtime goes in it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    /// For dlopen by a process that has a runtime already.
    Dylib,
    /// For linking into a program that has none.
    Staticlib,
}

impl Kind {
    /// The directory under a work dir that this one is built in.
    ///
    /// They are separate because their dependencies differ, and cargo would
    /// otherwise rebuild one over the other on every alternation.
    fn dir_name(self) -> &'static str {
        match self {
            Kind::Dylib => "native-component-dylib",
            Kind::Staticlib => "native-component-static",
        }
    }

    fn crate_type(self) -> &'static str {
        match self {
            Kind::Dylib => "cdylib",
            Kind::Staticlib => "staticlib",
        }
    }
}

/// Components already built by this process, keyed by work dir, riders and kind.
type ComponentKey = (PathBuf, Vec<(String, PathBuf)>, Kind);

static COMPONENT_CACHE: LazyLock<Mutex<HashMap<ComponentKey, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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

/// Build the riders as a shared library for the interpreter to dlopen.
///
/// The runtime is not in it. The process loading it has one, and the symbols
/// left undefined here resolve against that one, so there is a single
/// `RtLocal` and a single allocator however many riders are loaded.
pub fn build_rider_dylib(
    work_dir: &Path,
    riders: &[(String, PathBuf)],
) -> Result<PathBuf, RiderBuildError> {
    build(work_dir, riders, Kind::Dylib)
}

/// Build the runtime and the riders as one archive for a program to link.
///
/// An empty `riders` gives a runtime-only archive, which is what a program
/// that calls no rider still needs.
pub fn build_component_staticlib(
    work_dir: &Path,
    riders: &[(String, PathBuf)],
) -> Result<PathBuf, RiderBuildError> {
    build(work_dir, riders, Kind::Staticlib)
}

fn build(
    work_dir: &Path,
    riders: &[(String, PathBuf)],
    kind: Kind,
) -> Result<PathBuf, RiderBuildError> {
    let key: ComponentKey = (work_dir.to_path_buf(), riders.to_vec(), kind);

    // Held across the build so that concurrent callers wait for the first
    // build rather than each running cargo against the same directory.
    let mut cache = COMPONENT_CACHE.lock()
        .expect("native component cache poisoned");
    if let Some(path) = cache.get(&key) {
        return Ok(path.clone());
    }

    let path = build_uncached(work_dir, riders, kind)?;
    cache.insert(key, path.clone());
    Ok(path)
}

fn build_uncached(
    work_dir: &Path,
    riders: &[(String, PathBuf)],
    kind: Kind,
) -> Result<PathBuf, RiderBuildError> {
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

    let synth_dir = work_dir.join(kind.dir_name());
    let src_dir = synth_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .map_err(|e| build_err("native-component", format!("failed to create synth dir: {}", e)))?;

    let workspace_root = workspace_root_dir();

    // `index-64` widens the index type in `rtdt`, which changes the layout of
    // every value crossing the boundary. Naming these directly rather than
    // relying on a rider to declare the feature puts the decision here, where
    // it has to match the process this was built by.
    #[cfg(feature = "index-64")]
    let features = ", features = [\"index-64\"]";
    #[cfg(not(feature = "index-64"))]
    let features = "";

    let mut cargo_toml = String::new();
    cargo_toml.push_str(&format!(
        "[package]\nname = \"{NATIVE_COMPONENT_CRATE_NAME}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [lib]\ncrate-type = [\"{}\"]\n\n\
         # Empty workspace table prevents cargo from treating this as part of\n\
         # the parent workspace.\n\
         [workspace]\n\n\
         [dependencies]\n",
        kind.crate_type(),
    ));

    // Depended on for their features, whoever else pulls them in.
    for crate_name in ["datalove-rtdt", "datalove-rti"] {
        let dir = crate_dep_dir(&workspace_root, crate_name)?;
        cargo_toml.push_str(&format!(
            "{} = {{ path = \"{}\"{} }}\n", crate_name, dir.display(), features,
        ));
    }

    // The runtime goes in an archive a program links, and stays out of a
    // library the interpreter loads, which gets it from the host instead.
    if kind == Kind::Staticlib {
        let dir = crate_dep_dir(&workspace_root, "datalove-rt")?;
        cargo_toml.push_str(&format!(
            "datalove-rt = {{ path = \"{}\"{} }}\n", dir.display(), features,
        ));
    }

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
        let workspace_lock = workspace_root.join("Cargo.lock");
        if workspace_lock.is_file() {
            let _ = std::fs::copy(&workspace_lock, &synth_lock);
        }
    }

    // The `extern crate` declarations are what keep the linker from dropping
    // the `#[no_mangle]` symbols these crates exist to provide.
    let mut lib_rs = String::new();
    if kind == Kind::Staticlib {
        lib_rs.push_str("extern crate datalove_rt;\n");
    }
    for (_rider_name, crate_name, _abs_dir) in &rider_crates {
        let ident = crate_name.replace('-', "_");
        lib_rs.push_str(&format!("extern crate {};\n", ident));
    }
    write_if_changed(&src_dir.join("lib.rs"), &lib_rs)
        .map_err(|e| build_err("native-component", format!("failed to write lib.rs: {}", e)))?;

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
    match kind {
        Kind::Dylib => find_cdylib(&target_dir, &lib_name),
        Kind::Staticlib => find_staticlib(&target_dir, &lib_name),
    }
}

/// Where a workspace crate this depends on lives.
fn crate_dep_dir(workspace_root: &Path, name: &str) -> Result<PathBuf, RiderBuildError> {
    workspace_root.join("crates").join(name).canonicalize()
        .map_err(|e| build_err(
            "native-component",
            format!("failed to canonicalize {} path: {}", name, e),
        ))
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
/// component built with an empty rider set. The linking helpers in
/// [`aot`](super::aot) and [`c_aot`](super::c_aot) are handed an object file
/// rather than a workspace, so they share this directory. What lands in it is
/// fully determined by the empty rider set, so sharing costs one build for the
/// whole tree.
pub fn default_work_dir() -> PathBuf {
    workspace_root_dir().join("target").join("datalove-work").join("default")
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
