//! Package resolution and ModuleGraph conversion.
//!
//! Resolves package dependencies and converts to ModuleGraph for compilation.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};

use bct::package_resolve2::{
    ImportDemandMap,
    PackageWorldModuleGraphWithErrors,
    PackageWorldModuleGraph,
    ResolvedPackageModule,
    ValidationError,
    resolve_package_world,
};
use bct::lookup::{LookupMap, LookupSet};
use bct::module_graph::{ModuleGraph, ModuleGraphBuilder, ModuleId};
use bct::package2::PackageModule;

use crate::package::PackageWorld;

/// Resolve a package world with import demands.
///
/// Caller provides:
/// - `package_world`: The package world with loaded packages
/// - `import_demand_map`: Import demands extracted by the caller (using datafun's parser)
///
/// Returns a result with the resolved module graph.
#[salsa::tracked(returns(copy))]
pub fn resolve_package_world_with_imports<'db>(
    db: &'db dyn salsa::Database,
    package_world: PackageWorld<'db>,
    import_demand_map: ImportDemandMap<'db>,
) -> PackageWorldModuleGraphWithErrors<'db> {
    let package_world_map = crate::package::package_world_map(db, package_world);
    resolve_package_world(db, package_world_map, import_demand_map)
}

/// Result of converting PackageWorldModuleGraph to ModuleGraph.
pub struct ModuleGraphWithRequires<'db> {
    /// The module graph with modules in dependency order.
    pub graph: ModuleGraph<'db>,
    /// Resolved require aliases per module: (alias, target_module_id).
    pub resolved_requires: LookupMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
}

/// Convert a PackageWorldModuleGraph to a ModuleGraph with resolved requires.
///
/// This bridges the package system with the core compiler's module abstraction.
/// Returns the module graph plus resolved require aliases for use by the typechecker.
pub fn to_module_graph<'db>(
    db: &'db dyn salsa::Database,
    package_world: PackageWorld<'db>,
    graph: PackageWorldModuleGraph<'db>,
) -> ModuleGraphWithRequires<'db> {
    // Build a mapping from PackageModule to its module path string.
    let mut pkg_module_to_path: LookupMap<PackageModule<'db>, String> = LookupMap::new();

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

    let path_of = |module: PackageModule<'db>| pkg_module_to_path.get(&module)
        .cloned()
        .unwrap_or_else(|| module.name(db).to_string());

    // Topological sort modules.
    let sorted_modules = topological_sort_modules(db, graph, &path_of)
        .unwrap_or_else(|_| graph.map(db).sorted_by_key(|module| path_of(*module)).into_iter().map(|(module, _)| *module).collect());

    // Build ModuleGraph and collect path → ModuleId mapping.
    let mut builder = ModuleGraphBuilder::new(db);
    let mut path_to_module_id: HashMap<String, ModuleId<'db>> = HashMap::new();

    for pkg_module in &sorted_modules {
        let path = path_of(*pkg_module);
        let source = pkg_module.text(db);
        let module_id = builder.add_module(path.clone(), source);
        path_to_module_id.insert(path, module_id);
    }

    let module_graph = builder.build();

    // Extract resolved requires from PackageWorldModuleGraph.
    let mut resolved_requires: LookupMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>> = LookupMap::new();

    for (pkg_module, demands) in graph.map(db).iter_unordered() {
        let source_path = pkg_module_to_path.get(pkg_module);
        let source_module_id = source_path.and_then(|p| path_to_module_id.get(p));

        if let (Some(_source_path), Some(&source_id)) = (source_path, source_module_id) {
            let mut requires = Vec::new();

            for (demand, resolved) in demands {
                // demand is (ImportSpace, PackageAlias, ModuleAlias).
                // The module alias is the third element.
                let (_import_space, _package_alias, module_alias) = demand;

                if let ResolvedPackageModule::Resolved(target_pkg_module) = resolved {
                    // Get the target module's path and ModuleId.
                    if let Some(target_path) = pkg_module_to_path.get(target_pkg_module) {
                        if let Some(&target_id) = path_to_module_id.get(target_path) {
                            requires.push((module_alias.clone(), target_id));
                        }
                    }
                }
            }

            if !requires.is_empty() {
                resolved_requires.insert(source_id, requires);
            }
        }
    }

    ModuleGraphWithRequires {
        graph: module_graph,
        resolved_requires,
    }
}

/// Topologically sort modules so dependencies come before dependents.
///
/// Of the modules ready at each step the one first by path goes next, so the
/// order is the world's and not the order the modules happened to be interned.
fn topological_sort_modules<'db>(
    db: &'db dyn salsa::Database,
    graph: PackageWorldModuleGraph<'db>,
    path_of: &impl Fn(PackageModule<'db>) -> String,
) -> Result<Vec<PackageModule<'db>>, ValidationError> {
    let map = graph.map(db);

    // How many modules each still waits on, and who waits on it.
    let mut waiting_on: LookupMap<PackageModule<'db>, usize> = LookupMap::new();
    let mut dependents: LookupMap<PackageModule<'db>, Vec<PackageModule<'db>>> = LookupMap::new();
    for (module, demands) in map.iter_unordered() {
        let deps: LookupSet<PackageModule<'db>> = demands.values()
            .filter_map(|resolved| match resolved {
                ResolvedPackageModule::Resolved(dep) => Some(*dep),
                ResolvedPackageModule::Unresolved => None,
            })
            .collect();
        waiting_on.insert(*module, deps.len());
        for dep in deps.into_iter_unordered() {
            dependents.entry(dep).or_default().push(*module);
        }
    }

    let mut ready: BTreeMap<String, PackageModule<'db>> = waiting_on.iter_unordered()
        .filter(|&(_, waiting)| *waiting == 0)
        .map(|(module, _)| (path_of(*module), *module))
        .collect();

    let mut sorted = Vec::new();
    while let Some((_, module)) = ready.pop_first() {
        sorted.push(module);
        for dependent in dependents.get(&module).into_iter().flatten() {
            let waiting = waiting_on.get_mut(dependent).expect("every dependent is a module of the graph");
            *waiting -= 1;
            if *waiting == 0 {
                ready.insert(path_of(*dependent), *dependent);
            }
        }
    }

    if sorted.len() != map.len() {
        return Err(ValidationError::CycleDetected);
    }

    Ok(sorted)
}
