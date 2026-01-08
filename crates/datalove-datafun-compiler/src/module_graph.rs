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

use datalove_datafun_ast::ast::ParseResult;

/// Parse a single module with logging.
///
/// This is a tracked function so Salsa can cache per-module.
/// The logging only fires when the function actually executes.
///
/// Returns the full ParseResult containing both parsed statements and spans.
#[salsa::tracked]
pub fn parse_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
) -> ParseResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);
    let source = module.source(db);

    log_query("parse", module_path, QueryPhase::Start);
    let parse_result = datalove_datafun_parser::parse(db, source);
    log_query("parse", module_path, QueryPhase::End);

    parse_result
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
    use datalove_diagnostic::SpanEntry;
    use datalove_datafun_parser::{DatafunSpans, SpanMapEntry};

    let mut parsed_statements = Vec::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let parse_result = parse_module(db, module);
        let parsed = parse_result.parsed(db);
        // Convert ParseSpanEntry to SpanMapEntry for DatafunSpans.
        let span_entries: Vec<SpanMapEntry> = parse_result.expr_spans(db)
            .iter()
            .map(|e| SpanMapEntry {
                expr_id: e.expr_id,
                entry: SpanEntry::new(e.text_id, e.span.clone()),
            })
            .collect();
        let spans = DatafunSpans::new(db, span_entries);
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
    use salsa::Setter;

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
    fn test_query_log_per_module_caching() {
        let mut db = Database::default();

        // Build A -> B chain: create modules ONCE, keep Source objects for mutation.
        let source_b = bct::input::Source::new(&db, "fun helper(): i32\n  ret 1\nend fun".to_string());
        let source_a = bct::input::Source::new(&db, "let x = 1".to_string());

        let mut builder = ModuleGraphBuilder::new(&db);
        let id_b = builder.add_module("b".to_string(), source_b);
        let id_a = builder.add_module("a".to_string(), source_a);
        let graph = builder.build();

        let mut requires = BTreeMap::new();
        requires.insert(id_a, vec![("b".to_string(), id_b)]);

        // First run: both modules should be parsed.
        enable_query_logging();
        let _parsed1 = parse_module_graph(&db, graph.clone(), requires.clone());
        let log1 = disable_query_logging();

        let first_parsed = get_executed_modules(&log1, "parse");
        eprintln!("First run parsed: {:?}", first_parsed);
        assert_eq!(first_parsed.len(), 2, "first run should parse both modules");

        // Mutate only B's source text.
        source_b.set_text(&mut db).to("fun helper(): i32\n  ret 999\nend fun".to_string());

        // Second run: only B should be re-parsed, A should be cached.
        enable_query_logging();
        let _parsed2 = parse_module_graph(&db, graph, requires);
        let log2 = disable_query_logging();

        let second_parsed = get_executed_modules(&log2, "parse");
        eprintln!("Second run parsed: {:?}", second_parsed);
        // Per-module caching works! Only B should re-parse.
        assert_eq!(second_parsed.len(), 1, "only changed module should re-parse");
        assert!(second_parsed.contains(&"b".to_string()), "b should re-parse");
        assert!(!second_parsed.contains(&"a".to_string()), "a should be cached");
    }

    #[test]
    fn test_parse_module_direct_caching() {
        // Test parse_module caching directly, without going through parse_module_graph.
        let mut db = Database::default();

        // Create a single module.
        let source = bct::input::Source::new(&db, "let x = 1".to_string());
        let module_id = bct::module_graph::ModuleId::new(&db, "test".to_string());
        let module = bct::module_graph::Module::new(&db, module_id, source);

        // First call: should execute.
        enable_query_logging();
        let _result1 = parse_module(&db, module);
        let log1 = disable_query_logging();
        let first_parsed = get_executed_modules(&log1, "parse");
        eprintln!("Direct first call: {:?}", first_parsed);
        assert_eq!(first_parsed.len(), 1, "first call should execute");

        // Second call with same inputs: should be cached.
        enable_query_logging();
        let _result2 = parse_module(&db, module);
        let log2 = disable_query_logging();
        let second_parsed = get_executed_modules(&log2, "parse");
        eprintln!("Direct second call (same): {:?}", second_parsed);
        assert_eq!(second_parsed.len(), 0, "second call should be cached");

        // Change source text.
        source.set_text(&mut db).to("let x = 2".to_string());

        // Third call: should re-execute because source changed.
        enable_query_logging();
        let _result3 = parse_module(&db, module);
        let log3 = disable_query_logging();
        let third_parsed = get_executed_modules(&log3, "parse");
        eprintln!("Direct third call (changed): {:?}", third_parsed);
        assert_eq!(third_parsed.len(), 1, "third call should re-execute");
    }

    #[test]
    fn test_parse_module_two_modules_direct() {
        // Test that changing one module doesn't re-parse the other (direct calls).
        let mut db = Database::default();

        // Create two modules.
        let source_a = bct::input::Source::new(&db, "let x = 1".to_string());
        let source_b = bct::input::Source::new(&db, "let y = 2".to_string());
        let id_a = bct::module_graph::ModuleId::new(&db, "a".to_string());
        let id_b = bct::module_graph::ModuleId::new(&db, "b".to_string());
        let module_a = bct::module_graph::Module::new(&db, id_a, source_a);
        let module_b = bct::module_graph::Module::new(&db, id_b, source_b);

        // Parse both modules.
        enable_query_logging();
        let _result_a1 = parse_module(&db, module_a);
        let _result_b1 = parse_module(&db, module_b);
        let log1 = disable_query_logging();
        let first_parsed = get_executed_modules(&log1, "parse");
        eprintln!("Two modules first: {:?}", first_parsed);
        assert_eq!(first_parsed.len(), 2);

        // Change only B.
        source_b.set_text(&mut db).to("let y = 999".to_string());

        // Parse both again.
        enable_query_logging();
        let _result_a2 = parse_module(&db, module_a);
        let _result_b2 = parse_module(&db, module_b);
        let log2 = disable_query_logging();
        let second_parsed = get_executed_modules(&log2, "parse");
        eprintln!("Two modules after B change: {:?}", second_parsed);
        // Per-module caching works! Only B re-parses.
        assert_eq!(second_parsed.len(), 1, "only B should re-parse");
        assert!(second_parsed.contains(&"b".to_string()), "b should re-parse");
    }

    #[test]
    fn test_parse_module_no_change_still_cached() {
        // Verify that calling parse_module twice with no changes is cached.
        let db = Database::default();

        let source_a = bct::input::Source::new(&db, "let x = 1".to_string());
        let source_b = bct::input::Source::new(&db, "let y = 2".to_string());
        let id_a = bct::module_graph::ModuleId::new(&db, "a".to_string());
        let id_b = bct::module_graph::ModuleId::new(&db, "b".to_string());
        let module_a = bct::module_graph::Module::new(&db, id_a, source_a);
        let module_b = bct::module_graph::Module::new(&db, id_b, source_b);

        // First parse.
        let _r1 = parse_module(&db, module_a);
        let _r2 = parse_module(&db, module_b);

        // Second parse with NO changes - should be fully cached.
        enable_query_logging();
        let _r3 = parse_module(&db, module_a);
        let _r4 = parse_module(&db, module_b);
        let log = disable_query_logging();
        let parsed = get_executed_modules(&log, "parse");
        eprintln!("No change, second call: {:?}", parsed);
        assert_eq!(parsed.len(), 0, "no changes = fully cached");
    }

    #[test]
    fn test_salsa_events_on_change() {
        // Use LoggingDatabase to see what Salsa events fire when one input changes.
        let mut db = LoggingDatabase::new();

        let source_a = bct::input::Source::new(&db, "let x = 1".to_string());
        let source_b = bct::input::Source::new(&db, "let y = 2".to_string());
        let id_a = bct::module_graph::ModuleId::new(&db, "a".to_string());
        let id_b = bct::module_graph::ModuleId::new(&db, "b".to_string());
        let module_a = bct::module_graph::Module::new(&db, id_a, source_a);
        let module_b = bct::module_graph::Module::new(&db, id_b, source_b);

        // First parse.
        let _r1 = parse_module(&db, module_a);
        let _r2 = parse_module(&db, module_b);

        db.clear_events();

        // Change only B.
        source_b.set_text(&mut db).to("let y = 999".to_string());

        // Parse both again.
        enable_query_logging();
        let _r3 = parse_module(&db, module_a);
        let _r4 = parse_module(&db, module_b);
        let log = disable_query_logging();

        let executed = db.executed_queries();
        eprintln!("Salsa executed queries after B change: {:?}", executed);

        let parsed = get_executed_modules(&log, "parse");
        eprintln!("Query log parsed after B change: {:?}", parsed);
    }

    #[test]
    fn test_datafun_spans_caching_fixed() {
        // Verifies that datafun_spans caching is now correct after the side table fix.
        // Previously accumulators broke caching; now spans are in ParseResult.
        let mut db = Database::default();

        let source_a = bct::input::Source::new(&db, "let x = 1".to_string());
        let source_b = bct::input::Source::new(&db, "let y = 2".to_string());

        let spans_a1 = datalove_datafun_parser::datafun_spans(&db, source_a);
        let _ = datalove_datafun_parser::datafun_spans(&db, source_b);
        let id_a1 = salsa::plumbing::AsId::as_id(&spans_a1);
        let entries_a1: Vec<_> = spans_a1.entries(&db).iter().map(|e| e.expr_id).collect();

        // Change only B.
        source_b.set_text(&mut db).to("let y = 999".to_string());

        let spans_a2 = datalove_datafun_parser::datafun_spans(&db, source_a);
        let id_a2 = salsa::plumbing::AsId::as_id(&spans_a2);
        let entries_a2: Vec<_> = spans_a2.entries(&db).iter().map(|e| e.expr_id).collect();

        // FIX VERIFIED: A's spans have same ID because side table pattern works.
        assert_eq!(id_a1, id_a2, "A should have same ID (cached correctly)");
        assert!(!entries_a1.is_empty(), "First call has entries");
        assert!(!entries_a2.is_empty(), "Second call also has entries (not empty)");
        assert_eq!(entries_a1, entries_a2, "Entries should be identical");
    }
}
