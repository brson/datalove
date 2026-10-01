//! Truncating a session, and splicing a unit out of or into one.
//!
//! The three verbs section G of `botdocs/plan-script-reactivity.md` is about,
//! and they are one mechanism: splice the unit list, rebuild the chain, destroy
//! the suffix's runtime state, re-derive the suffix.
//!
//! **The whole suffix, and that is the right answer rather than a reach bug.** A
//! script value is identified by `(unit_index, ValueId)`, so taking a unit out
//! or putting one in renumbers every unit after the splice point and their IR
//! has to be built again against the indices they now have. The ids really did
//! change. `the_whole_suffix_is_rederived_not_just_the_reach` says so by
//! deriving the reach from the graph and showing the suffix is *wider* -- a test
//! that pinned the reach here would be pinning a bug.
//!
//! Truncating is the special case that renumbers nothing, because nothing sits
//! after what goes, and so re-derives nothing at all.
//!
//! **A splice whose suffix stops compiling is rejected and put back.** A unit
//! that fails to compile has no IR, so it has no frame, so it would leave a
//! hole in the numbering the frame store and every `(unit_index, ValueId)`
//! reference share. The tests for that assert a value computed *after* the
//! rejection, not merely that nothing panicked: "unchanged" has to mean the
//! session still works.
//!
//! **Values, not only reach.** A recorded lesson from this work is that the
//! scenario matrix once missed a real bug by measuring reach and never asking
//! what a program computed, so every case here checks a value.

use rmx::prelude::*;
use rmx::std::collections::BTreeSet;

mod scriptsession;
use scriptsession::Session;

/// A unit that logs the name it is known by, so the marks say which ran.
fn unit(name: &str, text: &str) -> String {
    format!("{text}\ndebuglog \"ran {name}\"\n")
}

// ============================================================================
// Truncate
// ============================================================================

/// **Truncating drops the units from `n` on and their bindings with them.**
///
/// Nothing is re-derived, because nothing after them moved. The values those
/// units held are destroyed rather than leaked, which `DATALOVE_LEAK_CHECK`
/// has the say on and only `just test` sets.
#[test]
fn truncate_drops_the_units_from_n_on() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = 2"));
    session.append(&unit("C", "let c = b + 30"));
    assert_eq!(session.binding("c"), "32");
    session.marks_run(&["A", "B", "C"]);

    session.truncate(1);

    assert_eq!(session.unit_count(), 1, "A alone is left");
    assert!(session.has_binding("a"), "A survives");
    assert!(!session.has_binding("b"), "B's binding is gone");
    assert!(!session.has_binding("c"), "C's binding is gone");
    assert_eq!(
        session.marks_run(&["A", "B", "C"]),
        BTreeSet::new(),
        "truncating runs nothing",
    );
}

/// **A unit appended after a truncation takes the index that was freed.**
///
/// The frame store is shortened rather than left with holes in it, so the next
/// unit is unit 1 again, and the values it computes are its own.
#[test]
fn appending_after_a_truncation_reuses_the_freed_index() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = 2"));
    session.append(&unit("C", "let c = b + 30"));
    assert_eq!(session.frame_count(), 3);
    session.marks_run(&["A", "B", "C"]);

    session.truncate(1);
    assert_eq!(session.frame_count(), 1, "two frames went with the two units");

    session.append(&unit("D", "let d = a + 9"));

    assert_eq!(session.frame_count(), 2, "D took the index B had");
    assert_eq!(session.binding("d"), "10", "and computed against the `a` that is left");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::from(["D"]));
}

/// **A function defined by a unit goes when that unit's frame does.**
///
/// A unit's frame and its *functions* live at the same index in two different
/// places -- `FrameStore::frames` and `UnitFunctionRegistry::unit_functions` --
/// and a cross-unit call reads the second by that index. The two are kept in
/// step by truncating both, and this is the case that needs it: appending
/// *pushes* a unit's functions on the end, so if the registry were left long
/// while the frame store was shortened, the appended unit's functions would go
/// on at the dropped unit's index and a later unit calling one of them would
/// land on the function that was dropped.
///
/// So `h` has to return 99 here. Getting 2 means the call reached `g`.
#[test]
fn a_truncated_unit_s_functions_go_with_it() {
    let mut session = Session::new();
    session.append(&unit("A", "fun f(): int\n    ret 1\nend fun"));
    session.append(&unit("B", "fun g(): int\n    ret 2\nend fun"));

    session.truncate(1);
    session.append(&unit("C", "fun h(): int\n    ret 99\nend fun"));
    session.append(&unit("D", "let v = h()"));

    assert_eq!(session.binding("v"), "99", "the call must reach `h`, not the dropped `g`");
}

/// Truncating to the length it already has changes nothing.
#[test]
fn truncating_to_the_current_length_is_nothing() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = a + 1"));

    session.truncate(2);

    assert_eq!(session.unit_count(), 2);
    assert_eq!(session.binding("b"), "2");
    session.append(&unit("C", "let c = b + 1"));
    assert_eq!(session.binding("c"), "3", "and the session goes on");
}

// ============================================================================
// Remove
// ============================================================================

/// A B C D, where D reads A's binding and nothing reads B's.
///
/// So removing B re-derives C and D -- the whole suffix -- where the *reach*
/// from the splice point is C alone.
fn four_units() -> Session {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = 2"));
    session.append(&unit("C", "let c = 30"));
    session.append(&unit("D", "let d = a + 5"));
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]),
        BTreeSet::from(["A", "B", "C", "D"]), "each unit ran as it was appended");
    session
}

/// **Removing a unit nothing later depends on re-derives the suffix, and the
/// units that survive still compute the right values.**
#[test]
fn removing_a_unit_nothing_depends_on_rederives_the_suffix() {
    let mut session = four_units();

    let redone = session.remove(1).expect("nothing reads `b`");

    assert_eq!(redone, BTreeSet::from([1, 2]), "C and D, at the indices they now have");
    assert_eq!(session.unit_count(), 3);
    assert!(!session.has_binding("b"), "B is gone");
    assert_eq!(session.binding("a"), "1", "A is upstream of the splice");
    assert_eq!(session.binding("c"), "30");
    assert_eq!(session.binding("d"), "6", "D still reads A's `a`");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::from(["C", "D"]));
}

/// **The suffix is re-derived whole, and the reach is strictly narrower.**
///
/// The expectation is derived from the graph rather than written down: `C` is
/// what the name edges reach from the splice point, and `D` is re-derived
/// anyway because its index moved. **This is the assertion that would be wrong
/// to narrow.** A unit's index is part of every value's identity, so D's values
/// are not the values they were, whatever D reads.
#[test]
fn the_whole_suffix_is_rederived_not_just_the_reach() {
    let mut session = four_units();
    let graph = session.graph();
    let reach = graph.reach(1);
    assert_eq!(reach, BTreeSet::from([1]), "nothing after B reads what B provides");

    let redone = session.remove(1).expect("nothing reads `b`");

    assert_eq!(redone, BTreeSet::from([1, 2]), "the suffix: both units after the splice");
    assert!(
        redone.len() > reach.len(),
        "the suffix is wider than the reach, which is the point: {redone:?} against {reach:?}",
    );
    assert_eq!(session.binding("d"), "6", "and D, re-derived, computes its own value");
}

/// **Removing the last unit is a truncation, and renumbers nothing.**
#[test]
fn removing_the_last_unit_rederives_nothing() {
    let mut session = four_units();

    let redone = session.remove(3).expect("nothing comes after D");

    assert_eq!(redone, BTreeSet::new(), "there is no suffix to re-derive");
    assert!(!session.has_binding("d"));
    assert_eq!(session.binding("c"), "30");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::new());
}

/// **Removing a unit a later unit depends on is rejected, and the session is
/// unchanged and still usable.**
///
/// D reads `a`, so taking A out leaves D with nothing to resolve it to. A unit
/// that fails to compile has no frame, and the numbering cannot have a hole in
/// it, so the removal is put back whole. What is asserted is that the session
/// still *computes*: the frame store was never touched, so nothing of it is
/// half spliced.
#[test]
fn removing_a_unit_a_later_unit_depends_on_is_rejected() {
    let mut session = four_units();

    let errors = session.remove(0).expect_err("D reads `a`, which A provides");

    assert!(!errors.is_empty(), "the suffix's errors are reported: {errors:?}");
    assert!(
        errors.iter().any(|error| error.contains('a')),
        "the error is about the name that stopped resolving: {errors:?}",
    );
    assert_eq!(session.unit_count(), 4, "the unit list is as it was");
    assert_eq!(session.binding("a"), "1");
    assert_eq!(session.binding("b"), "2");
    assert_eq!(session.binding("c"), "30");
    assert_eq!(session.binding("d"), "6");

    session.append(&unit("E", "let e = a + d"));
    assert_eq!(session.binding("e"), "7", "and the session still compiles new units");
}

// ============================================================================
// Insert
// ============================================================================

/// A B C D over one name: A binds `n`, B reads it, C rebinds it, D reads that.
///
/// So a unit inserted at 1 is seen by B, and C still shadows it for D.
fn shadowing_units() -> Session {
    let mut session = Session::new();
    session.append(&unit("A", "let n = 1"));
    session.append(&unit("B", "let b = n + 1"));
    session.append(&unit("C", "let n = 7"));
    session.append(&unit("D", "let d = n + 100"));
    assert_eq!(session.binding("b"), "2");
    assert_eq!(session.binding("d"), "107");
    session.marks_run(&["A", "B", "C", "D"]);
    session
}

/// **Inserting in the middle re-derives the suffix, the inserted unit's
/// bindings reach the units after it, and a later unit that shadows one still
/// wins.**
#[test]
fn inserting_in_the_middle_rederives_the_suffix() {
    let mut session = shadowing_units();

    let redone = session.insert(1, &unit("X", "let n = 5")).expect("the suffix compiles");

    assert_eq!(
        redone,
        BTreeSet::from([1, 2, 3, 4]),
        "the inserted unit and the three that moved up",
    );
    assert_eq!(session.unit_count(), 5);
    assert_eq!(session.binding("b"), "6", "B reads the `n` the inserted unit binds");
    assert_eq!(session.binding("d"), "107", "C still shadows it for D");
    assert_eq!(session.binding("n"), "7", "the nearest binding of `n` is still C's");
    assert_eq!(
        session.marks_run(&["A", "B", "C", "D", "X"]),
        BTreeSet::from(["B", "C", "D", "X"]),
        "A is upstream of the splice and did not run",
    );
}

/// **Inserting at the end is an append**, and renumbers nothing.
#[test]
fn inserting_at_the_end_runs_only_the_new_unit() {
    let mut session = shadowing_units();

    let redone = session.insert(4, &unit("X", "let x = n + 1")).expect("the suffix compiles");

    assert_eq!(redone, BTreeSet::from([4]), "there is no suffix after it");
    assert_eq!(session.binding("x"), "8", "which reads C's `n`");
    assert_eq!(session.marks_run(&["A", "B", "C", "D", "X"]), BTreeSet::from(["X"]));
}

/// **A unit that does not typecheck is rejected, and the session is unchanged.**
#[test]
fn inserting_a_unit_that_does_not_typecheck_is_rejected() {
    let mut session = shadowing_units();

    let errors = session
        .insert(1, &unit("X", "let x = n + \"five\""))
        .expect_err("an int plus a string is a type error");

    assert!(!errors.is_empty(), "the errors are reported: {errors:?}");
    assert_eq!(session.unit_count(), 4, "the unit list is as it was");
    assert!(!session.has_binding("x"));
    assert_eq!(session.binding("b"), "2");
    assert_eq!(session.binding("d"), "107");

    session.append(&unit("E", "let e = d + 1"));
    assert_eq!(session.binding("e"), "108", "and the session still compiles new units");
}

/// **A unit that compiles but breaks a later one is rejected too.**
///
/// This is the half that exercises the put-back: the inserted unit is fine on
/// its own, and B is what stops compiling, so the whole suffix has to be
/// re-derived against the old list again before the session is handed back.
#[test]
fn inserting_a_unit_that_breaks_a_later_unit_is_rejected() {
    let mut session = shadowing_units();

    let errors = session
        .insert(1, &unit("X", "let n = \"five\""))
        .expect_err("B would add 1 to a string");

    assert!(
        errors.iter().any(|error| error.starts_with("unit 2:")),
        "B is the unit that failed, at the index it would have had: {errors:?}",
    );
    assert_eq!(session.unit_count(), 4);
    assert_eq!(session.binding("b"), "2", "B is back to reading A's `n`");
    assert_eq!(session.binding("d"), "107");

    session.append(&unit("E", "let e = b + d"));
    assert_eq!(session.binding("e"), "109", "and the session still computes");
}

/// **A unit the session already failed to parse is not inserted at all.**
///
/// Parse errors are checked before anything is spliced, the way the appending
/// path checks them before a unit joins the script.
#[test]
fn inserting_a_unit_that_does_not_parse_is_rejected() {
    let mut session = shadowing_units();

    let errors = session
        .insert(1, "let = = =\n")
        .expect_err("that is not a statement");

    assert!(!errors.is_empty(), "the parse errors are reported: {errors:?}");
    assert_eq!(session.unit_count(), 4);
    assert_eq!(session.binding("b"), "2");
}

// ============================================================================
// The verbs together
// ============================================================================

/// A session stays usable across all three verbs in turn.
///
/// Each one leaves the indices, the frames and the bindings agreeing, which is
/// what the next one is taken against.
#[test]
fn the_three_verbs_compose() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = a + 1"));
    session.append(&unit("C", "let c = b + 1"));
    session.append(&unit("D", "let d = c + 1"));
    assert_eq!(session.binding("d"), "4");

    session.insert(1, &unit("X", "let a = 10")).expect("the suffix compiles");
    assert_eq!(session.binding("d"), "13", "B, C and D ran again against `a` at 10");
    assert_eq!(session.frame_count(), 5);

    session.remove(1).expect("nothing but B reads `a`, and A provides it again");
    assert_eq!(session.binding("d"), "4", "back to the values it started with");
    assert_eq!(session.frame_count(), 4);

    session.truncate(2);
    assert!(!session.has_binding("c"));
    assert_eq!(session.frame_count(), 2);

    session.append(&unit("E", "let e = b * 100"));
    assert_eq!(session.binding("e"), "200");
    assert_eq!(session.frame_count(), 3);
}
