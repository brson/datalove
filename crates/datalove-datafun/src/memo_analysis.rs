//! Memoization analysis for module compilation.
//!
//! Analyzes Salsa memoization behavior by processing worldfile modules
//! incrementally and tracking parse/typecheck/hash changes after each step.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use rmx::std::hash::{Hash, Hasher};
use rmx::std::collections::hash_map::DefaultHasher;
use serde::{Serialize, Deserialize};
use salsa::Setter;

use bct::input::Source;
use datalove_datafun_compiler::Database;
use datalove_datafun_compiler::module_graph::{
    ModuleGraphBuilder, ModuleId,
    parse_module_graph,
};
use datalove_ct::query_log::{enable_query_logging, disable_query_logging, get_executed_modules};
use datalove_datafun_tycheck::typecheck_module_graph;
use datalove_datafun_pkg::package_load_worldfile::WorldfileSection;

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
    pub parsed: bool,
    pub parse_ok: bool,
    pub typechecked: bool,
    pub typecheck_ok: bool,
    pub hash_changed: bool,
    pub expected: ExpectedBehavior,
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
struct MemoState {
    /// Source objects keyed by module path.
    sources: BTreeMap<String, Source>,
    /// Module IDs keyed by module path.
    module_ids: BTreeMap<String, ModuleId>,
    /// Previous content hashes.
    prev_hashes: BTreeMap<String, u64>,
    /// Previous source text hashes (to detect source changes).
    prev_source_hashes: BTreeMap<String, u64>,
}

impl MemoState {
    fn new() -> Self {
        Self {
            sources: BTreeMap::new(),
            module_ids: BTreeMap::new(),
            prev_hashes: BTreeMap::new(),
            prev_source_hashes: BTreeMap::new(),
        }
    }
}

/// Hash a string using DefaultHasher.
fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Parse worldfile content into module sections.
fn parse_module_sections(content: &str) -> AnyResult<Vec<(String, String, String, String)>> {
    let reader = std::io::Cursor::new(content);
    let parsed = datalove_datafun_pkg::package_load_worldfile::parse_worldfile_sections(reader)?;

    let mut modules = Vec::new();
    for section in parsed.sections {
        match section {
            WorldfileSection::Module { library, package, module, source } => {
                let path = format!("{}/{}/{}", library, package, module);
                modules.push((path, library, package, source));
            }
            _ => bail!("module_memo only supports module sections, not script sections"),
        }
    }
    Ok(modules)
}

/// Analyze memoization behavior for a worldfile.
///
/// Processes module sections sequentially, tracking which modules are
/// parsed, typechecked, and have hash changes after each step.
pub fn analyze_memo_worldfile(content: &str) -> AnyResult<MemoAnalysis> {
    let sections = parse_module_sections(content)?;

    let mut db = Database::default();
    let mut state = MemoState::new();
    let mut steps = Vec::new();
    let mut all_correct = true;

    for (module_path, _library, _package, source) in sections {
        let source_hash = hash_string(&source);
        let source_hash_str = format!("{:016x}", source_hash);

        // Determine action: add or redefine.
        let is_redefine = state.sources.contains_key(&module_path);
        let action = if is_redefine { "redefine" } else { "add" };

        // Track which modules had source changes this step.
        let source_changed = state.prev_source_hashes.get(&module_path)
            .map(|&prev| prev != source_hash)
            .unwrap_or(true); // New module counts as "changed"

        // Update or create source.
        if is_redefine {
            let existing_source = state.sources.get(&module_path).unwrap();
            existing_source.set_text(&mut db).to(source.clone());
        } else {
            let new_source = Source::new(&db, source.clone());
            state.sources.insert(module_path.clone(), new_source);
        }

        // Update source hash tracking.
        state.prev_source_hashes.insert(module_path.clone(), source_hash);

        // Rebuild the module graph with all current modules.
        let mut builder = ModuleGraphBuilder::new(&db);
        let mut new_module_ids = BTreeMap::new();
        for (path, src) in &state.sources {
            let id = builder.add_module(path.clone(), *src);
            new_module_ids.insert(path.clone(), id);
        }
        let graph = builder.build();
        state.module_ids = new_module_ids;

        // Empty requires for now (no import tracking in v1).
        let requires = BTreeMap::new();

        // Run parse with logging.
        enable_query_logging();
        let parsed_graph = parse_module_graph(&db, graph.clone(), requires.clone());
        let parse_log = disable_query_logging();
        let parsed_modules = get_executed_modules(&parse_log, "parse");

        // Run typecheck with logging.
        enable_query_logging();
        let typecheck_result = typecheck_module_graph(&db, parsed_graph);
        let typecheck_log = disable_query_logging();
        let typechecked_modules = get_executed_modules(&typecheck_log, "typecheck");

        // Get content hashes and errors.
        let current_hashes = parsed_graph.module_content_hashes(&db);
        let module_errors = typecheck_result.module_errors(&db);

        // Build results for all current modules.
        let mut results = BTreeMap::new();
        for (path, &id) in &state.module_ids {
            let parsed = parsed_modules.contains(path);
            let typechecked = typechecked_modules.contains(path);

            // Check for errors.
            let errors = module_errors.get(&id).map(|e| e.clone()).unwrap_or_default();
            let typecheck_ok = errors.is_empty();

            // Parse errors would show as the module being parsed but with issues.
            // For now, assume parse succeeded if module was added to graph.
            let parse_ok = true;

            // Check hash change.
            let current_hash = current_hashes.get(&id).copied().unwrap_or(0);
            let prev_hash = state.prev_hashes.get(path).copied();
            let hash_changed = prev_hash.map(|p| p != current_hash).unwrap_or(true);

            // Compute expected behavior.
            // Simple rule: if this module's source changed, expect all true.
            // Otherwise, expect all false.
            let this_source_changed = if path == &module_path {
                source_changed
            } else {
                false
            };

            let expected = ExpectedBehavior {
                parsed: this_source_changed,
                typechecked: this_source_changed,
                hash_changed: this_source_changed,
            };

            // Check correctness.
            let correct = parsed == expected.parsed
                && typechecked == expected.typechecked
                && hash_changed == expected.hash_changed;

            if !correct {
                all_correct = false;
            }

            results.insert(path.clone(), ModuleResult {
                parsed,
                parse_ok,
                typechecked,
                typecheck_ok,
                hash_changed,
                expected,
                correct,
            });

            // Update prev_hashes for next iteration.
            state.prev_hashes.insert(path.clone(), current_hash);
        }

        steps.push(StepResult {
            action: action.to_string(),
            module: module_path,
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
