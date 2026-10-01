//! Embeds this crate's own package library into it.
//!
//! The generated table names every package and its modules and pulls their
//! text in with `include_str!`, so that a built binary carries the system
//! library rather than reading it back out of the tree it was compiled from.
//!
//! The crate root is the library, so those paths are inside it and survive
//! being packaged. Reaching above a crate's root is the thing cargo will not
//! package, which is why this is not under `crates/` with everything else.
//!
//! Two things are named rather than embedded. A package's rider interface is a
//! Rust constant in the rider's own crate, which has to be a dependency
//! regardless -- cargo excludes a directory holding a `Cargo.toml` from the
//! package around it, so `std/rider/rider.dli` is not in this crate's files
//! whatever its `include` says, and silently so. The rider crate itself is
//! named by the package's manifest, for the compiler to build a native
//! component out of.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    embed_sys();
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

        // What the package manifest calls the rider crate, and which version.
        // `package_load` reads the same from the same file when it takes a
        // package off disk.
        match rider_crate(&package_dir) {
            Some((name, version)) => {
                // The interface is a constant in that crate rather than a file
                // here, cargo having left `rider/` out of this package.
                writeln!(
                    table,
                    "        rider_interface: Some({}::INTERFACE),",
                    name.replace('-', "_"),
                ).expect("writing to a string");
                writeln!(
                    table,
                    "        rider_crate: Some(({name:?}, {version:?})),",
                ).expect("writing to a string");
            }
            None => {
                writeln!(table, "        rider_interface: None,")
                    .expect("writing to a string");
                writeln!(table, "        rider_crate: None,")
                    .expect("writing to a string");
            }
        }

        let crate_dir = package_dir.join("rider");
        if crate_dir.join("Cargo.toml").is_file() {
            writeln!(
                table,
                "        rider_crate_dir: Some({:?}),",
                crate_dir.display().to_string(),
            ).expect("writing to a string");
        } else {
            writeln!(table, "        rider_crate_dir: None,").expect("writing to a string");
        }

        writeln!(table, "    }},").expect("writing to a string");
    }

    table.push_str("];\n");

    write_out("packages.rs", &table);
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

/// Package directories, in a fixed order so the generated table is stable.
///
/// The library root is this crate's directory, so `src` and `target` are in it
/// and are not packages. They are excluded by not looking like one rather than
/// by name: a package has modules, or a manifest, or it is some other
/// directory that happens to be here. `package_load` applies the same rule
/// when it reads a library off disk.
fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = read_dir(dir)
        .filter(|path| path.is_dir())
        .filter(|path| is_package(path))
        .collect();
    dirs.sort();
    dirs
}

/// Whether a directory is a datalove package.
fn is_package(dir: &Path) -> bool {
    if dir.join(datalove_pkg_manifest::FILE_NAME).is_file() {
        return true;
    }
    read_dir(dir).any(|path| path.extension().is_some_and(|ext| ext == "dfm"))
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

/// What the package's manifest calls its rider crate, and which version.
///
/// A package with a rider must declare it; `datalove_pkg_manifest::load` is
/// the one that says so, and it says the same thing whether a package is read
/// here or off disk at run time.
fn rider_crate(package_dir: &Path) -> Option<(String, String)> {
    let manifest_path = package_dir.join(datalove_pkg_manifest::FILE_NAME);
    if manifest_path.is_file() {
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }

    let has_rider = package_dir.join("rider").join("rider.dli").is_file();
    let manifest = datalove_pkg_manifest::load(package_dir, has_rider)
        .unwrap_or_else(|e| panic!("{e}"));

    manifest
        .and_then(|manifest| manifest.rider)
        .map(|rider| (rider.name, rider.version))
}
