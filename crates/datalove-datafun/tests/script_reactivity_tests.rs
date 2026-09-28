//! How far an edit to one script unit travels, and which units it reaches.
//!
//! One rule, asked of a script whose dependency graph is known: **editing a unit
//! re-typechecks that unit and the units that use what it provides, and nothing
//! else.** For units A B C D where C does not use anything B provides, editing B
//! reaches B and D. A is upstream. **C is spared even though it sits between
//! them**, which is the whole of `botdocs/plan-script-reactivity.md`.
//!
//! This used not to hold. `typecheck_script_unit` was keyed on
//! `AccumulatedBindings` -- every binding from every earlier unit -- so B's
//! outputs moving gave C a new key whether or not C mentioned any of them, and C
//! re-ran for an edit it could not see. The key is now a position in the script,
//! and what the earlier units left behind is read through `binding_at` on a
//! lookup miss, so a unit depends on the names it actually used.
//!
//! **The assertion is which units re-ran, not how many.** A count can be right
//! for the wrong reasons -- two units re-running is the correct answer here and
//! also what you get if the wrong two run. `asked_names`, recorded in stage A,
//! says what each unit uses, so the expectation is derived from the graph rather
//! than written down: the units that re-ran must be exactly those whose
//! `asked_names` intersect what the edited unit provides.
//!
//! Attribution works by position. `typecheck_script_unit` takes two arguments,
//! so salsa keys it on an interned tuple and cannot name the unit in the event
//! it reports; but a cold run typechecks the units oldest first, each exactly
//! once, so the order of the first run's events gives a key per unit. This is
//! `ModuleKeys` for script units, and the alternative -- reshaping a compiler
//! phase's signature so a test can read its events -- is the wrong trade.

use rmx::prelude::*;
use rmx::std::collections::BTreeSet;
use salsa::Setter as _;

use bct::input::Source;
use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun as datafun;
use datalove_datafun_tycheck::{
    type_check_script_units, AutoAdaptMode, Script, ScriptEnv, ScriptUnit,
};

/// A B C D, where C uses nothing B provides and D uses `b`.
///
/// The same four units the plan measured. No module is required, so the
/// environment is empty and this is the script layer on its own.
const UNITS: [&str; 4] = [
    "let a = 1\n",
    "let b = 2\n",
    "let c = 30\n",
    "let d = b + 5\n",
];

/// The script and the environment, built from sources that outlive an edit.
///
/// `ScriptUnit` and `Script` are interned over those sources, so building them
/// again after an edit gives back the very same handles -- which is why the
/// memo keys survive.
fn build<'db>(
    db: &'db datafun::Database,
    sources: &[Source],
) -> (Script<'db>, ScriptEnv<'db>) {
    let units: Vec<ScriptUnit<'db>> =
        sources.iter().map(|source| ScriptUnit::new(db, *source, false)).collect();
    let script = Script::from_units(db, &units).expect("at least one unit");
    (script, ScriptEnv::new(db, Vec::new(), AutoAdaptMode::Disabled))
}

/// What each unit provides and what it uses, as owned text.
///
/// Owned because it has to be read before the database is borrowed mutably for
/// the edit, and everything salsa hands back is tied to that borrow.
struct Graph {
    provides: Vec<Vec<String>>,
    uses: Vec<Vec<String>>,
    failed: Vec<bool>,
}

impl Graph {
    fn of(db: &datafun::Database, sources: &[Source]) -> Graph {
        let (script, env) = build(db, sources);
        let outputs = type_check_script_units(db, script, env).unit_outputs(db);

        let mut graph = Graph { provides: Vec::new(), uses: Vec::new(), failed: Vec::new() };
        for output in &outputs {
            let provides: Vec<String> = output.new_vars(db).iter()
                .map(|(name, _, _)| name.as_str(db).S())
                .chain(output.new_fns(db).iter().map(|(name, _)| name.as_str(db).S()))
                .collect();
            let uses: Vec<String> = output.asked_names(db).iter()
                .map(|name| name.as_str(db).S())
                .collect();
            graph.provides.push(provides);
            graph.uses.push(uses);
            graph.failed.push(!output.result(db).errors(db).is_empty());
        }
        graph
    }

    /// The units whose uses intersect what unit `edited` provides.
    ///
    /// The edited unit is always in here: `asked_names` records a name the unit
    /// binds itself, deliberately, and that over-reporting is what makes it
    /// sound. See `script_graph_tests`.
    fn dependents_of(&self, edited: usize) -> BTreeSet<usize> {
        let provided: BTreeSet<&String> = self.provides[edited].iter().collect();
        (0..self.uses.len())
            .filter(|unit| {
                *unit == edited
                    || self.uses[*unit].iter().any(|name| provided.contains(name))
            })
            .collect()
    }
}

/// The key salsa reported for each unit's typecheck, oldest unit first.
///
/// Taken from a cold run, where `type_check_script_units` asks for the units in
/// order and each executes exactly once.
fn unit_keys(cold: &[ExecutedQuery], units: usize) -> Vec<salsa::Id> {
    let keys: Vec<salsa::Id> = cold.iter()
        .filter(|e| e.query == "typecheck_script_unit")
        .map(|e| e.key)
        .collect();
    assert_eq!(
        keys.len(), units,
        "a cold run should typecheck each unit exactly once, so that the order \
         of its events attributes a key to each; it ran {} times",
        keys.len(),
    );
    keys
}

/// The key of every `typecheck_script_unit` that ran, in order.
fn typechecks(executed: &[ExecutedQuery]) -> Vec<salsa::Id> {
    executed.iter()
        .filter(|e| e.query == "typecheck_script_unit")
        .map(|e| e.key)
        .collect()
}

/// Which units ran `typecheck_script_unit` among `executed`.
fn units_reached(executed: &[ExecutedQuery], keys: &[salsa::Id]) -> BTreeSet<usize> {
    typechecks(executed).into_iter()
        .map(|ran| {
            keys.iter().position(|key| *key == ran).unwrap_or_else(|| panic!(
                "a typecheck of something that is not one of the script's units: {ran:?}",
            ))
        })
        .collect()
}

// ============================================================================
// The graph the reach is measured against
// ============================================================================

/// The fixture really is the shape the claim is about.
///
/// Every test below compares a measured reach against `dependents_of`, so if
/// this were wrong they would all agree with each other and with nothing real.
#[test]
fn c_uses_nothing_b_provides_and_d_uses_b() {
    let db = datafun::Database::default();
    let sources: Vec<Source> = UNITS.iter().map(|text| Source::new(&db, text.S())).collect();
    let graph = Graph::of(&db, &sources);

    assert_eq!(graph.provides, vec![vec!["a"], vec!["b"], vec!["c"], vec!["d"]]);
    assert!(!graph.failed.iter().any(|failed| *failed), "the fixture typechecks");
    assert_eq!(
        graph.dependents_of(1),
        BTreeSet::from([1, 3]),
        "B and D depend on B; A is upstream and C uses {:?}", graph.uses[2],
    );
}

// ============================================================================
// The measurement the plan is held to
// ============================================================================

/// **Editing B re-typechecks B and D. C is a memo hit.**
///
/// `let b` becomes `var b`, which changes what B provides -- the binding is
/// mutable now -- without changing whether anything in the script is legal. So
/// the reach is not confused with a unit failing to typecheck.
///
/// D in the result is the control: if nothing propagated at all this would be
/// `{1}` and the test would fail for the opposite reason to the one it is
/// guarding against.
#[test]
fn editing_b_reaches_b_and_d_and_not_c() {
    let recorder = QueryRecorder::new();
    let mut db = datafun::Database::recording(&recorder);
    let sources: Vec<Source> = UNITS.iter().map(|text| Source::new(&db, text.S())).collect();

    let graph = Graph::of(&db, &sources);
    let expected = graph.dependents_of(1);
    let keys = unit_keys(&recorder.take(), UNITS.len());

    sources[1].set_text(&mut db).to(S("var b = 2\n"));

    let after = Graph::of(&db, &sources);
    assert!(!after.failed.iter().any(|failed| *failed), "the edit leaves the script legal");
    let reached = units_reached(&recorder.take(), &keys);

    assert_eq!(
        reached, expected,
        "editing B should reach exactly the units that use what B provides",
    );
    assert!(reached.contains(&3), "D uses `b`, so the edit has to reach it");
    assert!(!reached.contains(&2), "C uses nothing B provides, so it must be a memo hit");
}

/// An edit that changes only a value reaches only the unit it is in.
///
/// `let b = 2` becoming `let b = 3` leaves B providing the same binding of the
/// same type, so nothing downstream's *types* moved and no other unit is
/// typechecked again. Re-*executing* D is stage C's business, not this one's.
#[test]
fn a_value_only_edit_reaches_only_its_own_unit() {
    let recorder = QueryRecorder::new();
    let mut db = datafun::Database::recording(&recorder);
    let sources: Vec<Source> = UNITS.iter().map(|text| Source::new(&db, text.S())).collect();

    let _ = Graph::of(&db, &sources);
    let keys = unit_keys(&recorder.take(), UNITS.len());

    sources[1].set_text(&mut db).to(S("let b = 3\n"));

    let _ = Graph::of(&db, &sources);
    assert_eq!(
        units_reached(&recorder.take(), &keys),
        BTreeSet::from([1]),
        "B provides the same binding of the same type, so nobody else's types moved",
    );
}

/// An edit that changes a binding's type reaches the units that read it.
///
/// D stops typechecking, which is the point: the edit has to reach D for the
/// error to be found at all, and it must still not reach C.
#[test]
fn a_type_edit_reaches_the_units_that_read_the_binding() {
    let recorder = QueryRecorder::new();
    let mut db = datafun::Database::recording(&recorder);
    let sources: Vec<Source> = UNITS.iter().map(|text| Source::new(&db, text.S())).collect();

    let graph = Graph::of(&db, &sources);
    let expected = graph.dependents_of(1);
    let keys = unit_keys(&recorder.take(), UNITS.len());

    sources[1].set_text(&mut db).to(S("let b = \"two\"\n"));

    let after = Graph::of(&db, &sources);
    let reached = units_reached(&recorder.take(), &keys);

    assert!(after.failed[3], "`b + 5` over a string is an error, and D is where it is filed");
    assert!(!after.failed[2], "C is untouched, so it still typechecks");
    assert_eq!(reached, expected, "the reach is the graph, error or no error");
}

/// Appending a unit typechecks that unit and no other.
///
/// This is the property the prefix aggregate already had and the one a
/// position-keyed query must not lose: a REPL grows by appending, and
/// re-analyzing the whole history on every line is the thing to avoid.
#[test]
fn appending_a_unit_typechecks_only_that_unit() {
    let recorder = QueryRecorder::new();
    let db = datafun::Database::recording(&recorder);
    let mut sources: Vec<Source> = UNITS.iter().map(|text| Source::new(&db, text.S())).collect();

    let _ = Graph::of(&db, &sources);
    let keys = unit_keys(&recorder.take(), UNITS.len());

    sources.push(Source::new(&db, S("let e = c + 1\n")));
    let graph = Graph::of(&db, &sources);
    assert!(!graph.failed[4], "the appended unit typechecks: {:?}", graph.uses[4]);

    // The appended unit has no key from the cold run, so the assertion is that
    // exactly one typecheck ran and that it was not one of the four.
    let ran = typechecks(&recorder.take());
    assert_eq!(ran.len(), 1, "appending should typecheck one unit, not {}", ran.len());
    assert!(
        !keys.contains(&ran[0]),
        "appending re-typechecked a unit that was already there",
    );
}

/// A unit that failed to typecheck provides nothing to the units after it.
///
/// Which is what accumulating forward did, and the reason `unit_provides` reads
/// the errors: a binding of a type the compiler never settled on would be
/// carried into every later unit.
#[test]
fn a_unit_that_failed_provides_nothing() {
    let db = datafun::Database::default();
    let sources: Vec<Source> = ["let x = nope\n", "let y = x\n"].iter()
        .map(|text| Source::new(&db, text.S()))
        .collect();

    let graph = Graph::of(&db, &sources);
    assert!(graph.failed[0], "`nope` is undefined");
    assert!(graph.failed[1], "so `x` is not available to the unit after it");
}
