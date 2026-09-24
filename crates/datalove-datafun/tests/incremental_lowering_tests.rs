//! Editing one module lowers one module.
//!
//! Phase 5a used to lower every function of every module on every compile, so
//! a one-character edit re-lowered the whole world and so did recompiling a
//! world that had not changed. `lower_module_functions` is tracked now, and
//! that is a property of the memo keys rather than of the output: every
//! fixture would still pass if the keys widened and the saving went away.
//! These ask salsa what it actually ran.
//!
//! `QueryRecorder` sees every query salsa executes, on whatever thread, which
//! the `module_memo` fixtures' thread-local log cannot.

use rmx::prelude::*;

use datalove_ct::query_events::QueryRecorder;
use datalove_datafun::pipeline::ModuleCompilationPipeline;
use datalove_datafun_compiler::Database;

const MODULES: usize = 8;

/// A module of three functions, the last of which returns `salt`.
///
/// Enough branching that lowering one is not free, so a test asserting that
/// one module lowered is asserting something worth having.
fn module_source(index: usize, salt: usize) -> String {
    let mut source = String::new();
    source.push_str(&format!("fun a{}(): int\n  ret {}\nend fun\n", index, index));
    source.push_str(&format!("fun b{}(ref x: int, y: int): int\n", index));
    source.push_str("  let s = x + y\n");
    source.push_str("  if s .> 100\n");
    source.push_str("    ret s - 50\n");
    source.push_str("  else\n");
    source.push_str("    ret s + 50\n");
    source.push_str("  end if\n");
    source.push_str("end fun\n");
    source.push_str(&format!("fun c{}(): int\n  ret {}\nend fun\n", index, salt));
    source
}

/// How many times `query` ran, out of what the recorder collected.
fn ran(executed: &[datalove_ct::query_events::ExecutedQuery], query: &str) -> usize {
    executed.iter().filter(|q| q.query == query).count()
}

/// Build the modules and compile them once, leaving the recorder cleared.
fn compiled_world(recorder: &QueryRecorder) -> (Database, ModuleCompilationPipeline) {
    let db = Database::recording(recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    for index in 0..MODULES {
        pipeline.add_module(&db, "local", "test", &format!("m{}", index), &module_source(index, 0));
    }
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "setup failed: {:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();
    (db, pipeline)
}

#[test]
fn a_first_compile_lowers_every_module() {
    let recorder = QueryRecorder::new();
    let db = Database::recording(&recorder);

    let mut pipeline = ModuleCompilationPipeline::default();
    for index in 0..MODULES {
        pipeline.add_module(&db, "local", "test", &format!("m{}", index), &module_source(index, 0));
    }
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "lower_module_functions"),
        MODULES,
        "a first compile has to lower all of them, or the test below is vacuous",
    );
}

#[test]
fn an_unchanged_recompile_lowers_nothing() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_world(&recorder);

    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(ran(&executed, "lower_module_functions"), 0);
    assert_eq!(ran(&executed, "lower_module"), 0);
    // Nothing at all, in fact. The glue around the queries still runs; no
    // query does.
    assert!(
        executed.is_empty(),
        "an unchanged recompile ran {:?}",
        executed.iter().map(|q| q.query.C()).collect::<Vec<_>>(),
    );
}

#[test]
fn an_edit_lowers_only_the_module_that_changed() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_world(&recorder);

    pipeline.update_source(&mut db, "local", "test", "m0", &module_source(0, 99));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "lower_module_functions"),
        1,
        "one module changed, so one module lowers; ran {:?}",
        executed.iter().map(|q| q.query.C()).collect::<Vec<_>>(),
    );
    assert_eq!(
        ran(&executed, "lower_module"), 1,
        "and it is assembled once, not eight times",
    );
}

/// Adding a module leaves the modules it cannot reach alone.
///
/// This is what `reachable_func_ids` buys: handing every module the whole
/// world's function ids would change all eight keys when a ninth appeared.
///
/// The new module has to sort last. `compute_func_id_map` numbers `IrModuleId`
/// by position, so one landing earlier renumbers every module after it and
/// they all lower again -- correctly, since their ids really did change.
#[test]
fn adding_a_module_does_not_relower_the_others() {
    let recorder = QueryRecorder::new();
    let (db, mut pipeline) = compiled_world(&recorder);

    let last = format!("m{}", MODULES);
    pipeline.add_module(&db, "local", "test", &last, &module_source(MODULES, 0));
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "lower_module_functions"),
        1,
        "only the new module should lower; ran {:?}",
        executed.iter().map(|q| q.query.C()).collect::<Vec<_>>(),
    );
}
