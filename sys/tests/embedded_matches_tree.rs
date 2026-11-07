//! The embedded library must be the tree it was built from.
//!
//! Everything else in the test suite compiles `sys/` off disk, which is only
//! a test of what ships if the two agree.

use std::path::{Path, PathBuf};

use datalove_datafun::driver::{PackageLibrary, load_library};

/// The library, which is this crate's own directory.
fn sys_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The library as the tree has it, read by the loader the embedded one goes
/// through.
///
/// Its files are named as the embedded ones are, from the directory above
/// the library, so `sys/std/list.dfm` for both. Everything else is as read.
fn library_on_disk() -> PackageLibrary {
    let fs = datalove_vio::HostFs::new().expect("the working directory");
    let mut library = load_library(&fs, "sys", &sys_dir()).expect("sys/ loads");
    let above = sys_dir().parent().expect("sys/ is in a directory").to_path_buf();
    let rename = |origin: &mut Option<PathBuf>| {
        if let Some(path) = origin {
            *path = path.strip_prefix(&above).expect("a file of sys/").to_path_buf();
        }
    };
    for package in library.packages.values_mut() {
        for file in package.modules.values_mut().chain(package.data.values_mut()) {
            rename(&mut file.origin);
        }
        if let Some(rider) = &mut package.rider {
            rename(&mut rider.origin);
        }
    }
    library
}

/// Every package, module, data file and rider interface is embedded verbatim,
/// under the name the tree gives it.
///
/// Riders included: each names the crate the tree's manifest names, and the
/// crate directory baked in at build time is the one the tree has. The
/// compiler hands that directory to cargo, so the two agreeing is what keeps
/// an embedded library and a disk one building the same component.
#[test]
fn the_embedded_library_is_the_tree() {
    let embedded = datalove_sys_packages::system_library().library;
    let on_disk = library_on_disk();

    assert_eq!(
        embedded.packages.keys().collect::<Vec<_>>(),
        on_disk.packages.keys().collect::<Vec<_>>(),
        "the embedded library and sys/ hold different packages",
    );
    for (name, package) in &on_disk.packages {
        let copy = &embedded.packages[name];
        for (kind, files, copies) in [
            ("modules", &package.modules, &copy.modules),
            ("data files", &package.data, &copy.data),
        ] {
            assert_eq!(
                copies.keys().collect::<Vec<_>>(),
                files.keys().collect::<Vec<_>>(),
                "sys/{name} holds different {kind} embedded than in the tree",
            );
            for (file, descriptor) in files {
                assert!(
                    &copies[file] == descriptor,
                    "{:?} differs from the embedded copy", descriptor.origin,
                );
            }
        }
        match (&package.rider, &copy.rider) {
            (Some(rider), Some(embedded)) => {
                assert!(
                    rider.interface_source == embedded.interface_source,
                    "sys/{name}/rider/rider.dli differs from the embedded copy",
                );
                assert_eq!(rider.origin, embedded.origin, "sys/{name}'s rider interface is named differently");
                assert_eq!(rider.crate_spec, embedded.crate_spec, "sys/{name}'s rider crate is named differently");
                assert_eq!(rider.crate_dir, embedded.crate_dir, "sys/{name}'s rider crate is somewhere else");
            }
            (rider, embedded) => assert_eq!(
                rider.is_some(), embedded.is_some(),
                "sys/{name} has a rider in one and not the other",
            ),
        }
    }
}

/// Every rider crate the embedded library names is a crate.
#[test]
fn every_rider_crate_is_a_crate() {
    let sys = datalove_sys_packages::system_library();

    for (name, package) in &sys.library.packages {
        let Some(dir) = package.rider.as_ref().and_then(|rider| rider.crate_dir.as_ref()) else {
            continue;
        };
        assert!(
            Path::new(dir).join("Cargo.toml").is_file(),
            "sys/{name}'s rider crate, {}, has no Cargo.toml", dir.display(),
        );
    }
}

/// Every function a rider interface declares is linked into this binary.
#[test]
fn every_declared_native_is_linked() {
    let sys = datalove_sys_packages::system_library();

    for (name, package) in &sys.library.packages {
        let Some(rider) = &package.rider else { continue };

        for declaration in rider.interface_source.lines() {
            let Some(rest) = declaration.trim().strip_prefix("native fun ") else { continue };
            let end = rest.find(|c| c == '<' || c == '(').expect("a parameter list");
            let symbol = format!("dlr_{name}__{}", rest[..end].trim());

            assert!(
                sys.natives.iter().any(|(linked, _)| *linked == symbol),
                "{symbol} is declared in sys/{name}/rider/rider.dli but is not linked in",
            );
        }
    }
}
