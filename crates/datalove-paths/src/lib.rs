//! Where a datalove installation writes what it generates.
//!
//! Only the native component so far. It lands under the user's cache rather
//! than beside the script being compiled, there being no reason for a build
//! artifact of the toolchain to turn up in somebody's source directory.
//!
//! Its own crate because more than one driver needs it and none of them is
//! the right owner: it is not the package library, and the REPL takes whatever
//! library it is handed rather than depending on the embedded one.

use rmx::prelude::*;
use rmx::std::path::PathBuf;

/// The directory the compiler may build native components in.
///
/// An installed binary has no source tree to write to, so this is under the
/// user's cache directory. Cargo does the caching within it: the component
/// for a given rider set is rebuilt only when one of the rider crates
/// changes.
///
/// The runtime interface names the directory because every datalove on a
/// machine shares this cache. Two of them built against different interfaces
/// would otherwise build over each other's components, and the one that
/// found the other's would be refused at load. Keeping them apart is
/// cheaper than explaining that.
pub fn work_dir() -> AnyResult<PathBuf> {
    let dir = cache_dir()?
        .join("work")
        .join(fmt!("{:016x}", datalove_rti::ABI_VERSION));
    rmx::std::fs::create_dir_all(&dir)
        .context(fmt!("unable to create {}", dir.display()))?;
    Ok(dir)
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
