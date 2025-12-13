#![allow(unused)]

use rmx::prelude::*;

pub mod package_load_worldfile;
pub mod import_demands;
pub mod package_resolve;

// Re-export key types from datafun for convenience.
pub use datalove_datafun::package::{PackageWorld, import_from_loader, package_world_map};
pub use datalove_datafun::package_load::{self, PackageWorldConfig};

pub use package_resolve::resolve_package_world_with_imports;
