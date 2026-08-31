//! The embedded library must be the tree it was built from.
//!
//! Everything else in the test suite compiles `sys/` off disk, which is only
//! a test of what ships if the two agree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn sys_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent().expect("crates dir")
        .parent().expect("repo root")
        .join("sys")
}

/// Package name to module name to source text, as the tree has it.
fn packages_on_disk() -> BTreeMap<String, BTreeMap<String, String>> {
    dir_entries(&sys_dir()).into_iter()
        .filter(|path| path.is_dir())
        .map(|package_dir| {
            let modules = dir_entries(&package_dir).into_iter()
                .filter(|path| path.extension().is_some_and(|ext| ext == "dfm"))
                .map(|path| (file_stem(&path), read(&path)))
                .collect();
            (file_name(&package_dir), modules)
        })
        .collect()
}

#[test]
fn every_module_is_embedded_verbatim() {
    let sys = datalove_stdlib::system_library();
    let on_disk = packages_on_disk();

    let embedded: BTreeMap<String, BTreeMap<String, String>> = sys.library.packages.iter()
        .map(|(name, package)| {
            let modules = package.modules.iter()
                .map(|(name, module)| (name.clone(), module.source.to_string()))
                .collect();
            (name.clone(), modules)
        })
        .collect();

    assert_eq!(
        embedded.keys().collect::<Vec<_>>(),
        on_disk.keys().collect::<Vec<_>>(),
        "the embedded library and sys/ hold different packages",
    );

    for (package, modules) in &on_disk {
        assert_eq!(
            embedded[package].keys().collect::<Vec<_>>(),
            modules.keys().collect::<Vec<_>>(),
            "package {package} holds different modules embedded than in sys/",
        );
        for (module, source) in modules {
            assert_eq!(
                &embedded[package][module], source,
                "sys/{package}/{module}.dfm differs from the embedded copy",
            );
        }
    }
}

#[test]
fn every_rider_interface_is_embedded_verbatim() {
    let sys = datalove_stdlib::system_library();

    for (name, package) in &sys.library.packages {
        let interface = sys_dir().join(name).join("rider.dli");
        match &package.rider {
            Some(rider) => assert_eq!(
                rider.interface_source.to_string(), read(&interface),
                "sys/{name}/rider.dli differs from the embedded copy",
            ),
            None => assert!(
                !interface.is_file(),
                "sys/{name} has a rider interface that was not embedded",
            ),
        }
    }
}

/// Every function a rider interface declares is linked into this binary.
#[test]
fn every_declared_native_is_linked() {
    let sys = datalove_stdlib::system_library();

    for (name, package) in &sys.library.packages {
        let Some(rider) = &package.rider else { continue };

        for declaration in rider.interface_source.lines() {
            let Some(rest) = declaration.trim().strip_prefix("native fun ") else { continue };
            let end = rest.find(|c| c == '<' || c == '(').expect("a parameter list");
            let symbol = format!("dlr_{name}__{}", rest[..end].trim());

            assert!(
                sys.natives.iter().any(|(linked, _)| *linked == symbol),
                "{symbol} is declared in sys/{name}/rider.dli but is not linked in",
            );
        }
    }
}

fn dir_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("unable to read {}: {e}", dir.display()))
        .map(|entry| entry.expect("a readable directory entry").path())
        .collect();
    entries.sort();
    entries
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("unable to read {}: {e}", path.display()))
}

fn file_name(path: &Path) -> String {
    path.file_name().expect("a named path")
        .to_str().expect("a utf-8 file name").to_string()
}

fn file_stem(path: &Path) -> String {
    path.file_stem().expect("a named file")
        .to_str().expect("a utf-8 file name").to_string()
}
