//! High-level package resolution for datafun.
//!
//! This module combines import_demands extraction (from compiler's parser)
//! with package resolution (from pkg crate).

use rmx::prelude::*;

use bct::package_resolve2::PackageWorldModuleGraphWithErrors;
use datalove_datafun_pkg::{PackageWorld, package_world_map};

/// Resolve a package world with automatic import demand extraction.
///
/// This is the high-level API for callers. It:
/// 1. Creates a PackageWorldMap from the PackageWorld
/// 2. Extracts import demands using the compiler's parser
/// 3. Resolves dependencies using pkg's resolver
#[salsa::tracked(returns(copy))]
pub fn resolve_package_world_with_imports<'db>(
    db: &'db dyn salsa::Database,
    package_world: PackageWorld<'db>,
) -> PackageWorldModuleGraphWithErrors<'db> {
    let map = package_world_map(db, package_world);
    let demands = crate::import_demands::import_demands(db, map);
    datalove_datafun_pkg::resolve_package_world_with_imports(db, package_world, demands)
}
