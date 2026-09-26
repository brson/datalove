//! Editing one module costs what changed, not what exists.
//!
//! `incremental_memo_tests` pins this for `extract_dependencies`, which is one
//! step of the pipeline. This pins it for a whole compile, which is where it
//! is hard to hold and where it kept being lost: a query that walks the module
//! graph and is keyed on it re-runs whenever anything moves, and the only
//! symptom is that an edit in a large world costs more than an edit in a small
//! one. Nothing about the output changes, so no fixture notices.
//!
//! Counting queries rather than timing, because the count is exact, the same
//! on every machine, and says *which* query went wrong when it moves.
//!
//! # What this does not catch
//!
//! A query that *executes once* but *walks every module* looks identical here,
//! because the count of executions is one either way. `graph_declares_consts`
//! was exactly that: keyed on the graph, re-run on every edit, and reading a
//! tracked field on every function in the program. It cost 22% of an edit and
//! this test passes with it reinstated -- which was checked, not assumed.
//!
//! No salsa event separates the two. `WillExecute`, `DidValidateMemoizedValue`
//! and the rest come out within one count of each other either way, because
//! the cost is tracked-field reads and edge recording and neither is an event.
//! `memory_usage().queries` does see it -- the bad version's single memo is
//! 1584 bytes against 552 at 64 modules, because it holds an edge per function
//! rather than one per module -- but both grow with the world, at 6.6x against
//! 5.3x, which is too close to assert on. It is a good thing to look at when
//! investigating and a bad thing to write a threshold against.
//!
//! So that class is caught by profiling and nothing else. The two examples in
//! `datalove-bench` are the guard: `recompile_profile` takes a module count for
//! this reason.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun::pipeline::ModuleCompilationPipeline;
use datalove_datafun_compiler::Database;

/// A module of three functions, one of which returns `salt`.
fn module_source(index: usize, salt: usize) -> String {
    format!(
        "fun a{index}(): int\n  ret {index}\nend fun\n\
         fun b{index}(x: int, y: int): int\n  let s = x + y\n  ret s + {index}\nend fun\n\
         fun c{index}(): int\n  ret {salt}\nend fun\n"
    )
}

/// What one edit of one module ran, in a world of `modules` modules.
fn queries_for_one_edit(modules: usize) -> Vec<ExecutedQuery> {
    let recorder = QueryRecorder::new();
    let mut db = Database::recording(&recorder);

    let mut pipeline = ModuleCompilationPipeline::default();
    for index in 0..modules {
        pipeline.add_module(&db, "local", "test", &format!("m{index}"), &module_source(index, 0));
    }
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "setup failed: {:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();

    pipeline.update_source(&mut db, "local", "test", "m0", &module_source(0, 7));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    recorder.take()
}

fn counts(executed: &[ExecutedQuery]) -> BTreeMap<String, usize> {
    let mut by_query: BTreeMap<String, usize> = BTreeMap::new();
    for one in executed {
        *by_query.entry(one.query.C()).or_default() += 1;
    }
    by_query
}

/// An edit runs the same queries whatever the world around it is.
///
/// Compared query by query rather than by total, so a failure names the query
/// that started scaling instead of leaving the next person to find it. A query
/// that legitimately has to run once per *edited* module still passes; one that
/// runs once per module in the world does not.
#[test]
fn an_edit_runs_the_same_queries_whatever_the_world_size() {
    let small = counts(&queries_for_one_edit(8));
    let large = counts(&queries_for_one_edit(64));

    assert!(!small.is_empty(), "an edit has to run something, or this is vacuous");

    let mut grew: Vec<String> = Vec::new();
    for (query, large_count) in &large {
        let small_count = small.get(query).copied().unwrap_or(0);
        if *large_count > small_count {
            grew.push(format!("{query}: {small_count} at 8 modules, {large_count} at 64"));
        }
    }

    assert!(
        grew.is_empty(),
        "editing one module ran more queries in a world of 64 than in a world \
         of 8, so the cost follows the world rather than the edit:\n  {}",
        grew.join("\n  "),
    );
}

/// And a first build does grow with the world, so the above is not vacuous.
#[test]
fn a_first_build_does_grow_with_the_world() {
    let recorder = QueryRecorder::new();

    let build = |modules: usize| {
        let db = Database::recording(&recorder);
        let mut pipeline = ModuleCompilationPipeline::default();
        for index in 0..modules {
            pipeline.add_module(&db, "local", "test", &format!("m{index}"), &module_source(index, 0));
        }
        recorder.clear();
        let compiled = pipeline.compile_fresh(&db);
        assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
        drop(compiled);
        recorder.take().len()
    };

    let small = build(8);
    let large = build(64);
    assert!(
        large > small * 4,
        "building 64 modules ran {large} queries against {small} for 8; if that \
         does not grow, the comparison in the test above is measuring nothing",
    );
}
