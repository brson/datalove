//! Compiling what the roots reach, rather than everything the world holds.
//!
//! The world is whatever a worldfile mentions, which for a program that uses the
//! system library is two dozen modules it may touch none of. `Roots` says which
//! of them a compilation is about: `All` is the world, `From` is the transitive
//! closure of `require` from a set of modules.
//!
//! Two things are worth holding here, and they pull in opposite directions.
//! `Roots::All` has to stay exactly what the compiler did before there was a
//! choice, because every other suite is written against it. And `Roots::From`
//! has to actually prune -- which is a *semantic* change, not only a faster one,
//! because an error in a module nothing requires stops being an error. That is
//! pinned below rather than left to be discovered.

use rmx::prelude::*;

use datalove_ct::query_events::QueryRecorder;
use datalove_datafun as datafun;
use datalove_datafun::incremental::Roots;
use datalove_datafun::pipeline::ModuleCompilationPipeline;

/// `top` requires `mid` requires `base`. Two islands require nothing.
///
/// Five modules, so that "compiled three of them" cannot be confused with
/// "compiled all of them" or "compiled one".
const MODULES: usize = 5;

fn add_fixture(db: &dyn salsa::Database, pipeline: &mut ModuleCompilationPipeline) {
    pipeline.add_module(db, "local", "test", "base",
        "fun base_fn(x: i32): i32\n    ret x\nend fun\n");
    pipeline.add_module(db, "local", "test", "mid",
        "require module local/test/base\n\
         import base.base_fn\n\
         \n\
         fun mid_fn(x: i32): i32\n    ret base_fn(x)\nend fun\n");
    pipeline.add_module(db, "local", "test", "top",
        "require module local/test/mid\n\
         import mid.mid_fn\n\
         \n\
         fun top_fn(): i32\n    ret mid_fn(1)\nend fun\n");
    pipeline.add_module(db, "local", "test", "zz_island0",
        "fun island0_fn(x: i32): i32\n    ret x\nend fun\n");
    pipeline.add_module(db, "local", "test", "zz_island1",
        "fun island1_fn(x: i32): i32\n    ret x\nend fun\n");
}

fn roots_of(paths: &[&str]) -> Roots {
    Roots::From(paths.iter().map(|p| p.S()).collect())
}

/// How many modules were typechecked compiling the fixture under `roots`.
fn modules_compiled(roots: Roots) -> usize {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    add_fixture(&db, &mut pipeline);
    pipeline.set_roots(roots);

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    recorder.take().iter().filter(|e| e.query == "typecheck_module").count()
}

/// `Roots::All` compiles the world, which is what it has always done.
#[test]
fn all_roots_compile_every_module() {
    assert_eq!(modules_compiled(Roots::All), MODULES);
}

/// A root pulls in what it requires, transitively, and nothing else.
#[test]
fn a_root_pulls_in_its_transitive_requires() {
    assert_eq!(
        modules_compiled(roots_of(&["local/test/top"])), 3,
        "`top` requires `mid` requires `base`; the two islands are not reached",
    );
}

/// A root in the middle of a chain pulls in what is below it, not above.
///
/// `require` points from a module to what it needs, so reachability runs that
/// way too: `mid` reaching `base` does not make `top` reachable.
#[test]
fn reachability_follows_requires_downward_only() {
    assert_eq!(
        modules_compiled(roots_of(&["local/test/mid"])), 2,
        "`mid` and `base`; `top` requires `mid` rather than the other way about",
    );
}

/// A root that requires nothing compiles only itself.
#[test]
fn an_island_root_compiles_only_itself() {
    assert_eq!(modules_compiled(roots_of(&["local/test/zz_island0"])), 1);
}

/// Several roots compile the union of what they reach.
#[test]
fn several_roots_compile_the_union() {
    assert_eq!(
        modules_compiled(roots_of(&["local/test/mid", "local/test/zz_island0"])), 3,
        "`mid` and `base` and the island, with the other island and `top` left out",
    );
}

/// No roots compiles nothing.
#[test]
fn no_roots_compile_nothing() {
    assert_eq!(modules_compiled(Roots::From(Default::default())), 0);
}

/// A root naming a module the world does not have is ignored.
///
/// The roots come from what a program requires, and a require resolving to
/// nothing is a diagnostic from resolution rather than a reason to refuse to
/// build a graph.
#[test]
fn a_root_that_is_not_in_the_world_is_ignored() {
    assert_eq!(modules_compiled(roots_of(&["local/test/nonexistent"])), 0);
    assert_eq!(
        modules_compiled(roots_of(&["local/test/nonexistent", "local/test/mid"])), 2,
        "the roots that do exist still bring in what they reach",
    );
}

/// **The semantic change, pinned.** An error in an unreachable module is not an
/// error.
///
/// This is the cost of reachability and the reason `Roots::All` is the default:
/// a library wants checking whether or not this program calls into it, and CI
/// wants it whether or not anything does. Compiling from roots is for running a
/// program, not for vouching for a world.
#[test]
fn an_error_in_an_unreachable_module_is_not_reported() {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::default();
    add_fixture(&db, &mut pipeline);
    // An island nothing requires, which does not typecheck.
    pipeline.add_module(&db, "local", "test", "zz_broken",
        "fun broken_fn(): i32\n    ret \"not an i32\"\nend fun\n");

    pipeline.set_roots(Roots::All);
    let compiled = pipeline.compile_fresh(&db);
    assert!(
        !compiled.is_successful(),
        "the whole world includes the broken module, so it must be reported",
    );
    drop(compiled);

    pipeline.set_roots(roots_of(&["local/test/top"]));
    let compiled = pipeline.compile_fresh(&db);
    assert!(
        compiled.is_successful(),
        "`top` does not reach the broken module, so nothing looks at it: {:?}",
        compiled.all_errors(),
    );
}

/// Changing the roots on a live pipeline is sound, not a stale memo.
///
/// The roots decide the module set, which decides the `ModuleGraph`, which is
/// interned and is the identity of everything keyed on it -- so a different root
/// set is a different graph rather than a cache to invalidate. Compiling one
/// pipeline under both in turn has to give both answers.
#[test]
fn the_roots_can_change_between_compiles() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    add_fixture(&db, &mut pipeline);

    pipeline.set_roots(roots_of(&["local/test/zz_island0"]));
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();

    pipeline.set_roots(Roots::All);
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let widened = recorder.take().iter()
        .filter(|e| e.query == "typecheck_module")
        .count();
    assert_eq!(
        widened, MODULES - 1,
        "widening to the world typechecks the four it had not compiled, \
         and takes the island from the memo",
    );
}
