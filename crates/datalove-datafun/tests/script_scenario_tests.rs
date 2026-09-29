//! The reach of an edit, over every kind of edit and every position for it.
//!
//! `script_reactivity_tests` and `script_exec_reactivity_tests` each hold one
//! claim against one fixture. This crosses the edit kinds against the positions
//! an edit can be made at, and asks both halves of the same question of every
//! combination: which units were **typechecked** again, and which were
//! **lowered and executed** again.
//!
//! **The two answers are different and that is the point.** A value-only edit
//! moves no binding, so nothing but the edited unit is typechecked again -- and
//! every unit downstream of it holds a stale value and has to run. A
//! signature-changing edit moves a binding, so the units that read it are
//! typechecked too. Writing both expectations down for each row is what stops
//! one being mistaken for the other.
//!
//! **Both expectations are derived**, from the typechecker's own per-unit record
//! before and after the edit:
//!
//! - The execution reach is a walk over the union of the two graphs, name
//!   edges only. The union rather than either alone, because a binding the edit
//!   removes is absent from the new graph although the unit that read it must be
//!   told, and one it introduces is absent from the old.
//! - The analysis reach compares what each name the unit asked about *resolves
//!   to*, before against after. That is what `binding_at` answers and so what a
//!   unit's memo turns on, which is why it is a smaller set than the execution
//!   reach rather than the same one.
//!
//! **A module edit is the one row where analysis over-propagates**, and the
//! matrix says so rather than glossing it: `ScriptEnv` is interned over every
//! module's parse and spans, so editing any module gives every unit's typecheck
//! a new key and every unit is checked again. The execution reach stays narrow,
//! which is the half that decides correctness. Narrowing the env is
//! `botdocs/plan-script-reactivity.md`'s "sound end state" and is not done here.
//!
//! See also the longer session and the removal and ownership cases at the foot
//! of this file, which are the other holes the plan's confidence section names.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, BTreeSet};

use datalove_ct::query_events::{ExecutedQuery, QueryRecorder};
use datalove_datafun as datafun;
use datafun::pipeline::ScriptCompilationResult;

mod scriptsession;
use scriptsession::{analysis_reach, Graph, Module, Session};

// ============================================================================
// The fixture
// ============================================================================

/// Five units over three modules, so that every position has both kinds of
/// edit available to it.
///
/// Units 0, 2 and 4 each require and import from a module of their own, which
/// is what lets a module edit be aimed at the first, the middle or the last
/// unit. Units 1 and 3 read what earlier units bound: `b` from `a`, and `d`
/// from both `c` and `b`, so unit 3 is reached from either side.
const UNITS: [&str; 5] = [
    "require module local/test/m0\nimport m0.f0\nlet a = f0(1)\ndebuglog \"ran A\"\n",
    "let b = a + 1\ndebuglog \"ran B\"\n",
    "require module local/test/m1\nimport m1.f1\nlet c = f1(2)\ndebuglog \"ran C\"\n",
    "let d = c + b\ndebuglog \"ran D\"\n",
    "require module local/test/m2\nimport m2.f2\nlet e = f2(3)\ndebuglog \"ran E\"\n",
];

/// The modules the fixture imports from, one per importing unit.
fn modules() -> Vec<Module<'static>> {
    vec![
        Module {
            library: "local", package: "test", module: "m0",
            source: "fun f0(x: int): int\n    ret x\nend fun\n",
        },
        Module {
            library: "local", package: "test", module: "m1",
            source: "fun f1(x: int): int\n    ret x\nend fun\n",
        },
        Module {
            library: "local", package: "test", module: "m2",
            source: "fun f2(x: int): int\n    ret x\nend fun\n",
        },
    ]
}

/// Where an edit is made: the first unit, one in the middle, or the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    First,
    Middle,
    Last,
}

impl Position {
    /// The unit this position names, which is also the unit that imports from
    /// the module of the same position.
    fn unit(self) -> usize {
        match self {
            Position::First => 0,
            Position::Middle => 2,
            Position::Last => 4,
        }
    }
}

/// What an edit does to the thing it edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A different value behind the same binding of the same type.
    ValueOnly,
    /// The same names, bound differently -- a `let` becoming a `var`.
    Signature,
    /// An edit that makes the unit stop compiling.
    Error,
    /// A module function's body, which moves no signature.
    ModuleBody,
    /// A module function's signature, which the importing unit can no longer
    /// call the way it did.
    ModuleSignature,
}

/// One row of the matrix: an edit, and what it is expected to reach.
struct Scenario {
    kind: Kind,
    position: Position,
    /// The text the edited unit or module becomes.
    text: &'static str,
    /// Whether the edited thing leaves the script still compiling throughout.
    breaks: bool,
}

impl Scenario {
    fn name(&self) -> String {
        format!("{:?}/{:?}", self.kind, self.position)
    }

    /// Whether this row edits a module rather than a unit.
    fn is_module(&self) -> bool {
        matches!(self.kind, Kind::ModuleBody | Kind::ModuleSignature)
    }

    /// The module this row edits.
    fn module(&self) -> Module<'static> {
        let name = ["m0", "m1", "m2"][match self.position {
            Position::First => 0,
            Position::Middle => 1,
            Position::Last => 2,
        }];
        Module { library: "local", package: "test", module: name, source: self.text }
    }
}

/// Every edit kind at every position.
///
/// A new case is a row. The texts keep each unit's `require` and `import`
/// lines, because dropping them would unbind the alias and make the row about
/// something else.
fn scenarios() -> Vec<Scenario> {
    let mut rows = Vec::new();
    for position in [Position::First, Position::Middle, Position::Last] {
        let (value_only, signature, error) = match position {
            Position::First => (
                "require module local/test/m0\nimport m0.f0\nlet a = f0(7)\ndebuglog \"ran A\"\n",
                "require module local/test/m0\nimport m0.f0\nvar a = f0(1)\ndebuglog \"ran A\"\n",
                "require module local/test/m0\nimport m0.f0\nlet a = f0(\"x\")\ndebuglog \"ran A\"\n",
            ),
            Position::Middle => (
                "require module local/test/m1\nimport m1.f1\nlet c = f1(9)\ndebuglog \"ran C\"\n",
                "require module local/test/m1\nimport m1.f1\nvar c = f1(2)\ndebuglog \"ran C\"\n",
                "require module local/test/m1\nimport m1.f1\nlet c = f1(\"x\")\ndebuglog \"ran C\"\n",
            ),
            Position::Last => (
                "require module local/test/m2\nimport m2.f2\nlet e = f2(8)\ndebuglog \"ran E\"\n",
                "require module local/test/m2\nimport m2.f2\nvar e = f2(3)\ndebuglog \"ran E\"\n",
                "require module local/test/m2\nimport m2.f2\nlet e = f2(\"x\")\ndebuglog \"ran E\"\n",
            ),
        };
        let function = ["f0", "f1", "f2"][match position {
            Position::First => 0,
            Position::Middle => 1,
            Position::Last => 2,
        }];
        rows.push(Scenario { kind: Kind::ValueOnly, position, text: value_only, breaks: false });
        rows.push(Scenario { kind: Kind::Signature, position, text: signature, breaks: false });
        rows.push(Scenario { kind: Kind::Error, position, text: error, breaks: true });
        rows.push(Scenario {
            kind: Kind::ModuleBody,
            position,
            text: match function {
                "f0" => "fun f0(x: int): int\n    ret x * 100\nend fun\n",
                "f1" => "fun f1(x: int): int\n    ret x * 100\nend fun\n",
                _ => "fun f2(x: int): int\n    ret x * 100\nend fun\n",
            },
            breaks: false,
        });
        rows.push(Scenario {
            kind: Kind::ModuleSignature,
            position,
            text: match function {
                "f0" => "fun f0(x: string): int\n    ret 5\nend fun\n",
                "f1" => "fun f1(x: string): int\n    ret 5\nend fun\n",
                _ => "fun f2(x: string): int\n    ret 5\nend fun\n",
            },
            breaks: true,
        });
    }
    rows
}

// ============================================================================
// The harness for one row
// ============================================================================

/// A session over the fixture, with a recorder watching what salsa runs.
struct Run {
    session: Session,
    recorder: QueryRecorder,
    /// The key salsa reported for each unit's typecheck on the cold run, oldest
    /// unit first, which is how an event is attributed to a unit.
    cold_keys: Vec<salsa::Id>,
}

fn build() -> Run {
    let recorder = QueryRecorder::new();
    let mut session =
        Session::with_modules_in(datafun::Database::recording(&recorder), &modules());
    for unit in UNITS {
        session.append(unit);
    }
    assert_eq!(
        session.units_run(),
        (0..UNITS.len()).collect(),
        "every unit runs once as it is appended",
    );

    let cold_keys = typecheck_keys(&recorder.take());
    assert_eq!(
        cold_keys.len(), UNITS.len(),
        "a cold run typechecks each unit exactly once, so that the order of its \
         events attributes a key to each",
    );
    Run { session, recorder, cold_keys }
}

/// The key of every `typecheck_script_unit` that ran, in order.
fn typecheck_keys(executed: &[ExecutedQuery]) -> Vec<salsa::Id> {
    executed.iter()
        .filter(|event| event.query == "typecheck_script_unit")
        .map(|event| event.key)
        .collect()
}

/// Every binding in the environment and what it reads.
fn environment(session: &mut Session) -> BTreeMap<String, String> {
    session.executor.get_environment().into_iter()
        .map(|(name, _kind, _ty, value)| (name, value))
        .collect()
}

/// The unit a name resolves to, which is the last one to provide it.
fn provider(graph: &Graph, name: &str) -> Option<usize> {
    (0..graph.provides.len()).rev().find(|unit| graph.provides[*unit].contains(&name.S()))
}

// ============================================================================
// The matrix
// ============================================================================

/// Every row of the matrix, each in a session of its own.
///
/// One test rather than one per row because each row builds a five-unit session
/// over three modules, and the failure message names the row.
#[test]
fn every_edit_kind_at_every_position() {
    for scenario in scenarios() {
        check(&scenario);
    }
}

fn check(scenario: &Scenario) {
    let name = scenario.name();
    let mut run = build();

    let before = run.session.graph();
    let before_env = environment(&mut run.session);

    // The units whose own text changed, which re-run whatever their environment
    // says: a module edit changes no unit's text at all.
    let seeds: BTreeSet<usize> = if scenario.is_module() {
        BTreeSet::new()
    } else {
        BTreeSet::from([scenario.position.unit()])
    };

    run.recorder.clear();
    let redone: Vec<(usize, ScriptCompilationResult)> = if scenario.is_module() {
        run.session.relower_module(&scenario.module())
    } else {
        run.session.edit(scenario.position.unit(), scenario.text);
        run.session.relower(scenario.position.unit())
    };
    let analysis = typecheck_keys(&run.recorder.take());

    let after = run.session.graph();
    let union = before.union(&after);

    // --- The execution reach ------------------------------------------------

    let expected_reach = if scenario.is_module() {
        union.reach_from(&after.module_importers(&scenario.module().path()))
    } else {
        union.reach_from(&seeds)
    };
    let reached: BTreeSet<usize> = redone.iter().map(|(unit, _)| *unit).collect();
    assert_eq!(reached, expected_reach, "{name}: the units re-lowered");

    let failed: BTreeSet<usize> = redone.iter()
        .filter(|(_, result)| result.ir_unit.is_none())
        .map(|(unit, _)| *unit)
        .collect();
    assert_eq!(
        !failed.is_empty(), scenario.breaks,
        "{name}: units that stopped compiling: {failed:?}",
    );

    let mut ran = BTreeSet::new();
    for (unit, result) in &redone {
        if let Some(ir_unit) = &result.ir_unit {
            let (_, output) = run.session.executor.reexecute_unit(*unit, ir_unit);
            assert!(!output.starts_with("Error:"), "{name}: re-running unit {unit}: {output}");
            ran.insert(*unit);
        }
    }
    assert_eq!(
        run.session.units_run(),
        ran,
        "{name}: every unit that re-lowered ran again, and only those",
    );
    assert_eq!(
        ran,
        expected_reach.difference(&failed).copied().collect::<BTreeSet<usize>>(),
        "{name}: the reach less what stopped compiling",
    );

    // --- What the edit did not reach ---------------------------------------

    let after_env = environment(&mut run.session);
    for (bound, value) in &before_env {
        let provider = provider(&union, bound)
            .unwrap_or_else(|| panic!("{name}: nothing provides {bound}"));
        if reached.contains(&provider) {
            continue;
        }
        assert_eq!(
            after_env.get(bound), Some(value),
            "{name}: {bound} comes from unit {provider}, which the edit did not reach",
        );
    }

    // --- The analysis reach -------------------------------------------------

    if scenario.is_module() {
        // **The known over-propagation.** `ScriptEnv` is interned over every
        // module's parse and spans, so a module edit gives every unit's
        // typecheck a new key -- not a stale answer, a different question --
        // and every unit is checked again the first time anything asks.
        assert_eq!(
            analysis.len(), UNITS.len(),
            "{name}: a module edit re-keys every unit's typecheck",
        );
        assert!(
            analysis.iter().all(|key| !run.cold_keys.contains(key)),
            "{name}: the keys moved rather than the answers, so none is a cold key",
        );
    } else {
        let reached_analysis: BTreeSet<usize> = analysis.iter()
            .map(|ran| {
                run.cold_keys.iter().position(|key| key == ran).unwrap_or_else(|| panic!(
                    "{name}: a typecheck of something that is not one of the units",
                ))
            })
            .collect();
        assert_eq!(
            reached_analysis,
            analysis_reach(&before, &after, &seeds),
            "{name}: the units typechecked again are the ones a name they asked \
             about now answers differently for",
        );
        assert!(
            reached_analysis.is_subset(&reached),
            "{name}: analysis cannot reach further than lowering, which re-derives \
             everything analysis had to look at",
        );
    }
}

/// The matrix distinguishes the two reaches rather than measuring one twice.
///
/// If `analysis_reach` and the execution reach agreed everywhere, every row
/// above would pass with one of them computed wrongly. So this says they
/// disagree where they are supposed to: a value-only edit is typechecked in one
/// unit and executed in three.
#[test]
fn a_value_only_edit_is_analyzed_narrowly_and_executed_widely() {
    let mut run = build();
    let before = run.session.graph();

    run.recorder.clear();
    run.session.edit(
        0, "require module local/test/m0\nimport m0.f0\nlet a = f0(7)\ndebuglog \"ran A\"\n");
    let reached = run.session.rederive(0);
    let analysis = typecheck_keys(&run.recorder.take());
    let after = run.session.graph();

    assert_eq!(
        analysis_reach(&before, &after, &BTreeSet::from([0])),
        BTreeSet::from([0]),
        "`a` keeps its type, so no later unit's `binding_at` answer moved",
    );
    assert_eq!(analysis.len(), 1, "and salsa ran one unit's typecheck");
    assert_eq!(
        reached,
        BTreeSet::from([0, 1, 3]),
        "but `b` is `a + 1` and `d` is `c + b`, so both hold stale values",
    );
    assert_eq!(run.session.binding("a"), "7");
    assert_eq!(run.session.binding("b"), "8");
    assert_eq!(run.session.binding("d"), "10", "2 from `c` and 8 from `b`");
}

// ============================================================================
// A longer session
// ============================================================================

/// Twelve units with a graph nothing about four units can exercise.
///
/// A diamond (`i` and `j` both from `h`, `k` from both), a unit drawing on
/// several at once, a chain of four, and four units that depend on nothing.
/// The point is scale: a reach that is right for three units and wrong at ten
/// -- an off-by-one in the walk, a fold that stops early -- has nowhere to hide
/// here.
const LONG: [&str; 12] = [
    // 0: a root.
    "let a = 1\ndebuglog \"ran A\"\n",
    // 1: a chain off it.
    "let b = a + 1\ndebuglog \"ran B\"\n",
    // 2: the chain continues, so 0 reaches 3 only through 1 and 2.
    "let c = b + 1\ndebuglog \"ran C\"\n",
    // 3: and again -- four deep.
    "let d = c + 1\ndebuglog \"ran D\"\n",
    // 4: a second root, nothing to do with the first.
    "let e = 100\ndebuglog \"ran E\"\n",
    // 5: draws on two units at once.
    "let f = b + e\ndebuglog \"ran F\"\n",
    // 6: a function, which is a provided name of the other kind.
    "fun g(n: int): int\n    ret n * 2\nend fun\ndebuglog \"ran G\"\n",
    // 7: the head of the diamond.
    "let h = g(e)\ndebuglog \"ran H\"\n",
    // 8: one side of it.
    "let i = h + 1\ndebuglog \"ran I\"\n",
    // 9: the other side.
    "let j = h + 2\ndebuglog \"ran J\"\n",
    // 10: the foot, reached from 7 by two routes at once.
    "let k = i + j\ndebuglog \"ran K\"\n",
    // 11: self-contained, and at the end, so it is the control for everything.
    "let l = 999\ndebuglog \"ran L\"\n",
];

fn long_session() -> Session {
    let mut session = Session::new();
    for unit in LONG {
        session.append(unit);
    }
    assert_eq!(session.units_run(), (0..LONG.len()).collect(), "every unit ran once");
    session
}

/// The fixture really has the shape the cases below are about.
#[test]
fn the_long_session_has_the_graph_it_claims() {
    let mut session = long_session();
    let graph = session.graph();

    assert_eq!(graph.provides.len(), 12);
    assert_eq!(
        graph.reach(0),
        BTreeSet::from([0, 1, 2, 3, 5]),
        "the chain of four and the unit that draws on `b`",
    );
    assert_eq!(
        graph.reach(7),
        BTreeSet::from([7, 8, 9, 10]),
        "the diamond, whose foot is reached by two routes",
    );
    assert_eq!(
        graph.reach(4),
        BTreeSet::from([4, 5, 7, 8, 9, 10]),
        "`e` feeds the unit that draws on two and the whole diamond",
    );
    assert_eq!(graph.reach(11), BTreeSet::from([11]), "nothing follows the last unit");
}

/// An edit at the head of a diamond reaches its foot once, through both sides.
#[test]
fn an_edit_at_the_head_of_a_diamond_reaches_its_foot() {
    let mut session = long_session();
    let expected = session.graph().reach(7);
    assert_eq!(session.binding("k"), "403", "201 and 202");

    session.edit(7, "let h = g(e) + 1000\ndebuglog \"ran H\"\n");
    assert_eq!(session.rederive(7), expected);
    assert_eq!(session.units_run(), expected);

    assert_eq!(session.binding("i"), "1201");
    assert_eq!(session.binding("j"), "1202");
    assert_eq!(session.binding("k"), "2403", "the foot was run once, after both sides");
    assert_eq!(session.binding("d"), "4", "the chain is nothing to do with the diamond");
    assert_eq!(session.binding("l"), "999");
}

/// An edit at a root reaches four units down a chain and stops.
///
/// A one-step reach would leave `d` stale, and a reach that took every later
/// unit would drag the diamond in. Twelve units is enough for the two mistakes
/// to give different answers.
#[test]
fn an_edit_at_a_root_reaches_down_a_chain_of_four() {
    let mut session = long_session();
    let expected = session.graph().reach(0);
    assert_eq!(session.binding("d"), "4");

    session.edit(0, "let a = 10\ndebuglog \"ran A\"\n");
    assert_eq!(session.rederive(0), expected);
    assert_eq!(session.units_run(), expected);

    assert_eq!(session.binding("b"), "11");
    assert_eq!(session.binding("c"), "12");
    assert_eq!(session.binding("d"), "13", "the far end of the chain");
    assert_eq!(session.binding("f"), "111", "the unit drawing on `b` and `e`");
    assert_eq!(session.binding("k"), "403", "the diamond never heard about it");
}

/// Editing a function reaches its callers and no further.
///
/// A provided name of the other kind, in a session long enough that "the units
/// after it" and "the units that call it" are different answers.
#[test]
fn editing_a_function_reaches_its_callers() {
    let mut session = long_session();
    let expected = session.graph().reach(6);
    assert_eq!(
        expected,
        BTreeSet::from([6, 7, 8, 9, 10]),
        "`g` is called by unit 7, and the diamond hangs off that",
    );

    session.edit(6, "fun g(n: int): int\n    ret n * 3\nend fun\ndebuglog \"ran G\"\n");
    assert_eq!(session.rederive(6), expected);
    assert_eq!(session.units_run(), expected);

    assert_eq!(session.binding("h"), "300");
    assert_eq!(session.binding("k"), "603");
    assert_eq!(session.binding("f"), "102", "unit 5 does not call `g`");
}

// ============================================================================
// Removing a unit
// ============================================================================

/// **A unit cannot be removed, and blanking it is what removal has to be.**
///
/// `botdocs/plan-script-reactivity.md` names this as untested and probably
/// wrong. What it turns out to be is *inexpressible*: the frame store is
/// indexed by unit and every script value is a `(unit_index, ValueId)` pair, so
/// taking unit 1 out of a five-unit session would leave every later unit's IR
/// pointing one frame too far along. Nothing in `ScriptSession`,
/// `ScriptCompiler`, `FrameStore` or `UnitFunctionRegistry` offers a removal,
/// and that is the numbering protecting itself rather than an omission.
///
/// What is expressible is emptying a unit, and it behaves: the unit keeps its
/// index and its frame, what it used to export stops being on offer, and the
/// units that read it are told. This is what a REPL's "delete that line" has to
/// be built out of until the values are addressed by something other than a
/// position.
#[test]
fn blanking_a_unit_is_the_removal_the_numbering_allows() {
    let mut session = Session::new();
    session.append("let x = 1\ndebuglog \"ran A\"\n");
    session.append("let y = 2\ndebuglog \"ran B\"\n");
    session.append("let z = x + y\ndebuglog \"ran C\"\n");
    let _ = session.units_run();
    assert_eq!(session.binding("z"), "3");

    let before = session.graph();
    session.edit(0, "debuglog \"ran A\"\n");
    let after = session.graph();
    let expected = before.union(&after).reach_from(&BTreeSet::from([0]));
    assert_eq!(expected, BTreeSet::from([0, 2]), "the blanked unit and the unit that read it");

    let redone = session.relower(0);
    assert_eq!(
        redone.iter().map(|(unit, _)| *unit).collect::<BTreeSet<usize>>(),
        expected,
    );
    let broken: Vec<usize> = redone.iter()
        .filter(|(_, result)| result.ir_unit.is_none())
        .map(|(unit, _)| *unit)
        .collect();
    assert_eq!(broken, vec![2], "`x` is gone, so the unit that read it stops compiling");

    for (unit, result) in &redone {
        if let Some(ir_unit) = &result.ir_unit {
            session.executor.reexecute_unit(*unit, ir_unit);
        }
    }
    assert!(!session.has_binding("x"), "the blanked unit exports nothing now");
    assert_eq!(session.binding("y"), "2", "the unit between them is untouched");
    assert_eq!(
        session.unit_count(), 3,
        "the index stays occupied: a hole in the numbering is the thing that \
         cannot happen",
    );
}

/// A unit appended after a blanking lands at the next index, not the blank one.
///
/// The cost of the numbering, stated: a blanked unit's slot is not reused, so a
/// session that deletes lines grows a frame per deletion. They are destroyed
/// when the session ends, so nothing leaks, but the index is spent.
#[test]
fn a_blanked_units_index_is_not_reused() {
    let mut session = Session::new();
    session.append("let p = 1\ndebuglog \"ran A\"\n");
    session.append("let q = 2\ndebuglog \"ran B\"\n");
    let _ = session.units_run();

    session.edit(0, "debuglog \"ran A\"\n");
    session.rederive(0);
    let _ = session.units_run();

    session.append("let r = q + 5\ndebuglog \"ran C\"\n");
    assert_eq!(session.unit_count(), 3, "the new unit is the third, not the first again");
    assert_eq!(session.binding("r"), "7");
    assert!(!session.has_binding("p"));
}

// ============================================================================
// Ownership across a re-execution
// ============================================================================

/// A unit **copies** out of an earlier unit's binding rather than moving from
/// it, and that survives the earlier unit being run again.
///
/// This is the property the whole design rests on -- re-running B cannot
/// invalidate what C took from it, because C took a copy -- and nothing put it
/// through an edit before. A list is the test because it owns heap memory: if
/// the copy were a move, re-executing the earlier unit would destroy what the
/// later unit is holding, and the leak checker would have something to say
/// about the double free at the end of the session.
///
/// Both directions are here: the copy is still good after its source is
/// re-executed, and re-executing it again with a different value gives the
/// later unit the new one.
#[test]
fn a_copy_out_of_an_earlier_unit_survives_its_re_execution() {
    let mut session = Session::new();
    session.append("let xs = [1, 2, 3]\ndebuglog \"ran A\"\n");
    session.append("let ys = xs\ndebuglog \"ran B\"\n");
    session.append("let zs = [9]\ndebuglog \"ran C\"\n");
    let _ = session.units_run();
    assert_eq!(session.binding("ys"), "[1, 2, 3]");

    let expected = session.graph().reach(0);
    assert_eq!(expected, BTreeSet::from([0, 1]), "`zs` has nothing to do with `xs`");

    session.edit(0, "let xs = [4, 5, 6]\ndebuglog \"ran A\"\n");
    assert_eq!(session.rederive(0), expected);
    assert_eq!(session.units_run(), expected);

    assert_eq!(session.binding("xs"), "[4, 5, 6]");
    assert_eq!(session.binding("ys"), "[4, 5, 6]", "the copy was taken again");
    assert_eq!(session.binding("zs"), "[9]");
}

/// A binding a unit **gave away** is still given away after the unit is
/// re-executed, and a unit compiled afterwards is still told so.
///
/// A unit may only move out of a binding it defined itself -- a use that
/// resolves into an earlier unit is lowered as a `Clone`, which is the previous
/// test -- and then the name outlives its value. `dead_exports` records that
/// and `dead_externals_over` folds the records to decide what a new unit may
/// name, so **a move has to survive a re-derivation**: if the record went
/// missing, a line typed after the edit would read a value that was given away.
///
/// D013 is the observable, and it is the only one: the frame still holds the
/// list, so reading the given-away name through the executor prints it. What is
/// gone is the *right* to name it, which is a compile-time fact. Asserted
/// before the edit and again after, because the fold is recomputed from the
/// records each time and the edited unit's record is the one that was replaced.
#[test]
fn a_move_across_units_survives_an_edit() {
    let mut session = Session::new();
    session.append("let xs = [1, 2, 3]\nlet moved = xs\ndebuglog \"ran A\"\n");
    session.append("let n = 5\ndebuglog \"ran B\"\n");
    session.append("let copied = moved\ndebuglog \"ran C\"\n");
    let _ = session.units_run();
    assert_eq!(session.binding("moved"), "[1, 2, 3]");
    assert_eq!(session.binding("copied"), "[1, 2, 3]", "unit 2 took a copy");
    assert!(gave_away(&mut session, "xs"), "unit 0 gave `xs` away to `moved`");

    let expected = session.graph().reach(0);
    assert_eq!(expected, BTreeSet::from([0, 2]), "the unit that copied it, and not `n`");

    session.edit(0, "let xs = [7, 8]\nlet moved = xs\ndebuglog \"ran A\"\n");
    assert_eq!(session.rederive(0), expected);
    assert_eq!(session.units_run(), expected);

    assert_eq!(session.binding("moved"), "[7, 8]", "the move ran again over the new list");
    assert_eq!(session.binding("copied"), "[7, 8]", "the copy was taken again");
    assert_eq!(session.binding("n"), "5");
    assert!(
        gave_away(&mut session, "xs"),
        "the record of the move survived the re-derivation",
    );
}

/// Whether a unit compiled now would be refused for naming `name`.
///
/// The unit is not kept: one that fails to compile comes back off the script,
/// so this asks the question without changing the session.
fn gave_away(session: &mut Session, name: &str) -> bool {
    let result = session.compile_append(&format!("let probe = {name}\n"));
    format!("{:?}", result.ownership).contains("D013")
}
