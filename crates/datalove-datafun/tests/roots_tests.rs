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

/// Every per-module phase sees only the reachable modules, not just some of them.
///
/// The pruning is of the `ModuleGraph`, and the phases take their work list from
/// it, so one change covers them all. This is the assertion that says so rather
/// than the reasoning.
///
/// The parse is not in this list, and that is the point of the test below it.
#[test]
fn every_phase_sees_only_the_reachable_modules() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    add_fixture(&db, &mut pipeline);
    pipeline.set_roots(roots_of(&["local/test/mid"]));

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    let ran = |q: &str| executed.iter().filter(|e| e.query == q).count();
    // `mid` and `base`, out of five in the world.
    for (phase, query) in [
        ("2 name resolution", "resolve_module_names"),
        ("3 typecheck", "typecheck_module"),
        ("4 ownership", "analyze_module"),
        ("5a lowering", "lower_module_functions"),
        ("5d assembly", "lower_module"),
    ] {
        assert_eq!(
            ran(query), 2,
            "phase {phase} ({query}) ran {} times for the 2 reachable modules of {MODULES}",
            ran(query),
        );
    }
}

/// **Parsing is not pruned, because resolution is not.** Every module in the
/// world is parsed however few of them are compiled.
///
/// Reading a module's `require` lines means parsing it, and `dependencies_of`
/// builds the whole world's dependency map before anything knows what the roots
/// reach -- so `import_demands` asks `parse_module_full` for every module, and
/// that is the parse phase 1 then gets from the memo.
///
/// This is the ceiling on what `Roots` currently buys: `cold_phases` has a
/// compile pruned to one module still spending 8.7ms of its 9.3ms in resolution.
/// Reachability is a walk and a walk need only parse what it reaches -- parse the
/// roots, read their requires, parse those. Doing that is the next piece of work,
/// and when it lands this count drops to the reachable set and this test wants
/// tightening.
#[test]
fn the_whole_world_is_still_parsed_however_few_modules_are_compiled() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    add_fixture(&db, &mut pipeline);
    pipeline.set_roots(roots_of(&["local/test/mid"]));

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    let parses = executed.iter().filter(|e| e.query == "parse_module_full").count();
    assert_eq!(
        parses, MODULES,
        "resolution parses the world to find its requires; it parsed {parses}",
    );
}

// ============================================================================
// Deciding the roots from a script, which is what the CLI does
// ============================================================================

/// Compile the fixture with roots narrowed to `script`, and report how many
/// modules were typechecked and whether narrowing happened at all.
fn compiled_for_script(script: &str, also: &[String]) -> (bool, usize) {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let mut pipeline = ModuleCompilationPipeline::default();
    add_fixture(&db, &mut pipeline);

    let narrowed = pipeline.narrow_roots_to_script(&db, script, also);
    recorder.clear();

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let count = recorder.take().iter()
        .filter(|e| e.query == "typecheck_module")
        .count();
    (narrowed, count)
}

/// A script's `require`s are its roots, and they pull in what they reach.
#[test]
fn a_script_is_compiled_against_what_it_requires() {
    let (narrowed, modules) = compiled_for_script(
        "require module local/test/top\nimport top.top_fn\n\ndebuglog top_fn()\n", &[]);
    assert!(narrowed);
    assert_eq!(modules, 3, "`top` reaches `mid` reaches `base`; the islands do not");
}

/// A script that requires nothing is compiled against nothing.
#[test]
fn a_script_requiring_nothing_compiles_no_modules() {
    let (narrowed, modules) = compiled_for_script("debuglog 42\n", &[]);
    assert!(narrowed);
    assert_eq!(modules, 0, "a program using only builtins needs no library");
}

/// **Narrowing refuses when a require does not resolve.**
///
/// Pruning on a program whose requires are wrong would drop the very module the
/// diagnostic is about. The whole world goes to resolution instead, so it reports
/// what is wrong exactly as it did before there was a choice -- which is what the
/// CLI's `error` fixtures expect.
#[test]
fn a_require_that_does_not_resolve_refuses_to_narrow() {
    let (narrowed, modules) = compiled_for_script(
        "require module local/test/nosuchmodule\n\ndebuglog 1\n", &[]);
    assert!(!narrowed, "it must refuse rather than prune on a program it cannot read");
    assert_eq!(modules, MODULES, "and leave the whole world for resolution to judge");
}

/// One bad require refuses even when the others are fine.
#[test]
fn one_unresolvable_require_refuses_for_the_whole_script() {
    let (narrowed, modules) = compiled_for_script(
        "require module local/test/mid\n\
         require module local/test/nosuchmodule\n\
         \n\
         debuglog 1\n", &[]);
    assert!(!narrowed);
    assert_eq!(modules, MODULES);
}

/// Modules named in `also` are roots whether the script reaches them or not.
///
/// This is what a worldfile's own modules get: the author wrote them in the file
/// they asked to be compiled, so an error in one is an error in the worldfile even
/// if the script ignores it. A library module gets no such treatment.
#[test]
fn modules_the_artifact_defines_are_roots_regardless() {
    let (narrowed, modules) = compiled_for_script(
        "debuglog 42\n", &["local/test/zz_island0".S()]);
    assert!(narrowed);
    assert_eq!(modules, 1, "the script reaches nothing, but the island was named");

    let (_, modules) = compiled_for_script(
        "require module local/test/mid\nimport mid.mid_fn\n\ndebuglog mid_fn(1)\n",
        &["local/test/zz_island0".S(), "local/test/zz_island1".S()]);
    assert_eq!(modules, 4, "`mid` and `base` from the script, plus the two islands");
}

/// An `also` naming a module the world does not have is dropped, not refused.
///
/// Unlike a script's `require`, this is not the program saying something that has
/// to make sense -- it is a caller listing what it happens to own, and a name that
/// is not there simply is not a root.
#[test]
fn an_also_that_is_not_in_the_world_is_dropped() {
    let (narrowed, modules) = compiled_for_script(
        "debuglog 42\n", &["local/test/nosuchmodule".S(), "local/test/zz_island0".S()]);
    assert!(narrowed);
    assert_eq!(modules, 1);
}
