//! Incremental module world for salsa memoization.
//!
//! Provides `IncrementalModuleWorld` which manages module identity correctly
//! for incremental compilation with proper salsa memoization.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet};
use bct::input::Source;
use bct::module_graph::{Module, ModuleGraph, ModuleId};
use salsa::Setter;

use datalove_datafun_compiler::module_graph::parse_module_graph;
use datalove_datafun_tycheck::typecheck_module_graph;

/// Manages modules with stable salsa identity for incremental compilation.
///
/// Salsa memoization requires careful identity management:
/// - Module objects must be reused (not recreated) across incremental changes
/// - Source text updates use `set_text` to preserve identity
/// - ModuleGraph is reused and updated via setters only when values change
///
/// This struct encapsulates these requirements, providing a clean API for
/// incremental module management.
pub struct IncrementalModuleWorld {
    /// Module objects keyed by path. Reused for memoization.
    modules: BTreeMap<String, Module>,
    /// Cached ModuleGraph for identity reuse.
    graph: Option<ModuleGraph>,
}

impl IncrementalModuleWorld {
    /// Create a new empty module world.
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
            graph: None,
        }
    }

    /// Add a new module.
    ///
    /// Creates new Source, ModuleId, and Module objects.
    pub fn add_module(&mut self, db: &dyn salsa::Database, path: &str, source: &str) {
        let new_source = Source::new(db, source.to_string());
        let module_id = ModuleId::new(db, path.to_string());
        let module = Module::new(db, module_id, new_source);
        self.modules.insert(path.to_string(), module);
    }

    /// Remove a module.
    pub fn remove_module(&mut self, path: &str) {
        self.modules.remove(path);
    }

    /// Update a module's source text.
    ///
    /// Reuses the existing Module identity and calls `set_text` on its Source.
    /// This is critical for salsa memoization - downstream queries see the
    /// same Module identity and can check if the source actually changed.
    pub fn update_source(&mut self, db: &mut dyn salsa::Database, path: &str, source: &str) {
        if let Some(module) = self.modules.get(path) {
            let existing_source = module.source(db);
            existing_source.set_text(db).to(source.to_string());
        }
    }

    /// Check if a module exists.
    pub fn contains(&self, path: &str) -> bool {
        self.modules.contains_key(path)
    }

    /// Get all module paths.
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.modules.keys()
    }

    /// Get a module by path.
    pub fn get(&self, path: &str) -> Option<Module> {
        self.modules.get(path).copied()
    }

    /// Build a fresh ModuleGraph, storing it for potential later incremental updates.
    ///
    /// This method only needs an immutable database reference since it creates
    /// new salsa objects without using setters. The graph is stored in `self.graph`
    /// so that later calls to `prepare_for_compile` can incrementally update it.
    pub fn build_fresh(
        &mut self,
        db: &dyn salsa::Database,
    ) -> (ModuleGraph, BTreeMap<ModuleId, Vec<(String, ModuleId)>>) {
        // Extract dependencies through the package resolution pipeline.
        let path_deps = self.extract_dependencies(db);

        // Topologically sort modules (dependencies before dependents).
        let all_paths: BTreeSet<String> = self.modules.keys().cloned().collect();
        let sorted_paths = topological_sort(&all_paths, &path_deps);

        // Build module list in dependency order.
        let modules: Vec<Module> = sorted_paths.iter()
            .filter_map(|p| self.modules.get(p).copied())
            .collect();

        // Build module_by_id map.
        let module_by_id: BTreeMap<ModuleId, Module> = self.modules.values()
            .map(|m| (m.id(db), *m))
            .collect();

        // Build dependencies map using our ModuleIds.
        let mut dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>> = BTreeMap::new();
        for (source_path, target_paths) in &path_deps {
            if let Some(source_module) = self.modules.get(source_path) {
                let source_id = source_module.id(db);
                let target_ids: BTreeSet<ModuleId> = target_paths.iter()
                    .filter_map(|p| self.modules.get(p).map(|m| m.id(db)))
                    .collect();
                dependencies.insert(source_id, target_ids);
            }
        }
        // Ensure all modules have an entry.
        for module in &modules {
            dependencies.entry(module.id(db)).or_default();
        }

        // Create fresh graph and store for later incremental use.
        let graph = ModuleGraph::new(db, modules, module_by_id, dependencies);
        self.graph = Some(graph);

        // Build resolved_requires for parse_module_graph.
        let resolved_requires = self.build_resolved_requires(db, &path_deps);

        (graph, resolved_requires)
    }

    /// Prepare for compilation by building/updating the ModuleGraph.
    ///
    /// This method requires a mutable database reference for updating an existing
    /// graph via setters. Use `build_fresh` for one-shot compilation.
    pub fn prepare_for_compile(
        &mut self,
        db: &mut dyn salsa::Database,
    ) -> (ModuleGraph, BTreeMap<ModuleId, Vec<(String, ModuleId)>>) {
        // Extract dependencies through the package resolution pipeline.
        let path_deps = self.extract_dependencies(db);

        // Topologically sort modules (dependencies before dependents).
        let all_paths: BTreeSet<String> = self.modules.keys().cloned().collect();
        let sorted_paths = topological_sort(&all_paths, &path_deps);

        // Build module list in dependency order.
        let modules: Vec<Module> = sorted_paths.iter()
            .filter_map(|p| self.modules.get(p).copied())
            .collect();

        // Build module_by_id map.
        let module_by_id: BTreeMap<ModuleId, Module> = self.modules.values()
            .map(|m| (m.id(db), *m))
            .collect();

        // Build dependencies map using our ModuleIds.
        let mut dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>> = BTreeMap::new();
        for (source_path, target_paths) in &path_deps {
            if let Some(source_module) = self.modules.get(source_path) {
                let source_id = source_module.id(db);
                let target_ids: BTreeSet<ModuleId> = target_paths.iter()
                    .filter_map(|p| self.modules.get(p).map(|m| m.id(db)))
                    .collect();
                dependencies.insert(source_id, target_ids);
            }
        }
        // Ensure all modules have an entry.
        for module in &modules {
            dependencies.entry(module.id(db)).or_default();
        }

        // Create or update the ModuleGraph with stable identity.
        let graph = self.update_graph(db, modules, module_by_id, dependencies);

        // Build resolved_requires for parse_module_graph.
        let resolved_requires = self.build_resolved_requires(db, &path_deps);

        (graph, resolved_requires)
    }

    /// Compile modules: parse and typecheck.
    ///
    /// Convenience method that calls `prepare_for_compile`, `parse_module_graph`,
    /// and `typecheck_module_graph`.
    pub fn compile<'db>(
        &mut self,
        db: &'db mut dyn salsa::Database,
    ) -> CompileResult<'db> {
        let (graph, resolved_requires) = self.prepare_for_compile(db);
        let parsed = parse_module_graph(db, graph, resolved_requires);
        let typechecked = typecheck_module_graph(db, parsed);
        CompileResult { graph, parsed, typechecked }
    }

    /// Get the cached ModuleGraph if it exists.
    pub fn graph(&self) -> Option<ModuleGraph> {
        self.graph
    }

    /// Get transitive dependents of a module.
    pub fn get_dependents(&self, db: &dyn salsa::Database, path: &str) -> BTreeSet<String> {
        let path_deps = self.extract_dependencies(db);

        // Build reverse dependency map.
        let mut dependents_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (source_path, target_paths) in &path_deps {
            for target_path in target_paths {
                dependents_map
                    .entry(target_path.clone())
                    .or_default()
                    .insert(source_path.clone());
            }
        }

        // Transitive closure.
        let mut result = BTreeSet::new();
        let mut queue = vec![path.to_string()];
        while let Some(current) = queue.pop() {
            if let Some(deps) = dependents_map.get(&current) {
                for dep in deps {
                    if result.insert(dep.clone()) {
                        queue.push(dep.clone());
                    }
                }
            }
        }
        result
    }

    // --- Private helpers ---

    /// Update the cached ModuleGraph, only calling setters when values change.
    fn update_graph(
        &mut self,
        db: &mut dyn salsa::Database,
        modules: Vec<Module>,
        module_by_id: BTreeMap<ModuleId, Module>,
        dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>>,
    ) -> ModuleGraph {
        match self.graph {
            Some(g) => {
                // Only update fields if they differ.
                // Calling a setter ALWAYS marks the input as "changed" in salsa,
                // even if the value is equal.
                if g.modules(db) != &modules {
                    g.set_modules(db).to(modules);
                }
                if g.module_by_id(db) != &module_by_id {
                    g.set_module_by_id(db).to(module_by_id);
                }
                if g.dependencies(db) != &dependencies {
                    g.set_dependencies(db).to(dependencies);
                }
                g
            }
            None => {
                let g = ModuleGraph::new(db, modules, module_by_id, dependencies);
                self.graph = Some(g);
                g
            }
        }
    }

    /// Extract dependencies by running through the package resolution pipeline.
    fn extract_dependencies(&self, db: &dyn salsa::Database) -> BTreeMap<String, BTreeSet<String>> {
        use datalove_datafun_pkg::package_load::{Package, PackageModule, PackageWorld};

        // Build raw PackageWorld from our modules.
        let mut pkglib_system = BTreeMap::new();
        let mut pkglib_local = BTreeMap::new();

        for (path, module) in &self.modules {
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

            let source_text = module.source(db).text(db).clone();

            let package = pkglib.entry(package_name.to_string())
                .or_insert_with(|| Package {
                    name: package_name.to_string(),
                    modules: BTreeMap::new(),
                });

            let pkg_module = PackageModule {
                name: module_name.to_string(),
                path: path.clone().into(),
                text: source_text,
            };
            package.modules.insert(module_name.to_string(), pkg_module);
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
            let source_path = source_id.path(db).clone();
            let target_paths: BTreeSet<String> = requires.iter()
                .map(|(_, target_id)| target_id.path(db).clone())
                .collect();
            deps.insert(source_path, target_paths);
        }
        deps
    }

    /// Build resolved_requires for parse_module_graph.
    fn build_resolved_requires(
        &self,
        db: &dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
    ) -> BTreeMap<ModuleId, Vec<(String, ModuleId)>> {
        let mut resolved_requires = BTreeMap::new();

        for (source_path, target_paths) in path_deps {
            if let Some(source_module) = self.modules.get(source_path) {
                let source_id = source_module.id(db);
                let requires: Vec<(String, ModuleId)> = target_paths.iter()
                    .filter_map(|p| {
                        self.modules.get(p).map(|m| {
                            // Use the last component of path as alias.
                            let alias = p.split('/').last().unwrap_or(p).to_string();
                            (alias, m.id(db))
                        })
                    })
                    .collect();
                resolved_requires.insert(source_id, requires);
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

/// Result of compiling modules.
pub struct CompileResult<'db> {
    pub graph: ModuleGraph,
    pub parsed: datalove_datafun_tycheck::ParsedModuleGraph<'db>,
    pub typechecked: datalove_datafun_tycheck::ModuleGraphTypecheckResult<'db>,
}

/// Topologically sort paths by dependencies.
///
/// Returns paths in dependency order: dependencies come before dependents.
fn topological_sort(
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
        visiting.insert(path.to_string());

        // Visit dependencies first.
        if let Some(deps) = path_deps.get(path) {
            for dep in deps {
                visit(dep, path_deps, visited, visiting, result);
            }
        }

        visiting.remove(path);
        visited.insert(path.to_string());
        result.push(path.to_string());
    }

    for path in paths {
        visit(path, path_deps, &mut visited, &mut visiting, &mut result);
    }

    result
}
