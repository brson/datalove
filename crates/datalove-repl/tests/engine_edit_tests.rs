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
