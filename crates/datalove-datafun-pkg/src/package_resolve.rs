//! Package resolution and ModuleGraph conversion.
//!
//! Resolves package dependencies and converts to ModuleGraph for compilation.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet, HashMap};

use bct::package_resolve2::{
    ImportDemandMap,
    PackageWorldModuleGraphWithErrors,
    PackageWorldModuleGraph,
    ResolvedPackageModule,
    ValidationError,
    resolve_package_world,
};
use bct::module_graph::{ModuleGraph, ModuleGraphBuilder};

use crate::package::PackageWorld;

/// Resolve a package world with import demands.
///
/// Caller provides:
/// - `package_world`: The package world with loaded packages
/// - `import_demand_map`: Import demands extracted by the caller (using datafun's parser)
///
/// Returns a result with the resolved module graph.
#[salsa::tracked]
pub fn resolve_package_world_with_imports<'db>(
    db: &'db dyn salsa::Database,
    package_world: PackageWorld,
    import_demand_map: ImportDemandMap<'db>,
) -> PackageWorldModuleGraphWithErrors<'db> {
    let package_world_map = crate::package::package_world_map(db, package_world);
    resolve_package_world(db, package_world_map, import_demand_map)
}

/// Convert a PackageWorldModuleGraph to a ModuleGraph.
///
/// This bridges the package system with the core compiler's module abstraction.
/// Function-level imports are handled by the typechecker, not this conversion.
pub fn to_module_graph(
    db: &dyn salsa::Database,
    package_world: PackageWorld,
    graph: PackageWorldModuleGraph<'_>,
) -> ModuleGraph {
    // Build a mapping from PackageModule to its module path string.
    let mut pkg_module_to_path: HashMap<bct::package2::PackageModule, String> = HashMap::new();

    // Traverse the package world to build paths.
    let world_map = crate::package::package_world_map(db, package_world);
    for (import_space, packages) in world_map.map(db) {
        for (package_name, package) in packages {
            for (module_name, package_module) in package.modules(db) {
                let path = format!("{}/{}/{}", import_space, package_name, module_name);
                pkg_module_to_path.insert(*package_module, path);
            }
        }
    }

    // Topological sort modules.
    let sorted_modules = topological_sort_modules(db, graph)
        .unwrap_or_else(|_| graph.map(db).keys().copied().collect());

    // Build ModuleGraph.
    let mut builder = ModuleGraphBuilder::new(db);

    // Add all modules in dependency order.
    for pkg_module in &sorted_modules {
        let path = pkg_module_to_path.get(pkg_module)
            .cloned()
            .unwrap_or_else(|| pkg_module.name(db).to_string());
        let source = pkg_module.text(db);
        builder.add_module(path, source);
    }

    // Note: Function-level imports are resolved by the typechecker.
    // The ModuleGraph.imports field will be empty here.

    builder.build()
}

/// Topologically sort modules so dependencies come before dependents.
fn topological_sort_modules(
    db: &dyn salsa::Database,
    graph: PackageWorldModuleGraph<'_>,
) -> Result<Vec<bct::package2::PackageModule>, ValidationError> {
    let map = graph.map(db);

    // Build dependency graph.
    let mut dependencies: BTreeMap<bct::package2::PackageModule, BTreeSet<bct::package2::PackageModule>> = BTreeMap::new();

    for (module, deps) in map.iter() {
        let mut module_deps = BTreeSet::new();
        for (_demand, resolved) in deps {
            if let ResolvedPackageModule::Resolved(dep_module) = resolved {
                module_deps.insert(*dep_module);
            }
        }
        dependencies.insert(*module, module_deps);
    }

    // Kahn's algorithm for topological sort.
    let mut in_degree: BTreeMap<bct::package2::PackageModule, usize> = BTreeMap::new();
    for module in map.keys() {
        in_degree.insert(*module, 0);
    }

    for (_module, deps) in &dependencies {
        for dep in deps {
            *in_degree.entry(*dep).or_insert(0) += 0; // Ensure dep is in map.
        }
    }

    for deps in dependencies.values() {
        for dep in deps {
            if let Some(count) = in_degree.get_mut(dep) {
                *count += 1;
            }
        }
    }

    // Wait, we have the deps backwards. dependencies[A] contains B means A depends on B.
    // So in_degree should count how many modules depend ON a module.
    // Let me fix this.

    // Reset and compute correctly.
    in_degree.clear();
    for module in map.keys() {
        in_degree.insert(*module, 0);
    }

    // A depends on B means B must come before A.
    // in_degree[A] = number of modules that A depends on.
    for (module, deps) in &dependencies {
        in_degree.insert(*module, deps.len());
    }

    // Start with modules that have no dependencies (in_degree = 0).
    let mut queue: Vec<bct::package2::PackageModule> = in_degree.iter()
        .filter(|&(_, degree)| *degree == 0)
        .map(|(module, _)| *module)
        .collect();

    let mut sorted = Vec::new();

    while let Some(module) = queue.pop() {
        sorted.push(module);

        // For each module that depends on this one, decrement its in_degree.
        for (dependent, deps) in &dependencies {
            if deps.contains(&module) {
                if let Some(count) = in_degree.get_mut(dependent) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push(*dependent);
                    }
                }
            }
        }
    }

    if sorted.len() != map.len() {
        return Err(ValidationError::CycleDetected);
    }

    Ok(sorted)
}
