//! The datalove system package library, carried inside the binary.
//!
//! A datalove binary compiles against `sys/` without reading it from disk:
//! the module sources are embedded here at build time, and the native rider
//! functions those modules call are linked in from the rider crate, so the
//! interpreter and the JIT need nothing on disk to run against `sys/`.
//!
//! An AOT-compiled program is different: it is a separate executable, so the
//! runtime and the riders it calls have to reach it as a library the linker
//! is given. That library is the native component, and the compiler builds
//! one per rider set with cargo. This crate names the std rider's crate
//! directory ([`PackageDescriptor::rider`]) rather than carrying a prebuilt
//! component, which is what lets a program mix `sys/` with riders of its own.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use rmx::std::path::PathBuf;
use rmx::std::sync::Arc;

use datalove_datafun::pipeline::{
    ModuleDescriptor, PackageDescriptor, PackageLibrary, RiderDescriptor, SystemLibrary,
};

/// One package of the embedded library.
struct EmbeddedPackage {
    name: &'static str,
    /// Module name and source text, one per module.
    modules: &'static [(&'static str, &'static str)],
    /// Source of the package's rider interface, if it has one.
    rider_interface: Option<&'static str>,
    /// What the package's manifest calls the rider crate, and which version.
    rider_crate: Option<(&'static str, &'static str)>,
    /// Where the rider's Rust crate was when this binary was built.
    rider_crate_dir: Option<&'static str>,
}

// The `PACKAGES` table, generated from the `sys/` directory by `build.rs`.
include!(concat!(env!("OUT_DIR"), "/packages.rs"));

/// The system library this binary carries.
pub fn system_library() -> SystemLibrary {
    SystemLibrary {
        library: PackageLibrary {
            name: "sys".S(),
            packages: PACKAGES.iter()
                .map(|package| (package.name.S(), descriptor(package)))
                .collect(),
        },
        natives: datalove_rider_sys_std::symbols().into_iter()
            .map(|(symbol, addr)| (symbol.S(), addr))
            .collect(),
    }
}

fn descriptor(package: &EmbeddedPackage) -> PackageDescriptor {
    let modules: BTreeMap<String, ModuleDescriptor> = package.modules.iter()
        .map(|(name, source)| (name.S(), ModuleDescriptor {
            name: name.S(),
            source: Arc::from(*source),
            origin: None,
        }))
        .collect();

    PackageDescriptor {
        name: package.name.S(),
        modules,
        rider: package.rider_interface.map(|source| RiderDescriptor {
            interface_source: Arc::from(source),
            crate_spec: package.rider_crate
                .map(|(name, version)| (name.S(), version.S())),
            crate_dir: package.rider_crate_dir.map(PathBuf::from),
        }),
    }
}
