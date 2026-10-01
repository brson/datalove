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

fn value_of(engine: &mut Engine, name: &str) -> String {
    engine.get_environment().into_iter()
        .find(|(bound, _, _)| bound == name)
        .map(|(_, _, value)| value)
        .unwrap_or_else(|| panic!("no binding named {name}"))
}

fn engine() -> Engine {
    Engine::new(datalove_stdlib::system_library()).expect("the engine starts")
}

/// An engine over the system library and one local module of our own.
///
/// The module is what a module edit needs something to be about: the system
/// library is the copy embedded in the binary and editing it would recompile
/// the standard library, which is not what these are measuring.
fn engine_with_module(source: &str) -> Engine {
    let sys = datalove_stdlib::system_library();
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

    let reports = engine.remove_unit(1).expect("nothing reads `b`");

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

/// **Removing a unit a later unit depends on is rejected, and the session is
/// unchanged and still usable.**
///
/// D is `b + 5`, so taking B out leaves it with nothing to resolve `b` to. A
/// unit that fails to compile has no frame, and the numbering the frame store
/// shares with every `(unit, value)` reference cannot have a hole in it, so the
/// removal is put back whole and the frame store is never touched.
#[test]
fn removing_a_unit_a_later_unit_depends_on_is_rejected() {
    let mut engine = engine();
    engine.run_source(SCRIPT);

    let error = engine.remove_unit(1).expect_err("D reads `b`, which B provides");
    assert!(!error.is_empty(), "the suffix's errors are reported");

    assert_eq!(value_of(&mut engine, "b"), "2", "the session is as it was");
    assert_eq!(value_of(&mut engine, "d"), "7");
    engine.run_source("let e = d + 1");
    assert_eq!(value_of(&mut engine, "e"), "8", "and it still compiles new lines");
}

/// **Inserting in the middle re-derives the suffix**, the inserted unit's
/// bindings reach the units after it, and a later unit that shadows one still
/// wins.
#[test]
fn inserting_a_unit_rederives_the_suffix() {
    let mut engine = engine();
    engine.run_source(SHADOWED);
    assert_eq!(value_of(&mut engine, "b"), "2");

    let reports = engine.insert_unit(1, "let n = 5", false).expect("the suffix compiles");

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

    let reports = engine.insert_unit(1, "a + 100", true).expect("the suffix compiles");

    assert_eq!(reports.len(), 2, "the expression and the unit it moved up");
    assert!(
        matches!(&reports[0].eval, Eval::SuccessExpr(expr) if expr.value == "101"),
        "the inserted expression's value: {:?}", reports[0].eval,
    );
    assert_eq!(value_of(&mut engine, "b"), "2", "B is re-derived at its new index");
}

/// **An insertion whose suffix does not compile is rejected, and the session is
/// unchanged.**
#[test]
fn an_insertion_that_breaks_a_later_unit_is_rejected() {
    let mut engine = engine();
    engine.run_source(SHADOWED);

    let error = engine
        .insert_unit(1, "let n = \"five\"", false)
        .expect_err("B would add 1 to a string");
    assert!(error.contains("unit 2"), "B is the unit that failed: {error}");

    assert_eq!(value_of(&mut engine, "b"), "2", "B is back to reading the first `n`");
    assert_eq!(value_of(&mut engine, "d"), "107");
    engine.run_source("let e = b + d");
    assert_eq!(value_of(&mut engine, "e"), "109", "and the session still computes");
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
