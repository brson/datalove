//! Package loading and resolution for datafun.
//!
//! This crate provides package loading from the filesystem and worldfiles,
//! plus resolution of package dependencies to produce a ModuleGraph.
//!
//! This crate does NOT depend on datafun - it only depends on bct.
//! The caller uses datafun's parser to extract import demands, then passes
//! them to this crate for resolution.


pub mod package_load;
pub mod package_load_worldfile;
pub mod package;
pub mod package_resolve;

// Re-export key types.
pub use datalove_pkg_manifest as manifest;
pub use package_load::{LOCAL_LIBRARY, PackageWorldConfig, SYSTEM_LIBRARY, load_world};
pub use package::{
    Package, PackageModule, PackageName, ModuleName,
    PackageWorld, import_from_loader, import_with_sources, library_durability,
};
pub use package_resolve::{resolve_package_world_with_imports, to_module_graph, ModuleGraphWithRequires};
