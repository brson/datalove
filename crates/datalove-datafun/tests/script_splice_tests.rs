//! Truncating a session, and splicing a unit out of or into one.
//!
//! **A splice re-derives its reach, as an edit does, and nothing else.** A
//! script value is identified by `(unit id, ValueId)`, and a unit's id is its
//! own wherever units are spliced in or out around it, so a unit that uses
//! nothing the spliced unit provided or provides holds no reference that moved.
//! An insertion is an edit from nothing and a deletion an edit to nothing, and
//! the expectations here are derived from the graph as the edit tests' are:
//! `removing_a_unit_rederives_its_reach_not_the_suffix` is the case where the
//! two differ.
//!
//! Truncating is a deletion of the units from a point on, with nothing after
//! them to reach.
//!
//! **A unit that a splice stops compiling stays**, as any unit that fails
//! does: it keeps its place and its id with no frame and provides nothing, and
//! an edit that makes it compile again runs it. The tests for that assert a
//! value computed *after* the failure, and after the fix, not merely that
//! nothing panicked.
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

/// The reach of a deletion at `at`, from the reach of an edit to that unit in
/// the graph before it: the units after it, at the indices they have once it
/// is gone.
fn reach_once_removed(reach: &BTreeSet<usize>, at: usize) -> BTreeSet<usize> {
    reach.iter()
        .filter(|unit| **unit != at)
        .map(|unit| if *unit > at { unit - 1 } else { *unit })
        .collect()
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

/// **A unit appended after a truncation takes the next id, not a freed one.**
///
/// An id is given once, so nothing that held one of the dropped units' can
/// mistake the new unit for it, and the values it computes are its own.
#[test]
fn appending_after_a_truncation_takes_the_next_id() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = 2"));
    session.append(&unit("C", "let c = b + 30"));
    assert_eq!(session.framed(), vec![0, 1, 2]);
    session.marks_run(&["A", "B", "C"]);

    session.truncate(1);
    assert_eq!(session.framed(), vec![0], "two frames went with the two units");

    session.append(&unit("D", "let d = a + 9"));

    assert_eq!(session.ids(), vec![1, 4], "D is the fourth unit made");
    assert_eq!(session.framed(), vec![0, 1]);
    assert_eq!(session.binding("d"), "10", "and computed against the `a` that is left");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::from(["D"]));
}

/// **A function defined by a unit goes when that unit's frame does.**
///
/// A unit's frame and its *functions* live under the same id in two different
/// places -- `FrameStore::frames` and `UnitFunctionRegistry::unit_functions` --
/// and a cross-unit call reads the second by that id. The two are kept in
/// step by dropping both, and this is a case that would show it: a later unit
/// calling a function has to land on the one it named, not on one that was
/// dropped.
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

/// **Removing a unit nothing later depends on re-derives nothing, and the
/// units after it keep their values.**
#[test]
fn removing_a_unit_nothing_depends_on_rederives_nothing() {
    let mut session = four_units();

    let rederived = session.remove(1);

    assert_eq!(rederived, BTreeSet::new(), "nothing read what B provided");
    assert_eq!(session.unit_count(), 3);
    assert!(!session.has_binding("b"), "B is gone");
    assert_eq!(session.binding("a"), "1", "A is upstream of the splice");
    assert_eq!(session.binding("c"), "30");
    assert_eq!(session.binding("d"), "6", "D still reads A's `a`");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::new());
}

/// **A deletion re-derives its reach, and the reach is narrower than the
/// suffix.**
///
/// The expectation is derived from the graph rather than written down: what
/// the name edges reach from the deleted unit, less the unit itself. When a
/// unit's position was its identity this was the whole suffix, since every
/// unit after the splice point moved; a unit's id does not move.
#[test]
fn removing_a_unit_rederives_its_reach_not_the_suffix() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = 2"));
    session.append(&unit("C", "let c = 30"));
    session.append(&unit("D", "let d = b + 5"));
    session.append(&unit("E", "let e = c + 1"));
    session.marks_run(&["A", "B", "C", "D", "E"]);
    let reach = session.graph().reach(1);
    assert_eq!(reach, BTreeSet::from([1, 3]), "B, and D, which reads `b`");
    let expected = reach_once_removed(&reach, 1);

    let rederived = session.remove(1);

    assert_eq!(rederived, expected, "D, at the index it now has");
    let suffix: BTreeSet<usize> = (1..session.unit_count()).collect();
    assert!(rederived.len() < suffix.len(), "{rederived:?} against the suffix {suffix:?}");
    assert_eq!(session.failed(), vec![2], "D, which read the `b` that went");
    assert_eq!(session.binding("e"), "31", "E is untouched and still holds its value");
    assert_eq!(session.marks_run(&["A", "B", "C", "D", "E"]), BTreeSet::new());
}

/// **Removing the last unit is a truncation.**
#[test]
fn removing_the_last_unit_rederives_nothing() {
    let mut session = four_units();

    let rederived = session.remove(3);

    assert_eq!(rederived, BTreeSet::new(), "there is nothing after it to reach");
    assert!(!session.has_binding("d"));
    assert_eq!(session.binding("c"), "30");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::new());
}

/// **Removing a unit a later unit depends on leaves that unit failing, and in
/// the session.**
///
/// D reads `a`, so taking A out leaves D with nothing to resolve it to. D keeps
/// its place and its id, with no frame and no bindings, and putting a unit
/// that binds `a` back in front of it makes it compile and run again.
#[test]
fn removing_a_unit_a_later_unit_depends_on_leaves_it_failing() {
    let mut session = four_units();

    let rederived = session.remove(0);

    assert_eq!(rederived, BTreeSet::from([2]), "D, at the index it now has");
    assert_eq!(session.unit_count(), 3, "D is still in the session");
    assert_eq!(session.failed(), vec![2], "D, which reads the `a` that went");
    assert!(!session.has_binding("d"));
    assert_eq!(session.binding("b"), "2");
    assert_eq!(session.binding("c"), "30");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::new(), "B and C are untouched");

    let rederived = session.insert(0, &unit("A", "let a = 1"));

    assert_eq!(rederived, BTreeSet::from([0, 3]), "the new A, and D, which asked for `a`");
    assert_eq!(session.failed(), Vec::<usize>::new(), "D compiles against the new `a`");
    assert_eq!(session.binding("d"), "6");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::from(["A", "D"]));
    session.append(&unit("E", "let e = a + d"));
    assert_eq!(session.binding("e"), "7", "and the session still compiles new units");
}

/// **Removing a unit that moved a binding out puts the binding back**, by
/// running the unit that made it again, and the units that read it after the
/// move see it again.
#[test]
fn removing_a_unit_that_moved_a_binding_reruns_its_provider() {
    let mut session = Session::new();
    session.append(&unit("A", "let xs = [1, 2, 3]"));
    session.append(&unit("B", "let n = 5"));
    session.append(&unit("C", "let ys = xs"));
    session.append(&unit("D", "let m = n + 1"));
    session.marks_run(&["A", "B", "C", "D"]);
    assert_eq!(session.binding("xs"), "given away");

    let rederived = session.remove(2);

    assert_eq!(rederived, BTreeSet::from([0]), "A, to build `xs` again; B and D read nothing C did");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::from(["A"]));
    assert_eq!(session.binding("xs"), "[1, 2, 3]");
    assert!(!session.has_binding("ys"));
    assert_eq!(session.binding("m"), "6");
}

/// **A unit that read a binding a removed unit wrote reads it as it was**:
/// the provider runs again, and so does the reader, which read the write.
#[test]
fn removing_a_unit_that_wrote_a_var_reruns_its_provider_and_readers() {
    let mut session = Session::new();
    session.append(&unit("A", "var v = 1"));
    session.append(&unit("B", "set v = 10"));
    session.append(&unit("C", "let c = 30"));
    session.append(&unit("D", "let d = v + 1"));
    session.marks_run(&["A", "B", "C", "D"]);
    assert_eq!(session.binding("d"), "11");

    let rederived = session.remove(1);

    assert_eq!(rederived, BTreeSet::from([0, 2]), "A, to build `v` again, and D, which read B's write");
    assert_eq!(session.binding("d"), "2", "D reads `v` as A made it");
    assert_eq!(session.marks_run(&["A", "B", "C", "D"]), BTreeSet::from(["A", "D"]));
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

/// **Inserting in the middle re-derives the new unit and the units that now
/// find a binding of it, and a later unit that shadows one still wins.**
///
/// D reads C's `n`, which the new unit does not reach, so D keeps its frame
/// and its value without running.
#[test]
fn inserting_in_the_middle_rederives_what_the_new_unit_reaches() {
    let mut session = shadowing_units();

    let rederived = session.insert(1, &unit("X", "let n = 5"));

    assert_eq!(rederived, BTreeSet::from([1, 2]), "X, and B, which now reads its `n`");
    assert_eq!(rederived, session.graph().reach(1), "an insertion's reach is an edit's");
    assert_eq!(session.unit_count(), 5);
    assert_eq!(session.binding("b"), "6", "B reads the `n` the inserted unit binds");
    assert_eq!(session.binding("d"), "107", "C still shadows it for D");
    assert_eq!(session.binding("n"), "7", "the nearest binding of `n` is still C's");
    assert_eq!(
        session.marks_run(&["A", "B", "C", "D", "X"]),
        BTreeSet::from(["B", "X"]),
        "A, C and D did not run",
    );
}

/// **Inserting at the end is an append.**
#[test]
fn inserting_at_the_end_runs_only_the_new_unit() {
    let mut session = shadowing_units();

    let rederived = session.insert(4, &unit("X", "let x = n + 1"));

    assert_eq!(rederived, BTreeSet::from([4]), "there is nothing after it");
    assert_eq!(session.binding("x"), "8", "which reads C's `n`");
    assert_eq!(session.marks_run(&["A", "B", "C", "D", "X"]), BTreeSet::from(["X"]));
}

/// **Inserting a unit before one that wrote a binding the new unit reads
/// reruns from the binding's provider**, since the new unit would otherwise
/// read what the later write left, and the write runs again after it.
#[test]
fn inserting_a_reader_before_a_write_reruns_from_the_provider() {
    let mut session = Session::new();
    session.append(&unit("A", "var v = 1"));
    session.append(&unit("B", "let n = 5"));
    session.append(&unit("C", "set v = v + 10"));
    session.marks_run(&["A", "B", "C"]);
    assert_eq!(session.binding("v"), "11");

    let rederived = session.insert(1, &unit("X", "let k = v + 1"));

    assert_eq!(rederived, BTreeSet::from([0, 1, 3]), "A builds `v` again, X reads it, C writes it again");
    assert_eq!(session.binding("k"), "2", "X reads `v` as A made it");
    assert_eq!(session.binding("v"), "11", "and C's write is made once");
    assert_eq!(session.marks_run(&["A", "B", "C", "X"]), BTreeSet::from(["A", "C", "X"]));
}

/// **A unit that does not typecheck is inserted all the same, and provides
/// nothing.**
#[test]
fn inserting_a_unit_that_does_not_typecheck_keeps_it() {
    let mut session = shadowing_units();

    let rederived = session.insert(1, &unit("X", "let x = n + \"five\""));

    assert_eq!(rederived, BTreeSet::from([1]), "X alone: it provides nothing");
    assert_eq!(session.unit_count(), 5);
    assert_eq!(session.failed(), vec![1], "X, and none of the units after it");
    assert!(!session.has_binding("x"));
    assert_eq!(session.binding("b"), "2");
    assert_eq!(session.binding("d"), "107");

    session.append(&unit("E", "let e = d + 1"));
    assert_eq!(session.binding("e"), "108", "and the session still compiles new units");
}

/// **A unit that compiles but breaks a later one leaves that one failing**, and
/// fixing the inserted unit makes it run again.
///
/// The inserted unit is fine on its own; B is what stops compiling. The fix is
/// an edit like any other, and B is in its reach because it asks for the `n`
/// the edit makes an int again.
#[test]
fn inserting_a_unit_that_breaks_a_later_unit_leaves_that_unit_failing() {
    let mut session = shadowing_units();

    let rederived = session.insert(1, &unit("X", "let n = \"five\""));

    assert_eq!(rederived, BTreeSet::from([1, 2]));
    assert_eq!(session.failed(), vec![2], "B, at the index it now has");
    assert!(!session.has_binding("b"));
    assert_eq!(session.binding("d"), "107", "C still shadows `n` for D");

    session.edit(1, &unit("X", "let n = 5"));
    let rederived = session.rederive(1);

    assert_eq!(rederived, BTreeSet::from([1, 2]), "X, and B, which reads its `n`");
    assert_eq!(session.failed(), Vec::<usize>::new());
    assert_eq!(session.binding("b"), "6", "B ran against the fixed `n`");
    assert_eq!(session.binding("d"), "107");
}

/// **A unit that does not parse is inserted too**, and is not typechecked: it
/// provides nothing and asks for nothing until an edit makes it parse.
#[test]
fn inserting_a_unit_that_does_not_parse_keeps_it() {
    let mut session = shadowing_units();

    let rederived = session.insert(1, "let = = =\n");

    assert_eq!(rederived, BTreeSet::from([1]));
    assert_eq!(session.unit_count(), 5);
    assert_eq!(session.failed(), vec![1]);
    assert_eq!(session.binding("b"), "2", "B reads A's `n`, as if X were not there");

    session.edit(1, &unit("X", "let n = 5"));
    let rederived = session.rederive(1);

    assert_eq!(rederived, BTreeSet::from([1, 2]), "X, and B, which asked for `n`");
    assert_eq!(session.binding("b"), "6");
}

// ============================================================================
// The verbs together
// ============================================================================

/// A session stays usable across all three verbs in turn.
///
/// Each one leaves the ids, the frames and the bindings agreeing, which is
/// what the next one is taken against.
#[test]
fn the_three_verbs_compose() {
    let mut session = Session::new();
    session.append(&unit("A", "let a = 1"));
    session.append(&unit("B", "let b = a + 1"));
    session.append(&unit("C", "let c = b + 1"));
    session.append(&unit("D", "let d = c + 1"));
    assert_eq!(session.binding("d"), "4");

    session.insert(1, &unit("X", "let a = 10"));
    assert_eq!(session.binding("d"), "13", "B, C and D ran again against `a` at 10");
    assert_eq!(session.framed(), vec![0, 1, 2, 3, 4]);
    assert_eq!(session.ids(), vec![1, 5, 2, 3, 4]);

    session.remove(1);
    assert_eq!(session.binding("d"), "4", "back to the values it started with");
    assert_eq!(session.ids(), vec![1, 2, 3, 4]);

    session.truncate(2);
    assert!(!session.has_binding("c"));
    assert_eq!(session.framed(), vec![0, 1]);

    session.append(&unit("E", "let e = b * 100"));
    assert_eq!(session.binding("e"), "200");
    assert_eq!(session.ids(), vec![1, 2, 6]);
}
