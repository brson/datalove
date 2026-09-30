//! Package loading from filesystem.
//!
//! Plain Rust types for loading packages before converting to Salsa types.

use rmx::prelude::*;
use rmx::std::path::PathBuf;
use rmx::futures::channel::mpsc;
use rmx::std::thread;
use rmx::std::collections::BTreeMap;
use rmx::std::fs;
use rmx::futures::executor::block_on;

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
                riders.push((pkg.name.clone(), source.clone()));
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
                dirs.push((pkg.name.clone(), dir.clone()));
            }
        }
        dirs
    }
}

pub struct Package {
    pub name: PackageName,
    pub modules: BTreeMap<ModuleName, PackageModule>,
    /// Source text of `rider/rider.dli` if present in the package.
    pub rider_source: Option<String>,
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

pub async fn load_world(
    config: PackageWorldConfig,
) -> AnyResult<PackageWorld> {
    let pkglib_system = load_library(&config.dir_pkglib_system).await?;
    let pkglib_local = if let Some(ref dir) = config.dir_pkglib_local {
        load_library(dir).await?
    } else {
        BTreeMap::new()
    };
    Ok(PackageWorld {
        pkglib_system, pkglib_local,
    })
}

async fn load_library(
    dir: &PathBuf,
) -> AnyResult<BTreeMap<PackageName, Package>> {
    if !dir.is_dir() {
        bail!("library path not a directory: '{dir:?}'");
    }

    let mut packages = BTreeMap::new();

    for file in fs::read_dir(dir)? {
        let file = file?;
        let path = file.path();
        if !path.is_dir() {
            continue;
        }

        let package = package_from_dir(path).await?;
        packages.insert(package.name.C(), package);
    }

    Ok(packages)
}

pub async fn package_from_dir(
    dir: PathBuf,
) -> AnyResult<Package> {
    let Some(file_name) = dir.file_name() else {
        bail!("no file name for dir '{dir:?}'")
    };
    let Some(name) = file_name.to_str() else {
        bail!("non-utf8 module directory '{file_name:?}'");
    };
    let name = S(name);
    let package = package_from_source_files(dir, name).await?;
    Ok(package)
}

pub async fn package_from_source_files(
    dir: PathBuf,
    package_name: PackageName,
) -> AnyResult<Package> {
    // Check for the rider before spawning the module-loading thread. Both the
    // interface and the crate live in `rider/`, so a packaged datalove package
    // that has had the Rust stripped out still declares what it needs.
    let rider_path = dir.join("rider").join("rider.dli");
    let rider_source = if rider_path.is_file() {
        Some(fs::read_to_string(&rider_path)
            .context(fmt!("unable to read rider file {}", rider_path.display()))?)
    } else {
        None
    };
    let rider_crate_dir = {
        let crate_dir = dir.join("rider");
        if crate_dir.join("Cargo.toml").is_file() {
            Some(crate_dir)
        } else {
            None
        }
    };

    // The manifest is what names the rider's crate. `rider/Cargo.toml` would
    // say the same thing where it exists, but it does not survive packaging,
    // and reading two sources for one fact invites them to disagree.
    let rider_crate = datalove_pkg_manifest::load(&dir, rider_source.is_some())?
        .and_then(|manifest| manifest.rider);

    let (tx, mut rx) = mpsc::channel(1);
    {
        thread::spawn(move || {
            send_modules_blocking(
                dir, tx,
            );
        });
    }

    let mut modules = BTreeMap::new();

    while let Some(next) = rx.next().await {
        match next {
            Err(e) => {
                return Err(e);
            }
            Ok(module) => {
                modules.insert(module.name.C(), module);
            }
        }
    }

    Ok(Package {
        name: package_name,
        modules,
        rider_source,
        rider_crate,
        rider_crate_dir,
    })
}

fn send_modules_blocking(
    dir: PathBuf,
    mut tx: mpsc::Sender<AnyResult<PackageModule>>,
) {
    if let Err(e) = send_modules_blocking_err(
        dir, tx.C(),
    ) {
        let _ = block_on(tx.send(Err(e)));
    }
}

fn send_modules_blocking_err(
    dir: PathBuf,
    mut tx: mpsc::Sender<AnyResult<PackageModule>>,
) -> AnyResult<()> {
    for file in fs::read_dir(dir)? {
        let file = file?;
        let Some(module) = load_module(file.path())? else {
            continue;
        };
        if let Err(_) = block_on(tx.send(Ok(module))) {
            return Ok(());
        }
    }

    Ok(())
}

fn load_module(path: PathBuf) -> AnyResult<Option<PackageModule>> {
    if let Some(ext) = path.extension() {
        if ext != "dfm" {
            return Ok(None);
        }
    } else {
        return Ok(None);
    }

    let Some(stem) = path.file_stem() else {
        return Ok(None);
    };
    let Some(name) = stem.to_str() else {
        bail!("file {} has non-utf-8 stem", path.display());
    };
    let name = S(name);
    let text = fs::read_to_string(&path)
        .context(fmt!("unable to read file {}", path.display()))?;
    return Ok(Some(PackageModule {
        name, path, text,
    }));
}
