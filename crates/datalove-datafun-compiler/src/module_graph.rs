//! Module graph abstraction for the core compiler.
//!
//! Re-exports core types from bct and adds datafun-specific parsing/orchestration.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use rmx::std::hash::{Hash, Hasher};
use rmx::std::collections::hash_map::DefaultHasher;
use bct::text::InternedText;
use datalove_ct::query_log::{log_query, QueryPhase};

// Re-export core module graph types from bct.
pub use bct::module_graph::{
    ModuleId,
    Module,
    ModuleGraph,
    ModuleGraphBuilder,
};

// Re-export typecheck result types from tycheck crate.
pub use datalove_datafun_tycheck::{
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use datalove_datafun_parser::DatafunSpans;
use datalove_datafun_ast::ast::ParsedStatements;

/// Parse a single module with logging.
///
/// This is a tracked function so Salsa can cache per-module.
/// The logging only fires when the function actually executes.
#[salsa::tracked]
pub fn parse_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
) -> (ParsedStatements<'db>, DatafunSpans<'db>) {
    let module_id = module.id(db);
    let module_path = module_id.path(db);
    let source = module.source(db);

    log_query("parse", module_path, QueryPhase::Start);
    let parse_result = datalove_datafun_parser::parse(db, source);
    let spans = datalove_datafun_parser::datafun_spans(db, source);
    log_query("parse", module_path, QueryPhase::End);

    (parse_result.parsed(db), spans)
}

/// Parse all modules in a graph with resolved requires.
///
/// Returns a ParsedModuleGraph containing the original graph, pre-parsed statements,
/// spans, and resolved require aliases from package resolution.
#[salsa::tracked]
pub fn parse_module_graph<'db>(
    db: &'db dyn salsa::Database,
    graph: ModuleGraph,
    resolved_requires_str: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
) -> ParsedModuleGraph<'db> {
    let mut parsed_statements = Vec::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let (parsed, spans) = parse_module(db, module);
        parsed_statements.push((module_id, parsed, spans));
    }

    // Convert String aliases to InternedText.
    let resolved_requires: BTreeMap<ModuleId, Vec<(InternedText<'db>, ModuleId)>> =
        resolved_requires_str.into_iter()
            .map(|(module_id, requires)| {
                let interned_requires: Vec<(InternedText<'db>, ModuleId)> = requires.into_iter()
                    .map(|(alias, target)| (InternedText::new(db, alias), target))
                    .collect();
                (module_id, interned_requires)
            })
            .collect();

    // Compute recursive content hashes for each module.
    let module_content_hashes = compute_module_content_hashes(db, &graph, &resolved_requires);

    ParsedModuleGraph::new(db, graph, parsed_statements, resolved_requires, module_content_hashes)
}

/// Compute recursive content hashes for each module in the graph.
///
/// Each module's hash incorporates:
/// - The module's source text
/// - The sorted (alias, dependency_hash) pairs for resolved requires
///
/// Because modules are processed in topological order (dependencies first),
/// each module's hash transitively includes all its dependencies' content.
fn compute_module_content_hashes<'db>(
    db: &'db dyn salsa::Database,
    graph: &ModuleGraph,
    resolved_requires: &BTreeMap<ModuleId, Vec<(InternedText<'db>, ModuleId)>>,
) -> BTreeMap<ModuleId, u64> {
    let mut hashes = BTreeMap::new();

    // Process in dependency order (graph.iter_modules is topologically sorted).
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let source = module.source(db);

        let mut hasher = DefaultHasher::new();

        // Hash source text.
        source.text(db).hash(&mut hasher);

        // Hash resolved requires with their content hashes (sorted for determinism).
        if let Some(requires) = resolved_requires.get(&module_id) {
            let mut dep_hashes: Vec<_> = requires.iter()
                .filter_map(|(alias, target_id)| {
                    hashes.get(target_id).map(|h| (alias.text(db).to_string(), *h))
                })
                .collect();
            dep_hashes.sort_by(|a, b| a.0.cmp(&b.0));
            dep_hashes.hash(&mut hasher);
        }

        hashes.insert(module_id, hasher.finish());
    }

    hashes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use rmx::std::sync::{Arc, Mutex};

    /// Database that logs Salsa events for memoization verification.
    #[salsa::db]
    #[derive(Clone)]
    struct LoggingDatabase {
        storage: salsa::Storage<Self>,
        /// Logged events (thread-safe for Salsa's requirements).
        events: Arc<Mutex<Vec<salsa::Event>>>,
    }

    #[salsa::db]
    impl salsa::Database for LoggingDatabase {}

    impl LoggingDatabase {
        fn new() -> Self {
            let events = Arc::new(Mutex::new(Vec::new()));
            let events_clone = events.clone();
            Self {
                storage: salsa::Storage::new(Some(Box::new(move |event| {
                    events_clone.lock().unwrap().push(event);
                }))),
                events,
            }
        }

        /// Get events where queries were executed (not cached).
        fn executed_queries(&self) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter_map(|event| {
                    if let salsa::EventKind::WillExecute { database_key } = &event.kind {
                        Some(format!("{:?}", database_key))
                    } else {
                        None
                    }
                })
                .collect()
        }

        /// Clear logged events.
        fn clear_events(&self) {
            self.events.lock().unwrap().clear();
        }
    }

    /// Build a module graph from source strings.
    ///
    /// Returns module IDs in the same order as the input sources.
    fn build_graph(db: &Database, sources: &[(&str, &str)]) -> (ModuleGraph, Vec<ModuleId>) {
        let mut builder = ModuleGraphBuilder::new(db);
        let mut ids = Vec::new();
        for (path, source) in sources {
            let source = bct::input::Source::new(db, (*source).to_string());
            let id = builder.add_module((*path).to_string(), source);
            ids.push(id);
        }
        (builder.build(), ids)
    }

    #[test]
    fn test_leaf_module_hash_changes_with_source() {
        let db = Database::default();

        // Create a single module.
        let (graph1, ids1) = build_graph(&db, &[("a", "let x = 1")]);
        let parsed1 = parse_module_graph(&db, graph1, BTreeMap::new());
        let hash1 = parsed1.module_content_hashes(&db)[&ids1[0]];

        // Create same module with different source.
        let (graph2, ids2) = build_graph(&db, &[("a", "let x = 2")]);
        let parsed2 = parse_module_graph(&db, graph2, BTreeMap::new());
        let hash2 = parsed2.module_content_hashes(&db)[&ids2[0]];

        assert_ne!(hash1, hash2, "hash should change when source changes");
    }

    #[test]
    fn test_identical_source_produces_same_hash() {
        let db = Database::default();

        let (graph1, ids1) = build_graph(&db, &[("a", "let x = 1")]);
        let parsed1 = parse_module_graph(&db, graph1, BTreeMap::new());
        let hash1 = parsed1.module_content_hashes(&db)[&ids1[0]];

        let (graph2, ids2) = build_graph(&db, &[("a", "let x = 1")]);
        let parsed2 = parse_module_graph(&db, graph2, BTreeMap::new());
        let hash2 = parsed2.module_content_hashes(&db)[&ids2[0]];

        assert_eq!(hash1, hash2, "identical source should produce identical hash");
    }

    #[test]
    fn test_dependent_hash_changes_when_dependency_changes() {
        let db = Database::default();

        // A depends on B (B is leaf).
        let (graph1, ids1) = build_graph(&db, &[
            ("b", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "require module /test/b\nlet x = b.helper()"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[1], vec![("b".to_string(), ids1[0])]);
        let parsed1 = parse_module_graph(&db, graph1, requires1);
        let hash_a1 = parsed1.module_content_hashes(&db)[&ids1[1]];
        let hash_b1 = parsed1.module_content_hashes(&db)[&ids1[0]];

        // Same structure but B has different source.
        let (graph2, ids2) = build_graph(&db, &[
            ("b", "fun helper(): i32\n  ret 2\nend fun"),
            ("a", "require module /test/b\nlet x = b.helper()"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("b".to_string(), ids2[0])]);
        let parsed2 = parse_module_graph(&db, graph2, requires2);
        let hash_a2 = parsed2.module_content_hashes(&db)[&ids2[1]];
        let hash_b2 = parsed2.module_content_hashes(&db)[&ids2[0]];

        // B's hash should change.
        assert_ne!(hash_b1, hash_b2, "dependency hash should change when its source changes");

        // A's hash should also change (even though A's source is unchanged).
        assert_ne!(hash_a1, hash_a2, "dependent hash should change when dependency changes");
    }

    #[test]
    fn test_unrelated_module_hash_unchanged() {
        let db = Database::default();

        // A and C are independent, B depends on C.
        let (graph1, ids1) = build_graph(&db, &[
            ("c", "fun leaf(): i32\n  ret 1\nend fun"),
            ("b", "require module /test/c\nlet y = c.leaf()"),
            ("a", "let x = 100"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[1], vec![("c".to_string(), ids1[0])]);
        let parsed1 = parse_module_graph(&db, graph1, requires1);
        let hash_a1 = parsed1.module_content_hashes(&db)[&ids1[2]];

        // Change C's source.
        let (graph2, ids2) = build_graph(&db, &[
            ("c", "fun leaf(): i32\n  ret 999\nend fun"),
            ("b", "require module /test/c\nlet y = c.leaf()"),
            ("a", "let x = 100"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("c".to_string(), ids2[0])]);
        let parsed2 = parse_module_graph(&db, graph2, requires2);
        let hash_a2 = parsed2.module_content_hashes(&db)[&ids2[2]];

        // A's hash should be unchanged (A doesn't depend on C).
        assert_eq!(hash_a1, hash_a2, "unrelated module hash should not change");
    }

    #[test]
    fn test_transitive_dependency_change_propagates() {
        let db = Database::default();

        // A -> B -> C (chain of dependencies).
        let (graph1, ids1) = build_graph(&db, &[
            ("c", "fun base(): i32\n  ret 1\nend fun"),
            ("b", "require module /test/c\nfun mid(): i32\n  ret c.base()\nend fun"),
            ("a", "require module /test/b\nlet x = b.mid()"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[1], vec![("c".to_string(), ids1[0])]);
        requires1.insert(ids1[2], vec![("b".to_string(), ids1[1])]);
        let parsed1 = parse_module_graph(&db, graph1, requires1);
        let hash_a1 = parsed1.module_content_hashes(&db)[&ids1[2]];
        let hash_b1 = parsed1.module_content_hashes(&db)[&ids1[1]];
        let hash_c1 = parsed1.module_content_hashes(&db)[&ids1[0]];

        // Change C (the leaf).
        let (graph2, ids2) = build_graph(&db, &[
            ("c", "fun base(): i32\n  ret 42\nend fun"),
            ("b", "require module /test/c\nfun mid(): i32\n  ret c.base()\nend fun"),
            ("a", "require module /test/b\nlet x = b.mid()"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("c".to_string(), ids2[0])]);
        requires2.insert(ids2[2], vec![("b".to_string(), ids2[1])]);
        let parsed2 = parse_module_graph(&db, graph2, requires2);
        let hash_a2 = parsed2.module_content_hashes(&db)[&ids2[2]];
        let hash_b2 = parsed2.module_content_hashes(&db)[&ids2[1]];
        let hash_c2 = parsed2.module_content_hashes(&db)[&ids2[0]];

        // All hashes should change.
        assert_ne!(hash_c1, hash_c2, "C hash should change");
        assert_ne!(hash_b1, hash_b2, "B hash should change (depends on C)");
        assert_ne!(hash_a1, hash_a2, "A hash should change (transitively depends on C)");
    }

    #[test]
    fn test_multiple_dependencies_all_affect_hash() {
        let db = Database::default();

        // A depends on both B and C.
        let (graph1, ids1) = build_graph(&db, &[
            ("b", "fun b_func(): i32\n  ret 1\nend fun"),
            ("c", "fun c_func(): i32\n  ret 2\nend fun"),
            ("a", "require module /test/b\nrequire module /test/c\nlet x = b.b_func() + c.c_func()"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[2], vec![
            ("b".to_string(), ids1[0]),
            ("c".to_string(), ids1[1]),
        ]);
        let parsed1 = parse_module_graph(&db, graph1, requires1);
        let hash_a1 = parsed1.module_content_hashes(&db)[&ids1[2]];

        // Change only C.
        let (graph2, ids2) = build_graph(&db, &[
            ("b", "fun b_func(): i32\n  ret 1\nend fun"),
            ("c", "fun c_func(): i32\n  ret 999\nend fun"),
            ("a", "require module /test/b\nrequire module /test/c\nlet x = b.b_func() + c.c_func()"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[2], vec![
            ("b".to_string(), ids2[0]),
            ("c".to_string(), ids2[1]),
        ]);
        let parsed2 = parse_module_graph(&db, graph2, requires2);
        let hash_a2 = parsed2.module_content_hashes(&db)[&ids2[2]];

        assert_ne!(hash_a1, hash_a2, "A's hash should change when any dependency changes");
    }

    #[test]
    fn test_alias_name_affects_hash() {
        let db = Database::default();

        // A depends on B with alias "b".
        let (graph1, ids1) = build_graph(&db, &[
            ("dep", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "let x = 1"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[1], vec![("b".to_string(), ids1[0])]);
        let parsed1 = parse_module_graph(&db, graph1, requires1);
        let hash_a1 = parsed1.module_content_hashes(&db)[&ids1[1]];

        // Same dependency but with different alias "c".
        let (graph2, ids2) = build_graph(&db, &[
            ("dep", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "let x = 1"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("c".to_string(), ids2[0])]);
        let parsed2 = parse_module_graph(&db, graph2, requires2);
        let hash_a2 = parsed2.module_content_hashes(&db)[&ids2[1]];

        // Different alias should produce different hash (module configuration differs).
        assert_ne!(hash_a1, hash_a2, "different alias should produce different hash");
    }

    // ========================================================================
    // Salsa Memoization Verification Tests
    // ========================================================================

    /// Helper to build a module graph using the logging database.
    fn build_graph_logging(db: &LoggingDatabase, sources: &[(&str, &str)]) -> (ModuleGraph, Vec<ModuleId>) {
        let mut builder = ModuleGraphBuilder::new(db);
        let mut ids = Vec::new();
        for (path, source) in sources {
            let source = bct::input::Source::new(db, (*source).to_string());
            let id = builder.add_module((*path).to_string(), source);
            ids.push(id);
        }
        (builder.build(), ids)
    }

    #[test]
    fn test_salsa_caches_identical_input() {
        let db = LoggingDatabase::new();

        // First parse.
        let (graph, _ids) = build_graph_logging(&db, &[("a", "let x = 1")]);
        let _parsed1 = parse_module_graph(&db, graph.clone(), BTreeMap::new());

        let first_run_queries = db.executed_queries();
        assert!(!first_run_queries.is_empty(), "first run should execute queries");

        db.clear_events();

        // Second parse with same inputs - should be cached.
        let _parsed2 = parse_module_graph(&db, graph, BTreeMap::new());

        let second_run_queries = db.executed_queries();
        assert!(
            second_run_queries.is_empty(),
            "second run should be fully cached, but executed: {:?}",
            second_run_queries
        );
    }

    #[test]
    fn test_salsa_recomputes_on_source_change() {
        let db = LoggingDatabase::new();

        // First parse.
        let (graph1, _ids1) = build_graph_logging(&db, &[("a", "let x = 1")]);
        let _parsed1 = parse_module_graph(&db, graph1, BTreeMap::new());

        db.clear_events();

        // Second parse with different source.
        let (graph2, _ids2) = build_graph_logging(&db, &[("a", "let x = 2")]);
        let _parsed2 = parse_module_graph(&db, graph2, BTreeMap::new());

        let recompute_queries = db.executed_queries();
        assert!(
            !recompute_queries.is_empty(),
            "changed source should trigger recomputation"
        );
    }

    #[test]
    fn test_salsa_memoization_matches_hash_changes() {
        let db = LoggingDatabase::new();

        // Create A -> B dependency chain.
        let (graph1, ids1) = build_graph_logging(&db, &[
            ("b", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "let x = 1"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[1], vec![("b".to_string(), ids1[0])]);
        let parsed1 = parse_module_graph(&db, graph1, requires1);
        let hash_a1 = parsed1.module_content_hashes(&db)[&ids1[1]];
        let hash_b1 = parsed1.module_content_hashes(&db)[&ids1[0]];

        db.clear_events();

        // Change B's source.
        let (graph2, ids2) = build_graph_logging(&db, &[
            ("b", "fun helper(): i32\n  ret 999\nend fun"),
            ("a", "let x = 1"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("b".to_string(), ids2[0])]);
        let parsed2 = parse_module_graph(&db, graph2, requires2);
        let hash_a2 = parsed2.module_content_hashes(&db)[&ids2[1]];
        let hash_b2 = parsed2.module_content_hashes(&db)[&ids2[0]];

        // Verify hash changes match expectations.
        assert_ne!(hash_b1, hash_b2, "B's hash should change");
        assert_ne!(hash_a1, hash_a2, "A's hash should change (depends on B)");

        // Verify Salsa recomputed.
        let recompute_queries = db.executed_queries();
        assert!(
            !recompute_queries.is_empty(),
            "changing B should trigger recomputation"
        );
    }

    // ========================================================================
    // Query Log Tests (using datalove_ct::query_log)
    // ========================================================================

    use datalove_ct::query_log::{
        enable_query_logging, disable_query_logging,
        get_executed_modules,
    };

    #[test]
    fn test_query_log_records_all_modules_parsed() {
        let db = Database::default();

        // Build A -> B -> C chain.
        let (graph, ids) = build_graph(&db, &[
            ("c", "fun base(): i32\n  ret 1\nend fun"),
            ("b", "require module /test/c\nfun mid(): i32\n  ret c.base()\nend fun"),
            ("a", "require module /test/b\nlet x = b.mid()"),
        ]);
        let mut requires = BTreeMap::new();
        requires.insert(ids[1], vec![("c".to_string(), ids[0])]);
        requires.insert(ids[2], vec![("b".to_string(), ids[1])]);

        enable_query_logging();
        let _parsed = parse_module_graph(&db, graph, requires);
        let log = disable_query_logging();

        // All 3 modules should have been parsed.
        let parsed_modules = get_executed_modules(&log, "parse");
        assert_eq!(parsed_modules.len(), 3, "should parse 3 modules");
        assert!(parsed_modules.contains(&"c".to_string()), "should parse c");
        assert!(parsed_modules.contains(&"b".to_string()), "should parse b");
        assert!(parsed_modules.contains(&"a".to_string()), "should parse a");
    }

    #[test]
    fn test_query_log_second_run_still_logs_iteration() {
        let db = Database::default();

        // First run.
        let (graph, _ids) = build_graph(&db, &[("a", "let x = 1")]);

        enable_query_logging();
        let _parsed1 = parse_module_graph(&db, graph.clone(), BTreeMap::new());
        let log1 = disable_query_logging();

        let first_parsed = get_executed_modules(&log1, "parse");
        assert_eq!(first_parsed.len(), 1, "first run parses module");

        // Second run with same inputs.
        // The outer parse_module_graph is cached, so the loop doesn't run.
        enable_query_logging();
        let _parsed2 = parse_module_graph(&db, graph, BTreeMap::new());
        let log2 = disable_query_logging();

        let second_parsed = get_executed_modules(&log2, "parse");
        // When Salsa caches parse_module_graph, the loop body doesn't execute.
        assert_eq!(second_parsed.len(), 0, "second run should be cached, no loop execution");
    }

    #[test]
    fn test_query_log_change_one_module_logs_all() {
        let db = Database::default();

        // Build A -> B chain.
        let (graph1, ids1) = build_graph(&db, &[
            ("b", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "let x = 1"),
        ]);
        let mut requires1 = BTreeMap::new();
        requires1.insert(ids1[1], vec![("b".to_string(), ids1[0])]);

        let _parsed1 = parse_module_graph(&db, graph1, requires1);

        // Change only B.
        let (graph2, ids2) = build_graph(&db, &[
            ("b", "fun helper(): i32\n  ret 999\nend fun"),
            ("a", "let x = 1"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("b".to_string(), ids2[0])]);

        enable_query_logging();
        let _parsed2 = parse_module_graph(&db, graph2, requires2);
        let log = disable_query_logging();

        // parse_module_graph re-runs, so it iterates all modules.
        // But the inner parse() for unchanged modules (a) should be cached.
        let parsed_modules = get_executed_modules(&log, "parse");
        assert_eq!(parsed_modules.len(), 2, "loop runs for all modules");
        assert!(parsed_modules.contains(&"b".to_string()));
        assert!(parsed_modules.contains(&"a".to_string()));
    }
}
