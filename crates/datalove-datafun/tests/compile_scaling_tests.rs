//! Every tracked pass depends on no more than its key says it does.
//!
//! The rest of the suite holds *what runs*. Nothing held *how much work a run
//! does*, and that is where the regressions of the last sessions lived: a
//! query keyed on the module graph that reads each of the program's functions
//! executes once either way, produces identical output, and costs time
//! proportional to the whole program. `graph_declares_consts` was exactly that
//! at 22% of an edit. No fixture saw it, no query count saw it, and no salsa
//! event saw it -- `WillExecute` and `DidValidateMemoizedValue` come out within
//! one count of each other with it reinstated, because the cost is
//! tracked-field reads and edge recording and neither fires an event.
//!
//! What does see it is the size of the memo. A memo holds the dependencies the
//! query read, so its size measures what the query looked at. That gives one
//! rule with two halves, and between them they cover every tracked function in
//! the compiler without naming any of them:
//!
//! - a query with **one memo** may depend on each module, and must not depend
//!   on each function -- so its memo must not grow when the modules are given
//!   more functions;
//! - a query with **one memo per module** may depend on its own module's
//!   functions, and must not depend on the rest of the world -- so its memo
//!   must not grow when there are more modules.
//!
//! Both are exact equalities on byte counts, which are the same on every
//! machine, and a failure names the query.
//!
//! In one line: **do not depend on anything finer-grained than your key.** A
//! pass that needs to know something per function should ask a query keyed per
//! module for it, the way `graph_declares_consts` asks `module_const_kinds`.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun::pipeline::ModuleCompilationPipeline;
use datalove_datafun_compiler::Database;

/// How many memos a query has, and what they cost between them.
type MemoSizes = BTreeMap<String, (usize, usize)>;

fn module_source(index: usize, functions: usize, salt: usize) -> String {
    let mut source = String::new();
    for f in 0..functions {
        source.push_str(&format!(
            "fun g{index}_{f}(x: int, y: int): int\n  let s = x + y\n  ret s + {salt}\nend fun\n"
        ));
    }
    source
}

/// Compile a world of this shape and report every query's memo footprint.
fn memo_sizes(modules: usize, functions_per_module: usize) -> MemoSizes {
    let db = Database::default();
    let mut pipeline = ModuleCompilationPipeline::default();
    for index in 0..modules {
        pipeline.add_module(
            &db, "local", "test", &format!("m{index}"),
            &module_source(index, functions_per_module, 0),
        );
    }
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    <dyn salsa::Database>::memory_usage(&db).queries.iter()
        .map(|(name, info)| {
            (name.S(), (info.count(), info.size_of_fields() + info.size_of_metadata()))
        })
        .collect()
}

/// A query keyed on the whole graph must not read every function.
///
/// Holds the module count still and puts four times as many functions in each
/// module. A query with one memo may look at each module -- that is what being
/// keyed on the graph means, and it is why the module count is what is held
/// still -- but it must not look at each function.
///
/// Checked rather than assumed: reinstating the `graph_declares_consts`
/// regression takes its memo from 816 bytes to 3120 here, while every other
/// single-memo query stays identical to the byte.
#[test]
fn a_graph_wide_query_does_not_depend_on_every_function() {
    const MODULES: usize = 32;
    let few = memo_sizes(MODULES, 3);
    let many = memo_sizes(MODULES, 12);

    let mut checked = 0;
    let mut grew: Vec<String> = Vec::new();
    for (query, (count, with_many)) in &many {
        let Some((few_count, with_few)) = few.get(query).copied() else { continue };
        // One memo in both worlds is what makes a query graph-wide. One with a
        // memo per module legitimately grows here, because its module did.
        if *count != 1 || few_count != 1 {
            continue;
        }
        checked += 1;
        if *with_many != with_few {
            grew.push(format!(
                "{query}: {with_few} bytes with 3 functions a module, {with_many} with 12"
            ));
        }
    }

    assert!(checked > 5, "only {checked} graph-wide queries found; this is not testing much");
    assert!(
        grew.is_empty(),
        "a query keyed on the whole graph grew when the modules gained \
         functions, so it is reading each one rather than asking something \
         keyed per module:\n  {}",
        grew.join("\n  "),
    );
}

/// A query keyed on a module must not read the rest of the world.
///
/// Holds the functions per module still and puts four times as many modules in.
/// Compared per memo, so a query that simply has more memos passes; one whose
/// *each* memo grew does not, because that means it started reading, or
/// returning, more than its own module.
///
/// **Weaker than its sibling, and here is the limit.** A memo's size counts
/// the dependencies it read and the value it holds. An *interned* aggregate is
/// one dependency however much it aggregates, so widening
/// `reachable_func_ids` from a module's reachable entries to the whole world's
/// was tried here and this does not notice -- the key changes, the memo does
/// not. That is the same property that makes interning worth doing, so it is
/// not a fault to fix, but it does bound what this test is worth.
///
/// No failing case has been constructed for it, because a per-module query in
/// this compiler has no handle on the world to fan out through. It is here to
/// notice when one gains that.
#[test]
fn a_per_module_query_does_not_depend_on_the_rest_of_the_world() {
    const FUNCTIONS: usize = 3;
    let small = memo_sizes(32, FUNCTIONS);
    let large = memo_sizes(128, FUNCTIONS);

    let mut checked = 0;
    let mut grew: Vec<String> = Vec::new();
    for (query, (large_count, large_bytes)) in &large {
        let Some((small_count, small_bytes)) = small.get(query).copied() else { continue };
        if *large_count <= 1 || small_count <= 1 {
            continue;
        }
        checked += 1;
        // Cross-multiplied rather than divided, so a count that is not a clean
        // multiple cannot round a difference away.
        if large_bytes * small_count != small_bytes * large_count {
            grew.push(format!(
                "{query}: {} bytes a memo across 32 modules, {} across 128",
                small_bytes / small_count, large_bytes / large_count,
            ));
        }
    }

    assert!(checked > 10, "only {checked} per-module queries found; this is not testing much");
    assert!(
        grew.is_empty(),
        "a query keyed on one module grew when other modules were added, so it \
         depends on more of the world than its key says:\n  {}",
        grew.join("\n  "),
    );
}

/// What one edit of one module ran, in a world of `modules` modules.
fn queries_for_one_edit(modules: usize) -> Vec<ExecutedQuery> {
    let recorder = QueryRecorder::new();
    let mut db = Database::recording(&recorder);

    let mut pipeline = ModuleCompilationPipeline::default();
    for index in 0..modules {
        pipeline.add_module(&db, "local", "test", &format!("m{index}"), &module_source(index, 3, 0));
    }
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "setup failed: {:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();

    pipeline.update_source(&mut db, "local", "test", "m0", &module_source(0, 3, 7));
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
/// The other half of the pair: the two above say no query *looks at* more than
/// its key allows, and this says no query *runs* more often than the edit
/// warrants. Compared query by query, so a failure names the one that scaled.
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
            pipeline.add_module(&db, "local", "test", &format!("m{index}"), &module_source(index, 3, 0));
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
