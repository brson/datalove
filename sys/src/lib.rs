//! The datalove system package library, carried inside the binary.
//!
//! A datalove binary compiles against `sys/` without reading it from disk:
//! the library's files are embedded here at build time, as a [`StaticFs`]
//! holding them under `sys/`, and loaded the way a library on disk is. The
//! native rider functions those modules call are linked in from the rider
//! crate, so the interpreter and the JIT need nothing on disk to run against
//! `sys/`.
//!
//! An AOT-compiled program is different: it is a separate executable, so the
//! runtime and the riders it calls have to reach it as a library the linker
//! is given. That library is the native component, and the compiler builds
//! one per rider set with cargo. This crate names the std rider's crate
//! directory ([`RiderDescriptor::crate_dir`]) rather than carrying a prebuilt
//! component, which is what lets a program mix `sys/` with riders of its own.
//!
//! [`RiderDescriptor::crate_dir`]: datalove_datafun::driver::RiderDescriptor::crate_dir

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};

use datalove_datafun::driver::{SystemLibrary, load_library};
use datalove_vio::StaticFs;

// `FILES` and `RIDER_CRATE_DIRS`, generated from the `sys/` directory by
// `build.rs`.
include!(concat!(env!("OUT_DIR"), "/files.rs"));

/// The library's files, with the library at `sys/`.
pub fn files() -> StaticFs {
    StaticFs::new(FILES)
}

/// The system library this binary carries.
pub fn system_library() -> SystemLibrary {
    let mut library = load_library(&files(), "sys", Path::new("sys"))
        .expect("the embedded system library loads");

    // Where each rider's crate was when this binary was built. That is a fact
    // about the host, which the embedded files cannot say: the toolchain
    // builds from it, or from the registry where there is none.
    for (package, dir) in RIDER_CRATE_DIRS {
        let rider = library.packages.get_mut(*package)
            .and_then(|package| package.rider.as_mut())
            .expect("a rider crate belongs to a package with a rider");
        rider.crate_dir = Some(PathBuf::from(dir));
    }

    SystemLibrary {
        library,
        natives: datalove_rider_sys_std::symbols().into_iter()
            .map(|(symbol, addr)| (symbol.S(), addr))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every rider interface the binary carries parses and resolves cleanly.
    ///
    /// An interface's diagnostics stop a compile, and the system library's
    /// are in every compile, so one here would stop them all. They were not
    /// reported at all until interfaces were checked, which is how four
    /// parameters named `out` sat in `std`'s for as long as they did.
    #[test]
    fn rider_interfaces_report_nothing() {
        let db = datalove_datafun::Database::default();
        for package in system_library().library.packages.values() {
            let Some(rider) = &package.rider else { continue };
            let source = bct::input::Source::new(&db, rider.interface_source.to_string());
            let (parse, types) = datalove_datafun::module_graph::rider_interface_diagnostics(&db, source);
            let messages: Vec<String> = parse.iter().map(|d| d.to_diagnostic(&db).message)
                .chain(types.iter().map(|d| d.to_diagnostic(&db).message))
                .map(|message| message.as_str(&db).S())
                .collect();
            assert!(messages.is_empty(), "{}'s rider interface: {messages:?}", package.name);
        }
    }
}
