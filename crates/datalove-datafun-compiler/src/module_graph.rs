//! Module graph abstraction for the core compiler.
//!
//! Re-exports core types from bct and adds datafun-specific parsing/orchestration.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use rmx::std::hash::Hash;
use rmx::rayon::prelude::*;
use bct::text::InternedText;
use datalove_datafun_ast::ast::{FunParam, FunSignature, ParsedStatements, StmtFun};
use datalove_datafun_tycheck::DbClone;

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
    ParallelMode,
    parallel_mode_from_env,
};


/// Parsing a module is the parser's business; these live there now, so that
/// name resolution can reach them without depending on this crate.
pub use datalove_datafun_parser::{parse_module_ast, parse_module_full, module_spans};

/// Parse all modules in a graph with resolved requires (internal tracked function).
///
/// This is the salsa-tracked implementation that handles caching. Use
/// `parse_module_graph_parallel` for parallel execution when you have a `&dyn Db`.
#[salsa::tracked(returns(copy))]
pub fn parse_module_graph<'db>(
    db: &'db dyn salsa::Database,
    graph: ModuleGraph<'db>,
    resolved_requires_str: BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
    rider_sources: Vec<(String, String)>,
) -> ParsedModuleGraph<'db> {
    // Sequential implementation for salsa tracking.
    //
    // Only the statements are collected. The spans are a query of their own
    // now, keyed on the module, so nothing here builds a span table for a
    // module no diagnostic ever names.
    let mut statements_only = Vec::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let parsed = parse_module_full(db, module).parsed.clone();
        statements_only.push((module_id, parsed));
    }

    // Convert String aliases to InternedText.
    let resolved_requires: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, ModuleId<'db>)>> =
        resolved_requires_str.into_iter()
            .map(|(module_id, requires)| {
                let interned_requires: Vec<(InternedText<'db>, ModuleId<'db>)> = requires.into_iter()
                    .map(|(alias, target)| (InternedText::new(db, alias), target))
                    .collect();
                (module_id, interned_requires)
            })
            .collect();

    // Build RiderInterfaces from rider sources inside this tracked function,
    // where salsa tracked struct creation (TypeFunction) is allowed.
    let resolved_riders = build_resolved_riders_from_sources(db, &rider_sources, &statements_only);

    ParsedModuleGraph::new(db, graph, statements_only, resolved_requires, resolved_riders)
}

/// Parse all modules in a graph with resolved requires, using parallel execution.
///
/// This function parses modules in parallel using rayon to warm salsa's memoization cache,
/// then delegates to the tracked `parse_module_graph` function which can create tracked
/// structs. The tracked function will hit the warmed cache for all `parse_module_full` calls.
pub fn parse_module_graph_parallel<'db>(
    db: &'db dyn DbClone,
    graph: ModuleGraph<'db>,
    resolved_requires_str: BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
    rider_sources: Vec<(String, String)>,
) -> ParsedModuleGraph<'db> {
    let db_salsa = db.as_salsa_db();

    // Collect modules to parse.
    let modules: Vec<_> = graph.iter_modules(db_salsa).collect();

    // Clone databases upfront - one per module for parallel execution.
    // Can't clone inside parallel section since &dyn DbClone isn't Sync.
    let work: Vec<_> = modules.iter()
        .map(|&module| (db.dyn_clone(), module))
        .collect();

    // Parse modules in parallel - populates salsa's memoization cache.
    // Results are discarded; the tracked function will re-fetch with correct lifetimes.
    work.into_par_iter()
        .for_each(|(db_clone, module)| {
            let _ = parse_module_full(db_clone.as_salsa_db(), module);
        });

    // Delegate to tracked function which can create ParsedModuleGraph.
    // All parse_module_full calls will be cache hits from the parallel phase.
    parse_module_graph(db_salsa, graph, resolved_requires_str, rider_sources)
}

/// Parse module graph with configurable parallelism.
///
/// This is the main entry point for parsing. Use `ParallelMode::Sequential` for
/// standard salsa-tracked behavior, or `ParallelMode::Parallel` to enable rayon
/// parallelization (useful for benchmarking).
pub fn parse_module_graph_with_mode<'db>(
    db: &'db dyn DbClone,
    graph: ModuleGraph<'db>,
    resolved_requires_str: BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>,
    rider_sources: Vec<(String, String)>,
    mode: ParallelMode,
) -> ParsedModuleGraph<'db> {
    match mode {
        ParallelMode::Sequential => parse_module_graph(db.as_salsa_db(), graph, resolved_requires_str, rider_sources),
        ParallelMode::Parallel => parse_module_graph_parallel(db, graph, resolved_requires_str, rider_sources),
    }
}

/// What each generic native declares about its type parameters.
///
/// The declaration order comes from the statement, because that is the order a
/// call site writes its type arguments in; whether a parameter is determined
/// comes from the resolved signature, because that is where a type hint has
/// become a type. See `NativeGenerics`.
fn generic_natives<'db>(
    db: &'db dyn salsa::Database,
    statements: &[datalove_datafun_ast::ast::Statement<'db>],
    functions: &[(InternedText<'db>, datalove_datafun_common::TypeFunction<'db>)],
) -> Vec<(InternedText<'db>, datalove_datafun_common::NativeGenerics<'db>)> {
    use datalove_datafun_ast::ast::Statement;
    use datalove_datafun_common::{NativeGenerics, generics::mentions_type_param};

    let mut found = Vec::new();
    for statement in statements {
        let Statement::NativeFun(native) = statement else { continue };
        if native.type_params.is_empty() {
            continue;
        }
        let Some((_, signature)) = functions.iter().find(|(n, _)| *n == native.name) else {
            continue;
        };
        let param_types = signature.param_types(db);
        let undetermined: Vec<u32> = native.type_params.iter().enumerate()
            .filter(|(_, name)| !param_types.iter().any(|ty| match ty {
                datalove_datafun_common::Type::Datalit(dt) =>
                    mentions_type_param(dt, **name),
                // A function type has no type parameters inside it to find.
                datalove_datafun_common::Type::Function(_) => false,
            }))
            .map(|(i, _)| i as u32)
            .collect();
        found.push((native.name, NativeGenerics {
            type_params: native.type_params.clone(),
            type_bounds: native.type_bounds.clone(),
            undetermined,
        }));
    }
    found
}

/// Build resolved riders from raw source strings inside a tracked context.
///
/// Parses each rider source, extracts function signatures via name resolution,
/// then maps riders to the modules whose `require rider X` statements name them.
///
/// The requires are read from the parsed statements. They used to be found by
/// scanning each module's source a line at a time, which took a trailing
/// comment for part of the rider's name and so left `require rider std // why`
/// resolving to nothing.
fn build_resolved_riders_from_sources<'db>(
    db: &'db dyn salsa::Database,
    rider_sources: &[(String, String)],
    statements: &[(ModuleId<'db>, ParsedStatements<'db>)],
) -> BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, datalove_datafun_common::RiderInterface<'db>)>> {
    use datalove_datafun_ast::ast::{Statement, StmtRequire};
    use datalove_datafun_common::RiderInterface;

    if rider_sources.is_empty() {
        return BTreeMap::new();
    }

    let rider_interfaces = rider_interfaces(db, RiderSources::new(db, rider_sources.to_vec()));

    let mut result: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, RiderInterface<'db>)>> = BTreeMap::new();
    for (module_id, parsed) in statements {
        for statement in parsed.statements.iter() {
            let Statement::Require(StmtRequire::Rider(require)) = statement else {
                continue;
            };
            if let Some(interface) = rider_interfaces.get(require.name.as_str(db)) {
                result.entry(*module_id)
                    .or_default()
                    .push((require.name, interface.clone()));
            }
        }
    }

    result
}

/// The rider sources, as something a query can be keyed on.
///
/// Interned, so the same sources give the same handle however often the graph is
/// rebuilt around them. That is the whole point: it is a key that the module set
/// cannot move.
#[salsa::interned]
pub struct RiderSources<'db> {
    #[returns(ref)]
    pub sources: Vec<(String, String)>,
}

/// Each rider's interface, parsed from its source.
///
/// **Keyed on the rider sources and nothing else.** A rider says nothing about
/// the module graph, and this is where its `TypeFunction`s and the statement
/// behind each native are minted -- so keying it on anything the module set
/// moves would re-mint them under fresh ids whenever a module appeared or
/// disappeared, and every module importing from a rider would re-typecheck,
/// re-analyze and re-lower. It did, until `edit_reach_tests` asked.
#[salsa::tracked(returns(ref))]
fn rider_interfaces<'db>(
    db: &'db dyn salsa::Database,
    rider_sources: RiderSources<'db>,
) -> HashMap<String, datalove_datafun_common::RiderInterface<'db>> {
    use datalove_datafun_common::RiderInterface;

    let mut interfaces: HashMap<String, RiderInterface<'db>> = HashMap::new();
    for (name, source) in rider_sources.sources(db) {
        let source_input = bct::input::Source::new(db, source.clone());
        let parse_result = datalove_datafun_parser::parse(db, source_input);
        let collected = datalove_datafun_resolve::resolve_names_impl(db, &parse_result.parsed.statements);
        let rider_name = InternedText::new(db, name.clone());
        let rider_path = format!("@rider/{}", name);
        let module_id = ModuleId::new(db, rider_path);
        let generic_functions = generic_natives(
            db, &parse_result.parsed.statements, &collected.functions);
        let function_stubs = collected.functions.iter()
            .map(|(func_name, func_type)| {
                (*func_name, native_stub(db, module_id, *func_name, *func_type, &generic_functions))
            })
            .collect();
        interfaces.insert(name.clone(), RiderInterface {
            name: rider_name,
            module_id,
            functions: collected.functions,
            function_stubs,
            type_aliases: collected.type_aliases,
            generic_functions,
        });
    }
    interfaces
}

/// The statement a native stands behind.
///
/// Its identity is `(module_id, name, 0)`, where the module id is the rider's
/// synthetic one, so it is stable as long as the query that mints it is. The
/// parameters carry the declared modes, which is what the ownership checker
/// reads them for; the type parameters come in declaration order, because that
/// is the order a call site writes its type arguments and so how the descriptors
/// a native wants are worked out. See `NativeGenerics`.
fn native_stub<'db>(
    db: &'db dyn salsa::Database,
    rider_module_id: ModuleId<'db>,
    name: InternedText<'db>,
    func_type: datalove_datafun_common::TypeFunction<'db>,
    generic_functions: &[(InternedText<'db>, datalove_datafun_common::NativeGenerics<'db>)],
) -> StmtFun<'db> {
    let params: Vec<FunParam<'db>> = func_type.param_modes(db)
        .iter()
        .enumerate()
        .map(|(i, mode)| FunParam {
            name: InternedText::new(db, format!("_p{}", i)),
            mode: *mode,
            is_comptime: false,
            type_hint: datalove_datalit::ast::TypeHint::AnonTuple(
                datalove_datalit::ast::TypeHintAnonTuple { fields: vec![] }),
        })
        .collect();

    let (type_params, type_bounds) = generic_functions.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, g)| (g.type_params.clone(), g.type_bounds.clone()))
        .unwrap_or_default();

    StmtFun::new(
        db,
        Some(rider_module_id),
        name,
        FunSignature { type_params, type_bounds, params, return_type: None },
        vec![],
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use rmx::std::sync::{Arc, Mutex};
    use salsa::Setter;

    /// Database that logs Salsa events for memoization verification.
    #[salsa::db]
    struct LoggingDatabase {
        storage: salsa::Storage<Self>,
        /// Logged events (thread-safe for Salsa's requirements).
        events: Arc<Mutex<Vec<salsa::Event>>>,
    }

    #[salsa::db]
    impl salsa::Database for LoggingDatabase {}

    impl Clone for LoggingDatabase {
        fn clone(&self) -> Self {
            // Clone storage to share memoization cache (Arc<Zalsa>). The event
            // callback lives on that shared `Zalsa`, so a clone's queries are
            // logged to the same vec, whichever thread runs them.
            Self {
                storage: self.storage.clone(),
                events: self.events.clone(),
            }
        }
    }

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

    impl DbClone for LoggingDatabase {
        fn dyn_clone(&self) -> Box<dyn DbClone + Send> {
            Box::new(self.clone())
        }

        fn as_salsa_db(&self) -> &dyn salsa::Database {
            self
        }
    }

    /// Build a module graph from source strings.
    ///
    /// Returns module IDs in the same order as the input sources.
    fn build_graph<'db>(db: &'db Database, sources: &[(&str, &str)]) -> (ModuleGraph<'db>, Vec<ModuleId<'db>>) {
        let mut builder = ModuleGraphBuilder::new(db);
        let mut ids = Vec::new();
        for (path, source) in sources {
            let source = bct::input::Source::new(db, (*source).to_string());
            let id = builder.add_module((*path).to_string(), source);
            ids.push(id);
        }
        (builder.build(), ids)
    }

    // ========================================================================
    // Salsa Memoization Verification Tests
    // ========================================================================

    /// Helper to build a module graph using the logging database.
    fn build_graph_logging<'db>(db: &'db LoggingDatabase, sources: &[(&str, &str)]) -> (ModuleGraph<'db>, Vec<ModuleId<'db>>) {
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
        let _parsed1 = parse_module_graph(&db, graph.clone(), BTreeMap::new(), Vec::new());

        let first_run_queries = db.executed_queries();
        assert!(!first_run_queries.is_empty(), "first run should execute queries");

        db.clear_events();

        // Second parse with same inputs - should be cached.
        let _parsed2 = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());

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
        let _parsed1 = parse_module_graph(&db, graph1, BTreeMap::new(), Vec::new());

        db.clear_events();

        // Second parse with different source.
        let (graph2, _ids2) = build_graph_logging(&db, &[("a", "let x = 2")]);
        let _parsed2 = parse_module_graph(&db, graph2, BTreeMap::new(), Vec::new());

        let recompute_queries = db.executed_queries();
        assert!(
            !recompute_queries.is_empty(),
            "changed source should trigger recomputation"
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
        let _parsed = parse_module_graph(&db, graph, requires, Vec::new());
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
        let _parsed1 = parse_module_graph(&db, graph.clone(), BTreeMap::new(), Vec::new());
        let log1 = disable_query_logging();

        let first_parsed = get_executed_modules(&log1, "parse");
        assert_eq!(first_parsed.len(), 1, "first run parses module");

        // Second run with same inputs.
        // The outer parse_module_graph is cached, so the loop doesn't run.
        enable_query_logging();
        let _parsed2 = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
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

        // The graph is interned, so building it again from the same sources
        // gives back the same graph. Building it per phase rather than holding
        // one keeps the handles inside a single revision.
        fn build<'db>(db: &'db Database, source_a: bct::input::Source, source_b: bct::input::Source)
            -> (ModuleGraph<'db>, ModuleId<'db>, ModuleId<'db>, BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>)
        {
            let mut builder = ModuleGraphBuilder::new(db);
            let id_b = builder.add_module("b".to_string(), source_b);
            let id_a = builder.add_module("a".to_string(), source_a);
            let mut requires = BTreeMap::new();
            requires.insert(id_a, vec![("b".to_string(), id_b)]);
            (builder.build(), id_a, id_b, requires)
        }

        // First run: both modules should be parsed.
        let (graph, _, _, requires) = build(&db, source_a, source_b);
        enable_query_logging();
        parse_module_graph(&db, graph.clone(), requires.clone(), Vec::new());
        let log1 = disable_query_logging();

        let first_parsed = get_executed_modules(&log1, "parse");
        eprintln!("First run parsed: {:?}", first_parsed);
        assert_eq!(first_parsed.len(), 2, "first run should parse both modules");

        // Mutate only B's source text.
        source_b.set_text(&mut db).to("fun helper(): i32\n  ret 999\nend fun".to_string());

        // Second run: only B should be re-parsed, A should be cached.
        let (graph, _, _, requires) = build(&db, source_a, source_b);
        enable_query_logging();
        parse_module_graph(&db, graph, requires, Vec::new());
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
        // Interned, so this gives the same module every time it is called.
        fn module<'db>(db: &'db Database, source: bct::input::Source)
            -> bct::module_graph::Module<'db>
        {
            bct::module_graph::Module::new(
                db, bct::module_graph::ModuleId::new(db, "test".to_string()), source,
            )
        }

        // First call: should execute.
        enable_query_logging();
        let _result1 = parse_module_ast(&db, module(&db, source));
        let log1 = disable_query_logging();
        let first_parsed = get_executed_modules(&log1, "parse");
        eprintln!("Direct first call: {:?}", first_parsed);
        assert_eq!(first_parsed.len(), 1, "first call should execute");

        // Second call with same inputs: should be cached.
        enable_query_logging();
        let _result2 = parse_module_ast(&db, module(&db, source));
        let log2 = disable_query_logging();
        let second_parsed = get_executed_modules(&log2, "parse");
        eprintln!("Direct second call (same): {:?}", second_parsed);
        assert_eq!(second_parsed.len(), 0, "second call should be cached");

        // Change source text.
        source.set_text(&mut db).to("let x = 2".to_string());

        // Third call: should re-execute because source changed.
        enable_query_logging();
        let _result3 = parse_module_ast(&db, module(&db, source));
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
        // Interned, so these give the same modules however often they run.
        fn module<'db>(db: &'db Database, path: &str, source: bct::input::Source)
            -> bct::module_graph::Module<'db>
        {
            bct::module_graph::Module::new(
                db, bct::module_graph::ModuleId::new(db, path.to_string()), source,
            )
        }

        // Parse both modules.
        enable_query_logging();
        let _result_a1 = parse_module_ast(&db, module(&db, "a", source_a));
        let _result_b1 = parse_module_ast(&db, module(&db, "b", source_b));
        let log1 = disable_query_logging();
        let first_parsed = get_executed_modules(&log1, "parse");
        eprintln!("Two modules first: {:?}", first_parsed);
        assert_eq!(first_parsed.len(), 2);

        // Change only B.
        source_b.set_text(&mut db).to("let y = 999".to_string());

        // Parse both again.
        enable_query_logging();
        let _result_a2 = parse_module_ast(&db, module(&db, "a", source_a));
        let _result_b2 = parse_module_ast(&db, module(&db, "b", source_b));
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
        // Interned, so these give the same modules however often they run.
        fn module<'db>(db: &'db Database, path: &str, source: bct::input::Source)
            -> bct::module_graph::Module<'db>
        {
            bct::module_graph::Module::new(
                db, bct::module_graph::ModuleId::new(db, path.to_string()), source,
            )
        }

        // First parse.
        let _r1 = parse_module_ast(&db, module(&db, "a", source_a));
        let _r2 = parse_module_ast(&db, module(&db, "b", source_b));

        // Second parse with NO changes - should be fully cached.
        enable_query_logging();
        let _r3 = parse_module_ast(&db, module(&db, "a", source_a));
        let _r4 = parse_module_ast(&db, module(&db, "b", source_b));
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
        // Interned, so these give the same modules however often they run.
        fn module<'db>(db: &'db LoggingDatabase, path: &str, source: bct::input::Source)
            -> bct::module_graph::Module<'db>
        {
            bct::module_graph::Module::new(
                db, bct::module_graph::ModuleId::new(db, path.to_string()), source,
            )
        }

        // First parse.
        let _r1 = parse_module_ast(&db, module(&db, "a", source_a));
        let _r2 = parse_module_ast(&db, module(&db, "b", source_b));

        db.clear_events();

        // Change only B.
        source_b.set_text(&mut db).to("let y = 999".to_string());

        // Parse both again.
        enable_query_logging();
        let _r3 = parse_module_ast(&db, module(&db, "a", source_a));
        let _r4 = parse_module_ast(&db, module(&db, "b", source_b));
        let log = disable_query_logging();

        let executed = db.executed_queries();
        eprintln!("Salsa executed queries after B change: {:?}", executed);

        let parsed = get_executed_modules(&log, "parse");
        eprintln!("Query log parsed after B change: {:?}", parsed);
    }

    #[test]
    fn test_datafun_spans_consistency() {
        // Verifies that datafun_spans produces consistent results for unchanged source.
        // DatafunSpans is now a plain struct (not salsa interned/tracked).
        let mut db = Database::default();

        let source_a = bct::input::Source::new(&db, "let x = 1".to_string());
        let source_b = bct::input::Source::new(&db, "let y = 2".to_string());

        // Take the keys as owned data: an ExprKey borrows the database, so it
        // cannot be carried across the edit below, which is the point of it.
        let owned_keys = |spans: &datalove_datafun_ast::spans::DatafunSpans<'_>, db: &Database| {
            spans.entries.iter()
                .map(|e| (
                    e.expr_key.fn_name.map(|n| n.as_str(db).to_string()),
                    e.expr_key.local_index,
                ))
                .collect::<Vec<_>>()
        };

        let entries_a1 = {
            let spans_a1 = datalove_datafun_parser::datafun_spans(&db, source_a);
            let _ = datalove_datafun_parser::datafun_spans(&db, source_b);
            owned_keys(&spans_a1, &db)
        };

        // Change only B.
        source_b.set_text(&mut db).to("let y = 999".to_string());

        let entries_a2 = {
            let spans_a2 = datalove_datafun_parser::datafun_spans(&db, source_a);
            owned_keys(&spans_a2, &db)
        };

        // Verify entries are consistent for unchanged source A.
        assert!(!entries_a1.is_empty(), "First call has entries");
        assert!(!entries_a2.is_empty(), "Second call also has entries (not empty)");
        assert_eq!(entries_a1, entries_a2, "Entries should be identical for unchanged source");
    }

    // ========================================================================
    // Typecheck Per-Module Caching Tests
    // ========================================================================

    use datalove_datafun_tycheck::{typecheck_module_graph, AutoAdaptMode};
    use datalove_datafun_resolve::{
        resolve_all_names,
        ParsedModuleGraph,
    };

    /// Helper that calls resolve functions and then typecheck_module_graph.
    fn resolve_and_typecheck<'db>(
        db: &'db dyn salsa::Database,
        parsed: ParsedModuleGraph<'db>,
    ) -> datalove_datafun_tycheck::ModuleGraphTypecheckResult<'db> {
        let all_names = resolve_all_names(db, parsed);
        typecheck_module_graph(db, parsed, all_names, AutoAdaptMode::Disabled)
    }

    #[test]
    fn test_typecheck_records_all_modules() {
        // Verify typecheck logging records all modules on first run.
        let db = Database::default();

        let (graph, ids) = build_graph(&db, &[
            ("b", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "fun main(): i32\n  ret 2\nend fun"),
        ]);
        let mut requires = BTreeMap::new();
        requires.insert(ids[1], vec![("b".to_string(), ids[0])]);

        let parsed = parse_module_graph(&db, graph, requires, Vec::new());

        enable_query_logging();
        let _result = resolve_and_typecheck(&db, parsed);
        let log = disable_query_logging();

        let typechecked = get_executed_modules(&log, "typecheck");
        eprintln!("Typechecked modules: {:?}", typechecked);
        assert_eq!(typechecked.len(), 2, "should typecheck both modules");
        assert!(typechecked.contains(&"a".to_string()), "should typecheck a");
        assert!(typechecked.contains(&"b".to_string()), "should typecheck b");
    }

    #[test]
    fn test_typecheck_per_module_caching() {
        // Verify only changed module re-typechecks when source mutated.
        let mut db = Database::default();

        // Create modules with Sources we can mutate.
        let source_b = bct::input::Source::new(&db, "fun helper(): i32\n  ret 1\nend fun".to_string());
        let source_a = bct::input::Source::new(&db, "fun main(): i32\n  ret 2\nend fun".to_string());

        // The graph is interned, so building it again from the same sources
        // gives back the same graph, which is what lets each phase build its
        // own rather than one being held across the edit.
        fn build<'db>(db: &'db Database, source_a: bct::input::Source, source_b: bct::input::Source)
            -> (ModuleGraph<'db>, ModuleId<'db>, ModuleId<'db>, BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>)
        {
            let mut builder = ModuleGraphBuilder::new(db);
            let id_b = builder.add_module("b".to_string(), source_b);
            let id_a = builder.add_module("a".to_string(), source_a);
            let mut requires = BTreeMap::new();
            requires.insert(id_a, vec![("b".to_string(), id_b)]);
            (builder.build(), id_a, id_b, requires)
        }

        // First run: both modules should typecheck.
        let (graph, _, _, requires) = build(&db, source_a, source_b);
        let parsed1 = parse_module_graph(&db, graph.clone(), requires.clone(), Vec::new());
        enable_query_logging();
        let _result1 = resolve_and_typecheck(&db, parsed1);
        let log1 = disable_query_logging();

        let first_tc = get_executed_modules(&log1, "typecheck");
        eprintln!("First run typechecked: {:?}", first_tc);
        assert_eq!(first_tc.len(), 2, "first run should typecheck both modules");

        // Mutate only B's source.
        source_b.set_text(&mut db).to("fun helper(): i32\n  ret 999\nend fun".to_string());

        // Second run: only B should re-typecheck.
        let (graph, _, _, requires) = build(&db, source_a, source_b);
        let parsed2 = parse_module_graph(&db, graph, requires, Vec::new());
        enable_query_logging();
        let _result2 = resolve_and_typecheck(&db, parsed2);
        let log2 = disable_query_logging();

        let second_tc = get_executed_modules(&log2, "typecheck");
        eprintln!("Second run typechecked: {:?}", second_tc);

        // Per-module caching: only B should re-typecheck.
        assert_eq!(second_tc.len(), 1, "only changed module should re-typecheck");
        assert!(second_tc.contains(&"b".to_string()), "b should re-typecheck");
        assert!(!second_tc.contains(&"a".to_string()), "a should be cached");
    }

    #[test]
    fn test_typecheck_no_change_fully_cached() {
        // Verify no re-typecheck when nothing changes.
        let db = Database::default();

        let source_a = bct::input::Source::new(&db, "fun main(): i32\n  ret 1\nend fun".to_string());
        let mut builder = ModuleGraphBuilder::new(&db);
        let _ = builder.add_module("a".to_string(), source_a);
        let graph = builder.build();

        // First run.
        let parsed1 = parse_module_graph(&db, graph.clone(), BTreeMap::new(), Vec::new());
        let _result1 = resolve_and_typecheck(&db, parsed1);

        // Second run with no changes.
        let parsed2 = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
        enable_query_logging();
        let _result2 = resolve_and_typecheck(&db, parsed2);
        let log = disable_query_logging();

        let typechecked = get_executed_modules(&log, "typecheck");
        eprintln!("No change, second run: {:?}", typechecked);
        assert_eq!(typechecked.len(), 0, "no changes = fully cached");
    }

    #[test]
    fn test_typecheck_with_import_caching() {
        // Verify caching works when modules have imports.
        let mut db = Database::default();

        // B exports a function, A imports it.
        let source_b = bct::input::Source::new(&db, "fun helper(): i32\n  ret 1\nend fun".to_string());
        let source_a = bct::input::Source::new(&db,
            "require module /test/b\nimport b.helper\nfun main(): i32\n  ret helper()\nend fun".to_string());

        // Interned, so each phase can build its own rather than one being held
        // across the edit; they are the same graph.
        fn build<'db>(db: &'db Database, source_a: bct::input::Source, source_b: bct::input::Source)
            -> (ModuleGraph<'db>, ModuleId<'db>, ModuleId<'db>, BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>)
        {
            let mut builder = ModuleGraphBuilder::new(db);
            let id_b = builder.add_module("test/b".to_string(), source_b);
            let id_a = builder.add_module("test/a".to_string(), source_a);
            let mut requires = BTreeMap::new();
            requires.insert(id_a, vec![("b".to_string(), id_b)]);
            (builder.build(), id_a, id_b, requires)
        }

        // First run.
        let (graph, _, _, requires) = build(&db, source_a, source_b);
        let parsed1 = parse_module_graph(&db, graph.clone(), requires.clone(), Vec::new());
        enable_query_logging();
        let _result1 = resolve_and_typecheck(&db, parsed1);
        let log1 = disable_query_logging();

        let first_tc = get_executed_modules(&log1, "typecheck");
        eprintln!("With imports, first run: {:?}", first_tc);
        assert_eq!(first_tc.len(), 2);

        // Change only A (the importing module).
        source_a.set_text(&mut db).to(
            "require module /test/b\nimport b.helper\nfun main(): i32\n  ret 42\nend fun".to_string());

        // Second run: only A should re-typecheck.
        let (graph, _, _, requires) = build(&db, source_a, source_b);
        let parsed2 = parse_module_graph(&db, graph, requires, Vec::new());
        enable_query_logging();
        let _result2 = resolve_and_typecheck(&db, parsed2);
        let log2 = disable_query_logging();

        let second_tc = get_executed_modules(&log2, "typecheck");
        eprintln!("With imports, second run after A change: {:?}", second_tc);
        assert_eq!(second_tc.len(), 1, "only A should re-typecheck");
        assert!(second_tc.contains(&"test/a".to_string()), "a should re-typecheck");
    }

    #[test]
    fn test_import_resolution_no_change_cached() {
        // Verify import resolution is fully cached when nothing changes.
        let db = Database::default();

        // B exports a function, A imports it.
        let source_b = bct::input::Source::new(&db, "fun helper(): i32\n  ret 1\nend fun".to_string());
        let source_a = bct::input::Source::new(&db,
            "require module /test/b\nimport b.helper\nfun main(): i32\n  ret helper()\nend fun".to_string());

        let mut builder = ModuleGraphBuilder::new(&db);
        let id_b = builder.add_module("test/b".to_string(), source_b);
        let id_a = builder.add_module("test/a".to_string(), source_a);
        let graph = builder.build();

        let mut requires = BTreeMap::new();
        requires.insert(id_a, vec![("b".to_string(), id_b)]);

        // First run: both modules should have imports resolved.
        let parsed1 = parse_module_graph(&db, graph.clone(), requires.clone(), Vec::new());
        enable_query_logging();
        let _result1 = resolve_and_typecheck(&db, parsed1);
        let log1 = disable_query_logging();

        let first_resolved = get_executed_modules(&log1, "resolve_imports");
        eprintln!("First run resolved imports: {:?}", first_resolved);
        assert_eq!(first_resolved.len(), 2, "first run should resolve imports for both modules");

        // Second run with same inputs: should be fully cached.
        let parsed2 = parse_module_graph(&db, graph, requires, Vec::new());
        enable_query_logging();
        let _result2 = resolve_and_typecheck(&db, parsed2);
        let log2 = disable_query_logging();

        let second_resolved = get_executed_modules(&log2, "resolve_imports");
        eprintln!("Second run (no change) resolved imports: {:?}", second_resolved);
        assert!(second_resolved.is_empty(), "no imports should be re-resolved when nothing changed");
    }

    #[test]
    fn test_parallel_import_resolution_is_memoized() {
        // Verify parallel import resolution warms the cache correctly.
        // Note: Salsa events from worker threads aren't captured by LoggingDatabase,
        // so we verify memoization by checking that sequential run after parallel
        // is fully cached (proving the parallel phase warmed the cache).
        let db = LoggingDatabase::new();

        // Create modules with imports.
        let (graph, ids) = build_graph_logging(&db, &[
            ("b", "fun helper(): i32\n  ret 1\nend fun"),
            ("a", "require module /test/b\nimport b.helper\nfun main(): i32\n  ret helper()\nend fun"),
        ]);
        let mut requires = BTreeMap::new();
        requires.insert(ids[1], vec![("b".to_string(), ids[0])]);

        // First: parallel typecheck populates cache (including import resolution).
        let parsed = parse_module_graph(&db, graph.clone(), requires.clone(), Vec::new());
        let _result1 = resolve_and_typecheck_parallel(&db, parsed);

        // Clear events after parallel phase.
        db.clear_events();

        // Second: sequential should hit cache for everything.
        let parsed2 = parse_module_graph(&db, graph, requires, Vec::new());
        let _result2 = resolve_and_typecheck(&db, parsed2);

        let executed = db.executed_queries();
        let resolve_queries: Vec<_> = executed.iter()
            .filter(|q| q.contains("resolve_module_imports"))
            .collect();

        assert!(
            resolve_queries.is_empty(),
            "sequential after parallel should have cached import resolution, but executed: {:?}",
            resolve_queries
        );
        eprintln!("Sequential after parallel: {} total events, {} resolve_imports queries (should be 0)",
            executed.len(), resolve_queries.len());
    }

    /// A clone shares the memo cache, and its queries reach the same event log.
    ///
    /// This used to parse on the original first and then check nothing, which
    /// made the clone's parse a cache hit with nothing left to see. It ended in
    /// an `eprintln!`. The order is the other way round now: the clone does the
    /// work and the original is what is asked about.
    #[test]
    fn test_clone_shares_cache_and_event_log() {
        let db = LoggingDatabase::new();
        let (graph, _ids) = build_graph_logging(&db, &[("a", "let x = 1")]);

        // Parse on a clone, with the original's log watching.
        db.clear_events();
        let db_clone = db.clone();
        let _parsed = parse_module_graph(&db_clone, graph.clone(), BTreeMap::new(), Vec::new());
        assert!(
            !db.executed_queries().is_empty(),
            "a clone's queries have to reach the log the original reads",
        );

        // And the original finds that work already done.
        db.clear_events();
        let _parsed = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
        assert!(
            db.executed_queries().is_empty(),
            "the original should see the clone's work as cached, but ran {:?}",
            db.executed_queries(),
        );
    }

    #[test]
    fn test_parallel_clone_doesnt_break_original() {
        // Test that using clones in parallel doesn't break the original db.
        let db = LoggingDatabase::new();
        let (graph, _ids) = build_graph_logging(&db, &[
            ("a", "let x = 1"),
            ("b", "let y = 2"),
        ]);

        // Create clones and use them in parallel.
        let modules: Vec<_> = graph.iter_modules(&db).collect();
        let work: Vec<_> = modules.iter()
            .map(|&module| (db.clone(), module))
            .collect();

        work.into_par_iter()
            .for_each(|(db_clone, module)| {
                let _ = parse_module_full(&db_clone, module);
            });

        eprintln!("Parallel phase complete, trying to use original db...");

        // Now try to use the original db.
        let _parsed = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
        eprintln!("Original db still works after parallel phase");
    }

    #[test]
    fn test_parallel_parse_is_memoized() {
        let db = LoggingDatabase::new();

        // Create multiple modules.
        let (graph, _ids) = build_graph_logging(&db, &[
            ("a", "let x = 1"),
            ("b", "let y = 2"),
            ("c", "let z = 3"),
        ]);

        // First parallel parse populates cache.
        let _parsed1 = parse_module_graph_parallel(&db, graph.clone(), BTreeMap::new(), Vec::new());

        // Record which queries executed during first parse.
        let first_executed = db.executed_queries();
        assert!(
            !first_executed.is_empty(),
            "first parse should execute queries"
        );
        eprintln!("First parse executed {} queries", first_executed.len());

        // Clear events.
        db.clear_events();

        // Second parse should be fully memoized.
        let _parsed2 = parse_module_graph_parallel(&db, graph.clone(), BTreeMap::new(), Vec::new());

        let second_executed = db.executed_queries();
        // Filter to just parse_module_full queries (not input lookups).
        let parse_queries: Vec<_> = second_executed.iter()
            .filter(|q| q.contains("parse_module_full"))
            .collect();

        assert!(
            parse_queries.is_empty(),
            "second parse should be memoized, but executed parse queries: {:?}",
            parse_queries
        );
        eprintln!("Second parse: {} total events, {} parse queries", second_executed.len(), parse_queries.len());
    }

    // ========================================================================
    // Parallel Typecheck Tests
    // ========================================================================

    use datalove_datafun_tycheck::{
        typecheck_module_graph_parallel,
        typecheck_module_graph_with_mode,
        ParallelMode as TypecheckParallelMode,
    };
    use datalove_datafun_resolve::{
        resolve_all_names_parallel,
        DbClone,
    };
    // Note: resolve_all_names, ParsedModuleGraph, AutoAdaptMode
    // are already imported in the earlier test section.

    /// Helper that calls resolve functions and then typecheck_module_graph_parallel.
    fn resolve_and_typecheck_parallel<'db>(
        db: &'db dyn DbClone,
        parsed: ParsedModuleGraph<'db>,
    ) -> datalove_datafun_tycheck::ModuleGraphTypecheckResult<'db> {
        let all_names = resolve_all_names_parallel(db, parsed);
        typecheck_module_graph_parallel(db, parsed, all_names, AutoAdaptMode::Disabled)
    }

    /// Helper that calls resolve functions and then typecheck_module_graph_with_mode.
    fn resolve_and_typecheck_with_mode<'db>(
        db: &'db dyn DbClone,
        parsed: ParsedModuleGraph<'db>,
        mode: TypecheckParallelMode,
    ) -> datalove_datafun_tycheck::ModuleGraphTypecheckResult<'db> {
        let all_names = resolve_all_names(db.as_salsa_db(), parsed);
        typecheck_module_graph_with_mode(db, parsed, all_names, mode, AutoAdaptMode::Disabled)
    }

    #[test]
    fn test_parallel_typecheck_is_memoized() {
        // Verify that parallel typechecking warms the cache correctly: a
        // sequential run after it executes nothing.
        let db = LoggingDatabase::new();

        // Create multiple modules.
        let (graph, _ids) = build_graph_logging(&db, &[
            ("a", "fun fa(): i32\n  ret 1\nend fun"),
            ("b", "fun fb(): i32\n  ret 2\nend fun"),
            ("c", "fun fc(): i32\n  ret 3\nend fun"),
        ]);

        // First: parallel typecheck populates cache.
        let parsed = parse_module_graph(&db, graph.clone(), BTreeMap::new(), Vec::new());
        let _result1 = resolve_and_typecheck_parallel(&db, parsed);

        // Clear events after parallel phase.
        db.clear_events();

        // Second: sequential typecheck should hit cache (warmed by parallel).
        // If parallel didn't populate the cache, this would show typecheck_module executions.
        let parsed2 = parse_module_graph(&db, graph, BTreeMap::new(), Vec::new());
        let _result2 = resolve_and_typecheck(&db, parsed2);

        let executed = db.executed_queries();
        let typecheck_queries: Vec<_> = executed.iter()
            .filter(|q| q.contains("typecheck_module"))
            .collect();

        assert!(
            typecheck_queries.is_empty(),
            "sequential after parallel should be cached, but executed: {:?}",
            typecheck_queries
        );
        eprintln!("Sequential after parallel: {} total events, {} typecheck queries (should be 0)",
            executed.len(), typecheck_queries.len());
    }

    #[test]
    fn test_parallel_typecheck_produces_same_results() {
        // Verify that parallel and sequential typechecking produce identical results.
        let db = Database::default();

        // Create modules with imports to test cross-module resolution.
        let (graph, ids) = build_graph(&db, &[
            ("b", "fun helper(): i32\n  ret 42\nend fun"),
            ("a", "require module /test/b\nimport b.helper\nfun main(): i32\n  ret helper()\nend fun"),
        ]);
        let mut requires = BTreeMap::new();
        requires.insert(ids[1], vec![("b".to_string(), ids[0])]);

        let parsed = parse_module_graph(&db, graph, requires, Vec::new());

        // Typecheck sequentially.
        let result_seq = resolve_and_typecheck_with_mode(&db, parsed, TypecheckParallelMode::Sequential);

        // Typecheck in parallel (on fresh db to avoid cache).
        let db2 = Database::default();
        let (graph2, ids2) = build_graph(&db2, &[
            ("b", "fun helper(): i32\n  ret 42\nend fun"),
            ("a", "require module /test/b\nimport b.helper\nfun main(): i32\n  ret helper()\nend fun"),
        ]);
        let mut requires2 = BTreeMap::new();
        requires2.insert(ids2[1], vec![("b".to_string(), ids2[0])]);
        let parsed2 = parse_module_graph(&db2, graph2, requires2, Vec::new());
        let result_par = resolve_and_typecheck_with_mode(&db2, parsed2, TypecheckParallelMode::Parallel);

        // Compared per module, and by what a module says rather than by how
        // much it says: the tracked structs hold salsa ids, which are two
        // different databases' ids here and would never match.
        //
        // What a module says is its path, what it exports, how many
        // expressions got a type, and what it complained about. An earlier
        // version of this compared the length of two graph-wide tables that
        // existed for it to compare, which is a test agreeing with itself.
        fn summary<'db>(
            db: &'db Database,
            result: datalove_datafun_tycheck::ModuleGraphTypecheckResult<'db>,
        ) -> Vec<(String, Vec<String>, usize, Vec<String>)> {
            let mut per_module: Vec<_> = result.module_results(db).iter()
                .map(|(module_id, single)| {
                    let mut exports: Vec<String> = single.exports(db).iter()
                        .map(|(name, _)| name.text(db).S())
                        .collect();
                    exports.sort();
                    let errors: Vec<String> = single.errors(db).iter()
                        .map(|e| format!("{:?}", e))
                        .collect();
                    (
                        module_id.path(db).clone(),
                        exports,
                        single.expr_types(db).len(),
                        errors,
                    )
                })
                .collect();
            per_module.sort();
            per_module
        }

        let seq = summary(&db, result_seq);
        let par = summary(&db2, result_par);
        assert!(!seq.is_empty(), "the comparison has to be over something");
        assert_eq!(seq, par, "parallel and sequential should agree per module");

        // Both should succeed (no errors).
        assert!(result_seq.is_ok(&db), "sequential should succeed");
        assert!(result_par.is_ok(&db2), "parallel should succeed");
    }

    /// An edit under the parallel path re-typechecks the module that changed.
    ///
    /// Both runs used to call the *sequential* path, so the test did not test
    /// what it is named for. It went that way because `query_log` is
    /// thread-local and cannot see what rayon ran. `QueryRecorder` listens for
    /// salsa's own `WillExecute`, and salsa's event callback lives on the
    /// `Zalsa` a database clone shares, so a worker's queries are recorded like
    /// any other.
    ///
    /// Counted rather than attributed: `typecheck_module` takes six arguments,
    /// so salsa keys it on an interned tuple whose id names no module.
    #[test]
    fn test_parallel_typecheck_per_module_caching() {
        use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};

        fn typechecked(executed: &[ExecutedQuery]) -> usize {
            executed.iter().filter(|q| q.query == "typecheck_module").count()
        }

        let recorder = QueryRecorder::new();
        let mut db = Database::recording(&recorder);

        // Create modules with mutable sources.
        let source_b = bct::input::Source::new(&db, "fun helper(): i32\n  ret 1\nend fun".to_string());
        let source_a = bct::input::Source::new(&db, "fun main(): i32\n  ret 2\nend fun".to_string());

        // Interned, so each phase builds the same graph rather than one being
        // held across the edit.
        fn build<'db>(db: &'db Database, source_a: bct::input::Source, source_b: bct::input::Source)
            -> (ModuleGraph<'db>, BTreeMap<ModuleId<'db>, Vec<(String, ModuleId<'db>)>>)
        {
            let mut builder = ModuleGraphBuilder::new(db);
            let id_b = builder.add_module("b".to_string(), source_b);
            let id_a = builder.add_module("a".to_string(), source_a);
            let mut requires = BTreeMap::new();
            requires.insert(id_a, vec![("b".to_string(), id_b)]);
            (builder.build(), requires)
        }

        // First run: both modules are cold, so both typecheck. Without this the
        // assertion below would pass on a run that typechecked nothing.
        let (graph, requires) = build(&db, source_a, source_b);
        let parsed1 = parse_module_graph(&db, graph.clone(), requires.clone(), Vec::new());
        recorder.clear();
        let _result1 = resolve_and_typecheck_parallel(&db, parsed1);
        assert_eq!(
            typechecked(&recorder.take()), 2,
            "a first run has to typecheck both modules",
        );

        // Mutate only B's source.
        source_b.set_text(&mut db).to("fun helper(): i32\n  ret 999\nend fun".to_string());

        let (graph, requires) = build(&db, source_a, source_b);
        let parsed2 = parse_module_graph(&db, graph, requires, Vec::new());
        recorder.clear();
        let _result2 = resolve_and_typecheck_parallel(&db, parsed2);
        assert_eq!(
            typechecked(&recorder.take()), 1,
            "one module changed, so one module typechecks",
        );
    }
}
