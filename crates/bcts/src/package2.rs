use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use crate::input::Source;
use crate::package_resolve2::PackageWorldMap;

pub type PackageName = String;
pub type ModuleName = String;

/// A package: a name and the modules under it.
///
/// Interned; see `Module` for why these are not inputs.
#[salsa::interned(revisions = usize::MAX, unsafe(no_lifetime))]
#[derive(Debug)]
pub struct Package {
    #[returns(ref)]
    pub name: PackageName,
    #[returns(ref)]
    pub modules: BTreeMap<ModuleName, PackageModule>,
}

#[salsa::interned(revisions = usize::MAX, unsafe(no_lifetime))]
#[derive(Debug, Ord, PartialOrd)]
pub struct PackageModule {
    #[returns(ref)]
    pub name: ModuleName,
    #[returns(copy)]
    pub text: Source,
}

/// A package world containing system and local package libraries.
///
/// Interned, so recompiling an unchanged world is recognised as the same
/// world rather than a new one that happens to hold the same packages.
#[salsa::interned(revisions = usize::MAX, unsafe(no_lifetime))]
#[derive(Debug)]
pub struct PackageWorld {
    #[returns(ref)]
    pub pkglib_system: BTreeMap<PackageName, Package>,
    #[returns(ref)]
    pub pkglib_local: BTreeMap<PackageName, Package>,
}

/// Create a PackageWorldMap from a PackageWorld.
#[salsa::tracked(returns(copy))]
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

