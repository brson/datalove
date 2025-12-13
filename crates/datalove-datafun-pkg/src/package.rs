//! Package types with Salsa integration.
//!
//! Re-exports bct types and provides conversion from loader types.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
pub use bct::package2::{
    Package, PackageModule, PackageName, ModuleName,
    PackageWorld, package_world_map,
};

use crate::package_load as pl;

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
