//! Incremental module compilation with package resolution.
//!
//! This module re-exports [`IncrementalModuleWorld`] from the compiler and adds
//! package resolution integration via [`extract_dependencies`].
//!
//! # Usage
//!
//! ```ignore
//! let mut world = IncrementalModuleWorld::new();
//!
//! // Add modules.
//! world.add_module(&db, "local/pkg/main", source);
//!
//! // Extract dependencies via package resolution.
//! let path_deps = extract_dependencies(&world, &db);
//!
//! // Initial compilation (only needs &db).
//! let (graph, requires) = world.build_fresh(&db, &path_deps);
//!
//! // Incremental update (needs &mut db for setters).
//! world.update_source(&mut db, "local/pkg/main", new_source);
//! let path_deps = extract_dependencies(&world, &db);
//! let (graph, requires) = world.prepare_for_compile(&mut db, &path_deps);
//! ```

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet};

// Re-export core types from compiler.
pub use datalove_datafun_compiler::incremental::{
    IncrementalModuleWorld,
    topological_sort,
};

/// Extract module dependencies via the package resolution pipeline.
///
/// This function computes the dependency graph for all modules in the world
/// using the package resolution system. The result maps each module path
/// to the set of module paths it depends on.
pub fn extract_dependencies(
    world: &IncrementalModuleWorld,
    db: &dyn salsa::Database,
) -> BTreeMap<String, BTreeSet<String>> {
    use datalove_datafun_pkg::package_load::{Package, PackageModule, PackageWorld};

    // Build raw PackageWorld from the modules.
    let mut pkglib_system = BTreeMap::new();
    let mut pkglib_local = BTreeMap::new();

    for (path, module) in world.modules() {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 3 {
            continue;
        }
        let library = parts[0];
        let package_name = parts[1];
        let module_name = parts[2];

        let pkglib = match library {
            "sys" => &mut pkglib_system,
            "local" => &mut pkglib_local,
            _ => continue,
        };

        let source_text = module.source(db).text(db).C();

        let package = pkglib.entry(package_name.S())
            .or_insert_with(|| Package {
                name: package_name.S(),
                modules: BTreeMap::new(),
            });

        let pkg_module = PackageModule {
            name: module_name.S(),
            path: path.C().into(),
            text: source_text,
        };
        package.modules.insert(module_name.S(), pkg_module);
    }

    let raw_package_world = PackageWorld {
        pkglib_system,
        pkglib_local,
    };

    // Run resolution pipeline.
    let package_world = datalove_datafun_pkg::import_from_loader(db, raw_package_world);
    let resolution = crate::package_resolve::resolve_package_world_with_imports(db, package_world);
    let pkg_graph = match resolution.result(db) {
        Ok(graph) => graph,
        Err(_) => return BTreeMap::new(),
    };

    // Get dependency info.
    let graph_with_requires = datalove_datafun_pkg::to_module_graph(db, package_world, pkg_graph);

    // Convert to path-based dependencies.
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (source_id, requires) in &graph_with_requires.resolved_requires {
        let source_path = source_id.path(db).C();
        let target_paths: BTreeSet<String> = requires.iter()
            .map(|(_, target_id)| target_id.path(db).C())
            .collect();
        deps.insert(source_path, target_paths);
    }
    deps
}
