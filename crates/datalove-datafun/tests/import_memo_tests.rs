//! An edit reaches the modules that import what changed, and no further.
//!
//! `resolve_module_imports` is keyed per module, but it used to take gathered
//! maps of *every* module's exports and function ASTs. Those are tracked structs
//! whose identity is a hash of the lot, so a signature edit anywhere handed the
//! query a new key for every module in the world. A new key is a new query
//! instance, and a tracked struct's identity map belongs to the instance that
//! created it -- so the synthetic `StmtFun` a rider import stands behind was
//! re-minted under a fresh id, which made the resolved imports unequal, which
//! re-typechecked, re-analyzed and re-lowered every module that imports from a
//! rider. On the system library that was eleven modules out of twenty-five, for
//! an edit none of them could see.
//!
//! None of the existing suites saw it. `compile_scaling_tests` measures whether
//! a per-module memo grows with the world, and a tracked-struct key is one
//! dependency however much it aggregates -- the same blind spot its own header
//! records for interned aggregates. What catches it is asking how far *downstream*
//! an edit travelled, which is what these do.

use rmx::prelude::*;

use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun as datafun;
use datalove_datafun::pipeline::{CompilerOptions, ModuleCompilationPipeline};
use datalove_datafun_pkg::package_load_worldfile;

/// How many times `query` ran, out of what the recorder collected.
fn ran(executed: &[ExecutedQuery], query: &str) -> usize {
    executed.iter().filter(|q| q.query == query).count()
}

// ============================================================================
// A rider-importing world, which is where the bug lived
// ============================================================================

/// How many modules import from the rider. More than one, so that a regression
/// reports a number that could not be the edited module.
const RIDER_USERS: usize = 8;

/// A world of `RIDER_USERS` rider-importing modules plus one that imports nothing.
///
/// This is the shape of the system library in miniature: most modules take their
/// primitives from a rider, and one module is the program.
fn rider_worldfile(plain: &str) -> String {
    let mut worldfile = String::from(
        "\n----------\nrider testlib\n----------\n\
         native fun rider_add(a: i32, b: i32): i32\n\
         native fun rider_neg(a: i32): i32\n",
    );
    for index in 0..RIDER_USERS {
        worldfile.push_str(&format!(
            "\n----------\nmodule local/test/r{index}\n----------\n\
             require rider testlib\n\
             import testlib.rider_add\n\
             \n\
             fun use_rider{index}(x: i32): i32\n\
             \x20   ret rider_add(x, {index})\n\
             end fun\n",
        ));
    }
    worldfile.push_str(&format!(
        "\n----------\nmodule local/test/plain\n----------\n{plain}",
    ));
    worldfile
}

/// The module nothing imports from, whose `takes` are the parameters of a
/// function nothing calls.
fn plain_source(takes: &str) -> String {
    format!(
        "fun plain_own(x: i32): i32\n\x20   ret x\nend fun\n\
         fun plain_edited({takes}): i32\n\x20   ret 7\nend fun\n",
    )
}

/// Build the rider world, compile it once, and leave the recorder cleared.
fn compiled_rider_world(
    recorder: &QueryRecorder,
) -> (datafun::Database, ModuleCompilationPipeline) {
    let db = datafun::Database::recording(recorder);
    let worldfile = rider_worldfile(&plain_source(""));
    let parsed = package_load_worldfile::parse_worldfile_sections(worldfile.as_bytes()).X();
    let mut pipeline =
        ModuleCompilationPipeline::from_sections(&db, &parsed.sections, CompilerOptions::default());

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "setup failed: {:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();
    (db, pipeline)
}

/// A first compile typechecks every module, so the tests below are not vacuous.
#[test]
fn a_first_compile_of_the_rider_world_typechecks_every_module() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let worldfile = rider_worldfile(&plain_source(""));
    let parsed = package_load_worldfile::parse_worldfile_sections(worldfile.as_bytes()).X();
    let mut pipeline =
        ModuleCompilationPipeline::from_sections(&db, &parsed.sections, CompilerOptions::default());

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "typecheck_module"),
        RIDER_USERS + 1,
        "a first compile has to typecheck all of them",
    );
}

/// **The regression.** A signature edit in a module nobody imports from
/// typechecks that module and nothing else.
///
/// Before the fix this typechecked all `RIDER_USERS` of the others too, because
/// every one of them imports from a rider and the edit moved the key of the
/// query that minted the rider stubs.
#[test]
fn a_signature_edit_does_not_reach_modules_that_import_from_a_rider() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_rider_world(&recorder);

    // `plain_edited` gains a parameter. Nothing imports or calls it.
    pipeline.update_source(&mut db, "local", "test", "plain", &plain_source("a: i32"));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "typecheck_module"), 1,
        "only the edited module's signatures changed, so only it re-typechecks",
    );
    assert_eq!(
        ran(&executed, "analyze_module"), 1,
        "ownership analysis follows typechecking, so it must not spread either",
    );
    assert_eq!(
        ran(&executed, "lower_module"), 1,
        "and neither must assembly",
    );
}

/// The rider stubs are minted once for the graph, and an edit does not re-mint them.
///
/// This is the mechanism underneath the test above, asserted directly: the stub
/// table is keyed on the graph rather than on anything a module's text moves, so
/// an edit does not re-run it and the stub ids stay put.
///
/// The cold-compile count is here so that the zero below means "did not re-run"
/// rather than "there is no such query"; inline the mint back into
/// `resolve_module_imports` and this test fails on the first assertion.
#[test]
fn the_rider_stubs_are_minted_once_and_an_edit_does_not_re_mint_them() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let worldfile = rider_worldfile(&plain_source(""));
    let parsed = package_load_worldfile::parse_worldfile_sections(worldfile.as_bytes()).X();
    let mut pipeline =
        ModuleCompilationPipeline::from_sections(&db, &parsed.sections, CompilerOptions::default());

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);
    assert_eq!(
        ran(&recorder.take(), "rider_function_stubs"), 1,
        "one table for the graph, however many modules import from the rider",
    );

    let mut db = db;
    pipeline.update_source(&mut db, "local", "test", "plain", &plain_source("a: i32"));
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);
    assert_eq!(
        ran(&recorder.take(), "rider_function_stubs"), 0,
        "the riders did not change, so nothing should re-mint their stubs",
    );
}

/// An unchanged recompile of a rider-importing world runs nothing at all.
#[test]
fn an_unchanged_recompile_of_the_rider_world_runs_no_queries() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_rider_world(&recorder);

    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert!(
        executed.is_empty(),
        "an unchanged recompile ran {} queries: {:?}",
        executed.len(),
        executed.iter().map(|q| q.query.C()).collect::<Vec<_>>(),
    );
}

// ============================================================================
// A world of plain module imports, where the dependency graph is the bound
// ============================================================================

/// `lib` exports two functions. `user` requires it and imports one. `bystander`
/// requires nothing.
fn import_world(shared_param: &str, private_param: &str) -> (String, String, String) {
    let lib = format!(
        "fun shared({shared_param}): i32\n\x20   ret 1\nend fun\n\
         fun private({private_param}): i32\n\x20   ret 2\nend fun\n",
    );
    let user = "require module local/test/lib\n\
                import lib.shared\n\
                \n\
                fun call_shared(): i32\n\x20   ret shared(1)\nend fun\n"
        .S();
    let bystander = "fun alone(x: i32): i32\n\x20   ret x\nend fun\n".S();
    (lib, user, bystander)
}

/// Build the three-module world, compile it once, leave the recorder cleared.
fn compiled_import_world(
    recorder: &QueryRecorder,
) -> (datafun::Database, ModuleCompilationPipeline) {
    let db = datafun::Database::recording(recorder);
    let (lib, user, bystander) = import_world("x: i32", "y: i32");
    let mut pipeline = ModuleCompilationPipeline::default();
    pipeline.add_module(&db, "local", "test", "lib", &lib);
    pipeline.add_module(&db, "local", "test", "user", &user);
    pipeline.add_module(&db, "local", "test", "bystander", &bystander);

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "setup failed: {:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();
    (db, pipeline)
}

/// An edit to a module nobody requires does not re-resolve anybody's imports.
///
/// This is the aggregate key gone, asserted on its own. `resolve_module_imports`
/// used to be keyed on every module's exports, so it re-ran for all three
/// modules here whatever was edited. It is keyed on the module and the graph
/// now, and asks the modules it requires for their exports, so an edit to a
/// module on nobody's require list does not reach it at all.
#[test]
fn an_edit_to_an_unrequired_module_re_resolves_no_imports() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_import_world(&recorder);

    pipeline.update_source(
        &mut db, "local", "test", "bystander",
        "fun alone(x: i32, extra: i32): i32\n\x20   ret x\nend fun\n",
    );
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    // Not even the edited module's own. `bystander` has no import statements, so
    // its import resolution reads only its `ParsedStatements` -- and a signature
    // lives in a tracked field of `StmtFun`, whose identity is
    // `(module_id, name, local_index)`, so the statements compare equal and
    // backdate. The parse firewall reaches this far.
    assert_eq!(
        ran(&executed, "resolve_module_imports"), 0,
        "nobody requires `bystander`, and it imports nothing itself",
    );
    assert_eq!(
        ran(&executed, "typecheck_module"), 1,
        "only the edited module re-typechecks, since only it reads the signature",
    );
}

/// Changing an export nobody imported re-resolves the importer, and stops there.
///
/// `user` requires `lib`, so it asks for `lib`'s exports and re-runs when they
/// move -- and editing any of `lib`'s signatures moves them. But the import it
/// resolves is unchanged, so the value it returns is equal and `user`'s
/// typechecking is spared. This is the firewall one level up from the parse.
#[test]
fn changing_an_unimported_export_stops_at_the_importer_s_import_resolution() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_import_world(&recorder);

    // `private` gains a parameter. `user` imports `shared`, not `private`.
    let (lib, _, _) = import_world("x: i32", "y: i32, extra: i32");
    pipeline.update_source(&mut db, "local", "test", "lib", &lib);
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "typecheck_module"), 1,
        "the import `user` holds did not change, so only `lib` re-typechecks",
    );
    // The one is `user`'s: it requires `lib`, so it asks for `lib`'s exports and
    // those moved. `bystander` requires nothing of `lib` and is not reached at
    // all -- which is the whole point, and used to be false.
    assert_eq!(
        ran(&executed, "resolve_module_imports"), 1,
        "only the importer of the changed module re-resolves its imports",
    );
}

/// Changing an export somebody *did* import re-typechecks the importer.
///
/// The control for the three above: they assert that an edit stops early, and
/// would all pass if imports had stopped propagating at all.
#[test]
fn changing_an_imported_export_does_reach_the_importer() {
    let recorder = QueryRecorder::new();
    let (mut db, mut pipeline) = compiled_import_world(&recorder);

    // `shared`'s parameter type changes. `user` calls it with a literal, which
    // takes its type from context, so the world still compiles.
    let (lib, _, _) = import_world("x: u32", "y: i32");
    pipeline.update_source(&mut db, "local", "test", "lib", &lib);
    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let executed = recorder.take();
    assert_eq!(
        ran(&executed, "typecheck_module"), 2,
        "`lib` changed and `user` imports what changed, so both re-typecheck",
    );
}
