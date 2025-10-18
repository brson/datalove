use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
pub use bct::package2::{Package, PackageModule, PackageName, ModuleName};
use bct::package_resolve2::PackageWorldMap;

#[salsa::input]
pub struct PackageWorld {
    pub package_main: Package,
    #[returns(ref)]
    pub pkglib_system: BTreeMap<PackageName, Package>,
    #[returns(ref)]
    pub pkglib_local: BTreeMap<PackageName, Package>,
}

use crate::package_load as pl;

pub fn import_from_loader<'db>(
    db: &'db dyn crate::Db,
    package_world_raw: pl::PackageWorld,
) -> PackageWorld {
    PackageWorld::new(
        db,
        make_package(db, package_world_raw.package_main),
        make_library(db, package_world_raw.pkglib_system),
        make_library(db, package_world_raw.pkglib_local),
    )
}

fn make_library<'db>(
    db: &'db dyn crate::Db,
    library: BTreeMap<pl::PackageName, pl::Package>,
) -> BTreeMap<PackageName, Package> {
    library.into_iter().map(|(name, package)| {
        (name, make_package(db, package))
    }).collect()
}

fn make_package<'db>(
    db: &'db dyn crate::Db,
    package: pl::Package,
) -> Package {
    Package::new(
        db,
        package.name,
        make_modules(db, package.modules),
    )
}

fn make_modules<'db>(
    db: &'db dyn crate::Db,
    modules: BTreeMap<pl::ModuleName, pl::PackageModule>,
) -> BTreeMap<ModuleName, PackageModule> {
    modules.into_iter().map(|(name, module)| {
        (name, make_module(db, module))
    }).collect()
}

fn make_module<'db>(
    db: &'db dyn crate::Db,
    module: pl::PackageModule,
) -> PackageModule {
    PackageModule::new(
        db,
        module.name,
        Source::new(db, module.text),
    )
}

#[salsa::tracked]
pub fn package_world_map<'db>(
    db: &'db dyn crate::Db,
    package_world: PackageWorld,
) -> PackageWorldMap<'db> {
    let main = BTreeMap::from([(
        package_world.package_main(db).name(db).C(),
        package_world.package_main(db).C())]);
    let pkglib_system = package_world.pkglib_system(db).C();
    let pkglib_local = package_world.pkglib_local(db).C();
    PackageWorldMap::new(
        db,
        BTreeMap::from([
            (S("main"), main),
            (S("sys"), pkglib_system),
            (S("local"), pkglib_local),
        ]),
    )
}
