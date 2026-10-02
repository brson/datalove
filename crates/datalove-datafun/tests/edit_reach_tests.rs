//! How far downstream an edit travels, for every shape of edit there is.
//!
//! One rule, asked of a world whose dependency graph is known: **an edit reaches
//! the module it changed and the modules that import what changed, and nothing
//! else.** Everything below is that rule with a different edit substituted in.
//!
//! This is the generalisation of `import_memo_tests`, which asks the same
//! question about one edit shape. The reason to ask it this way rather than by
//! measuring memo sizes is that a memo holds an *aggregate* as a single
//! dependency however much it aggregates, so a whole-world map creeping into a
//! per-module query's key or dependency set is invisible to
//! `compile_scaling_tests` and to every fixture that records a yes-or-no per
//! module. It has been invisible twice. What sees it is counting how many
//! modules re-ran a phase and comparing that against the graph.
//!
//! The fixture has both mechanisms in it on purpose: plain module imports, and
//! rider imports. `incremental_lowering_tests` holds the add-a-module case
//! already, but against a world with neither, so it cannot see a rider stub or
//! an import resolution being disturbed.

use rmx::prelude::*;

use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun as datafun;
use datalove_datafun::pipeline::{CompilerOptions, ModuleCompilationPipeline};
use datalove_datafun_pkg::package_load_worldfile;

/// The per-module phases, and how many modules ran each.
///
/// These four are the phases an edit is supposed to cost one of. The first two
/// are the frontend, the second two are lowering and assembly, so between them
/// they say whether an edit stopped where it should have or carried on.
#[derive(Debug, PartialEq, Eq)]
struct Reach {
    imports: usize,
    typechecks: usize,
    analyses: usize,
    lowerings: usize,
    assemblies: usize,
}

impl Reach {
    fn of(executed: &[ExecutedQuery]) -> Self {
        let ran = |q: &str| executed.iter().filter(|e| e.query == q).count();
        Reach {
            imports: ran("resolve_module_imports"),
            typechecks: ran("typecheck_module"),
            analyses: ran("analyze_module"),
            lowerings: ran("lower_module_functions"),
            assemblies: ran("lower_module"),
        }
    }
}

// ============================================================================
// The fixture
// ============================================================================

/// `base` is imported by `mid`, which is imported by `top`. Three modules take
/// a function from a rider. Two are islands nothing imports.
///
/// The islands are named to sort after everything else, and the module the
/// add-a-module test appends sorts after them. That is not cosmetic:
/// `ir_module_ids` numbers modules by position, so a module appearing or
/// disappearing anywhere but the end shifts the numbers of everything after it,
/// and those modules really do have to lower again -- their functions have
/// different ids. Sorting last is what isolates the question these tests are
/// asking from that one.
///
/// `shared_takes` and `private_takes` are the parameter lists of `base`'s two
/// functions, so a test can move the signature of the one `mid` imports or the
/// one nobody does. `base_salt` is a literal in a body, for an edit that should
/// reach nothing at all.
struct World {
    shared_takes: &'static str,
    private_takes: &'static str,
    base_salt: i32,
    island_takes: &'static str,
}

impl Default for World {
    fn default() -> Self {
        World { shared_takes: "x: i32", private_takes: "y: i32", base_salt: 1, island_takes: "z: i32" }
    }
}

/// How many modules the fixture has, so a regression reports a number that
/// could not be mistaken for a correct one.
const MODULES: usize = 8;
const RIDER_USERS: usize = 3;
const ISLANDS: usize = 2;

impl World {
    fn worldfile(&self) -> String {
        let mut w = String::from(
            "\n----------\nrider testlib\n----------\nnative fun r_add(a: i32, b: i32): i32\n",
        );

        w.push_str(&format!(
            "\n----------\nmodule local/test/base\n----------\n\
             fun base_shared({}): i32\n\x20   ret {}\nend fun\n\
             fun base_private({}): i32\n\x20   ret 2\nend fun\n",
            self.shared_takes, self.base_salt, self.private_takes,
        ));

        w.push_str(
            "\n----------\nmodule local/test/mid\n----------\n\
             require module local/test/base\n\
             import base.base_shared\n\
             \n\
             fun mid_shared(): i32\n\x20   ret base_shared(1)\nend fun\n",
        );

        w.push_str(
            "\n----------\nmodule local/test/top\n----------\n\
             require module local/test/mid\n\
             import mid.mid_shared\n\
             \n\
             fun top_calls(): i32\n\x20   ret mid_shared()\nend fun\n",
        );

        for i in 0..RIDER_USERS {
            w.push_str(&format!(
                "\n----------\nmodule local/test/rider{i}\n----------\n\
                 require rider testlib\n\
                 import testlib.r_add\n\
                 \n\
                 fun uses_rider{i}(v: i32): i32\n\x20   ret r_add(v, {i})\nend fun\n",
            ));
        }

        for i in 0..ISLANDS {
            w.push_str(&format!(
                "\n----------\nmodule local/test/zz_island{i}\n----------\n\
                 fun island_fn{i}({}): i32\n\x20   ret {i}\nend fun\n",
                self.island_takes,
            ));
        }

        w
    }
}

/// Compile the fixture, apply `edit`, and report how far the edit reached.
fn reach_of(edit: impl FnOnce(&mut datafun::Database, &mut ModuleCompilationPipeline)) -> Reach {
    let recorder = QueryRecorder::new();
    let mut db = datafun::Database::recording(&recorder);

    let world = World::default();
    let parsed = package_load_worldfile::parse_worldfile_sections(world.worldfile().as_bytes()).X();
    let mut pipeline =
        ModuleCompilationPipeline::from_sections(&db, &parsed.sections, CompilerOptions::default());

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "setup failed: {:?}", compiled.all_errors());
    drop(compiled);
    recorder.clear();

    edit(&mut db, &mut pipeline);

    let (compiled, _) = pipeline.compile(&mut db);
    assert!(compiled.is_successful(), "after the edit: {:?}", compiled.all_errors());
    drop(compiled);

    Reach::of(&recorder.take())
}

/// Replace one module's text with what `world` says it should be.
fn put(
    db: &mut datafun::Database,
    pipeline: &mut ModuleCompilationPipeline,
    module: &str,
    world: &World,
) {
    let text = package_load_worldfile::parse_worldfile_sections(world.worldfile().as_bytes())
        .X()
        .sections
        .into_iter()
        .find_map(|s| match s {
            package_load_worldfile::WorldfileSection::Module { module: m, source, .. }
                if m == module => Some(source),
            _ => None,
        })
        .expect("the fixture has that module");
    pipeline.update_source(db, "local", "test", module, &text);
}

// ============================================================================
// The first compile, so the counts below mean something
// ============================================================================

#[test]
fn a_first_compile_runs_every_phase_for_every_module() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let parsed =
        package_load_worldfile::parse_worldfile_sections(World::default().worldfile().as_bytes()).X();
    let mut pipeline =
        ModuleCompilationPipeline::from_sections(&db, &parsed.sections, CompilerOptions::default());

    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    drop(compiled);

    let reach = Reach::of(&recorder.take());
    assert_eq!(
        reach,
        Reach { imports: MODULES, typechecks: MODULES, analyses: MODULES,
                lowerings: MODULES, assemblies: MODULES },
        "a cold compile has to run all of them, or every test below is vacuous",
    );
}

// ============================================================================
// Edits that should reach nothing
// ============================================================================

/// A literal in a body reaches the module it is in, and no further.
///
/// This is the parse firewall at the phase level: the statements compare equal
/// because a body is a tracked field of `StmtFun`, so name resolution and import
/// resolution are never asked again, and only the module whose body moved is
/// typechecked.
#[test]
fn a_body_edit_reaches_only_its_own_module() {
    let reach = reach_of(|db, pipeline| {
        put(db, pipeline, "base", &World { base_salt: 99, ..World::default() });
    });
    assert_eq!(
        reach,
        Reach { imports: 0, typechecks: 1, analyses: 1, lowerings: 1, assemblies: 1 },
        "a body edit moves no signature, so nobody's imports are re-resolved",
    );
}

/// A signature edit in an island reaches only the island.
#[test]
fn a_signature_edit_in_an_island_reaches_only_the_island() {
    let reach = reach_of(|db, pipeline| {
        put(db, pipeline, "zz_island0", &World { island_takes: "z: i32, extra: i32", ..World::default() });
    });
    assert_eq!(
        reach,
        Reach { imports: 0, typechecks: 1, analyses: 1, lowerings: 1, assemblies: 1 },
        "nobody imports an island, and it imports nothing itself",
    );
}

/// Changing an export nobody imported reaches the importer's import resolution
/// and stops there.
///
/// `imports` counting more than the edited module is not in itself a fault --
/// `resolve_module_imports` re-running and returning an equal value is the
/// firewall working, and it is a handful of map lookups. What matters is that
/// nothing expensive downstream of it moved.
///
/// `mid` requires `base`, so it asks for `base`'s exports and re-runs when any
/// of them moves. The import it holds is `base_shared`, which did not, so what
/// it returns is equal and `mid` is not typechecked again.
#[test]
fn changing_an_unimported_export_stops_at_the_importer_s_imports() {
    let reach = reach_of(|db, pipeline| {
        put(db, pipeline, "base", &World { private_takes: "y: i32, extra: i32", ..World::default() });
    });
    assert_eq!(
        reach,
        Reach { imports: 1, typechecks: 1, analyses: 1, lowerings: 1, assemblies: 1 },
        "`mid` re-resolves because `base`'s exports moved, and is spared the rest",
    );
}

// ============================================================================
// Edits that should reach exactly their dependents
// ============================================================================

/// Changing an imported export reaches the direct importer, and not past it.
///
/// `mid` imports `base_shared`, so it re-typechecks. `top` imports
/// `mid_shared`, whose signature did not move, so the edit stops at `mid`.
#[test]
fn changing_an_imported_export_reaches_the_importer_and_no_further() {
    let reach = reach_of(|db, pipeline| {
        put(db, pipeline, "base", &World { shared_takes: "x: u32", ..World::default() });
    });
    assert_eq!(
        reach,
        Reach { imports: 1, typechecks: 2, analyses: 2, lowerings: 2, assemblies: 2 },
        "`base` changed and `mid` imports what changed; `top` does not",
    );
}

// ============================================================================
// Changing the module set
// ============================================================================

/// Adding a module that sorts last leaves every existing module alone.
///
/// `incremental_lowering_tests` holds the lowering half of this against a world
/// with no imports and no riders. This holds the frontend half against a world
/// with both, which is where it can fail: the rider interfaces carry the
/// statement behind each native, and minting those anywhere keyed on the module
/// graph re-mints them under fresh ids when a ninth module appears, which used
/// to re-typecheck every module importing from the rider.
///
/// It has to sort last: `compute_func_id_map` numbers `IrModuleId` by position,
/// so one landing earlier renumbers everything after it and those modules really
/// do have to lower again.
#[test]
fn adding_a_module_leaves_the_others_alone() {
    let reach = reach_of(|db, pipeline| {
        pipeline.add_module(
            db, "local", "test", "zzz_added",
            "fun added_fn(q: i32): i32\n\x20   ret q\nend fun\n",
        );
    });
    assert_eq!(
        (reach.typechecks, reach.analyses, reach.lowerings, reach.assemblies), (1, 1, 1, 1),
        "only the new module is new; the other {MODULES} did not change -- got {reach:?}",
    );
}

/// Removing an island leaves every remaining module alone.
#[test]
fn removing_an_island_leaves_the_others_alone() {
    let reach = reach_of(|_db, pipeline| {
        pipeline.remove_module("local", "test", "zz_island1");
    });
    assert_eq!(
        (reach.typechecks, reach.analyses, reach.lowerings, reach.assemblies), (0, 0, 0, 0),
        "nothing that remains changed, and the island sorted last -- got {reach:?}",
    );
}


/// An unchanged recompile of this world runs nothing.
#[test]
fn an_unchanged_recompile_reaches_nothing() {
    let reach = reach_of(|_db, _pipeline| {});
    assert_eq!(
        reach,
        Reach { imports: 0, typechecks: 0, analyses: 0, lowerings: 0, assemblies: 0 },
        "nothing was edited",
    );
}

