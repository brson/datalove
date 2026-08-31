//! Embeds the `sys/` package library and the native component into this crate.
//!
//! The generated table names every package, its modules and its rider
//! interface, and pulls their text in with `include_str!` so that a built
//! binary carries the system library rather than reading it back out of the
//! source tree it was compiled from. The native component -- the runtime and
//! the riders, which an AOT-compiled program links against -- is built here
//! and embedded compressed, for the same reason.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use rmx::sha2::Digest as _;

/// The package holding the runtime and riders an AOT program links.
const COMPONENT_PACKAGE: &str = "datalove-native-component";

/// The profile it is built with, defined in the workspace manifest.
const COMPONENT_PROFILE: &str = "native-component";

fn main() {
    embed_sys();
    embed_native_component();
}

/// Generate the table of package sources.
fn embed_sys() {
    let sys_dir = sys_dir();
    println!("cargo:rerun-if-changed={}", sys_dir.display());

    let mut table = String::from("static PACKAGES: &[EmbeddedPackage] = &[\n");

    for package_dir in sorted_dirs(&sys_dir) {
        let name = file_name(&package_dir);
        println!("cargo:rerun-if-changed={}", package_dir.display());

        writeln!(table, "    EmbeddedPackage {{").expect("writing to a string");
        writeln!(table, "        name: {name:?},").expect("writing to a string");
        writeln!(table, "        modules: &[").expect("writing to a string");

        for module in sorted_modules(&package_dir) {
            let module_name = module.file_stem().expect("a .dfm file has a stem")
                .to_str().expect("a utf-8 file name");
            println!("cargo:rerun-if-changed={}", module.display());
            writeln!(
                table,
                "            ({module_name:?}, include_str!({:?})),",
                module.display().to_string(),
            ).expect("writing to a string");
        }

        writeln!(table, "        ],").expect("writing to a string");

        let interface = package_dir.join("rider.dli");
        if interface.is_file() {
            println!("cargo:rerun-if-changed={}", interface.display());
            writeln!(
                table,
                "        rider_interface: Some(include_str!({:?})),",
                interface.display().to_string(),
            ).expect("writing to a string");
        } else {
            writeln!(table, "        rider_interface: None,").expect("writing to a string");
        }

        writeln!(table, "    }},").expect("writing to a string");
    }

    table.push_str("];\n");

    write_out("packages.rs", table.as_bytes());
}

/// Build the native component and embed it, compressed.
///
/// The component has its own profile whatever profile datalove itself is
/// built in, because it ends up inside the programs the AOT backend emits
/// rather than inside datalove. It also has its own target directory: cargo
/// holds a lock on the one this build is running under.
fn embed_native_component() {
    for dir in ["crates/datalove-rt", "crates/datalove-rtdt", "sys/std/rider"] {
        println!("cargo:rerun-if-changed={}", repo_root().join(dir).display());
    }

    let out_dir = out_dir();
    let target_dir = out_dir.join("native-component");

    let status = Command::new(std::env::var("CARGO").expect("CARGO"))
        .arg("build")
        .arg("--profile").arg(COMPONENT_PROFILE)
        .arg("--package").arg(COMPONENT_PACKAGE)
        .arg("--target-dir").arg(&target_dir)
        .current_dir(repo_root())
        .status()
        .unwrap_or_else(|e| panic!("unable to run cargo for {COMPONENT_PACKAGE}: {e}"));

    assert!(status.success(), "building {COMPONENT_PACKAGE} failed with {status}");

    let staticlib = staticlib_path(&target_dir.join(COMPONENT_PROFILE));
    let bytes = std::fs::read(&staticlib)
        .unwrap_or_else(|e| panic!("unable to read {}: {e}", staticlib.display()));

    // The digest names the unpacked file, so that a binary never links a
    // component left behind by a different build.
    let digest = rmx::sha2::Sha256::digest(&bytes);
    let digest = digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>();

    // This script re-runs whenever any of its inputs change, including the
    // stdlib sources, but compressing tens of megabytes is worth doing only
    // when the component itself is new.
    let stamp = out_dir.join("native-component.sha256");
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(digest.as_str()) {
        let mut encoder = rmx::flate2::write::GzEncoder::new(
            Vec::new(),
            rmx::flate2::Compression::default(),
        );
        std::io::Write::write_all(&mut encoder, &bytes).expect("compressing the native component");
        let compressed = encoder.finish().expect("compressing the native component");

        write_out("native-component.a.gz", &compressed);
        write_out("native-component.sha256", digest.as_bytes());
    }
    write_out(
        "native-component.rs",
        format!(
            "/// The native component, gzipped.\n\
             static NATIVE_COMPONENT_GZ: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/native-component.a.gz\"));\n\
             \n\
             /// Digest of the uncompressed component, which names the unpacked file.\n\
             const NATIVE_COMPONENT_DIGEST: &str = {digest:?};\n\
             \n\
             /// File name of the unpacked component.\n\
             const NATIVE_COMPONENT_NAME: &str = {:?};\n",
            staticlib.file_name().expect("a named file").to_str().expect("a utf-8 file name"),
        ).as_bytes(),
    );
}

/// The static library cargo produced for the native component.
fn staticlib_path(dir: &Path) -> PathBuf {
    let candidates = ["libdatalove_native_component.a", "datalove_native_component.lib"];
    candidates.iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no static library in {}", dir.display()))
}

fn write_out(name: &str, contents: &[u8]) {
    let path = out_dir().join(name);
    std::fs::write(&path, contents)
        .unwrap_or_else(|e| panic!("unable to write {}: {e}", path.display()));
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
}

/// The source tree this crate is being built from.
fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    // crates/datalove-stdlib -> crates -> repo root.
    manifest_dir.parent().expect("crates dir")
        .parent().expect("repo root")
        .to_path_buf()
}

/// The `sys/` directory of the source tree this crate is being built from.
fn sys_dir() -> PathBuf {
    repo_root().join("sys")
}

/// Package directories, in a fixed order so the generated table is stable.
fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = read_dir(dir)
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Module files of a package, in a fixed order.
fn sorted_modules(dir: &Path) -> Vec<PathBuf> {
    let mut modules: Vec<PathBuf> = read_dir(dir)
        .filter(|path| path.extension().is_some_and(|ext| ext == "dfm"))
        .collect();
    modules.sort();
    modules
}

fn read_dir(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("unable to read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
}

fn file_name(path: &Path) -> &str {
    path.file_name().expect("a named path")
        .to_str().expect("a utf-8 file name")
}
