use rmx::prelude::*;
use rmx::std::path::{PathBuf, Path};
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

pub struct Package {
    pub name: PackageName,
    pub modules: BTreeMap<ModuleName, PackageModule>,
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
    })
}

fn send_modules_blocking(
    dir: PathBuf,
    mut tx: mpsc::Sender<AnyResult<PackageModule>>,
) {
    if let Err(e) = send_modules_blocking_err(
        dir, tx.C(),
    ) {
        block_on(tx.send(Err(e)));
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
