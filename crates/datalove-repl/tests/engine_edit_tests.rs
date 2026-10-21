//! The edit path, at the level a session actually uses it.
//!
//! `crates/datalove-datafun/tests/script_exec_reactivity_tests.rs` and
//! `script_scenario_tests.rs` are where the reach itself is measured, against
//! graphs they derive. These say the engine wires it up: an edit reaches the
//! units it should and the environment afterwards holds the values it should.
//!
//! These used to sit inline in `engine.rs`. The other integration target for
//! the same type, `engine_tests`, is `harness = false` -- it is a fixture
//! runner with a `main` of its own -- so `#[test]` functions cannot go there,
//! and this is a target of their own instead.

use rmx::prelude::*;

use datalove_datafun::pipeline::{
    ModuleDescriptor, PackageDescriptor, PackageLibrary, WorkspaceDescriptor,
};
use datalove_repl::{Engine, Eval};

/// A B C D, where C uses nothing B provides and D uses `b`.
const SCRIPT: &str = "let a = 1\n---\nlet b = 2\n---\nlet c = 30\n---\nlet d = b + 5";

fn bound(engine: &mut Engine, name: &str) -> bool {
    engine.get_environment().iter().any(|(bound, _, _)| bound == name)
}

fn value_of(engine: &mut Engine, name: &str) -> String {
    engine.get_environment().into_iter()
        .find(|(bound, _, _)| bound == name)
        .map(|(_, _, value)| value)
        .unwrap_or_else(|| panic!("no binding named {name}"))
}

fn engine() -> Engine {
    Engine::new(datalove_sys_packages::system_library()).expect("the engine starts")
}

/// An engine over the system library and one local module of our own.
///
/// The module is what a module edit needs something to be about: the system
/// library is the copy embedded in the binary and editing it would recompile
/// the standard library, which is not what these are measuring.
fn engine_with_module(source: &str) -> Engine {
    let sys = datalove_sys_packages::system_library();
    let mut workspace = WorkspaceDescriptor::from_system_library(&sys);
    workspace.user_libraries.push(PackageLibrary {
        name: "local".S(),
        packages: [(
            "test".S(),
            PackageDescriptor {
                name: "test".S(),
                modules: [(
                    "m".S(),
                    ModuleDescriptor { name: "m".S(), source: source.into(), origin: None },
                )].into_iter().collect(),
                data: Default::default(),
                rider: None,
            },
        )].into_iter().collect(),
    });
    Engine::with_workspace(sys, workspace).expect("the engine starts")
}

/// Editing B re-derives B and D, and `d` holds the new answer.
///
/// A value-only edit, which is the case that needs execution to cascade
/// where analysis did not: no type moved, so nothing but B was
/// re-typechecked, and `d` would sit at 7 if D had not run again.
#[test]
fn editing_a_unit_rederives_what_it_reaches() {
    let mut engine = engine();
    engine.run_source(SCRIPT);
    assert_eq!(value_of(&mut engine, "d"), "7");

    let reports = engine.edit_unit(1, "let b = 3");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![1, 3],
        "B and D; C uses nothing B provides",
    );
    assert!(
        reports.iter().all(|report| !matches!(report.eval, Eval::Error(_))),
        "both units re-derive cleanly: {reports:?}",
    );
    assert_eq!(value_of(&mut engine, "b"), "3");
    assert_eq!(value_of(&mut engine, "d"), "8", "D ran again against the new `b`");
    assert_eq!(value_of(&mut engine, "c"), "30", "C's frame is untouched");
}

/// An edit that breaks a later unit reports the error against that unit.
#[test]
fn an_edit_that_breaks_a_later_unit_says_which() {
    let mut engine = engine();
    engine.run_source(SCRIPT);

    let reports = engine.edit_unit(1, "let b = \"two\"");

    let failed: Vec<usize> = reports.iter()
        .filter(|report| matches!(report.eval, Eval::Error(_)))
        .map(|report| report.unit)
        .collect();
    assert_eq!(failed, vec![3], "`\"two\" + 5` is D's error to report");
    assert_eq!(value_of(&mut engine, "c"), "30", "C never heard about it");
}

/// A line submitted after an edit is compiled against the edited session.
#[test]
fn a_unit_appended_after_an_edit_sees_the_new_values() {
    let mut engine = engine();
    engine.run_source(SCRIPT);
    engine.edit_unit(1, "let b = 3");

    engine.run_source("let e = c + d");

    assert_eq!(value_of(&mut engine, "e"), "38", "30 from C and 8 from the new D");
}

// ============================================================================
// Truncating, removing and inserting
// ============================================================================

/// A B C D, where D reads A's binding and nothing reads B's.
///
/// So a splice at B re-derives C and D, which the *name* edges do not reach.
const INDEPENDENT: &str = "let a = 1\n---\nlet b = 2\n---\nlet c = 30\n---\nlet d = a + 5";

/// A B C D over one name, so an inserted unit is seen and then shadowed.
const SHADOWED: &str = "let n = 1\n---\nlet b = n + 1\n---\nlet n = 7\n---\nlet d = n + 100";

/// **Truncating drops the units from `n` on, and the next line takes the index
/// the first of them had.**
#[test]
fn truncating_drops_the_units_from_n_on() {
    let mut engine = engine();
    engine.run_source(SCRIPT);
    assert_eq!(value_of(&mut engine, "d"), "7");

    engine.truncate_units(2);

    let bound: Vec<String> = engine.get_environment().into_iter()
        .map(|(name, _, _)| name).collect();
    assert_eq!(bound, vec!["a".S(), "b".S()], "C and D are gone with their units");

    engine.run_source("let e = b + 1");
    assert_eq!(value_of(&mut engine, "e"), "3", "and the session goes on");
}

/// **Removing a unit nothing later depends on re-derives the whole suffix**, and
/// what survives computes the right values.
///
/// The suffix rather than the reach: `d` reads `a` and not `b`, so the name
/// edges reach only C, but D's index moved and so did the identity of every
/// value in it.
#[test]
fn removing_a_unit_rederives_the_suffix() {
    let mut engine = engine();
    engine.run_source(INDEPENDENT);

    let reports = engine.remove_unit(1);

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![1, 2],
        "C and D, at the indices they now have",
    );
    assert_eq!(value_of(&mut engine, "a"), "1", "A is upstream of the splice");
    assert_eq!(value_of(&mut engine, "c"), "30");
    assert_eq!(value_of(&mut engine, "d"), "6", "D still reads A's `a`");
    assert!(
        !engine.get_environment().iter().any(|(name, _, _)| name == "b"),
        "`b` went with the unit that bound it",
    );
}

/// **Removing a unit a later unit depends on leaves that unit failing**, in the
/// session, and putting the binding back makes it run again.
#[test]
fn removing_a_unit_a_later_unit_depends_on_leaves_it_failing() {
    let mut engine = engine();
    engine.run_source(SCRIPT);

    let reports = engine.remove_unit(1);

    let failed: Vec<usize> = reports.iter()
        .filter(|report| matches!(report.eval, Eval::Error(_)))
        .map(|report| report.unit)
        .collect();
    assert_eq!(failed, vec![2], "D, at the index it now has, reads the `b` that went");
    assert!(!bound(&mut engine, "d"), "D provides nothing");
    assert_eq!(value_of(&mut engine, "c"), "30");

    engine.insert_unit(1, "let b = 2", false);
    assert_eq!(value_of(&mut engine, "d"), "7", "D compiled and ran against the new `b`");
}

/// **Inserting in the middle re-derives the suffix**, the inserted unit's
/// bindings reach the units after it, and a later unit that shadows one still
/// wins.
#[test]
fn inserting_a_unit_rederives_the_suffix() {
    let mut engine = engine();
    engine.run_source(SHADOWED);
    assert_eq!(value_of(&mut engine, "b"), "2");

    let reports = engine.insert_unit(1, "let n = 5", false);

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "the inserted unit and the three that moved up",
    );
    assert_eq!(value_of(&mut engine, "b"), "6", "B reads the inserted `n`");
    assert_eq!(value_of(&mut engine, "d"), "107", "the later `n` still shadows it");
}

/// An inserted expression unit reports the value it came to.
///
/// The flag is what says which it is -- nothing in the text does -- and an
/// expression unit has to be run as one, because it computes a value that needs
/// somewhere to land.
#[test]
fn an_inserted_expression_unit_reports_its_value() {
    let mut engine = engine();
    engine.run_source("let a = 1\n---\nlet b = a + 1");

    let reports = engine.insert_unit(1, "a + 100", true);

    assert_eq!(reports.len(), 2, "the expression and the unit it moved up");
    assert!(
        matches!(&reports[0].eval, Eval::SuccessExpr(expr) if expr.value == "101"),
        "the inserted expression's value: {:?}", reports[0].eval,
    );
    assert_eq!(value_of(&mut engine, "b"), "2", "B is re-derived at its new index");
}

/// **An insertion that breaks a later unit leaves it failing**, and fixing the
/// inserted unit makes it run again.
#[test]
fn an_insertion_that_breaks_a_later_unit_leaves_it_failing() {
    let mut engine = engine();
    engine.run_source(SHADOWED);

    let reports = engine.insert_unit(1, "let n = \"five\"", false);

    assert!(
        reports.iter().any(|report| report.unit == 2 && matches!(report.eval, Eval::Error(_))),
        "B would add 1 to a string: {reports:?}",
    );
    assert!(!bound(&mut engine, "b"));
    assert_eq!(value_of(&mut engine, "d"), "107", "C still shadows `n` for D");

    engine.edit_unit(1, "let n = 5");
    assert_eq!(value_of(&mut engine, "b"), "6", "B ran against the fixed `n`");
}

// ============================================================================
// Units that did not run to their end
// ============================================================================

/// **A unit that does not compile stays, and an edit upstream can make it.**
///
/// B asks for a `c` nothing binds. Editing A to bind one reaches B, because
/// B's question is in the graph although the answer was no, and B compiles and
/// runs. C, which reads nothing of either, is untouched.
#[test]
fn a_unit_that_did_not_compile_goes_green_when_an_edit_lets_it() {
    let mut engine = engine();
    let results = engine.run_source("let a = 1\n---\nlet b = c + 1\n---\nlet z = a + 100");
    assert!(matches!(results[1].eval, Some(Eval::Error(_))), "{:?}", results[1].eval);
    assert_eq!(value_of(&mut engine, "z"), "101", "C is numbered as the compiler numbered it");

    let reports = engine.edit_unit(0, "let a = 1\nlet c = 10");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "A, B which asked for `c`, and C which reads `a`",
    );
    assert_eq!(value_of(&mut engine, "b"), "11");
    assert_eq!(value_of(&mut engine, "z"), "101");
}

/// **A unit that does not parse stays too**, and is not typechecked: it is as if
/// it were not there until an edit makes it parse.
#[test]
fn a_unit_that_did_not_parse_stays_and_can_be_fixed() {
    let mut engine = engine();
    let results = engine.run_source("let a = 1\n---\nlet = = =\n---\nlet b = a + 1");
    assert!(matches!(results[1].eval, Some(Eval::Error(_))), "{:?}", results[1].eval);
    assert_eq!(value_of(&mut engine, "b"), "2", "B reads A's `a`, as if B were not there");

    engine.edit_unit(1, "let a = 5");

    assert_eq!(value_of(&mut engine, "a"), "5");
    assert_eq!(value_of(&mut engine, "b"), "6", "B now reads the fixed unit's `a`");
}

/// A B C, where B returns early through `!` and C reads what B binds.
const EARLY: &str = "let xs: [u32] = []\n---\nlet x = xs[(: index / 5)]!\n---\n\
    let y = icall add_wrapping_u32(x, : u32 / 1)";

/// **A unit that returns early stays, offers nothing, and blocks what reads it.**
///
/// B's `x` is never written, so C compiles against it and is not run. Neither
/// binding is on offer, and A is untouched by any of it.
#[test]
fn a_unit_that_returned_early_blocks_what_reads_it() {
    let mut engine = engine();
    let results = engine.run_source(EARLY);

    assert!(
        matches!(&results[1].eval, Some(Eval::ReturnedEarly(returned)) if returned.contains("out of bounds")),
        "{:?}", results[1].eval,
    );
    assert!(
        matches!(&results[2].eval, Some(Eval::Blocked { on }) if *on == vec![1]),
        "{:?}", results[2].eval,
    );
    assert!(!bound(&mut engine, "x"), "B did not get as far as binding `x`");
    assert!(!bound(&mut engine, "y"), "C did not run");
    assert_eq!(value_of(&mut engine, "xs"), "[]");

    let results = engine.run_source("x");
    assert!(
        matches!(&results[0].eval, Some(Eval::Blocked { on }) if *on == vec![1]),
        "showing `x` reads B too: {:?}", results[0].eval,
    );
}

/// **Fixing what made a unit return early runs it, and what it blocked.**
#[test]
fn a_unit_that_returned_early_goes_green_when_an_edit_lets_it() {
    let mut engine = engine();
    engine.run_source(EARLY);

    let reports = engine.edit_unit(0, "let xs: [u32] = [1, 2, 3, 4, 5, 6]");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0, 1, 2],
    );
    assert_eq!(value_of(&mut engine, "x"), "6");
    assert_eq!(value_of(&mut engine, "y"), "7");

    let reports = engine.edit_unit(0, "let xs: [u32] = [1]");
    assert!(matches!(reports[1].eval, Eval::ReturnedEarly(_)), "{:?}", reports[1].eval);
    assert!(matches!(&reports[2].eval, Eval::Blocked { on } if *on == vec![1]), "{:?}", reports[2].eval);
    assert!(!bound(&mut engine, "x"), "and back again: B's frame was destroyed");
    assert!(!bound(&mut engine, "y"));
}

/// **A unit that returns early takes nothing with it but itself.**
///
/// What it bound before it returned is not on offer, and a name it binds
/// shadows an earlier one for the units after it, which read it and are
/// blocked, rather than reading past it.
#[test]
fn a_unit_that_returned_early_shadows_and_offers_nothing() {
    let mut engine = engine();
    engine.run_source(
        "let n: u32 = 1\n---\nlet xs: [u32] = []\n---\n\
         let made = \"made\"\nlet n = xs[(: index / 5)]!\n---\n\
         let m = icall add_wrapping_u32(n, : u32 / 1)");

    assert!(!bound(&mut engine, "made"), "bound before the return, and not offered");
    assert!(!bound(&mut engine, "n"), "the nearest `n` is the one that was never written");
    assert!(!bound(&mut engine, "m"), "and the unit reading it did not run");
    assert_eq!(value_of(&mut engine, "xs"), "[]");
}

/// **A unit that returns early keeps what it wrote into an earlier unit.**
///
/// Pinned as it is rather than as it should be: the `set` lands in A's frame
/// before C returns, and nothing puts it back, so a failed unit does influence
/// the session. `botdocs/issues.md` has what fixing it would take; this test
/// should flip when it is fixed.
#[test]
fn a_unit_that_returned_early_keeps_its_writes_for_now() {
    let mut engine = engine();
    let results = engine.run_source(
        "var p = (1, \"a\")\n---\nlet xs: [u32] = []\n---\n\
         set p.1 = \"mutated\"\nlet x = xs[(: index / 5)]!");

    assert!(matches!(results[2].eval, Some(Eval::ReturnedEarly(_))), "{:?}", results[2].eval);
    assert_eq!(value_of(&mut engine, "p"), "(1, \"mutated\")");
}

// ============================================================================
// Editing a module
// ============================================================================

/// **Editing a module reaches the units that import from it**, through the
/// engine.
///
/// The gap section E of `botdocs/plan-script-reactivity.md` is about, closed at
/// the level a session uses it: nothing in the engine could edit a module
/// before, which is the only reason the staleness was not a live bug.
///
/// Two things had to happen and the second is what a unit edit does not need:
/// the units that import from the module are re-lowered and re-executed, and
/// the recompiled modules are put in front of the executor. A script unit names
/// a module function by `CodeRef::Module`, so without the second step the call
/// would land on the module as it was when the session started.
#[test]
fn editing_a_module_rederives_the_units_that_import_from_it() {
    let mut engine = engine_with_module("fun f(x: int): int\n    ret x\nend fun\n");
    engine.run_source(
        "require module local/test/m\nimport m.f\nlet a = f(1)\n---\nlet b = 20");
    assert_eq!(value_of(&mut engine, "a"), "1");

    let reports = engine
        .edit_module("local", "test", "m", "fun f(x: int): int\n    ret x + 41\nend fun\n")
        .expect("the edited module compiles");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0],
        "the importing unit, and not the unit that imports nothing",
    );
    assert!(
        reports.iter().all(|report| !matches!(report.eval, Eval::Error(_))),
        "it re-derives cleanly: {reports:?}",
    );
    assert_eq!(value_of(&mut engine, "a"), "42", "the call landed on the new body");
    assert_eq!(value_of(&mut engine, "b"), "20", "the other unit is untouched");
}

/// A module edit carries on to the units that read what the importing unit
/// computed.
#[test]
fn a_module_edit_reaches_the_dependents_of_the_importing_unit() {
    let mut engine = engine_with_module("fun f(x: int): int\n    ret x\nend fun\n");
    engine.run_source(
        "require module local/test/m\nimport m.f\nlet a = f(1)\n---\nlet b = a + 5\n---\nlet c = 100");

    let reports = engine
        .edit_module("local", "test", "m", "fun f(x: int): int\n    ret x * 10\nend fun\n")
        .expect("the edited module compiles");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0, 1],
        "the importing unit and the unit that reads its binding",
    );
    assert_eq!(value_of(&mut engine, "a"), "10");
    assert_eq!(value_of(&mut engine, "b"), "15");
    assert_eq!(value_of(&mut engine, "c"), "100");
}

/// A module edit that does not compile is put back, and the session goes on.
///
/// Nothing in the engine can work against a module set with errors -- every
/// later line would fail to build a compiler at all -- so the edit is rejected
/// whole rather than left in place with the errors reported. What is asserted
/// here is that the session still works afterwards, which is the reason for
/// doing it that way.
#[test]
fn a_module_edit_that_does_not_compile_is_rejected() {
    let mut engine = engine_with_module("fun f(x: int): int\n    ret x\nend fun\n");
    engine.run_source("require module local/test/m\nimport m.f\nlet a = f(1)");

    let error = engine
        .edit_module("local", "test", "m", "fun f(x: int): int\n    ret \"nope\"\nend fun\n")
        .expect_err("returning a string where an int was promised is an error");
    assert!(!error.is_empty(), "the module's errors are reported");

    assert_eq!(value_of(&mut engine, "a"), "1", "the session is as it was");
    engine.run_source("let b = a + 1");
    assert_eq!(value_of(&mut engine, "b"), "2", "and it still compiles new lines");
}
