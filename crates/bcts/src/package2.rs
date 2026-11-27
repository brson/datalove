use rmx::std::collections::BTreeMap;

use crate::input::Source;
use crate::package_resolve2::ImportSpace;

pub type PackageName = String;
pub type ModuleName = String;

/// A package: a name and the modules under it.
///
/// Interned; see `Module` for why these are not inputs.
#[salsa::interned]
#[derive(Debug)]
pub struct Package<'db> {
    #[returns(ref)]
    pub name: PackageName,
    #[returns(ref)]
    pub modules: BTreeMap<ModuleName, PackageModule<'db>>,
}

/// A module of a package: its name and its source.
///
/// The derived `Ord` compares salsa ids, the order of first interning; see
/// `ModuleId`.
#[salsa::interned]
#[derive(Debug)]
pub struct PackageModule<'db> {
    #[returns(ref)]
    pub name: ModuleName,
    #[returns(copy)]
    pub text: Source,
}

/// The libraries of packages a compile can require from.
///
/// A library is an import space of its own: `require module lib/pkg/mod`
/// names package `pkg` of library `lib`. Which libraries there are, and what
/// any of them means, is the caller's.
///
/// Interned, so recompiling an unchanged world is recognised as the same
/// world rather than a new one that happens to hold the same packages.
#[salsa::interned]
#[derive(Debug)]
pub struct PackageWorld<'db> {
    #[returns(ref)]
    pub libraries: BTreeMap<ImportSpace, BTreeMap<PackageName, Package<'db>>>,
}

