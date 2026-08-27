//! Incremental module compilation with correct salsa memoization.
//!
//! Salsa requires careful identity management for memoization to work:
//! - Tracked structs must be reused, not recreated, across incremental updates
//! - Input setters mark values as "changed" even if the value is the same
//! - ModuleGraph setters must be called conditionally to avoid spurious invalidation
//!
//! [`IncrementalModuleWorld`] encapsulates these requirements. It maintains stable
//! Module and ModuleGraph identities across edits, only calling setters when values
//! actually differ.
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
use bct::input::Source;
use bct::module_graph::{Module, ModuleGraph, ModuleId};
use salsa::Setter;

/// Module world with stable salsa identity for incremental compilation.
///
/// `Module` is interned, so holding one here is no longer what keeps its
/// identity stable - building the same module again would give the same
/// handle. The map is a lookup by path, and the cached `ModuleGraph` is the
/// part that still has to be held: it is an input, so a second one would be a
/// different graph.
pub struct IncrementalModuleWorld {
    /// The source of each module, by path.
    ///
    /// A `Source` is an input, so it is the one handle worth holding across an
    /// edit: an edit changes a source's text, not the source. Everything else -
    /// the `ModuleId`, the `Module`, the `ModuleGraph` - is interned and gets
    /// rebuilt on demand, which gives back the same handle and is what the
    /// cached graph used to be for.
    sources: BTreeMap<String, Source>,
}

impl IncrementalModuleWorld {
    /// Create a new empty module world.
    pub fn new() -> Self {
        Self { sources: BTreeMap::new() }
    }

    /// Add a module with the given path and source text.
    pub fn add_module(&mut self, db: &dyn salsa::Database, path: &str, source: &str) {
        self.sources.insert(path.S(), Source::new(db, source.S()));
    }

    /// Remove a module.
    pub fn remove_module(&mut self, path: &str) {
        self.sources.remove(path);
    }

    /// Update a module's source text, preserving its identity for memoization.
    pub fn update_source(&mut self, db: &mut dyn salsa::Database, path: &str, source: &str) {
        if let Some(existing) = self.sources.get(path) {
            existing.set_text(db).to(source.S());
        }
    }

    /// Check if a module exists.
    pub fn contains(&self, path: &str) -> bool {
        self.sources.contains_key(path)
    }

    /// Get all module paths.
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.sources.keys()
    }

    /// The source of each module, by path.
    pub fn sources(&self) -> &BTreeMap<String, Source> {
        &self.sources
    }

    /// The module at a path, built from that path and its source.
    pub fn module(&self, db: &dyn salsa::Database, path: &str) -> Option<Module> {
        let source = *self.sources.get(path)?;
        Some(Module::new(db, ModuleId::new(db, path.S()), source))
    }

    /// Build the `ModuleGraph` and the resolved requires that go with it.
    ///
    /// There is nothing to cache: the graph is interned, so building it again
    /// from unchanged parts gives back the same graph.
    pub fn build_graph(
        &self,
        db: &dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
    ) -> (ModuleGraph, BTreeMap<ModuleId, Vec<(String, ModuleId)>>) {
        // Dependencies before dependents.
        let all_paths: BTreeSet<String> = self.sources.keys().cloned().collect();
        let sorted_paths = topological_sort(&all_paths, path_deps);

        let modules: Vec<Module> = sorted_paths.iter()
            .filter_map(|p| self.module(db, p))
            .collect();

        let module_by_id: BTreeMap<ModuleId, Module> = self.sources.keys()
            .filter_map(|p| self.module(db, p).map(|m| (m.id(db), m)))
            .collect();

        let mut dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>> = BTreeMap::new();
        for (source_path, target_paths) in path_deps {
            if let Some(source_module) = self.module(db, source_path) {
                let target_ids: BTreeSet<ModuleId> = target_paths.iter()
                    .filter_map(|p| self.module(db, p).map(|m| m.id(db)))
                    .collect();
                dependencies.insert(source_module.id(db), target_ids);
            }
        }
        for module in &modules {
            dependencies.entry(module.id(db)).or_default();
        }

        let graph = ModuleGraph::new(db, modules, module_by_id, dependencies);
        (graph, self.build_resolved_requires(db, path_deps))
    }

    /// Build the graph for a first compilation.
    pub fn build_fresh(
        &self,
        db: &dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
    ) -> (ModuleGraph, BTreeMap<ModuleId, Vec<(String, ModuleId)>>) {
        self.build_graph(db, path_deps)
    }

    /// Build the graph for a recompilation.
    pub fn prepare_for_compile(
        &self,
        db: &dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
    ) -> (ModuleGraph, BTreeMap<ModuleId, Vec<(String, ModuleId)>>) {
        self.build_graph(db, path_deps)
    }

    /// Get all modules that transitively depend on the given module.
    ///
    /// The `path_deps` parameter provides pre-computed module dependencies as a map
    /// from source module path to set of dependency module paths.
    pub fn get_dependents(&self, path: &str, path_deps: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
        // Build reverse dependency map.
        let mut dependents_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (source_path, target_paths) in path_deps {
            for target in target_paths {
                dependents_map.entry(target.C()).or_default().insert(source_path.C());
            }
        }

        let mut result = BTreeSet::new();
        let mut queue = vec![path.S()];
        while let Some(current) = queue.pop() {
            if let Some(deps) = dependents_map.get(&current) {
                for dep in deps {
                    if result.insert(dep.C()) {
                        queue.push(dep.C());
                    }
                }
            }
        }
        result
    }

    /// Build the resolved_requires map needed by parse_module_graph.
    fn build_resolved_requires(
        &self,
        db: &dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
    ) -> BTreeMap<ModuleId, Vec<(String, ModuleId)>> {
        let mut resolved_requires = BTreeMap::new();

        for (source_path, target_paths) in path_deps {
            if let Some(source_module) = self.module(db, source_path) {
                let requires: Vec<(String, ModuleId)> = target_paths.iter()
                    .filter_map(|p| {
                        self.module(db, p).map(|m| {
                            // Use the last component of path as alias.
                            let alias = p.split('/').last().unwrap_or(p).to_string();
                            (alias, m.id(db))
                        })
                    })
                    .collect();
                resolved_requires.insert(source_module.id(db), requires);
            }
        }

        resolved_requires
    }
}

impl Default for IncrementalModuleWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// Sort paths so dependencies come before dependents.
pub fn topological_sort(
    paths: &BTreeSet<String>,
    path_deps: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut visited = BTreeSet::new();
    let mut visiting = BTreeSet::new();

    fn visit(
        path: &str,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
        visited: &mut BTreeSet<String>,
        visiting: &mut BTreeSet<String>,
        result: &mut Vec<String>,
    ) {
        if visited.contains(path) {
            return;
        }
        if visiting.contains(path) {
            // Cycle detected; skip to avoid infinite loop.
            return;
        }
        visiting.insert(path.S());

        // Visit dependencies first.
        if let Some(deps) = path_deps.get(path) {
            for dep in deps {
                visit(dep, path_deps, visited, visiting, result);
            }
        }

        visiting.remove(path);
        visited.insert(path.S());
        result.push(path.S());
    }

    for path in paths {
        visit(path, path_deps, &mut visited, &mut visiting, &mut result);
    }

    result
}

/// Extract module dependencies via the package resolution pipeline.
///
/// This function computes the dependency graph for all modules in the world
/// using the package resolution system. The result maps each module path
/// to the set of module paths it depends on.
pub fn extract_dependencies(
    world: &IncrementalModuleWorld,
    db: &dyn salsa::Database,
) -> BTreeMap<String, BTreeSet<String>> {
    // Pass the sources the world already holds. Reading their text out and
    // handing it to `import_from_loader` would make a second `Source` for
    // every module on every call, and a `Source` is an input, so that is a
    // whole new world each time and nothing downstream of resolution can be
    // reused - even when nothing has been edited at all.
    let mut pkglib_system: BTreeMap<String, BTreeMap<String, Source>> = BTreeMap::new();
    let mut pkglib_local: BTreeMap<String, BTreeMap<String, Source>> = BTreeMap::new();

    for (path, source) in world.sources() {
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

        pkglib.entry(package_name.S())
            .or_default()
            .insert(module_name.S(), *source);
    }

    // Run resolution pipeline.
    let package_world = datalove_datafun_pkg::import_with_sources(db, pkglib_system, pkglib_local);
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
