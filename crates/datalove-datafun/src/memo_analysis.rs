//! Memoization analysis for module compilation.
//!
//! Analyzes Salsa memoization behavior by processing worldfile modules
//! incrementally and tracking parse/typecheck/hash changes after each step.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use rmx::std::collections::BTreeSet;
use rmx::std::hash::{Hash, Hasher};
use rmx::std::collections::hash_map::DefaultHasher;
use serde::{Serialize, Deserialize};
use salsa::Setter;

use bct::input::Source;
use bct::module_graph::Module;
use datalove_datafun_compiler::Database;
use datalove_datafun_compiler::module_graph::{
    ModuleGraph, ModuleId,
    parse_module_graph,
};
use datalove_ct::query_log::{enable_query_logging, disable_query_logging, get_executed_modules};
use datalove_datafun_tycheck::typecheck_module_graph;
use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;
use datalove_datafun_pkg::package_load::{PackageWorld as RawPackageWorld, Package, PackageModule};

/// Action types for memoization testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// Add a new module (initial load).
    Add,
    /// Add a new module after initial load.
    ModuleAdd,
    /// Remove an existing module.
    ModuleRemove,
    /// Change module with whitespace-only changes.
    ModuleChangeWs,
    /// Change module with AST changes but same types.
    ModuleChangeAst,
    /// Change module with type-level changes.
    ModuleChangeTy,
}

impl Action {
    fn as_str(&self) -> &'static str {
        match self {
            Action::Add => "add",
            Action::ModuleAdd => "module-add",
            Action::ModuleRemove => "module-remove",
            Action::ModuleChangeWs => "module-change-ws",
            Action::ModuleChangeAst => "module-change-ast",
            Action::ModuleChangeTy => "module-change-ty",
        }
    }
}

/// Complete memoization analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoAnalysis {
    pub steps: Vec<StepResult>,
    pub summary: Summary,
}

/// Result of processing one worldfile section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub action: String,
    pub module: String,
    pub source_hash: String,
    pub results: BTreeMap<String, ModuleResult>,
}

/// Per-module result within a step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleResult {
    pub is_direct: bool,
    pub is_dependent: bool,
    pub parsed: bool,
    pub parse_ok: bool,
    pub typechecked: bool,
    pub typecheck_ok: bool,
    pub hash_changed: bool,
    /// Expected memoization behavior, or None if not applicable
    /// (e.g., dependents of removed modules where the module graph can't resolve).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<ExpectedBehavior>,
    /// Whether actual matches expected. Always true when expected is None.
    pub correct: bool,
}

/// Expected behavior based on memoization rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedBehavior {
    pub parsed: bool,
    pub typechecked: bool,
    pub hash_changed: bool,
}

/// Summary of analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_steps: usize,
    pub all_correct: bool,
}

/// Internal state for tracking modules across steps.
///
/// Stores salsa input objects (`Module`, which contains `ModuleId` and `Source`)
/// so they can be reused across incremental steps. This is critical for salsa
/// memoization to work - reusing the same object identity allows salsa to
/// detect what actually changed and cache appropriately.
struct MemoState {
    /// Module objects keyed by module path.
    ///
    /// These are salsa inputs that must be reused (not recreated) for
    /// memoization to work. When a module is redefined, we mutate the
    /// underlying Source's text via `set_text()`.
    modules: BTreeMap<String, Module>,
    /// Raw module sources keyed by path.
    raw_sources: BTreeMap<String, String>,
    /// Previous content hashes.
    prev_hashes: BTreeMap<String, u64>,
    /// Reverse dependency graph: module -> modules that depend on it.
    dependents: BTreeMap<String, BTreeSet<String>>,
}

impl MemoState {
    fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
            raw_sources: BTreeMap::new(),
            prev_hashes: BTreeMap::new(),
            dependents: BTreeMap::new(),
        }
    }

    /// Get all dependents of a module (transitive).
    fn get_dependents(&self, path: &str) -> BTreeSet<String> {
        let mut result = BTreeSet::new();
        let mut queue = vec![path.to_string()];
        while let Some(current) = queue.pop() {
            if let Some(deps) = self.dependents.get(&current) {
                for dep in deps {
                    if result.insert(dep.clone()) {
                        queue.push(dep.clone());
                    }
                }
            }
        }
        result
    }
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

/// Hash a string using DefaultHasher.
fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Parsed worldfile section with action type.
struct ParsedSection {
    action: Action,
    path: String,
    #[allow(dead_code)]
    library: String,
    #[allow(dead_code)]
    package: String,
    #[allow(dead_code)]
    module: String,
    source: Option<String>,
}

/// Parse worldfile content into sections.
fn parse_sections(content: &str) -> AnyResult<Vec<ParsedSection>> {
    let reader = std::io::Cursor::new(content);
    let parsed = datalove_datafun_pkg::package_load_worldfile::parse_worldfile_sections(reader)?;

    let mut sections = Vec::new();
    for section in parsed.sections {
        let parsed_section = match section {
            WorldfileSection::Module { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection {
                    action: Action::Add,
                    path,
                    library,
                    package,
                    module,
                    source: Some(source),
                }
            }
            WorldfileSection::ModuleAdd { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection {
                    action: Action::ModuleAdd,
                    path,
                    library,
                    package,
                    module,
                    source: Some(source),
                }
            }
            WorldfileSection::ModuleRemove { library, package, module } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection {
                    action: Action::ModuleRemove,
                    path,
                    library,
                    package,
                    module,
                    source: None,
                }
            }
            WorldfileSection::ModuleChangeWs { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection {
                    action: Action::ModuleChangeWs,
                    path,
                    library,
                    package,
                    module,
                    source: Some(source),
                }
            }
            WorldfileSection::ModuleChangeAst { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection {
                    action: Action::ModuleChangeAst,
                    path,
                    library,
                    package,
                    module,
                    source: Some(source),
                }
            }
            WorldfileSection::ModuleChangeTy { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection {
                    action: Action::ModuleChangeTy,
                    path,
                    library,
                    package,
                    module,
                    source: Some(source),
                }
            }
            WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. } => {
                bail!("module_memo only supports module sections, not script sections");
            }
        };
        sections.push(parsed_section);
    }
    Ok(sections)
}

/// Build raw PackageWorld from current state.
fn build_raw_package_world(state: &MemoState) -> RawPackageWorld {
    let mut pkglib_system = BTreeMap::new();
    let mut pkglib_local = BTreeMap::new();

    for (path, source) in &state.raw_sources {
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

        let package = pkglib.entry(package_name.to_string())
            .or_insert_with(|| Package {
                name: package_name.to_string(),
                modules: BTreeMap::new(),
            });

        let pkg_module = PackageModule {
            name: module_name.to_string(),
            path: path.clone().into(),
            text: source.clone(),
        };
        package.modules.insert(module_name.to_string(), pkg_module);
    }

    RawPackageWorld {
        pkglib_system,
        pkglib_local,
    }
}

/// Extract dependency edges (as path -> path) from the pipeline.
fn extract_dependencies(
    db: &Database,
    state: &MemoState,
) -> BTreeMap<String, BTreeSet<String>> {
    // Build PackageWorld from raw state.
    let raw_package_world = build_raw_package_world(state);
    let package_world = datalove_datafun_pkg::import_from_loader(db, raw_package_world);

    // Run resolution to get dependencies.
    let resolution = crate::package_resolve::resolve_package_world_with_imports(db, package_world);
    let pkg_graph = match resolution.result(db) {
        Ok(graph) => graph,
        Err(_) => return BTreeMap::new(),
    };

    // Get module graph with resolved requires - we only need the dependency info.
    let graph_with_requires = datalove_datafun_pkg::to_module_graph(db, package_world, pkg_graph);
    let resolved_requires = graph_with_requires.resolved_requires;

    // Convert ModuleId-based dependencies to path-based.
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (source_id, requires) in &resolved_requires {
        let source_path = source_id.path(db).clone();
        let target_paths: BTreeSet<String> = requires.iter()
            .map(|(_, target_id)| target_id.path(db).clone())
            .collect();
        deps.insert(source_path, target_paths);
    }
    deps
}

/// Calculate expected behavior based on action and module relationship.
///
/// Returns None for cases where memoization expectations don't apply
/// (e.g., dependents of a removed module - the module graph can't resolve).
fn expected_behavior(action: Action, is_direct: bool, is_dependent: bool) -> Option<ExpectedBehavior> {
    // Based on design table (mandocs/design-notes.md):
    // | action            | direct-ast | direct-ty | direct-hash | depend-ast | depend-ty | depend-hash |
    // |-------------------|------------|-----------|-------------|------------|-----------|-------------|
    // | add-module        | y          | y         | y           | n/a        | n/a       | n/a         |
    // | remove-module     | y*         | y*        | y*          | **         | **        | **          |
    // | change-module-ws  | y          | n         | y           | n          | n         | y           |
    // | change-module-ast | y          | n         | y           | n          | n         | y           |
    // | change-module-ty  | y          | y         | y           | n          | y         | y           |
    //
    // *: removed module
    // **: impossible case - module graph can't resolve when dependency is removed

    if is_direct {
        Some(match action {
            Action::Add | Action::ModuleAdd => ExpectedBehavior {
                parsed: true,
                typechecked: true,
                hash_changed: true,
            },
            Action::ModuleRemove => ExpectedBehavior {
                parsed: true,
                typechecked: true,
                hash_changed: true,
            },
            Action::ModuleChangeWs => ExpectedBehavior {
                parsed: true,
                typechecked: false,
                hash_changed: true,
            },
            Action::ModuleChangeAst => ExpectedBehavior {
                parsed: true,
                typechecked: false,
                hash_changed: true,
            },
            Action::ModuleChangeTy => ExpectedBehavior {
                parsed: true,
                typechecked: true,
                hash_changed: true,
            },
        })
    } else if is_dependent {
        match action {
            Action::Add | Action::ModuleAdd => Some(ExpectedBehavior {
                parsed: false,
                typechecked: false,
                hash_changed: false,
            }),
            // Dependents of a removed module: module graph can't resolve.
            // This is an error state, not a memoization test case.
            Action::ModuleRemove => None,
            Action::ModuleChangeWs => Some(ExpectedBehavior {
                parsed: false,
                typechecked: false,
                hash_changed: true,
            }),
            Action::ModuleChangeAst => Some(ExpectedBehavior {
                parsed: false,
                typechecked: false,
                hash_changed: true,
            }),
            Action::ModuleChangeTy => Some(ExpectedBehavior {
                parsed: false,
                typechecked: true,
                hash_changed: true,
            }),
        }
    } else {
        Some(ExpectedBehavior {
            parsed: false,
            typechecked: false,
            hash_changed: false,
        })
    }
}

/// Analyze memoization behavior for a worldfile.
pub fn analyze_memo_worldfile(content: &str) -> AnyResult<MemoAnalysis> {
    let sections = parse_sections(content)?;

    let mut db = Database::default();
    let mut state = MemoState::new();
    let mut steps = Vec::new();
    let mut all_correct = true;

    for section in sections {
        let source_hash_str = section.source.as_ref()
            .map(|s| format!("{:016x}", hash_string(s)))
            .unwrap_or_else(|| "removed".to_string());

        // Capture dependents BEFORE applying the action (important for remove).
        let dependents_before = state.get_dependents(&section.path);

        // Apply action to state.
        match section.action {
            Action::Add | Action::ModuleAdd => {
                let source = section.source.as_ref()
                    .ok_or_else(|| anyhow!("Add action requires source"))?;
                state.raw_sources.insert(section.path.clone(), source.clone());

                // Create new Module for salsa.
                let new_source = Source::new(&db, source.clone());
                let module_id = ModuleId::new(&db, section.path.clone());
                let module = Module::new(&db, module_id, new_source);
                state.modules.insert(section.path.clone(), module);
            }
            Action::ModuleRemove => {
                state.raw_sources.remove(&section.path);
                state.modules.remove(&section.path);
            }
            Action::ModuleChangeWs | Action::ModuleChangeAst | Action::ModuleChangeTy => {
                let source = section.source.as_ref()
                    .ok_or_else(|| anyhow!("Change action requires source"))?;
                state.raw_sources.insert(section.path.clone(), source.clone());

                // Update existing Module's source via set_text for salsa memoization.
                if let Some(module) = state.modules.get(&section.path) {
                    let existing_source = module.source(&db);
                    existing_source.set_text(&mut db).to(source.clone());
                }
            }
        }

        // Update dependency info from pipeline (just for tracking, not memoization).
        let path_deps = extract_dependencies(&db, &state);
        state.dependents.clear();
        for (source_path, target_paths) in &path_deps {
            for target_path in target_paths {
                state.dependents
                    .entry(target_path.clone())
                    .or_default()
                    .insert(source_path.clone());
            }
        }

        // Use dependents from before the action (for remove) or after (for changes).
        let dependents = if section.action == Action::ModuleRemove {
            dependents_before
        } else {
            state.get_dependents(&section.path)
        };

        // Build ModuleGraph from tracked Module objects (for memoization).
        // Must be in dependency order (dependencies first) for hash computation.
        let all_paths: BTreeSet<String> = state.modules.keys().cloned().collect();
        let sorted_paths = topological_sort(&all_paths, &path_deps);
        let modules: Vec<Module> = sorted_paths.iter()
            .filter_map(|p| state.modules.get(p).copied())
            .collect();
        let module_by_id: BTreeMap<ModuleId, Module> = state.modules.values()
            .map(|m| (m.id(&db), *m))
            .collect();

        // Build dependency map using our ModuleIds.
        let mut dependencies: BTreeMap<ModuleId, BTreeSet<ModuleId>> = BTreeMap::new();
        for (source_path, target_paths) in &path_deps {
            if let Some(source_module) = state.modules.get(source_path) {
                let source_id = source_module.id(&db);
                let target_ids: BTreeSet<ModuleId> = target_paths.iter()
                    .filter_map(|p| state.modules.get(p).map(|m| m.id(&db)))
                    .collect();
                dependencies.insert(source_id, target_ids);
            }
        }
        // Ensure all modules have an entry.
        for module in &modules {
            dependencies.entry(module.id(&db)).or_default();
        }

        let graph = ModuleGraph::new(&db, modules, module_by_id, dependencies);

        // Build resolved requires for parse_module_graph.
        let mut resolved_requires: BTreeMap<ModuleId, Vec<(String, ModuleId)>> = BTreeMap::new();
        for (source_path, target_paths) in &path_deps {
            if let Some(source_module) = state.modules.get(source_path) {
                let source_id = source_module.id(&db);
                let requires: Vec<(String, ModuleId)> = target_paths.iter()
                    .filter_map(|p| {
                        state.modules.get(p).map(|m| {
                            // Use the last component of path as alias.
                            let alias = p.split('/').last().unwrap_or(p).to_string();
                            (alias, m.id(&db))
                        })
                    })
                    .collect();
                resolved_requires.insert(source_id, requires);
            }
        }

        // Run parse with logging.
        enable_query_logging();
        let parsed_graph = parse_module_graph(&db, graph.clone(), resolved_requires.clone());
        let parse_log = disable_query_logging();
        let parsed_modules: BTreeSet<String> = get_executed_modules(&parse_log, "parse").into_iter().collect();

        // Run typecheck with logging.
        enable_query_logging();
        let typecheck_result = typecheck_module_graph(&db, parsed_graph);
        let typecheck_log = disable_query_logging();
        let typechecked_modules: BTreeSet<String> = get_executed_modules(&typecheck_log, "typecheck").into_iter().collect();

        // Get content hashes and errors.
        let current_hashes: BTreeMap<String, u64> = parsed_graph.module_content_hashes(&db)
            .iter()
            .map(|(id, hash)| (id.path(&db).clone(), *hash))
            .collect();
        let module_errors: BTreeMap<String, Vec<String>> = typecheck_result.module_errors(&db)
            .iter()
            .map(|(id, errors)| {
                let path = id.path(&db).clone();
                let error_strings: Vec<String> = errors.iter()
                    .map(|e| format!("{:?}", e))
                    .collect();
                (path, error_strings)
            })
            .collect();

        // Build results for all current modules.
        let mut results = BTreeMap::new();

        // Handle removed module specially.
        if section.action == Action::ModuleRemove {
            let expected = expected_behavior(section.action, true, false);
            results.insert(section.path.clone(), ModuleResult {
                is_direct: true,
                is_dependent: false,
                parsed: true,
                parse_ok: false,
                typechecked: true,
                typecheck_ok: false,
                hash_changed: true,
                expected,
                correct: true,
            });
        }

        for path in state.raw_sources.keys() {
            let is_direct = path == &section.path;
            let is_dependent = dependents.contains(path);

            let parsed = parsed_modules.contains(path);
            let typechecked = typechecked_modules.contains(path);

            let errors = module_errors.get(path).cloned().unwrap_or_default();
            let typecheck_ok = errors.is_empty();
            let parse_ok = true;

            let current_hash = current_hashes.get(path).copied().unwrap_or(0);
            let prev_hash = state.prev_hashes.get(path).copied();
            let hash_changed = prev_hash.map(|p| p != current_hash).unwrap_or(true);

            let expected = expected_behavior(section.action, is_direct, is_dependent);

            // When expected is None, this is not a memoization test case (e.g., broken module graph).
            // Mark as correct since there's no expectation to check.
            let correct = match &expected {
                Some(exp) => {
                    parsed == exp.parsed
                        && typechecked == exp.typechecked
                        && hash_changed == exp.hash_changed
                }
                None => true,
            };

            if !correct {
                all_correct = false;
            }

            results.insert(path.clone(), ModuleResult {
                is_direct,
                is_dependent,
                parsed,
                parse_ok,
                typechecked,
                typecheck_ok,
                hash_changed,
                expected,
                correct,
            });

            state.prev_hashes.insert(path.clone(), current_hash);
        }

        steps.push(StepResult {
            action: section.action.as_str().to_string(),
            module: section.path,
            source_hash: source_hash_str,
            results,
        });
    }

    let total_steps = steps.len();
    Ok(MemoAnalysis {
        steps,
        summary: Summary {
            total_steps,
            all_correct,
        },
    })
}
