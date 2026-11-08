//! Package loading from a filesystem.
//!
//! Plain Rust types for loading packages before converting to Salsa types.

use rmx::prelude::*;
use rmx::std::path::{Path, PathBuf};
use rmx::std::collections::BTreeMap;
use datalove_vio::Fs;

pub type PackageName = String;
pub type ModuleName = String;

pub struct PackageWorldConfig {
    pub dir_pkglib_system: PathBuf,
    pub dir_pkglib_local: Option<PathBuf>,
}

pub struct PackageWorld {
    pub pkglib_system: BTreeMap<PackageName, Package>,
    pub pkglib_local: BTreeMap<PackageName, Package>,
}

impl PackageWorld {
    /// Collect rider sources from all packages in both libraries.
    ///
    /// Returns (rider_name, source_text) pairs for each package that has a `rider/rider.dli`.
    pub fn rider_sources(&self) -> Vec<(String, String)> {
        let mut riders = Vec::new();
        for pkg in self.pkglib_system.values().chain(self.pkglib_local.values()) {
            if let Some(ref source) = pkg.rider_source {
                riders.push((pkg.name.C(), source.C()));
            }
        }
        riders
    }

    /// Collect rider crate directories from all packages in both libraries.
    ///
    /// Returns (rider_name, crate_dir) pairs for each package whose rider
    /// crate has its source beside it.
    pub fn rider_crate_dirs(&self) -> Vec<(String, PathBuf)> {
        let mut dirs = Vec::new();
        for pkg in self.pkglib_system.values().chain(self.pkglib_local.values()) {
            if let Some(ref dir) = pkg.rider_crate_dir {
                dirs.push((pkg.name.C(), dir.C()));
            }
        }
        dirs
    }
}

pub struct Package {
    pub name: PackageName,
    pub modules: BTreeMap<ModuleName, PackageModule>,
    /// The package's data files, `.dlt`, which `require data` names.
    pub data: BTreeMap<String, PackageModule>,
    /// Source text of `rider/rider.dli` if present in the package.
    pub rider_source: Option<String>,
    /// Where `rider_source` was read from, for diagnostics.
    pub rider_path: Option<PathBuf>,
    /// What the manifest calls the rider's crate, and which version of it.
    ///
    /// Present whenever `rider_source` is: a package declaring a rider has to
    /// name it, there being no Cargo.toml left to read once the package is
    /// packaged.
    pub rider_crate: Option<datalove_pkg_manifest::RiderManifest>,
    /// Path to the `rider/` Cargo crate, when its source is beside the
    /// package. Absent in a packaged one, where the crate comes from a
    /// registry instead.
    pub rider_crate_dir: Option<PathBuf>,
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
pub struct PackageModule {
    pub name: String,
    pub path: PathBuf,
    pub text: String,
}

pub fn load_world(
    fs: &dyn Fs,
    config: PackageWorldConfig,
) -> AnyResult<PackageWorld> {
    let pkglib_system = load_library_dir(fs, &config.dir_pkglib_system)?;
    let pkglib_local = if let Some(ref dir) = config.dir_pkglib_local {
        load_library_dir(fs, dir)?
    } else {
        BTreeMap::new()
    };
    Ok(PackageWorld {
        pkglib_system, pkglib_local,
    })
}

/// Load one library from its directory: every package directory in it.
pub fn load_library_dir(
    fs: &dyn Fs,
    dir: &Path,
) -> AnyResult<BTreeMap<PackageName, Package>> {
    if !fs.is_dir(dir) {
        bail!("library path not a directory: '{dir:?}'");
    }

    let mut packages = BTreeMap::new();

    for path in fs.list_dir(dir)? {
        if !fs.is_dir(&path) {
            continue;
        }

        if !is_package(fs, &path) {
            continue;
        }

        let package = package_from_dir(fs, path)?;
        packages.insert(package.name.C(), package);
    }

    Ok(packages)
}

/// Whether a directory in a library is a datalove package.
///
/// A package has modules or data, or has a manifest and intends to. A library root can
/// hold other things --- `sys` is itself a Rust crate, so `src` and `target`
/// are beside the packages in it --- and those are left alone by not looking
/// like a package rather than by being named here. The embedded system
/// library is loaded through this as well, so the rule is applied here only.
fn is_package(fs: &dyn Fs, dir: &Path) -> bool {
    if fs.is_file(&dir.join(datalove_pkg_manifest::FILE_NAME)) {
        return true;
    }

    let Ok(entries) = fs.list_dir(dir) else { return false };
    entries.iter().any(|path| source_kind(path).is_some())
}

pub fn package_from_dir(
    fs: &dyn Fs,
    dir: PathBuf,
) -> AnyResult<Package> {
    let Some(file_name) = dir.file_name() else {
        bail!("no file name for dir '{dir:?}'")
    };
    let Some(name) = file_name.to_str() else {
        bail!("non-utf8 module directory '{file_name:?}'");
    };
    let name = S(name);

    // Both the interface and the crate live in `rider/`, so a packaged
    // datalove package that has had the Rust stripped out still declares
    // what it needs.
    let rider_path = dir.join("rider").join("rider.dli");
    let rider_source = if fs.is_file(&rider_path) {
        Some(fs.read_to_string(&rider_path)
            .context(fmt!("unable to read rider file {}", rider_path.display()))?)
    } else {
        None
    };
    let rider_path = rider_source.is_some().then_some(rider_path);
    let rider_crate_dir = {
        let crate_dir = dir.join("rider");
        if fs.is_file(&crate_dir.join("Cargo.toml")) {
            Some(crate_dir)
        } else {
            None
        }
    };

    // The manifest is what names the rider's crate. `rider/Cargo.toml` would
    // say the same thing where it exists, but it does not survive packaging,
    // and reading two sources for one fact invites them to disagree.
    let rider_crate = datalove_pkg_manifest::load(fs, &dir, rider_source.is_some())?
        .and_then(|manifest| manifest.rider);

    let mut modules = BTreeMap::new();
    let mut data = BTreeMap::new();

    for path in fs.list_dir(&dir)? {
        let Some(kind) = source_kind(&path) else {
            continue;
        };
        let file = load_source_file(fs, path)?;
        match kind {
            FileKind::Module => modules.insert(file.name.C(), file),
            FileKind::Data => data.insert(file.name.C(), file),
        };
    }

    Ok(Package {
        name,
        modules,
        data,
        rider_source,
        rider_path,
        rider_crate,
        rider_crate_dir,
    })
}

/// Which of a package's files a source is.
enum FileKind {
    Module,
    Data,
}

/// Which kind of source `path` is, by its extension, if it is one.
fn source_kind(path: &Path) -> Option<FileKind> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("dfm") => Some(FileKind::Module),
        Some("dlt") => Some(FileKind::Data),
        _ => None,
    }
}

fn load_source_file(fs: &dyn Fs, path: PathBuf) -> AnyResult<PackageModule> {
    let stem = path.file_stem().expect("a file with an extension has a stem");
    let Some(name) = stem.to_str() else {
        bail!("file {} has non-utf-8 stem", path.display());
    };
    let name = S(name);
    let text = fs.read_to_string(&path)
        .context(fmt!("unable to read file {}", path.display()))?;
    Ok(PackageModule {
        name, path, text,
    })
}
