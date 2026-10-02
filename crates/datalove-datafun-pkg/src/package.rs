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
pub fn import_from_loader<'db>(
    db: &'db dyn salsa::Database,
    package_world_raw: pl::PackageWorld,
) -> PackageWorld<'db> {
    PackageWorld::new(
        db,
        make_library(db, package_world_raw.pkglib_system, Durability::HIGH),
        make_library(db, package_world_raw.pkglib_local, Durability::LOW),
    )
}

/// Build a `PackageWorld` over sources that already exist.
///
/// `import_from_loader` makes a `Source` for every module it is handed, which
/// is right the first time and wrong every time after. A `Source` is an input,
/// so a fresh one is a different input however alike its text, and the
/// `PackageModule`, `Package` and `PackageWorld` built over it are all new in
/// turn - which puts package resolution and everything downstream of it back
/// to square one, and leaves the old inputs behind, since inputs are not
/// collected.
///
/// A caller that already holds its sources - which an incremental rebuild
/// does - passes them here, and the interning on those three types does the
/// rest: same sources, same world, nothing to redo.
pub fn import_with_sources<'db>(
    db: &'db dyn salsa::Database,
    pkglib_system: BTreeMap<PackageName, BTreeMap<ModuleName, Source>>,
    pkglib_local: BTreeMap<PackageName, BTreeMap<ModuleName, Source>>,
) -> PackageWorld<'db> {
    PackageWorld::new(
        db,
        library_from_sources(db, pkglib_system),
        library_from_sources(db, pkglib_local),
    )
}

fn library_from_sources<'db>(
    db: &'db dyn salsa::Database,
    library: BTreeMap<PackageName, BTreeMap<ModuleName, Source>>,
) -> BTreeMap<PackageName, Package<'db>> {
    library.into_iter().map(|(package_name, modules)| {
        let modules: BTreeMap<ModuleName, PackageModule<'db>> = modules.into_iter()
            .map(|(module_name, source)| {
                let module = PackageModule::new(db, module_name.C(), source);
                (module_name, module)
            })
            .collect();
        let package = Package::new(db, package_name.C(), modules);
        (package_name, package)
    }).collect()
}

fn make_library<'db>(
    db: &'db dyn salsa::Database,
    library: BTreeMap<pl::PackageName, pl::Package>,
    durability: Durability,
) -> BTreeMap<PackageName, Package<'db>> {
    library.into_iter().map(|(name, package)| {
        (name, make_package(db, package, durability))
    }).collect()
}

fn make_package<'db>(
    db: &'db dyn salsa::Database,
    package: pl::Package,
    durability: Durability,
) -> Package<'db> {
    Package::new(
        db,
        package.name,
        make_modules(db, package.modules, durability),
    )
}

fn make_modules<'db>(
    db: &'db dyn salsa::Database,
    modules: BTreeMap<pl::ModuleName, pl::PackageModule>,
    durability: Durability,
) -> BTreeMap<ModuleName, PackageModule<'db>> {
    modules.into_iter().map(|(name, module)| {
        (name, make_module(db, module, durability))
    }).collect()
}

fn make_module<'db>(
    db: &'db dyn salsa::Database,
    module: pl::PackageModule,
    durability: Durability,
) -> PackageModule<'db> {
    PackageModule::new(
        db,
        module.name,
        Source::builder(module.text).text_durability(durability).new(db),
    )
}
