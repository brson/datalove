//! The edit path, at the level a session actually uses it.
//!
//! `crates/datalove-datafun/tests/script_exec_reactivity_tests.rs` and
//! `script_scenario_tests.rs` are where the reach itself is measured, against
//! graphs they derive. These say the engine wires it up: an edit reaches the
//! units it should and the environment afterwards holds the values it should.
//!
//! These used to sit inline in `engine.rs`.

use rmx::prelude::*;

use datalove_datafun::pipeline::{
    ModuleDescriptor, PackageDescriptor, PackageLibrary, WorkspaceDescriptor,
};
use datalove_script_engine::{Engine, Eval, Unit};

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

/// Evaluate each `---`-separated fragment of `source`, in order.
fn run(engine: &mut Engine, source: &str) -> Vec<Eval> {
    source.split("\n---\n")
        .map(|fragment| engine.eval(Unit::Fragment(fragment.trim().S())))
        .collect()
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
    engine_with_modules(&[("m", source)])
}

/// The same with several modules, all in `local/test`.
fn engine_with_modules(modules: &[(&str, &str)]) -> Engine {
    let sys = datalove_sys_packages::system_library();
    let mut workspace = WorkspaceDescriptor::from_system_library(&sys);
    workspace.user_libraries.push(PackageLibrary {
        name: "local".S(),
        packages: [(
            "test".S(),
            PackageDescriptor {
                name: "test".S(),
                modules: modules.iter()
                    .map(|(name, source)| (
                        name.S(),
                        ModuleDescriptor { name: name.S(), source: (*source).into(), origin: None },
                    ))
                    .collect(),
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
    run(&mut engine, SCRIPT);
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
    run(&mut engine, SCRIPT);

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
    run(&mut engine, SCRIPT);
    engine.edit_unit(1, "let b = 3");

    run(&mut engine, "let e = c + d");

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
    run(&mut engine, SCRIPT);
    assert_eq!(value_of(&mut engine, "d"), "7");

    engine.truncate_units(2);

    let bound: Vec<String> = engine.get_environment().into_iter()
        .map(|(name, _, _)| name).collect();
    assert_eq!(bound, vec!["a".S(), "b".S()], "C and D are gone with their units");

    run(&mut engine, "let e = b + 1");
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
    run(&mut engine, INDEPENDENT);

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
    run(&mut engine, SCRIPT);

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
    run(&mut engine, SHADOWED);
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
    run(&mut engine, "let a = 1\n---\nlet b = a + 1");

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
    run(&mut engine, SHADOWED);

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
// Units that took or wrote an earlier unit's binding
// ============================================================================

/// **Editing a unit that moved an earlier binding re-runs the binding's unit
/// first**, rather than moving out of a binding that holds nothing.
#[test]
fn editing_a_unit_that_moved_a_binding_rebuilds_it_first() {
    let mut engine = engine();
    run(&mut engine, "let a = [1, 2]\n---\nlet b = a\n---\nlet c = 30");
    assert_eq!(value_of(&mut engine, "a"), "<moved>");

    let reports = engine.edit_unit(1, "let b = a");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0, 1],
        "A, to build `a` again, and B; C has nothing to do with it",
    );
    assert_eq!(value_of(&mut engine, "b"), "[1, 2]");
    assert_eq!(value_of(&mut engine, "a"), "<moved>");
}

/// **Editing a unit that reads a `var` a later unit wrote** re-runs it
/// against the `var` as it was, and the write after it.
#[test]
fn editing_a_reader_of_a_later_write_rewinds_to_the_var() {
    let mut engine = engine();
    run(&mut engine, "var v = 1\n---\nlet b = v + 1\n---\nset v = 10");

    engine.edit_unit(1, "let b = v + 2");

    assert_eq!(value_of(&mut engine, "b"), "3");
    assert_eq!(value_of(&mut engine, "v"), "10");
}

/// **Removing the unit that moved a binding gives the binding back.**
#[test]
fn removing_a_unit_that_moved_a_binding_gives_it_back() {
    let mut engine = engine();
    run(&mut engine, "let a = [1, 2]\n---\nlet n = 5\n---\nlet b = a");
    assert_eq!(value_of(&mut engine, "a"), "<moved>");

    let reports = engine.remove_unit(2);

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0, 1],
        "rewound to A, and B after it",
    );
    assert_eq!(value_of(&mut engine, "a"), "[1, 2]");
    assert!(!bound(&mut engine, "b"));

    run(&mut engine, "let c = a");
    assert_eq!(value_of(&mut engine, "c"), "[1, 2]", "and `a` can be named again");
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
    let results = run(&mut engine, "let a = 1\n---\nlet b = c + 1\n---\nlet z = a + 100");
    assert!(matches!(results[1], Eval::Error(_)), "{:?}", results[1]);
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
    let results = run(&mut engine, "let a = 1\n---\nlet = = =\n---\nlet b = a + 1");
    assert!(matches!(results[1], Eval::Error(_)), "{:?}", results[1]);
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
    let results = run(&mut engine, EARLY);

    assert!(
        matches!(&results[1], Eval::ReturnedEarly(returned) if returned.contains("out of bounds")),
        "{:?}", results[1],
    );
    assert!(
        matches!(&results[2], Eval::Blocked { on } if *on == vec![1]),
        "{:?}", results[2],
    );
    assert!(!bound(&mut engine, "x"), "B did not get as far as binding `x`");
    assert!(!bound(&mut engine, "y"), "C did not run");
    assert_eq!(value_of(&mut engine, "xs"), "[]");

    let results = [engine.eval(Unit::Expression("x".S()))];
    assert!(
        matches!(&results[0], Eval::Blocked { on } if *on == vec![1]),
        "showing `x` reads B too: {:?}", results[0],
    );
}

/// **Fixing what made a unit return early runs it, and what it blocked.**
#[test]
fn a_unit_that_returned_early_goes_green_when_an_edit_lets_it() {
    let mut engine = engine();
    run(&mut engine, EARLY);

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
    run(&mut engine, 
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
    let results = run(&mut engine, 
        "var p = (1, \"a\")\n---\nlet xs: [u32] = []\n---\n\
         set p.1 = \"mutated\"\nlet x = xs[(: index / 5)]!");

    assert!(matches!(results[2], Eval::ReturnedEarly(_)), "{:?}", results[2]);
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
    run(&mut engine, 
        "require module local/test/m\nimport m.f\nlet a = f(1)\n---\nlet b = 20");
    assert_eq!(value_of(&mut engine, "a"), "1");

    let reports = engine
        .edit_module("local", "test", "m", "fun f(x: int): int\n    ret x + 41\nend fun\n");

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
    run(&mut engine, 
        "require module local/test/m\nimport m.f\nlet a = f(1)\n---\nlet b = a + 5\n---\nlet c = 100");

    let reports = engine
        .edit_module("local", "test", "m", "fun f(x: int): int\n    ret x * 10\nend fun\n");

    assert_eq!(
        reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
        vec![0, 1],
        "the importing unit and the unit that reads its binding",
    );
    assert_eq!(value_of(&mut engine, "a"), "10");
    assert_eq!(value_of(&mut engine, "b"), "15");
    assert_eq!(value_of(&mut engine, "c"), "100");
}

/// A module edit that does not compile is kept, and fails what requires it.
///
/// The module leaves the compile, so the unit importing from it fails and its
/// binding goes, saying it is the module; the unit that does not is untouched.
/// The edit that fixes the module brings the unit back with the new value.
#[test]
fn a_module_edit_that_does_not_compile_fails_its_units_until_fixed() {
    let mut engine = engine_with_module("fun f(x: int): int\n    ret x\nend fun\n");
    run(&mut engine, "require module local/test/m\nimport m.f\nlet a = f(1)\n---\nlet b = 20");

    let reports = engine.edit_module("local", "test", "m", "fun f(x: int): int\n    ret \"nope\"\nend fun\n");
    assert_eq!(reports.iter().map(|report| report.unit).collect::<Vec<_>>(), vec![0]);
    let Eval::Error(error) = &reports[0].eval else {
        panic!("the importing unit fails: {reports:?}");
    };
    assert!(error.starts_with("module `local/test/m` does not compile: "), "{error}");
    assert!(engine.module_errors().broken.contains_key("local/test/m"));
    assert!(!bound(&mut engine, "a"), "its binding goes with it");
    assert_eq!(value_of(&mut engine, "b"), "20", "the other unit is untouched");
    run(&mut engine, "let c = b + 1");
    assert_eq!(value_of(&mut engine, "c"), "21", "and new lines still compile");

    let reports = engine.edit_module("local", "test", "m", "fun f(x: int): int\n    ret x + 1\nend fun\n");
    assert_eq!(reports.iter().map(|report| report.unit).collect::<Vec<_>>(), vec![0]);
    assert!(engine.module_errors().is_empty());
    assert_eq!(value_of(&mut engine, "a"), "2", "the unit is back, calling the fixed module");
}

/// A module that did not compile when a unit first required it comes in when
/// an edit fixes it, and the unit with it.
///
/// That unit's `require` failed, so when it last compiled it imported nothing
/// from the module; it is found by its imports as they resolve now.
#[test]
fn fixing_a_module_brings_back_a_unit_that_never_ran() {
    let mut engine = engine_with_module("fun f(): int\n    ret \"nope\"\nend fun\n");
    let eval = eval_of(&mut engine, "require module local/test/m\nimport m.f\nlet a = f()");
    assert!(matches!(eval, Eval::Error(_)), "{eval:?}");

    let reports = engine.edit_module("local", "test", "m", "fun f(): int\n    ret 7\nend fun\n");

    assert_eq!(reports.iter().map(|report| report.unit).collect::<Vec<_>>(), vec![0]);
    assert_eq!(value_of(&mut engine, "a"), "7");
}

/// An edit to a module reaches the units importing from a module that
/// requires it, whose functions call into it.
#[test]
fn a_module_edit_reaches_the_units_of_the_modules_requiring_it() {
    let mut engine = engine_with_modules(&[
        ("m", "fun f(): int\n    ret 1\nend fun\n"),
        ("n", "require module local/test/m\nimport m.f\nfun g(): int\n    ret f() + 10\nend fun\n"),
    ]);
    run(&mut engine, "require module local/test/n\nimport n.g\nlet a = g()");
    assert_eq!(value_of(&mut engine, "a"), "11");

    let reports = engine.edit_module("local", "test", "m", "fun f(): int\n    ret 2\nend fun\n");

    assert_eq!(reports.iter().map(|report| report.unit).collect::<Vec<_>>(), vec![0]);
    assert_eq!(value_of(&mut engine, "a"), "12");
}

/// Breaking a module fails the units of the modules requiring it too, which
/// leave the compile with it, and fixing it brings them back.
#[test]
fn breaking_a_module_fails_the_units_of_the_modules_requiring_it() {
    let mut engine = engine_with_modules(&[
        ("m", "fun f(): int\n    ret 1\nend fun\n"),
        ("n", "require module local/test/m\nimport m.f\nfun g(): int\n    ret f() + 10\nend fun\n"),
    ]);
    run(&mut engine, "require module local/test/n\nimport n.g\nlet a = g()");

    let reports = engine.edit_module("local", "test", "m", "fun f(): int\n    ret \"nope\"\nend fun\n");
    assert_eq!(reports.iter().map(|report| report.unit).collect::<Vec<_>>(), vec![0]);
    let Eval::Error(error) = &reports[0].eval else {
        panic!("the unit importing from `n` fails: {reports:?}");
    };
    assert!(error.starts_with("module `local/test/n` does not compile: "), "{error}");
    assert!(!bound(&mut engine, "a"));

    engine.edit_module("local", "test", "m", "fun f(): int\n    ret 3\nend fun\n");
    assert_eq!(value_of(&mut engine, "a"), "13");
}

/// A module compiled whether or not a unit requires it, which does not
/// compile, is reported and leaves the engine running.
#[test]
fn an_engine_starts_over_a_module_that_does_not_compile() {
    let sys = datalove_sys_packages::system_library();
    let workspace = engine_with_modules(&[("m", "fun f(): int\n    ret \"nope\"\nend fun\n")])
        .workspace()
        .C();
    let mut engine = Engine::with_options(
        sys, workspace, datalove_datafun::DebugOutputMode::Disabled, vec!["local/test/m".S()])
        .expect("the engine starts");

    let errors = engine.module_errors();
    assert_eq!(errors.broken.keys().collect::<Vec<_>>(), vec!["local/test/m"]);
    assert!(!errors.type_diagnostics.is_empty(), "with diagnostics to render");

    run(&mut engine, "let a = 1");
    assert_eq!(value_of(&mut engine, "a"), "1");
}

/// What one appended unit came to.
fn eval_of(engine: &mut Engine, source: &str) -> Eval {
    let mut results = run(engine, source);
    assert_eq!(results.len(), 1, "one unit");
    results.remove(0)
}

/// A unit requiring a module nothing required before compiles against it,
/// and the units already lowered still call the modules they named.
///
/// `b` is the only module when `g` is lowered, and `a`, which sorts ahead of
/// it, joins after. When modules were numbered by position `g` then called
/// `fa`; see `crates/datalove-datafun/tests/module_id_tests.rs`.
#[test]
fn a_module_a_later_unit_requires_joins_without_moving_the_others() {
    let mut engine = engine_with_modules(&[
        ("a", "fun fa(): int\n    ret 1\nend fun\n"),
        ("b", "fun fb(): int\n    ret 2\nend fun\n"),
    ]);
    run(&mut engine, 
        "require module local/test/b\nimport b.fb\nfun g(): int\n    ret fb()\nend fun\nlet x = g()");
    run(&mut engine, "require module local/test/a\nimport a.fa\nlet y = fa()");
    run(&mut engine, "let z = g()");

    assert_eq!(value_of(&mut engine, "x"), "2");
    assert_eq!(value_of(&mut engine, "y"), "1");
    assert_eq!(value_of(&mut engine, "z"), "2", "`g` still calls `fb`");
}

/// A module of the standard library joins the same way, natives and all.
#[test]
fn a_library_module_a_later_unit_requires_brings_its_natives() {
    let mut engine = engine();
    run(&mut engine, "let a = 1");
    run(&mut engine, "require module sys/std/string\nlet s = \"four\"\nlet n = string.len(ref s)");
    assert_eq!(value_of(&mut engine, "n"), "4");
}

/// A module that does not compile is a problem only for a unit that requires
/// it: the engine starts, and units that do not require it run.
#[test]
fn a_broken_module_fails_only_the_units_that_require_it() {
    let mut engine = engine_with_module("fun f(): int\n    ret \"nope\"\nend fun\n");

    run(&mut engine, "let a = 1");
    let eval = eval_of(&mut engine, "require module local/test/m\nimport m.f\nlet b = f()");
    let Eval::Error(error) = eval else {
        panic!("the requiring unit fails: {eval:?}");
    };
    assert!(
        error.starts_with("module `local/test/m` does not compile: ") && error.contains("F016"),
        "it says the module is why, and what is wrong with it: {error}",
    );
    run(&mut engine, "let c = a + 1");
    assert_eq!(value_of(&mut engine, "c"), "2", "the session goes on without the module");
    assert!(!bound(&mut engine, "b"));

    // A module required after the broken one still joins.
    run(&mut engine, "require module sys/std/string\nlet s = \"four\"\nlet n = string.len(ref s)");
    assert_eq!(value_of(&mut engine, "n"), "4", "the broken module does not hold the others back");
}

/// Taking away the only unit that requires a module takes the module out of
/// the compile, so the session carries on as if it had never been required.
#[test]
fn truncating_the_unit_that_required_a_module_leaves_the_rest_running() {
    let mut engine = engine_with_module("fun f(): int\n    ret 5\nend fun\n");
    run(&mut engine, "let a = 1");
    run(&mut engine, "require module local/test/m\nimport m.f\nlet b = f()");
    assert_eq!(value_of(&mut engine, "b"), "5");

    engine.truncate_units(1);
    run(&mut engine, "let c = a + 1");

    assert!(!bound(&mut engine, "b"));
    assert_eq!(value_of(&mut engine, "c"), "2");
}

// ============================================================================
// What a unit does to earlier units' bindings
// ============================================================================

/// **A write is an overwrite only if it happens on every way through the unit
/// before anything reads what the binding held.** Otherwise it is an update:
/// running the unit again needs the binding as it was. Each case is a unit
/// after a session that makes the `var`s, and says which of them it
/// overwrites and which it updates.
#[test]
fn a_write_is_an_overwrite_only_when_it_needs_nothing_the_binding_held() {
    let mut engine = engine();
    let setup = [
        "var b = 125",
        "var p = (1, \"a\")",
        "let flag = true",
        "let d = 0",
        "fun fill(out n: int)\n  set n = 42\nend fun\n\
         fun bump(mut n: int)\n  set n = n + 1\nend fun\n\
         fun do_nothing()\nend fun",
        "type Flag: enum { atom Yes, atom No }\nlet f: Flag = atom Yes",
    ];
    for source in setup {
        let eval = engine.eval(Unit::Fragment(source.S()));
        assert!(!matches!(eval, Eval::Error(_)), "{source} compiles: {eval:?}");
    }
    let setup = setup.len();

    let cases: &[(&str, &[&str], &[&str])] = &[
        ("set b = 500", &["b"], &[]),
        ("set b = b * 2", &[], &["b"]),
        ("set b += 1", &[], &["b"]),
        ("set b = 500\nlet c = b + 1", &["b"], &[]),
        ("let c = b + 1\nset b = 500", &[], &["b"]),
        ("set p.1 = \"b\"", &[], &["p"]),
        ("call fill(out b)", &["b"], &[]),
        ("call bump(mut b)", &[], &["b"]),
        ("if flag\n  set b = 500\nend if", &[], &["b"]),
        ("if flag\n  set b = 500\nelse\n  set b = 600\nend if", &["b"], &[]),
        ("match f\ncase atom Yes\n  set b = 500\ncase atom No\n  set b = 600\nend match", &["b"], &[]),
        ("match f\ncase atom Yes\n  set b = 500\ncase atom No\n  call do_nothing()\nend match", &[], &["b"]),
        ("var i = 0\nloop while i .< 3\n  set b = 500\n  set i = i + 1\nend loop", &[], &["b"]),
        // Leaving early, before the write, leaves `b` as it was.
        ("let q = 10 /! d\nset b = 500", &[], &["b"]),
        ("set b = 500\nlet q = 10 /! d", &["b"], &[]),
    ];
    for (source, overwritten, updated) in cases {
        let eval = engine.eval(Unit::Fragment(source.S()));
        assert!(!matches!(eval, Eval::Error(_)), "{source} compiles: {eval:?}");
        let effects = engine.unit_effects(setup);
        assert_eq!(effects.overwritten, *overwritten, "overwritten by: {source}");
        assert_eq!(effects.updated, *updated, "updated by: {source}");
        engine.truncate_units(setup);
    }
}

/// **A runtime error in a rerun empties the session**, as one in an appended
/// unit does, and the engine goes on from empty.
#[test]
fn a_runtime_error_in_a_rerun_empties_the_session() {
    let mut engine = engine();
    run(&mut engine, "fun down(s: string): u32\n  let t = s@\n  ret icall add_wrapping_u32(down(t), : u32 / 1)\nend fun\n\
        ---\nlet a = 1\n---\nlet b = a");

    let reports = engine.edit_unit(1, "let a = down(\"abc\")");

    let last = reports.last().expect("the edited unit is reported");
    assert_eq!(last.unit, 1);
    assert!(matches!(last.eval, Eval::ErrorReset(_)), "{:?}", last.eval);
    assert_eq!(engine.unit_count(), 0);
    run(&mut engine, "let c = 3");
    assert_eq!(value_of(&mut engine, "c"), "3");
}
