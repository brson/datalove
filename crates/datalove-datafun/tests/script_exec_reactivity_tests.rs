//! How far an edit to one script unit travels through lowering and execution.
//!
//! The rule `script_reactivity_tests` holds for *analysis* -- editing a unit
//! reaches that unit and the units that use what it provides, and nothing else
//! -- asked here of the other half of the pipeline. For units A B C D where C
//! uses nothing B provides, editing B re-lowers and re-executes B and D. A is
//! upstream. **C keeps its previous lowering and its previous frame**, which is
//! the whole of stage C in `botdocs/plan-script-reactivity.md`.
//!
//! **Why leaving C's IR alone is sound**, and it is one property: a script
//! value is identified by `(unit_index, ValueId)`, so a unit's IR names the
//! units it reads from, and a unit that uses nothing of B's holds no `(B, *)`
//! reference at all. `c_holds_no_reference_to_b` checks that on the IR itself
//! rather than taking it on trust, and it does so by walking the serialized
//! unit, so an instruction form nobody thought of here is still covered.
//!
//! **Execution has to cascade where analysis does not.** `let b = 2` becoming
//! `let b = 3` moves no type, so stage B re-typechecks B alone -- correctly,
//! there is nothing else to check. But `d` is `b + 5` and its value is now
//! wrong, so D must run again. That case is the one that distinguishes this
//! suite from `script_reactivity_tests`.
//!
//! **What each half is measured by.** Execution is measured from outside: every
//! unit ends in a `debuglog`, so the debug buffer says which units ran, and
//! `debuglog` is the only effect a `fun` can have. Lowering has no per-unit
//! salsa query to record -- phases 2 to 5 are plain functions -- so the
//! measurement is which units `relower_reach` compiled, against an expectation
//! the test derives from `asked_names` independently of how the compiler
//! derives it. `a_unit_that_does_use_b_is_reached` is what says that
//! measurement is not vacuous: change the graph and the answer changes with it.

use rmx::prelude::*;
use rmx::std::collections::BTreeSet;

use datalove_datafun as datafun;

mod scriptsession;
use scriptsession::{mark, Module, Session};

/// A B C D, where C uses nothing B provides and D uses `b`.
///
/// The same four units the plan measured, each ending in a `debuglog` so that
/// running it leaves a mark. No module is required, so this is the script layer
/// on its own.
const UNITS: [&str; 4] = [
    "let a = 1\ndebuglog \"ran A\"\n",
    "let b = 2\ndebuglog \"ran B\"\n",
    "let c = 30\ndebuglog \"ran C\"\n",
    "let d = b + 5\ndebuglog \"ran D\"\n",
];

/// A script built from `UNITS` and run through, with the marks cleared.
fn four_units() -> Session {
    let mut session = Session::new();
    for unit in UNITS {
        session.append(unit);
    }
    assert_eq!(
        session.units_run(),
        BTreeSet::from([0, 1, 2, 3]),
        "every unit runs once as it is appended",
    );
    session
}

/// The fixture really is the shape the claim is about.
///
/// Every test below compares a measured reach against `reach`, so if this were
/// wrong they would all agree with each other and with nothing real.
#[test]
fn c_uses_nothing_b_provides_and_d_uses_b() {
    let mut session = four_units();
    let graph = session.graph();

    assert_eq!(graph.provides, vec![vec!["a"], vec!["b"], vec!["c"], vec!["d"]]);
    assert_eq!(
        graph.reach(1),
        BTreeSet::from([1, 3]),
        "B and D depend on B; A is upstream and C uses {:?}", graph.uses[2],
    );
}

// ============================================================================
// The property that makes not re-lowering C sound
// ============================================================================

/// **C's IR holds no reference to unit B at all.**
///
/// This is what the whole stage rests on: B's values being rebuilt cannot reach
/// a unit that never named one. It is checked by walking the serialized code
/// unit for every `ExternalValue`, `ExternalSlot` and `External` -- the three
/// forms that carry a unit index -- rather than by matching on the instruction
/// set, so a form added later is covered without this test being revisited.
#[test]
fn c_holds_no_reference_to_b() {
    let mut session = Session::new();
    let mut units = Vec::new();
    for text in UNITS {
        let ir_unit = session.compile_append(text).ir_unit.expect("the fixture compiles");
        session.executor.execute_fragment(&ir_unit);
        units.push(ir_unit);
    }

    assert_eq!(
        external_units(&units[2]),
        BTreeSet::new(),
        "C is self-contained, so it refers to no earlier unit",
    );
    assert_eq!(
        external_units(&units[3]),
        BTreeSet::from([1]),
        "D reads `b`, so it refers to unit 1 and to nothing else",
    );
}

/// Every unit index an IR unit refers to.
fn external_units(unit: &datalove_datafun_ir::IrCodeUnit) -> BTreeSet<u32> {
    let json = rmx::serde_json::to_value(unit).expect("an IR unit serializes");
    let mut found = BTreeSet::new();
    collect_external_units(&json, &mut found);
    found
}

/// Walk `value` for the externally tagged forms that carry a unit index.
///
/// `Operand::ExternalValue`, `Operand::ExternalSlot`, `SlotDest::External` and
/// `CodeRef::External` all serialize as a one-key object whose payload holds a
/// `unit` field, and nothing else in the IR spells `unit` that way.
fn collect_external_units(value: &rmx::serde_json::Value, found: &mut BTreeSet<u32>) {
    match value {
        rmx::serde_json::Value::Object(fields) => {
            for (key, payload) in fields {
                if matches!(key.as_str(), "ExternalValue" | "ExternalSlot" | "External") {
                    let unit = payload.get("unit")
                        .and_then(|unit| unit.as_u64())
                        .expect("an external reference names the unit it is into");
                    found.insert(unit as u32);
                }
                collect_external_units(payload, found);
            }
        }
        rmx::serde_json::Value::Array(items) => {
            for item in items {
                collect_external_units(item, found);
            }
        }
        _ => {}
    }
}

// ============================================================================
// The measurement the plan is held to
// ============================================================================

/// **Editing B re-lowers and re-executes B and D. C does neither.**
///
/// `let b` becomes `var b`, which changes what B provides -- the binding is
/// mutable now -- without changing whether anything in the script is legal.
#[test]
fn editing_b_rederives_b_and_d_and_not_c() {
    let mut session = four_units();
    let expected = session.graph().reach(1);

    session.edit(1, "var b = 2\ndebuglog \"ran B\"\n");
    let after = session.graph();
    assert_eq!(after.reach(1), expected, "the edit leaves the graph the same shape");

    let relowered = session.rederive(1);
    let reran = session.units_run();

    assert_eq!(relowered, expected, "the units re-lowered are the units that use B's");
    assert_eq!(reran, expected, "and they are the units that ran again");
    assert!(relowered.contains(&3), "D uses `b`, so the edit has to reach it");
    assert!(!relowered.contains(&2), "C uses nothing B provides, so it is left alone");
    assert!(!relowered.contains(&0), "A is upstream of the edit");
}

/// **A value-only edit re-executes B and D, where analysis reached only B.**
///
/// `let b = 2` becoming `let b = 3` leaves B providing the same binding of the
/// same type, so nothing downstream's types moved and stage B re-typechecks B
/// alone. `d` is `b + 5` though, so its value is stale until D runs again.
/// **This is the case that distinguishes lowering and execution reactivity from
/// analysis reactivity**, and the assertion on `d` is what makes it one: a
/// suite that only counted re-runs would pass with `d` left at 7.
#[test]
fn a_value_only_edit_reexecutes_b_and_d() {
    let mut session = four_units();
    let expected = session.graph().reach(1);
    assert_eq!(session.binding("d"), "7", "`d` is `b + 5` with `b` at 2");

    session.edit(1, "let b = 3\ndebuglog \"ran B\"\n");
    let relowered = session.rederive(1);

    assert_eq!(relowered, expected);
    assert_eq!(session.units_run(), expected, "B and D run again; A and C do not");
    assert_eq!(session.binding("b"), "3");
    assert_eq!(session.binding("d"), "8", "D was re-executed against the new `b`");
}

/// C's bindings still work after B is re-executed around them.
///
/// The values, through an executor, rather than only that lowering got through:
/// C's frame is the one thing re-executing B must not disturb, and `c` reading
/// 30 afterwards is what says so. A unit appended after the edit reads `c` as
/// well, so the binding is good for new code and not just for a lookup.
#[test]
fn c_keeps_its_frame_when_b_is_reexecuted() {
    let mut session = four_units();
    assert_eq!(session.binding("c"), "30");

    session.edit(1, "let b = 3\ndebuglog \"ran B\"\n");
    session.rederive(1);
    let _ = session.units_run();

    assert_eq!(session.binding("c"), "30", "C's frame is untouched");
    assert_eq!(session.binding("a"), "1", "and so is A's");

    session.append("let e = c + d\ndebuglog \"ran E\"\n");
    assert_eq!(session.binding("e"), "38", "30 from C's old frame and 8 from D's new one");
}

/// A unit that does use `b` **is** reached, which is what says the measurement
/// is not vacuous.
///
/// Every other test here asserts that the reach is `{1, 3}`, and a measurement
/// stuck on that answer would pass them all. So this is the same script with
/// one dependency added -- C reads `b` -- and the reach has to become `{1, 2,
/// 3}`. It fails if `relower_reach` ignores the graph, if `Graph::reach`
/// ignores it, or if either is hard-wired to the four-unit answer.
///
/// It also covers the transitive step. With C at `b + 30` and D at `c + 5`,
/// D reaches B only through C, so a one-step walk would report `{1, 2}` and
/// leave `d` stale.
#[test]
fn a_unit_that_does_use_b_is_reached() {
    let mut session = Session::new();
    session.append(UNITS[0]);
    session.append(UNITS[1]);
    session.append("let c = b + 30\ndebuglog \"ran C\"\n");
    session.append("let d = c + 5\ndebuglog \"ran D\"\n");
    let _ = session.units_run();

    let graph = session.graph();
    assert_eq!(
        graph.reach(1),
        BTreeSet::from([1, 2, 3]),
        "C reads `b` and D reads `c`, so the edit reaches both",
    );

    session.edit(1, "let b = 3\ndebuglog \"ran B\"\n");
    assert_eq!(session.rederive(1), BTreeSet::from([1, 2, 3]));
    assert_eq!(session.units_run(), BTreeSet::from([1, 2, 3]));
    assert_eq!(session.binding("c"), "33");
    assert_eq!(session.binding("d"), "38", "D is reached through C, not directly");
}

/// Editing the last unit reaches only itself.
///
/// The other end of the same rule: nothing sits after D to be told.
#[test]
fn editing_the_last_unit_reaches_only_itself() {
    let mut session = four_units();

    session.edit(3, "let d = b + 6\ndebuglog \"ran D\"\n");
    assert_eq!(session.rederive(3), BTreeSet::from([3]));
    assert_eq!(session.units_run(), BTreeSet::from([3]));
    assert_eq!(session.binding("d"), "8");
}

/// Appending a unit costs one unit's lowering and one unit's execution.
///
/// The property a REPL grows by, and the one a re-derivation must not have cost
/// anything: appending goes through `compile_fragment`, which touches the
/// records of the units before it and rewrites none of them.
#[test]
fn appending_lowers_and_runs_one_unit() {
    let mut session = four_units();

    session.append("let e = c + 1\ndebuglog \"ran E\"\n");

    let buffer = session.executor.get_debug_buffer();
    session.executor.clear_debug_buffer();
    assert!(buffer.contains(&mark(4)), "the appended unit ran: {buffer:?}");
    for index in 0..UNITS.len() {
        assert!(
            !buffer.contains(&mark(index)),
            "appending re-ran unit {index}: {buffer:?}",
        );
    }
    assert_eq!(session.binding("e"), "31");
}

/// An edit reaches an expression unit in the history as readily as a fragment.
///
/// A session's history holds both kinds, and an expression unit computes a
/// value that needs somewhere to land: running one again without a destination
/// for it panics in the interpreter, where `UnitEnd` carries a result. So this
/// is not only about the reach.
///
/// Both shapes of expression unit are here. `b + 10` computes a value, and a
/// bare `b` does not -- it names a binding, which lowering records rather than
/// computing -- and the two come back through different branches.
#[test]
fn an_edit_reaches_an_expression_unit() {
    let mut session = Session::new();
    session.append(UNITS[1]);
    let values = session.append_expr("b + 10");
    assert_eq!(values, (Some("int".S()), "12".S()));
    let named = session.append_expr("b");
    assert_eq!(named, (Some("int".S()), "2".S()));

    session.edit(0, "let b = 5\ndebuglog \"ran B\"\n");
    let redone = session.relower(0);

    let mut values = Vec::new();
    for (index, result) in &redone {
        let ir_unit = result.ir_unit.as_ref().expect("every unit re-lowers");
        values.push((*index, session.executor.reexecute_unit(*index, ir_unit)));
    }

    assert_eq!(
        values.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "both expression units read `b`",
    );
    assert_eq!(values[1].1, (Some("int".S()), "15".S()), "`b + 10` recomputed");
    assert_eq!(values[2].1, (Some("int".S()), "5".S()), "a bare `b` reads the new binding");
}

// ============================================================================
// Failure and re-derivation
// ============================================================================

/// An edit that breaks a later unit is reported against that unit, and the
/// units it does not reach keep working.
///
/// `b` becoming a string makes `b + 5` an error, which is filed against D
/// because D is where the edit reaches. C is untouched, so `c` still reads.
#[test]
fn an_edit_that_breaks_a_later_unit_leaves_the_rest_alone() {
    let mut session = four_units();

    session.edit(1, "let b = \"two\"\ndebuglog \"ran B\"\n");

    let redone = session.relower(1);

    let reached: BTreeSet<usize> = redone.iter().map(|(index, _)| *index).collect();
    assert_eq!(reached, BTreeSet::from([1, 3]));

    let failed: BTreeSet<usize> = redone.iter()
        .filter(|(_, result)| result.ir_unit.is_none())
        .map(|(index, _)| *index)
        .collect();
    assert_eq!(failed, BTreeSet::from([3]), "`\"two\" + 5` is D's error to report");

    // B re-lowered and can be re-executed; D did not, so its frame stays as it
    // was and its binding drops out of the environment.
    for (index, result) in &redone {
        if let Some(ir_unit) = &result.ir_unit {
            session.executor.reexecute_unit(*index, ir_unit);
        }
    }
    assert_eq!(session.binding("c"), "30", "C never heard about any of this");
}

// ============================================================================
// Nothing leaks
// ============================================================================

/// Re-executing a unit whose bindings own heap memory does not leak them.
///
/// A list binding is one allocation per unit, and the old frame's copy has to be
/// destroyed as the new frame takes its place. Nothing here asserts that
/// directly: the runtime's leak checker does, when the interpreter's runtime is
/// shut down as the executor drops, and it panics on a non-empty allocation
/// table. Dropping the frame without destroying what it holds leaves two
/// allocations behind per re-execution, which is what this catches.
///
/// It only checks anything with the leak checker on, which the runtime leaves
/// off unless asked: `DATALOVE_LEAK_CHECK=panic`, which the justfile sets on
/// every recipe that runs tests. Without it this passes vacuously. See "Leak
/// Checking" in `botdocs/compiler-guide.md`.
///
/// The other tests here happen to cover the same ground, since a `debuglog` of
/// a string literal allocates too. This one is about nothing else, and says so
/// where a reader would otherwise have to work it out.
#[test]
fn reexecuting_a_unit_does_not_leak_its_bindings() {
    let mut session = Session::new();
    session.append("let xs = [1, 2, 3]\n");
    session.append("let ys = [4, 5]\n");
    session.append("let zs = xs\n");
    assert_eq!(session.binding("zs"), "[1, 2, 3]");

    // Several times over, so that a leak of one frame's worth is not lost in
    // the noise of a one-off.
    for value in 2..6 {
        session.edit(0, &format!("let xs = [{value}, 2, 3]\n"));
        let reached = session.rederive(0);
        assert_eq!(reached, BTreeSet::from([0, 2]), "`ys` has nothing to do with `xs`");
        assert_eq!(session.binding("zs"), format!("[{value}, 2, 3]"));
    }

    assert_eq!(session.binding("ys"), "[4, 5]", "the untouched unit's list is intact");
}

// ============================================================================
// Recording, for the record
// ============================================================================

/// Re-lowering the units an edit reaches re-typechecks exactly the same units
/// analysis would have.
///
/// A cross-check between the two halves rather than a new claim: the reach
/// walked here is the reach `script_reactivity_tests` measures salsa taking,
/// so re-deriving must not drag in a unit analysis would have spared. Recorded
/// with `QueryRecorder`, whose events are attributed to units by the order a
/// cold run typechecks them in.
#[test]
fn rederiving_typechecks_no_more_than_analysis_would() {
    let recorder = datalove_ct::query_events::QueryRecorder::new();
    let mut session = Session::with_db(datafun::Database::recording(&recorder));
    for unit in UNITS {
        session.append(unit);
    }
    let _ = session.units_run();

    let keys: Vec<salsa::Id> = recorder.take().iter()
        .filter(|event| event.query == "typecheck_script_unit")
        .map(|event| event.key)
        .collect();
    assert_eq!(keys.len(), UNITS.len(), "a cold run typechecks each unit once");

    let expected = session.graph().reach(1);
    recorder.clear();

    session.edit(1, "var b = 2\ndebuglog \"ran B\"\n");
    session.rederive(1);

    let reached: BTreeSet<usize> = recorder.take().iter()
        .filter(|event| event.query == "typecheck_script_unit")
        .map(|event| {
            keys.iter().position(|key| *key == event.key).unwrap_or_else(|| {
                panic!("a typecheck of something that is not one of the units: {:?}", event.key)
            })
        })
        .collect();
    assert_eq!(reached, expected, "re-deriving typechecks the units the edit reaches");
}

// ============================================================================
// The alias edge, which does not come from `asked_names`
// ============================================================================

/// Editing a `require` line reaches a later unit's `import` of that alias.
///
/// **This edge is the one place the graph is not derived from a read.** Stage A
/// records the names asked of `TypeContext`'s lookups, and an alias does not go
/// through them -- it resolves through `module_alias_at`, which walks the units'
/// `module_aliases`. So the edge is added to the graph by hand, from a
/// `require`'s provides and an `import`'s uses, and a test is the only thing
/// holding it: nothing about the way it is built makes it right.
///
/// The edit points the `require` at a different module. An alias is the last
/// component of the path and there is no `as` renaming, so this unbinds `utils`
/// and the import has to be told.
///
/// The third unit is the control. It reaches nothing of unit 0's, so a reach
/// that simply took every later unit would be caught here.
#[test]
fn editing_a_require_reaches_a_later_import_and_nothing_else() {
    let mut session = Session::with_modules(&[
        Module {
            library: "local", package: "test", module: "utils",
            source: "fun ident(x: u32): u32\n    ret x\nend fun\n",
        },
        Module {
            library: "local", package: "test", module: "other",
            source: "fun ident(x: u32): u32\n    ret x\nend fun\n",
        },
    ]);

    session.append("require module local/test/utils\n");
    session.append("import utils.ident\nlet v = ident(7)\ndebuglog v\n");
    session.append("let unrelated = 100\ndebuglog unrelated\n");
    assert_eq!(session.binding("v"), "7");

    session.edit(0, "require module local/test/other\n");

    // `relower` rather than `rederive`, because the unit this reaches is
    // supposed to stop compiling and `rederive` asserts that everything it
    // touches lowers.
    let redone = session.relower(0);
    let reached: BTreeSet<usize> = redone.iter().map(|(index, _)| *index).collect();
    let broken: Vec<usize> = redone.iter()
        .filter(|(_, result)| result.ir_unit.is_none())
        .map(|(index, _)| *index)
        .collect();

    assert_eq!(
        reached,
        BTreeSet::from([0, 1]),
        "the require and the import that names its alias, and not the third unit",
    );
    assert_eq!(
        broken, vec![1],
        "unit 1 imports an alias that no longer exists, so it must stop compiling",
    );
}

// ============================================================================
// Modules, which the reach now covers
// ============================================================================

/// **Editing a module reaches the script units that import from it.**
///
/// This test used to pin the opposite, which was the gap section E of
/// `botdocs/plan-script-reactivity.md` is about. `a` is `f(1)` under a module
/// whose `f` returns its argument; change `f` to return 42 and `a` reads 42.
///
/// **Why it needed a driver rather than a keying change.** Stage B made
/// typechecking lazy: an earlier unit re-runs when a later one asks
/// `binding_at` about a name it provides, and a module edit makes nobody ask.
/// So there was nothing wrong to fix -- there was nothing to say "a module
/// changed, go and re-derive what depended on it".
///
/// **A module body change moves no type**, and that is still true and still
/// correct: `ModuleSpec` holds the module's `ParsedStatements`, which compare
/// equal because a `StmtFun`'s identity is `(module_id, name, local_index)` and
/// its body rides a tracked field. It is execution that has to cascade where
/// analysis does not, which is the same distinction stage C exists for.
///
/// The second unit is the control: it uses nothing of the first unit's and
/// imports from no module, so it must be left alone.
#[test]
fn editing_a_module_reaches_the_units_that_import_from_it() {
    let module = |body: &'static str| Module {
        library: "local", package: "test", module: "m", source: body,
    };
    let mut session = Session::with_modules(&[
        module("fun f(x: u32): u32\n    ret x\nend fun\n"),
    ]);

    session.append(
        "require module local/test/m\nimport m.f\nlet a = f(1)\ndebuglog \"ran A\"\n");
    session.append("let b = 2\ndebuglog \"ran B\"\n");
    assert_eq!(session.binding("a"), "1");
    let _ = session.units_run();

    let graph = session.graph();
    assert_eq!(
        graph.imports,
        vec![vec!["local/test/m".S()], Vec::<String>::new()],
        "the first unit imports from the module and the second imports from none",
    );
    let expected = graph.module_reach("local/test/m");
    assert_eq!(expected, BTreeSet::from([0]), "only the importing unit");

    // A body change, so no signature and no type moves.
    let edited = module("fun f(x: u32): u32\n    ret 42\nend fun\n");
    assert_eq!(session.rederive_module(&edited), expected);
    assert_eq!(session.units_run(), expected, "the importing unit ran again");

    assert_eq!(session.binding("a"), "42", "the edit reached the unit that called `f`");
    assert_eq!(session.binding("b"), "2", "the control unit was left alone");
}

/// A module edit carries on to the units that read what the importing unit
/// computed.
///
/// The transitive half, and the reason the module reach is the name reach with
/// different seeds rather than a rule of its own: a unit that never heard of
/// the module is as stale as the one that imported from it if it read that
/// unit's binding. The fourth unit is the control.
#[test]
fn a_module_edit_reaches_the_dependents_of_an_importing_unit() {
    let module = |body: &'static str| Module {
        library: "local", package: "test", module: "m", source: body,
    };
    let mut session = Session::with_modules(&[
        module("fun f(x: int): int\n    ret x\nend fun\n"),
    ]);

    session.append("let a = 1\ndebuglog \"ran A\"\n");
    session.append(
        "require module local/test/m\nimport m.f\nlet b = f(2)\ndebuglog \"ran B\"\n");
    session.append("let c = b + 1\ndebuglog \"ran C\"\n");
    session.append("let d = a + 100\ndebuglog \"ran D\"\n");
    let _ = session.units_run();
    assert_eq!(session.binding("c"), "3");

    let graph = session.graph();
    let expected = graph.module_reach("local/test/m");
    assert_eq!(
        expected, BTreeSet::from([1, 2]),
        "the importing unit and the unit that reads `b`; A is upstream and D \
         reads only `a`",
    );

    let edited = module("fun f(x: int): int\n    ret x * 10\nend fun\n");
    assert_eq!(session.rederive_module(&edited), expected);
    assert_eq!(session.units_run(), expected);
    assert_eq!(session.binding("b"), "20");
    assert_eq!(session.binding("c"), "21", "C was reached through B, not directly");
    assert_eq!(session.binding("d"), "101", "D was not reached");
}

/// A module edit no unit imports from reaches nothing.
///
/// The other end of the rule, and what says the seeds are the imports rather
/// than "there was a module edit": a session with two modules, importing from
/// one of them, must not re-derive anything when the other changes.
#[test]
fn editing_an_unimported_module_reaches_nothing() {
    let mut session = Session::with_modules(&[
        Module {
            library: "local", package: "test", module: "used",
            source: "fun f(x: u32): u32\n    ret x\nend fun\n",
        },
        Module {
            library: "local", package: "test", module: "unused",
            source: "fun g(x: int): int\n    ret x\nend fun\n",
        },
    ]);

    session.append(
        "require module local/test/used\nimport used.f\nlet a = f(1)\ndebuglog \"ran A\"\n");
    let _ = session.units_run();

    let graph = session.graph();
    assert_eq!(graph.module_reach("local/test/unused"), BTreeSet::new());

    let edited = Module {
        library: "local", package: "test", module: "unused",
        source: "fun g(x: int): int\n    ret x + 1\nend fun\n",
    };
    assert_eq!(session.rederive_module(&edited), BTreeSet::new());
    assert_eq!(session.units_run(), BTreeSet::new(), "nothing ran again");
    assert_eq!(session.binding("a"), "1");
}

/// A module *signature* change reaches the importing unit through lowering as
/// well, and the unit that can no longer call it says so.
///
/// The execution reach is the same either way -- it is the imports that decide
/// it -- but a signature change is where the units that are *asked* re-typecheck,
/// so this is the case the driver had to be narrow for.
#[test]
fn a_module_signature_change_is_reported_against_the_importing_unit() {
    let mut session = Session::with_modules(&[
        Module {
            library: "local", package: "test", module: "m",
            source: "fun f(x: u32): u32\n    ret x\nend fun\n",
        },
    ]);

    session.append(
        "require module local/test/m\nimport m.f\nlet a = f(1)\ndebuglog \"ran A\"\n");
    session.append("let b = 2\ndebuglog \"ran B\"\n");
    let _ = session.units_run();

    // `f` now takes a string, so `f(1)` is no longer a call anything answers.
    let edited = Module {
        library: "local", package: "test", module: "m",
        source: "fun f(x: string): u32\n    ret 7\nend fun\n",
    };
    let redone = session.relower_module(&edited);

    let reached: BTreeSet<usize> = redone.iter().map(|(index, _)| *index).collect();
    assert_eq!(reached, BTreeSet::from([0]), "the importing unit and nothing else");
    let broken: Vec<usize> = redone.iter()
        .filter(|(_, result)| result.ir_unit.is_none())
        .map(|(index, _)| *index)
        .collect();
    assert_eq!(broken, vec![0], "`f(1)` no longer typechecks, and unit 0 is where it is");
    assert_eq!(session.binding("b"), "2", "the control unit is untouched");
}
