//! Embeds this crate's own package library into it.
//!
//! The generated table is the library as files: every module, data file and
//! manifest in it, pulled in with `include_bytes!` and named by its path under
//! `sys/`. The binary reads them back through a `StaticFs`, with the loader
//! that reads a library off disk, so which directories are packages and what
//! a package holds is decided there and not here.
//!
//! The crate root is the library, so those paths are inside it and survive
//! being packaged. Reaching above a crate's root is the thing cargo will not
//! package, which is why this is not under `crates/` with everything else.
//!
//! Two things are not this crate's files. A package's rider interface is a
//! Rust constant in the rider's own crate, which has to be a dependency
//! regardless -- cargo excludes a directory holding a `Cargo.toml` from the
//! package around it, so `std/rider/rider.dli` is not in this crate's files
//! whatever its `include` says, and silently so. The table holds the constant
//! at the path the file would have. And the rider crate's directory is a fact
//! about the host this is built on, for the compiler to hand cargo, so it goes
//! in a table of its own rather than among the files.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    embed_sys();
}

/// Generate the tables of files and rider crate directories.
fn embed_sys() {
    let sys_dir = sys_dir();
    println!("cargo:rerun-if-changed={}", sys_dir.display());

    let mut files = String::from("static FILES: &[(&str, &[u8])] = &[\n");
    let mut crate_dirs = String::from("static RIDER_CRATE_DIRS: &[(&str, &str)] = &[\n");

    for dir in sorted_dirs(&sys_dir) {
        let name = file_name(&dir);
        println!("cargo:rerun-if-changed={}", dir.display());

        for file in sorted_files(&dir) {
            println!("cargo:rerun-if-changed={}", file.display());
            writeln!(
                files,
                "    ({:?}, include_bytes!({:?})),",
                format!("sys/{name}/{}", file_name(&file)),
                file.display().to_string(),
            ).expect("writing to a string");
        }

        // The interface is a constant in the rider crate rather than a file
        // here, cargo having left `rider/` out of this package.
        if let Some(crate_name) = rider_crate(&dir) {
            writeln!(
                files,
                "    ({:?}, {}::INTERFACE.as_bytes()),",
                format!("sys/{name}/rider/rider.dli"),
                crate_name.replace('-', "_"),
            ).expect("writing to a string");
        }

        let crate_dir = dir.join("rider");
        if crate_dir.join("Cargo.toml").is_file() {
            writeln!(
                crate_dirs,
                "    ({name:?}, {:?}),",
                crate_dir.display().to_string(),
            ).expect("writing to a string");
        }
    }

    files.push_str("];\n");
    crate_dirs.push_str("];\n");

    write_out("files.rs", &(files + &crate_dirs));
}

fn write_out(name: &str, contents: &str) {
    let path = out_dir().join(name);
    std::fs::write(&path, contents)
        .unwrap_or_else(|e| panic!("unable to write {}: {e}", path.display()));
}

fn out_dir() -> PathBuf {
    PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"))
}

/// The package library, which is this crate's own directory.
fn sys_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
}

/// The directories of the library, in a fixed order so the table is stable.
///
/// The library root is this crate's directory, so `src` and `tests` are among
/// them. They hold nothing `sorted_files` takes, so nothing of theirs is
/// embedded, and the loader does not take an empty directory for a package.
fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = read_dir(dir)
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// A package directory's modules, data files and manifest, in a fixed order.
fn sorted_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = read_dir(dir)
        .filter(|path| path.is_file())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "dfm" || ext == "dlt")
                || path.file_name().is_some_and(|name| name == datalove_pkg_manifest::FILE_NAME)
        })
        .collect();
    files.sort();
    files
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

/// What the package's manifest calls its rider crate, if it has a rider.
///
/// A package with a rider must declare it; `datalove_pkg_manifest::load` is
/// the one that says so, and it says the same thing whether a package is read
/// here or off disk at run time.
fn rider_crate(package_dir: &Path) -> Option<String> {
    let has_rider = package_dir.join("rider").join("rider.dli").is_file();
    let fs = datalove_vio::HostFs::new().expect("the working directory");
    let manifest = datalove_pkg_manifest::load(&fs, package_dir, has_rider)
        .unwrap_or_else(|e| panic!("{e}"));

    manifest
        .and_then(|manifest| manifest.rider)
        .map(|rider| rider.name)
}
