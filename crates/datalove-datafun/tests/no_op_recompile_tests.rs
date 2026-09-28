//! Recompiling an unchanged world runs no queries at all.
//!
//! The `module_memo` fixtures ask a narrower question - which modules were
//! parsed, resolved, typechecked, lowered - and answer it from fourteen logging
//! calls placed by hand in the compiler, out of a hundred and sixty-eight
//! tracked functions. A query nobody annotated is invisible to them, and so is
//! anything rayon runs, because that log is thread-local.
//!
//! These ask a wider question that needs no annotation: did salsa run *any*
//! query. Salsa reports each one it executes, so nothing can hide, wherever it
//! ran. The narrower question is the more useful one when it fails, but this
//! one covers every query there is, which is the part the fixtures cannot do.

use rmx::prelude::*;

use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun::incremental::{IncrementalModuleWorld, Roots, extract_dependencies};
use datalove_datafun_compiler::Database;
use datalove_datafun_compiler::module_graph::parse_module_graph;

/// Build a world of `modules` modules, compile it, then compile it again
/// unchanged and report everything salsa ran the second time.
fn queries_on_unchanged_recompile(modules: usize) -> Vec<ExecutedQuery> {
    let recorder = QueryRecorder::new();
    let db = Database::recording(&recorder);

    let mut world = IncrementalModuleWorld::new();
    for index in 0..modules {
        world.add_module(
            &db,
            &format!("local/test/m{}", index),
            &format!("fun f{}(): i32\n  ret {}\nend fun\n", index, index),
        );
    }

    let compile = |world: &IncrementalModuleWorld| {
        let deps = extract_dependencies(world, &db, &Roots::All);
        let (graph, requires) = world.build_graph(&db, &deps, &Roots::All);
        let _ = parse_module_graph(&db, graph, requires, Vec::new());
    };

    compile(&world);
    recorder.clear();
    compile(&world);
    recorder.take()
}

/// Nothing changed, so nothing should run.
#[test]
fn an_unchanged_recompile_runs_no_queries() {
    for modules in [1, 4, 12] {
        let ran = queries_on_unchanged_recompile(modules);
        assert!(
            ran.is_empty(),
            "recompiling {} unchanged modules ran {} queries, starting with {:?}",
            modules,
            ran.len(),
            ran.iter().take(5).map(|q| q.query.C()).collect::<Vec<_>>(),
        );
    }
}

/// The first compile does run, so the check above is not vacuous.
#[test]
fn a_first_compile_does_run_queries() {
    let recorder = QueryRecorder::new();
    let db = Database::recording(&recorder);

    let mut world = IncrementalModuleWorld::new();
    world.add_module(&db, "local/test/a", "fun f(): i32\n  ret 1\nend fun\n");

    recorder.clear();
    let deps = extract_dependencies(&world, &db, &Roots::All);
    let (graph, requires) = world.build_graph(&db, &deps, &Roots::All);
    let _ = parse_module_graph(&db, graph, requires, Vec::new());

    let ran = recorder.take();
    assert!(!ran.is_empty(), "a first compile has to run something");

    // And it reaches past the queries the fixtures know to look at: those cover
    // fourteen annotated sites, this sees whatever actually ran.
    let names: std::collections::BTreeSet<String> =
        ran.iter().map(|q| q.query.C()).collect();
    assert!(
        names.len() > 5,
        "a compile should run more than a handful of distinct queries, saw {:?}",
        names,
    );
}

/// Editing one module leaves the work proportional to the edit.
#[test]
fn an_edit_runs_fewer_queries_than_a_first_compile() {
    let recorder = QueryRecorder::new();
    let db = Database::recording(&recorder);

    let mut world = IncrementalModuleWorld::new();
    for index in 0..8 {
        world.add_module(
            &db,
            &format!("local/test/m{}", index),
            &format!("fun f{}(): i32\n  ret {}\nend fun\n", index, index),
        );
    }

    let mut compile = |world: &IncrementalModuleWorld| {
        let deps = extract_dependencies(world, &db, &Roots::All);
        let (graph, requires) = world.build_graph(&db, &deps, &Roots::All);
        let _ = parse_module_graph(&db, graph, requires, Vec::new());
    };

    recorder.clear();
    compile(&world);
    let first = recorder.take().len();

    // A different database is needed to edit, since the world holds sources
    // that the compile above borrowed; recompiling in place is what the other
    // tests cover, so here the edit is made and the world recompiled.
    let mut db2 = Database::recording(&recorder);
    let mut world2 = IncrementalModuleWorld::new();
    for index in 0..8 {
        world2.add_module(
            &db2,
            &format!("local/test/m{}", index),
            &format!("fun f{}(): i32\n  ret {}\nend fun\n", index, index),
        );
    }
    {
        let deps = extract_dependencies(&world2, &db2, &Roots::All);
        let (graph, requires) = world2.build_graph(&db2, &deps, &Roots::All);
        let _ = parse_module_graph(&db2, graph, requires, Vec::new());
    }
    world2.update_source(&mut db2, "local/test/m0", "fun f0(): i32\n  ret 99\nend fun\n");

    recorder.clear();
    {
        let deps = extract_dependencies(&world2, &db2, &Roots::All);
        let (graph, requires) = world2.build_graph(&db2, &deps, &Roots::All);
        let _ = parse_module_graph(&db2, graph, requires, Vec::new());
    }
    let after_edit = recorder.take().len();

    assert!(after_edit > 0, "an edit has to run something");
    assert!(
        after_edit < first,
        "editing one of eight modules ran {} queries against {} for the first \
         compile; the work should follow the edit",
        after_edit,
        first,
    );
}
