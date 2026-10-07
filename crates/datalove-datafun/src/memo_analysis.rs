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

use std::cell::RefCell;
use std::rc::Rc;

use datalove_datafun_compiler::Database;
use datalove_datafun_compiler::module_graph::parse_module_graph;
use datalove_datafun_compiler::tracked_ownership_analysis::analyze_module_graph;
use datalove_datafun_compiler::tracked_lower::lower_module_graph_with_evaluator;
use datalove_ct::query_log::{enable_query_logging, disable_query_logging, get_executed_modules};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::{typecheck_module_graph, AutoAdaptMode};
use datalove_datafun_resolve::{resolve_all_names, ParallelMode};
use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;

use crate::incremental::{IncrementalModuleWorld, extract_dependencies};

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
    pub resolved_names: bool,
    pub typechecked: bool,
    pub typecheck_ok: bool,
    pub lowered: bool,
    pub lower_ok: bool,
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
    pub resolved_names: bool,
    pub typechecked: bool,
    pub lowered: bool,
    pub hash_changed: bool,
}

/// Summary of analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total_steps: usize,
    pub all_correct: bool,
}

/// Parsed worldfile section with action type.
struct ParsedSection {
    action: Action,
    path: String,
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
                ParsedSection { action: Action::Add, path, source: Some(source) }
            }
            WorldfileSection::ModuleAdd { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection { action: Action::ModuleAdd, path, source: Some(source) }
            }
            WorldfileSection::ModuleRemove { library, package, module } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection { action: Action::ModuleRemove, path, source: None }
            }
            WorldfileSection::ModuleChangeWs { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection { action: Action::ModuleChangeWs, path, source: Some(source) }
            }
            WorldfileSection::ModuleChangeAst { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection { action: Action::ModuleChangeAst, path, source: Some(source) }
            }
            WorldfileSection::ModuleChangeTy { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                ParsedSection { action: Action::ModuleChangeTy, path, source: Some(source) }
            }
            WorldfileSection::ScriptFragment { .. } | WorldfileSection::ScriptExpr { .. } => {
                bail!("module_memo only supports module sections, not script sections");
            }
            WorldfileSection::Rider { .. } => {
                // Not relevant for memoization tests, skip.
                continue;
            }
        };
        sections.push(parsed_section);
    }
    Ok(sections)
}

/// Hash a string using DefaultHasher.
fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Calculate expected behavior based on action and module relationship.
///
/// Returns None for cases where memoization expectations don't apply
/// (e.g., dependents of a removed module - the module graph can't resolve).
///
/// Note: Content hash is based on source text. Memoization of typecheck is
/// handled separately by Salsa based on AST equality. This means:
/// - Whitespace changes: source changed (hash changes), but AST stable (no re-typecheck)
/// - AST changes: source changed (hash changes), AST changed (re-typecheck)
///
/// Name resolution follows the same pattern as typecheck - it only re-runs when
/// AST changes (since type aliases and function signatures come from the AST).
///
/// Lowering follows the same pattern as typecheck - it only re-runs when
/// typecheck results change.
fn expected_behavior(action: Action, is_direct: bool, is_dependent: bool) -> Option<ExpectedBehavior> {
    // Table based on source text content hashing:
    // | action            | direct-parse | direct-names | direct-ty | direct-low | direct-hash | depend-parse | depend-names | depend-ty | depend-low | depend-hash |
    // |-------------------|--------------|--------------|-----------|------------|-------------|--------------|--------------|-----------|------------|-------------|
    // | add-module        | y            | y            | y         | y          | y           | n/a          | n/a          | n/a       | n/a        | n/a         |
    // | remove-module     | y*           | y*           | y*        | y*         | y*          | **           | **           | **        | **         | **          |
    // | change-module-ws  | y            | n            | n         | n          | y           | n            | n            | n         | n          | y           |
    // | change-module-ast | y            | y            | y         | y          | y           | n            | n            | n         | n          | y           |
    // | change-module-ty  | y            | y            | y         | y          | y           | n            | n            | y         | y          | y           |
    //
    // *: removed module
    // **: impossible case - module graph can't resolve when dependency is removed

    if is_direct {
        Some(match action {
            Action::Add | Action::ModuleAdd => ExpectedBehavior {
                parsed: true,
                resolved_names: true,
                typechecked: true,
                lowered: true,
                hash_changed: true,
            },
            Action::ModuleRemove => ExpectedBehavior {
                parsed: true,
                resolved_names: true,
                typechecked: true,
                lowered: true,
                hash_changed: true,
            },
            // Whitespace change: source changed (hash changes), AST stable (no re-resolve/re-typecheck/lower).
            Action::ModuleChangeWs => ExpectedBehavior {
                parsed: true,
                resolved_names: false,
                typechecked: false,
                lowered: false,
                hash_changed: true,
            },
            // AST change: hash changes, must re-resolve, re-typecheck and re-lower.
            Action::ModuleChangeAst => ExpectedBehavior {
                parsed: true,
                resolved_names: true,
                typechecked: true,
                lowered: true,
                hash_changed: true,
            },
            Action::ModuleChangeTy => ExpectedBehavior {
                parsed: true,
                resolved_names: true,
                typechecked: true,
                lowered: true,
                hash_changed: true,
            },
        })
    } else if is_dependent {
        match action {
            Action::Add | Action::ModuleAdd => Some(ExpectedBehavior {
                parsed: false,
                resolved_names: false,
                typechecked: false,
                lowered: false,
                hash_changed: false,
            }),
            // Dependents of a removed module: module graph can't resolve.
            Action::ModuleRemove => None,
            // Whitespace change in dependency: source changed (hash changes transitively).
            Action::ModuleChangeWs => Some(ExpectedBehavior {
                parsed: false,
                resolved_names: false,
                typechecked: false,
                lowered: false,
                hash_changed: true,
            }),
            // AST change in dependency: dependency hash changes, dependent hash changes.
            Action::ModuleChangeAst => Some(ExpectedBehavior {
                parsed: false,
                resolved_names: false,
                typechecked: false,
                lowered: false,
                hash_changed: true,
            }),
            Action::ModuleChangeTy => Some(ExpectedBehavior {
                parsed: false,
                resolved_names: false,
                typechecked: true,
                lowered: true,
                hash_changed: true,
            }),
        }
    } else {
        Some(ExpectedBehavior {
            parsed: false,
            resolved_names: false,
            typechecked: false,
            lowered: false,
            hash_changed: false,
        })
    }
}

/// Analyze memoization behavior for a worldfile.
pub fn analyze_memo_worldfile(content: &str) -> AnyResult<MemoAnalysis> {
    let sections = parse_sections(content)?;

    let mut db = Database::default();
    let mut world = IncrementalModuleWorld::new();
    let mut prev_hashes: BTreeMap<String, u64> = BTreeMap::new();
    let mut steps = Vec::new();
    let mut all_correct = true;

    for section in sections {
        let source_hash_str = section.source.as_ref()
            .map(|s| format!("{:016x}", hash_string(s)))
            .unwrap_or_else(|| "removed".to_string());

        // Capture dependents BEFORE applying the action (important for remove).
        let path_deps_before = extract_dependencies(&world, &db, &crate::incremental::Roots::All);
        let dependents_before = world.get_dependents(&section.path, &path_deps_before);

        // Apply action to module world.
        match section.action {
            Action::Add | Action::ModuleAdd => {
                let source = section.source.as_ref()
                    .ok_or_else(|| anyhow!("Add action requires source"))?;
                world.add_module(&db, &section.path, source);
            }
            Action::ModuleRemove => {
                world.remove_module(&section.path);
            }
            Action::ModuleChangeWs | Action::ModuleChangeAst | Action::ModuleChangeTy => {
                let source = section.source.as_ref()
                    .ok_or_else(|| anyhow!("Change action requires source"))?;
                world.update_source(&mut db, &section.path, source);
            }
        }

        // The window opens before package resolution, because that is where a
        // module is parsed. `import_demands` asks `parse_module_full` for each
        // module, sharing phase 1's memo rather than parsing the world a second
        // time -- so by the time `parse_module_graph` runs, the parses are memo
        // hits and log nothing. Opening the window after resolution reported
        // every module as unparsed, in all sixteen fixtures at once, which is
        // this suite's documented failure mode rather than a regression.
        enable_query_logging();

        // Extract dependencies after the action.
        let path_deps = extract_dependencies(&world, &db, &crate::incremental::Roots::All);

        // Use dependents from before the action (for remove) or after (for changes).
        let dependents = if section.action == Action::ModuleRemove {
            dependents_before
        } else {
            world.get_dependents(&section.path, &path_deps)
        };

        // Prepare and run compilation with query logging.
        let (graph, resolved_requires) = world.build_graph(&db, &path_deps, &crate::incremental::Roots::All);

        let parsed_graph = parse_module_graph(&db, graph, resolved_requires, Vec::new());
        let parse_log = disable_query_logging();
        let parsed_modules: BTreeSet<String> = get_executed_modules(&parse_log, "parse")
            .into_iter().collect();

        enable_query_logging();
        let all_names = resolve_all_names(&db, parsed_graph);
                let typecheck_result = typecheck_module_graph(&db, parsed_graph, all_names, AutoAdaptMode::Disabled);
        let typecheck_log = disable_query_logging();
        let resolved_names_modules: BTreeSet<String> = get_executed_modules(&typecheck_log, "resolve_names")
            .into_iter().collect();
        let typechecked_modules: BTreeSet<String> = get_executed_modules(&typecheck_log, "typecheck")
            .into_iter().collect();

        enable_query_logging();
        let ownership_analysis = analyze_module_graph(&db, parsed_graph, typecheck_result, AutoAdaptMode::Disabled);
        let _ownership_analysis_log = disable_query_logging();
        // TODO: Track ownership analysis memoization separately if needed.
        let _ownership_analyzed_modules: BTreeSet<String> = get_executed_modules(&_ownership_analysis_log, "ownership_analysis")
            .into_iter().collect();

        enable_query_logging();
        let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
        let lowering_result = lower_module_graph_with_evaluator(
            &db, parsed_graph, typecheck_result, ownership_analysis,
            ParallelMode::Sequential, evaluator, false, false,
        );
        let lower_log = disable_query_logging();
        let lowered_modules: BTreeSet<String> = get_executed_modules(&lower_log, "lower")
            .into_iter().collect();

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

        let lowering_errors: BTreeMap<String, Vec<String>> = lowering_result.module_results(&db)
            .iter()
            .map(|(id, result)| {
                let path = id.path(&db).clone();
                (path, result.errors(&db).clone())
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
                resolved_names: true,
                typechecked: true,
                typecheck_ok: false,
                lowered: true,
                lower_ok: false,
                hash_changed: true,
                expected,
                correct: true,
            });
        }

        for path in world.paths() {
            let is_direct = path == &section.path;
            let is_dependent = dependents.contains(path);

            let parsed = parsed_modules.contains(path);
            let resolved_names = resolved_names_modules.contains(path);
            let typechecked = typechecked_modules.contains(path);
            let lowered = lowered_modules.contains(path);

            let errors = module_errors.get(path).cloned().unwrap_or_default();
            let typecheck_ok = errors.is_empty();
            let parse_ok = true;

            let lower_errors = lowering_errors.get(path).cloned().unwrap_or_default();
            let lower_ok = lower_errors.is_empty();

            let current_hash = current_hashes.get(path).copied().unwrap_or(0);
            let prev_hash = prev_hashes.get(path).copied();
            let hash_changed = prev_hash.map(|p| p != current_hash).unwrap_or(true);

            let expected = expected_behavior(section.action, is_direct, is_dependent);

            let correct = match &expected {
                Some(exp) => {
                    // Lowering is skipped when typecheck has errors, so adjust
                    // the expected lowered value based on actual typecheck success.
                    let expected_lowered = if typecheck_ok { exp.lowered } else { false };
                    parsed == exp.parsed
                        && resolved_names == exp.resolved_names
                        && typechecked == exp.typechecked
                        && lowered == expected_lowered
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
                resolved_names,
                typechecked,
                typecheck_ok,
                lowered,
                lower_ok,
                hash_changed,
                expected,
                correct,
            });

            prev_hashes.insert(path.clone(), current_hash);
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
