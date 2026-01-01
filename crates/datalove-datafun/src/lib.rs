//! Datafun language support.
//!
//! This crate re-exports both the compiler and package system,
//! providing bridge modules that connect them.



// Re-export compiler.
pub use datalove_datafun_compiler::*;

// Re-export pkg.
pub use datalove_datafun_pkg::{
    Package, PackageModule, PackageName, ModuleName,
    PackageWorld, PackageWorldConfig, load_world,
    package_world_map, import_from_loader,
    to_module_graph,
    // Re-export submodules for access to types.
    package, package_load, package_load_worldfile,
};

// Bridge modules.
pub mod import_demands;
pub mod package_resolve;
pub mod worldfile_analysis;
pub mod worldfile_analysis_modules;
pub mod worldfile_pipeline_ir3;
pub mod worldfile_analysis_modules_ir3;
pub mod worldfile_analysis_ir3;
