//! Package types with Salsa integration.
//!
//! Wraps the plain loading types with Salsa inputs for incremental computation.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
pub use bct::package2::{Package, PackageModule, PackageName, ModuleName};
use bct::package_resolve2::PackageWorldMap;

use crate::package_load as pl;

/// A package world containing system and local package libraries.
#[salsa::input]
pub struct PackageWorld {
    #[returns(ref)]
    pub pkglib_system: BTreeMap<PackageName, Package>,
    #[returns(ref)]
    pub pkglib_local: BTreeMap<PackageName, Package>,
}

/// Convert loaded packages to Salsa types.
pub fn import_from_loader(
    db: &dyn salsa::Database,
    package_world_raw: pl::PackageWorld,
) -> PackageWorld {
    PackageWorld::new(
        db,
        make_library(db, package_world_raw.pkglib_system),
        make_library(db, package_world_raw.pkglib_local),
    )
}

fn make_library(
    db: &dyn salsa::Database,
    library: BTreeMap<pl::PackageName, pl::Package>,
) -> BTreeMap<PackageName, Package> {
    library.into_iter().map(|(name, package)| {
        (name, make_package(db, package))
    }).collect()
}

fn make_package(
    db: &dyn salsa::Database,
    package: pl::Package,
) -> Package {
    Package::new(
        db,
        package.name,
        make_modules(db, package.modules),
    )
}

fn make_modules(
    db: &dyn salsa::Database,
    modules: BTreeMap<pl::ModuleName, pl::PackageModule>,
) -> BTreeMap<ModuleName, PackageModule> {
    modules.into_iter().map(|(name, module)| {
        (name, make_module(db, module))
    }).collect()
}

fn make_module(
    db: &dyn salsa::Database,
    module: pl::PackageModule,
) -> PackageModule {
    PackageModule::new(
        db,
        module.name,
        Source::new(db, module.text),
    )
}

/// Create a PackageWorldMap from a PackageWorld.
#[salsa::tracked]
pub fn package_world_map(
    db: &dyn salsa::Database,
    package_world: PackageWorld,
) -> PackageWorldMap<'_> {
    let pkglib_system = package_world.pkglib_system(db).C();
    let pkglib_local = package_world.pkglib_local(db).C();
    PackageWorldMap::new(
        db,
        BTreeMap::from([
            (S("sys"), pkglib_system),
            (S("local"), pkglib_local),
        ]),
    )
}
