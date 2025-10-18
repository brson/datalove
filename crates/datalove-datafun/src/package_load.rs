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
    pub file_package_main: PathBuf,
    pub dir_pkglib_system: PathBuf,
    pub dir_pkglib_local: Option<PathBuf>,
}

pub struct PackageWorld {
    pub package_main: Package,
    pub pkglib_system: BTreeMap<PackageName, Package>,
    pub pkglib_local: BTreeMap<PackageName, Package>,
}

pub struct Package {
    pub name: PackageName,
    pub main_module: ModuleName,
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
    let package_main = package_from_source_file(
        config.file_package_main.C(),
        None,
    ).await?;
    let pkglib_system = load_library(&config.dir_pkglib_system).await?;
    let pkglib_local = if let Some(ref dir) = config.dir_pkglib_local {
        load_library(dir).await?
    } else {
        BTreeMap::new()
    };
    Ok(PackageWorld {
        package_main, pkglib_system, pkglib_local,
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
    let Some(name) = dir.file_name() else {
        bail!("no file name for dir '{dir:?}'")
    };
    let Some(name) = name.to_str() else {
        bail!("non-utf8 module directory '{name:?}'");
    };
    let main_file = dir.join(&format!("{name}.dfm"));
    let package = package_from_source_file(main_file, Some(S(name))).await?;
    Ok(package)
}

pub async fn package_from_source_file(
    file: PathBuf,
    package_name: Option<PackageName>,
) -> AnyResult<Package> {
    if let Some(ext) = file.extension() {
        if ext != "dfm" {
            bail!("file {}, does not end with '.dfm' extension", file.display());
        }
    } else {
        bail!("file {}, does not end with '.dfm' extension", file.display());
    }

    let (file, dir) = if let Some(parent) = file.parent() {
        if parent == Path::new("") {
            let file = PathBuf::from(".").join(file);
            let parent = PathBuf::from(".");
            (file, parent)
        } else {
            let parent = parent.to_owned();
            (file, parent)
        }
    } else {
        bail!("file {} has strange directory", file.display());
    };
    let (tx, mut rx) = mpsc::channel(1);
    {
        thread::spawn(move || {
            send_modules_blocking(
                dir, tx,
            );
        });
    }

    let mut main_module = None;
    let mut modules = BTreeMap::new();

    while let Some(next) = rx.next().await {
        match next {
            Err(e) => {
                return Err(e);
            }
            Ok(module) => {
                if module.path == file {
                    main_module = Some(module.name.C());
                }
                modules.insert(module.name.C(), module);
            }
        }
    }

    match main_module {
        None => {
            bail!("main module not found at {}", file.display());
        }
        Some(main_module) => {
            let name = package_name.unwrap_or_else(|| main_module.C());
            return Ok(Package {
                name,
                main_module,
                modules,
            });
        }
    }
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
