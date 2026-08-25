//! Package types with Salsa integration.
//!
//! Re-exports bct types and provides conversion from loader types.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
use salsa::Durability;
pub use bct::package2::{
    Package, PackageModule, PackageName, ModuleName,
    PackageWorld, package_world_map,
};

use crate::package_load as pl;

/// Convert loaded packages to Salsa types.
///
/// The system library is the standard library, which is read once and does not
/// change while the session runs, so its sources are marked high durability.
/// Editing a local module then leaves salsa able to skip revalidating anything
/// that depends only on the system library, which is most of the compiler's
/// view of `sys/std`.
pub fn import_from_loader(
    db: &dyn salsa::Database,
    package_world_raw: pl::PackageWorld,
) -> PackageWorld {
    PackageWorld::new(
        db,
        make_library(db, package_world_raw.pkglib_system, Durability::HIGH),
        make_library(db, package_world_raw.pkglib_local, Durability::LOW),
    )
}

fn make_library(
    db: &dyn salsa::Database,
    library: BTreeMap<pl::PackageName, pl::Package>,
    durability: Durability,
) -> BTreeMap<PackageName, Package> {
    library.into_iter().map(|(name, package)| {
        (name, make_package(db, package, durability))
    }).collect()
}

fn make_package(
    db: &dyn salsa::Database,
    package: pl::Package,
    durability: Durability,
) -> Package {
    Package::new(
        db,
        package.name,
        make_modules(db, package.modules, durability),
    )
}

fn make_modules(
    db: &dyn salsa::Database,
    modules: BTreeMap<pl::ModuleName, pl::PackageModule>,
    durability: Durability,
) -> BTreeMap<ModuleName, PackageModule> {
    modules.into_iter().map(|(name, module)| {
        (name, make_module(db, module, durability))
    }).collect()
}

fn make_module(
    db: &dyn salsa::Database,
    module: pl::PackageModule,
    durability: Durability,
) -> PackageModule {
    PackageModule::new(
        db,
        module.name,
        Source::builder(module.text).text_durability(durability).new(db),
    )
}
