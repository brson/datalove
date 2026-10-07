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
//! // Build the graph. `Roots::All` compiles the whole world; `Roots::From`
//! // compiles only what those modules reach by `require`.
//! let (graph, requires) = world.build_graph(&db, &path_deps, &Roots::All);
//!
//! // Incremental update (needs &mut db for the setter).
//! world.update_source(&mut db, "local/pkg/main", new_source);
//! let path_deps = extract_dependencies(&world, &db);
//! let (graph, requires) = world.build_graph(&db, &path_deps, &Roots::All);
//! ```

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet};
use bct::input::Source;
use bct::module_graph::{Module, ModuleGraph, ModuleId};
use salsa::Setter;

/// Which of the world's modules a compilation is about.
///
/// The world is everything a worldfile mentions, which for any program that
/// uses the system library is two dozen modules it may touch none of. What
/// decides whether a module is compiled is whether something reaches it by
/// `require`, and that is a property of the roots rather than of the world.
///
/// **This is a parameter and not a mode, deliberately.** `All` is what every
/// existing caller wants and is bit-for-bit what the compiler did before
/// there was a choice, so the two paths are one path with a different
/// argument rather than two that drift. `ModuleGraph` is interned, so the
/// roots become part of the graph's identity for free and switching between
/// them cannot poison a memo.
///
/// Reachability changes what is an *error*: a type error in a module nothing
/// requires stops being one. That is why `All` stays the default and why
/// checking a whole library wants it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Roots {
    /// Every module in the world.
    All,
    /// Only the modules these reach, by transitive `require`, and these.
    From(BTreeSet<String>),
}

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
    /// The source of each data file, by path.
    ///
    /// Kept apart from the modules: data takes no part in resolution and has
    /// no requires, and a module and a data file may share a path.
    data: BTreeMap<String, Source>,
}

impl IncrementalModuleWorld {
    /// Create a new empty module world.
    pub fn new() -> Self {
        Self { sources: BTreeMap::new(), data: BTreeMap::new() }
    }

    /// Add a module with the given path and source text.
    pub fn add_module(&mut self, db: &dyn salsa::Database, path: &str, source: &str) {
        self.add_module_with_durability(db, path, source, salsa::Durability::LOW);
    }

    /// The same, saying how likely the text is to change.
    ///
    /// The system library is read once and does not change while a session
    /// runs. Salsa records the last revision at which each durability level
    /// changed, so an edit to a low-durability input lets it skip revalidating
    /// anything reached only through high-durability ones.
    pub fn add_module_with_durability(
        &mut self,
        db: &dyn salsa::Database,
        path: &str,
        source: &str,
        durability: salsa::Durability,
    ) {
        self.sources.insert(
            path.S(),
            Source::builder(source.S()).text_durability(durability).new(db),
        );
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

    /// Add a data file with the given path and text.
    pub fn add_data_with_durability(
        &mut self,
        db: &dyn salsa::Database,
        path: &str,
        text: &str,
        durability: salsa::Durability,
    ) {
        self.data.insert(
            path.S(),
            Source::builder(text.S()).text_durability(durability).new(db),
        );
    }

    /// Remove a data file.
    pub fn remove_data(&mut self, path: &str) {
        self.data.remove(path);
    }

    /// Update a data file's text, preserving its identity for memoization.
    pub fn update_data(&mut self, db: &mut dyn salsa::Database, path: &str, text: &str) {
        if let Some(existing) = self.data.get(path) {
            existing.set_text(db).to(text.S());
        }
    }

    /// The source of each data file, by path.
    pub fn data(&self) -> &BTreeMap<String, Source> {
        &self.data
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
    pub fn module<'db>(&self, db: &'db dyn salsa::Database, path: &str) -> Option<Module<'db>> {
        let source = *self.sources.get(path)?;
        Some(Module::new(db, ModuleId::new(db, path.S()), source))
    }

    /// Build the `ModuleGraph` and the resolved requires that go with it.
    ///
    /// There is nothing to cache: the graph is interned, so building it again
    /// from unchanged parts gives back the same graph.
    pub fn build_graph<'db>(
        &self,
        db: &'db dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
        roots: &Roots,
    ) -> (ModuleGraph<'db>, BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>) {
        let paths = self.paths_in_scope(path_deps, roots);

        // Dependencies before dependents.
        let sorted_paths = topological_sort(&paths, path_deps);

        let modules: Vec<Module> = sorted_paths.iter()
            .filter_map(|p| self.module(db, p))
            .collect();

        let module_by_id: BTreeMap<ModuleId<'db>, Module> = paths.iter()
            .filter_map(|p| self.module(db, p).map(|m| (m.id(db), m)))
            .collect();

        let mut dependencies: BTreeMap<ModuleId<'db>, BTreeSet<ModuleId<'db>>> = BTreeMap::new();
        for (source_path, target_paths) in path_deps {
            if !paths.contains(source_path) {
                continue;
            }
            if let Some(source_module) = self.module(db, source_path) {
                let target_ids: BTreeSet<ModuleId<'db>> = target_paths.iter()
                    .filter(|p| paths.contains(*p))
                    .filter_map(|p| self.module(db, p).map(|m| m.id(db)))
                    .collect();
                dependencies.insert(source_module.id(db), target_ids);
            }
        }
        for module in &modules {
            dependencies.entry(module.id(db)).or_default();
        }

        let graph = ModuleGraph::new(db, modules, module_by_id, dependencies, self.data.clone());
        (graph, self.build_resolved_requires(db, path_deps, &paths))
    }

    /// The modules the roots reach, or all of them.
    ///
    /// A root naming a path the world does not have is ignored rather than
    /// refused: the roots come from what a program requires, and a require that
    /// resolves to nothing is a diagnostic from resolution, not a reason to
    /// build no graph.
    fn paths_in_scope(
        &self,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
        roots: &Roots,
    ) -> BTreeSet<String> {
        let roots = match roots {
            Roots::All => return self.sources.keys().cloned().collect(),
            Roots::From(roots) => roots,
        };

        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut queue: Vec<String> = roots.iter()
            .filter(|p| self.sources.contains_key(*p))
            .cloned()
            .collect();
        while let Some(path) = queue.pop() {
            if !reached.insert(path.C()) {
                continue;
            }
            if let Some(deps) = path_deps.get(&path) {
                queue.extend(deps.iter().filter(|p| self.sources.contains_key(*p)).cloned());
            }
        }
        reached
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
    fn build_resolved_requires<'db>(
        &self,
        db: &'db dyn salsa::Database,
        path_deps: &BTreeMap<String, BTreeSet<String>>,
        paths: &BTreeSet<String>,
    ) -> BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>> {
        let mut resolved_requires = BTreeMap::new();

        for (source_path, target_paths) in path_deps {
            if !paths.contains(source_path) {
                continue;
            }
            if let Some(source_module) = self.module(db, source_path) {
                let requires: Vec<(String, ModuleId<'db>)> = target_paths.iter()
                    .filter(|p| paths.contains(*p))
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

/// The dependency graph of the modules `roots` reach, by path.
///
/// **This follows `require`s from the roots rather than reading every module.**
/// Working out what a module requires means parsing it, so resolution over the
/// whole world is a parse of the whole world -- 8.45ms of an 8.58ms package
/// resolution on the system library, and 93% of a compile already pruned to two
/// modules. Reachability is a walk, and a walk need only parse what it reaches.
///
/// `Roots::All` walks from every module, which reaches every module, so it is the
/// whole-world answer by the same code rather than by a second path.
pub fn extract_dependencies<'db>(
    world: &IncrementalModuleWorld,
    db: &'db dyn salsa::Database,
    roots: &Roots,
) -> &'db BTreeMap<String, BTreeSet<String>> {
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

    let package_world = datalove_datafun_pkg::import_with_sources(db, pkglib_system, pkglib_local);
    let roots = match roots {
        Roots::All => CompileRoots::new(db, None),
        Roots::From(paths) => CompileRoots::new(db, Some(paths.iter().cloned().collect())),
    };
    dependencies_from_roots(db, package_world, roots)
}

/// A root set, as something a query can be keyed on.
///
/// Interned, so the same roots give the same handle and an unchanged recompile
/// asks the same question. `None` is every module in the world.
#[salsa::interned]
pub struct CompileRoots<'db> {
    #[returns(ref)]
    pub paths: Option<Vec<String>>,
}

/// Walk `require`s from the roots, parsing only what is reached.
///
/// Tracked and keyed on the world and the roots, both interned, so an unchanged
/// recompile takes this from the memo rather than walking again.
///
/// A module's requires come from `module_import_demands`, which is keyed on the
/// `Module` and so shares phase 1's parse -- reaching a module here is what makes
/// phase 1's parse of it a memo hit later.
///
/// **An unresolvable require is dropped, which is what resolution already did**:
/// `resolve_package_world` records one as `ResolvedPackageModule::Unresolved` and
/// `to_module_graph` leaves it out of the requires. The error is the
/// typechecker's to report, from the import that has nothing to resolve against.
///
/// A cycle is broken by dropping the require that closes it; see
/// [`break_cycles`]. A cycle among modules the roots do not reach is not seen at
/// all, which follows from reachability.
#[salsa::tracked(returns(ref))]
fn dependencies_from_roots<'db>(
    db: &'db dyn salsa::Database,
    package_world: datalove_datafun_pkg::PackageWorld<'db>,
    roots: CompileRoots<'db>,
) -> BTreeMap<String, BTreeSet<String>> {
    let world_map = datalove_datafun_pkg::package_world_map(db, package_world);

    // Every module the world has, by the path a `require` names it with. Built
    // from the package world rather than parsed, so this is a few string
    // formats -- 0.01ms for the system library.
    let by_path: BTreeMap<String, Module<'db>> = world_map.flatten_iter(db)
        .map(|record| {
            let path = format!(
                "{}/{}/{}",
                record.import_space, record.package_name, record.package_module.name(db),
            );
            let module = Module::new(
                db, ModuleId::new(db, path.clone()), record.package_module.text(db));
            (path, module)
        })
        .collect();

    let start: Vec<String> = match roots.paths(db) {
        None => by_path.keys().cloned().collect(),
        Some(paths) => paths.iter().filter(|p| by_path.contains_key(*p)).cloned().collect(),
    };

    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut queue = start;
    while let Some(path) = queue.pop() {
        if deps.contains_key(&path) {
            continue;
        }
        let module = *by_path.get(&path).expect("the queue only holds paths the world has");
        let targets: BTreeSet<String> = crate::import_demands::module_import_demands(db, module)
            .iter()
            .map(|(space, package, module)| format!("{}/{}/{}", space, package, module))
            .filter(|target| by_path.contains_key(target))
            .collect();
        queue.extend(targets.iter().cloned());
        deps.insert(path, targets);
    }

    break_cycles(&mut deps);
    deps
}

/// Drop each require that closes a cycle, which a program may not have.
///
/// The graph left is acyclic and still reaches every module, so the rest of
/// the program compiles and typecheck reports the cycle on the require that
/// was dropped (F083): it names a module that is in the graph and yet did not
/// resolve. Which require of a cycle that is depends on the walk, which starts
/// from each module in path order.
///
/// This used to give up on the whole graph, an empty map, so a program with a
/// cycle anywhere had every module's requires go unresolved and said only
/// that nothing was required under each alias called through.
fn break_cycles(deps: &mut BTreeMap<String, BTreeSet<String>>) {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark { Open, Done }

    fn visit(
        path: &str,
        deps: &BTreeMap<String, BTreeSet<String>>,
        marks: &mut BTreeMap<String, Mark>,
        closing: &mut Vec<(String, String)>,
    ) {
        if marks.contains_key(path) {
            return;
        }
        marks.insert(path.S(), Mark::Open);
        if let Some(targets) = deps.get(path) {
            for target in targets {
                match marks.get(target.as_str()) {
                    Some(Mark::Open) => closing.push((path.S(), target.C())),
                    Some(Mark::Done) => {}
                    None => visit(target, deps, marks, closing),
                }
            }
        }
        marks.insert(path.S(), Mark::Done);
    }

    let mut marks: BTreeMap<String, Mark> = BTreeMap::new();
    let mut closing = Vec::new();
    for path in deps.keys() {
        visit(path, deps, &mut marks, &mut closing);
    }
    for (from, to) in closing {
        deps.get_mut(&from).expect("a closing require starts at a walked module").remove(&to);
    }
}

/// The dependency graph of an already-imported package world.
///
/// Tracked and keyed on the world, which is interned, so the same sources give
/// the same world and this is answered from the memo. Resolution below it was
/// already tracked; what was not is `to_module_graph` and the walk after it,
/// which builds a path string for every module and every edge between them --
/// on every compile, including one where nothing had been edited.
#[salsa::tracked(returns(ref))]
fn dependencies_of<'db>(
    db: &'db dyn salsa::Database,
    package_world: datalove_datafun_pkg::PackageWorld<'db>,
) -> BTreeMap<String, BTreeSet<String>> {
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
