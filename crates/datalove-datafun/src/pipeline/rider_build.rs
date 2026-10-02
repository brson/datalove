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
//!   that already contains a runtime, so this must not contain another. A
//!   rider reaches the runtime through the table on the handle it is passed
//!   rather than by name, so this resolves nothing at load time and the
//!   process loading it exports nothing. That is what `datalove-rti` is for.
//! - A **staticlib**, which an AOT-compiled program links. That program is a
//!   separate executable and calls the runtime by name like any other linked
//!   library, so the runtime has to be in it.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use super::workspace::RiderCrate;

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

/// The subdirectory a given set of riders is built in.
///
/// One directory per rider set, because the synthesized crate is written into it
/// and the library comes out under a name that says nothing about what went in:
/// every set built in one directory would be the same
/// `libdatalove_native_component.so`, so a second set would overwrite the first
/// and whoever loaded the path afterwards would not find its symbols. The same
/// reason `Kind` has a directory of its own, for the same reason it rebuilt on
/// every alternation.
///
/// Hashed rather than spelled out because a set can be long and a rider is named
/// by a path. The hash need only be stable within a toolchain: a different one
/// is a directory nothing is in yet, which costs a rebuild and no correctness.
fn rider_set_dir(riders: &[RiderCrate]) -> String {
    let mut hasher = DefaultHasher::new();
    riders.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Components already built by this process, keyed by work dir, riders and kind.
type ComponentKey = (PathBuf, Vec<RiderCrate>, Kind);

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
/// The runtime is not in it. A rider reaches the one in the process that
/// loads this through the table on its handle, so there is a single
/// `RtLocal` and a single allocator however many riders are loaded.
pub fn build_rider_dylib(
    work_dir: &Path,
    riders: &[RiderCrate],
) -> Result<PathBuf, RiderBuildError> {
    build(work_dir, riders, Kind::Dylib)
}

/// Build the runtime and the riders as one archive for a program to link.
///
/// An empty `riders` gives a runtime-only archive, which is what a program
/// that calls no rider still needs.
pub fn build_component_staticlib(
    work_dir: &Path,
    riders: &[RiderCrate],
) -> Result<PathBuf, RiderBuildError> {
    build(work_dir, riders, Kind::Staticlib)
}

fn build(
    work_dir: &Path,
    riders: &[RiderCrate],
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
    riders: &[RiderCrate],
    kind: Kind,
) -> Result<PathBuf, RiderBuildError> {
    // How each rider is named. A rider whose source sits beside its package
    // is named by path; one that does not --- a packaged datalove package,
    // whose `rider/` holds the interface and no Rust --- is named by the
    // version its manifest asks for. That is a question about the package,
    // not about this build, so a checkout can use a published rider and a
    // release can use neither.
    let mut rider_deps = Vec::new();
    for rider in riders {
        let dep = match &rider.dir {
            Some(dir) => Dep::Path(dir.canonicalize()
                .map_err(|e| RiderBuildError {
                    rider_name: rider.rider_name.clone(),
                    message: format!("failed to canonicalize rider path: {}", e),
                    stderr: String::new(),
                })?),
            None => Dep::Registry(rider.version.clone()),
        };
        rider_deps.push((rider.crate_name.clone(), dep));
    }

    let synth_dir = work_dir.join(kind.dir_name()).join(rider_set_dir(riders));
    let src_dir = synth_dir.join("src");
    std::fs::create_dir_all(&src_dir)
        .map_err(|e| build_err("native-component", format!("failed to create synth dir: {}", e)))?;

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
        let dep = runtime_dep(&datalove_buildinfo::BUILD_INFO, crate_name)?;
        cargo_toml.push_str(&format!("{} = {}\n", crate_name, dep.spec(features)));
    }

    // The runtime goes in an archive a program links, and stays out of a
    // library the interpreter loads, which gets it from the host instead.
    if kind == Kind::Staticlib {
        let dep = runtime_dep(&datalove_buildinfo::BUILD_INFO, "datalove-rt")?;
        cargo_toml.push_str(&format!("datalove-rt = {}\n", dep.spec(features)));
    }

    for (crate_name, dep) in &rider_deps {
        cargo_toml.push_str(&format!("{} = {}\n", crate_name, dep.spec("")));
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
        if let Some(checkout) = datalove_buildinfo::BUILD_INFO.checkout() {
            let workspace_lock = checkout.join("Cargo.lock");
            if workspace_lock.is_file() {
                let _ = std::fs::copy(&workspace_lock, &synth_lock);
            }
        }
    }

    // The `extern crate` declarations are what keep the linker from dropping
    // the `#[no_mangle]` symbols these crates exist to provide.
    let mut lib_rs = String::new();
    if kind == Kind::Staticlib {
        lib_rs.push_str("extern crate datalove_rt;\n");
    }

    // What the loader checks before it trusts anything else in here. The
    // value describes this side, being compiled against whatever `rti` the
    // riders resolved; the process loading it compares against its own. Only
    // a dylib is loaded, so only a dylib says.
    if kind == Kind::Dylib {
        lib_rs.push_str(
            "\n/// The runtime interface these riders were built against.\n\
             #[no_mangle]\n\
             pub static DLR_ABI_VERSION: u64 = datalove_rti::ABI_VERSION;\n\n",
        );
    }
    for (crate_name, _dep) in &rider_deps {
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

/// How a crate the component depends on is named in its manifest.
enum Dep {
    /// Source beside us, named by where it is.
    Path(PathBuf),
    /// Published, named by exactly which release.
    Registry(String),
}

impl Dep {
    /// The dependency as cargo wants it written.
    ///
    /// A registry version is pinned with `=` rather than left as a
    /// requirement. A rider and the runtime it is loaded into have to agree on
    /// the layout of everything crossing between them, and a range says only
    /// that something semver-compatible will do, which is not the same claim.
    /// `datalove-rti`'s `ABI_VERSION` is what catches a disagreement; this is
    /// what avoids one.
    fn spec(&self, features: &str) -> String {
        match self {
            Dep::Path(dir) => format!("{{ path = \"{}\"{} }}", dir.display(), features),
            Dep::Registry(version) => format!("{{ version = \"={}\"{} }}", version, features),
        }
    }
}

/// How the runtime crates are named.
///
/// Unlike a rider, these have no package to say where they came from, so it
/// is this datalove's own provenance that decides: built from a checkout, they
/// are in it; built from a release, they are published alongside at the same
/// version, the whole workspace sharing one.
///
/// Takes the provenance rather than reading it, so that the release answer can
/// be tested from a checkout. It is the answer that cannot otherwise be tried
/// until there is something published to try it against.
fn runtime_dep(
    info: &datalove_buildinfo::BuildInfo,
    name: &str,
) -> Result<Dep, RiderBuildError> {
    match info {
        datalove_buildinfo::BuildInfo::Prod { version } => {
            Ok(Dep::Registry(version.to_string()))
        }
        datalove_buildinfo::BuildInfo::Local { checkout, .. } => {
            let checkout = Path::new(checkout);
            if !checkout.is_dir() {
                return Err(build_err("native-component", format!(
                    "this datalove was built from {}, which is no longer there, \
                     and the runtime has to be compiled from it to build a \
                     program. Build from that checkout again, or put it back.",
                    checkout.display(),
                )));
            }

            checkout.join("crates").join(name).canonicalize()
                .map(Dep::Path)
                .map_err(|e| build_err("native-component", format!(
                    "{} is missing from the checkout at {}: {}",
                    name, checkout.display(), e,
                )))
        }
    }
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
    let base = match datalove_buildinfo::BUILD_INFO.checkout() {
        Some(checkout) => checkout.join("target"),
        // A release build has no tree to write into, so the cache it is.
        // `paths::work_dir` is the same directory; this reaches it
        // without depending on that crate, which sits above this one.
        None => std::env::temp_dir().join("datalove"),
    };
    base.join("datalove-work").join("default")
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

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_buildinfo::BuildInfo;

    fn rider(name: &str) -> RiderCrate {
        RiderCrate {
            rider_name: name.to_string(),
            crate_name: format!("datalove-rider-{}", name),
            version: "0.1.0".to_string(),
            dir: None,
        }
    }

    /// Two rider sets are built apart, so neither's library is the other's.
    ///
    /// They shared one directory once, and the library's name says nothing
    /// about what went into it, so the second set built over the first and a
    /// load of the first's path came up without its symbols.
    #[test]
    fn a_rider_set_is_built_in_its_own_directory() {
        let one = rider_set_dir(&[rider("std")]);
        let two = rider_set_dir(&[rider("std"), rider("other")]);
        let none = rider_set_dir(&[]);

        assert_ne!(one, two);
        assert_ne!(one, none);
        assert_ne!(two, none);

        // The same set asks for the same directory, or nothing would ever be
        // reused between runs.
        assert_eq!(one, rider_set_dir(&[rider("std")]));
    }

    /// Order is part of the set, since it is the order the deps are written in.
    #[test]
    fn a_reordered_rider_set_is_a_different_directory() {
        let forward = rider_set_dir(&[rider("a"), rider("b")]);
        let backward = rider_set_dir(&[rider("b"), rider("a")]);
        assert_ne!(forward, backward);
    }

    /// A release names published versions, which is the arrangement that
    /// cannot be tried for real until something is published.
    #[test]
    fn a_release_names_versions() {
        let info = BuildInfo::Prod { version: "0.1.0" };
        let dep = runtime_dep(&info, "datalove-rt").expect("a release can name it");
        assert_eq!(dep.spec(""), "{ version = \"=0.1.0\" }");
    }

    /// Pinned exactly, not left as a requirement. A range would let cargo
    /// resolve a rider against a different layout of everything that crosses
    /// between it and the runtime.
    #[test]
    fn a_version_is_pinned() {
        assert!(Dep::Registry("0.1.0".into()).spec("").contains("=0.1.0"));
    }

    /// `index-64` has to reach the dependency whichever way it is named,
    /// being the one feature that moves layout.
    #[test]
    fn features_survive_either_naming() {
        let features = ", features = [\"index-64\"]";
        assert!(Dep::Registry("0.1.0".into()).spec(features).contains("index-64"));
        assert!(Dep::Path("/somewhere".into()).spec(features).contains("index-64"));
    }

    /// A checkout that has gone says so, rather than leaving cargo to report
    /// a path dependency it cannot find and none of the reasons for it.
    #[test]
    fn a_missing_checkout_is_reported() {
        let info = BuildInfo::Local {
            git_sha: Some("deadbeef"),
            checkout: "/nowhere/this/tree/went",
        };
        let error = runtime_dep(&info, "datalove-rt")
            .err().expect("a missing checkout cannot be named");
        let message = error.to_string();
        assert!(message.contains("/nowhere/this/tree/went"), "{message}");
        assert!(message.contains("no longer there"), "{message}");
    }
}
