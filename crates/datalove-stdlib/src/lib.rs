//! The datalove system library, carried inside the binary.
//!
//! A datalove binary compiles against `sys/` without reading it from disk:
//! the module sources are embedded here at build time, and the native rider
//! functions those modules call are linked in from the rider crate. The
//! native component an AOT-compiled program links against is embedded too.
//! An installed binary therefore needs neither a source checkout nor cargo.

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
}

// The `PACKAGES` table, generated from the `sys/` directory by `build.rs`.
include!(concat!(env!("OUT_DIR"), "/packages.rs"));

// The compressed native component and its digest, from `build.rs`.
include!(concat!(env!("OUT_DIR"), "/native-component.rs"));

/// The system library this binary carries.
pub fn system_library() -> SystemLibrary {
    SystemLibrary {
        library: PackageLibrary {
            name: "sys".S(),
            packages: PACKAGES.iter()
                .map(|package| (package.name.S(), descriptor(package)))
                .collect(),
        },
        natives: datalove_rider_std::symbols().into_iter()
            .map(|(symbol, addr)| (symbol.S(), addr))
            .collect(),
    }
}

/// Unpack the native component and return the path to link against.
///
/// An AOT-compiled program links the runtime and the rider functions it
/// calls. The linker needs them as a file, so the embedded copy is written
/// to the user's cache directory the first time it is asked for; the digest
/// in the name means a later datalove writes its own rather than reusing
/// this one.
pub fn native_component_staticlib() -> AnyResult<PathBuf> {
    let dir = cache_dir()?.join("lib");
    let path = dir.join(fmt!("{NATIVE_COMPONENT_DIGEST}-{NATIVE_COMPONENT_NAME}"));

    if path.is_file() {
        return Ok(path);
    }

    rmx::std::fs::create_dir_all(&dir)
        .context(fmt!("unable to create {}", dir.display()))?;

    let mut decoder = rmx::flate2::read::GzDecoder::new(NATIVE_COMPONENT_GZ);
    let mut unpacked = rmx::tempfile::NamedTempFile::new_in(&dir)
        .context(fmt!("unable to create a file in {}", dir.display()))?;
    rmx::std::io::copy(&mut decoder, &mut unpacked)
        .context("unable to unpack the native component")?;

    // Another process may be unpacking the same bytes; last one wins, and
    // both wrote the same content under the same digest.
    unpacked.persist(&path)
        .context(fmt!("unable to write {}", path.display()))?;

    Ok(path)
}

/// The directory datalove keeps generated files in.
fn cache_dir() -> AnyResult<PathBuf> {
    let base = match rmx::std::env::var_os("XDG_CACHE_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(rmx::std::env::var_os("HOME")
            .ok_or_else(|| anyhow!("neither XDG_CACHE_HOME nor HOME is set"))?)
            .join(".cache"),
    };
    Ok(base.join("datalove"))
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
            crate_dir: None,
        }),
    }
}
