//! What each script unit provides, and what it uses.
//!
//! Editing a unit should re-analyze and re-execute the units that depend on it
//! and no others -- for A B C D with C independent of B, editing B reaches B and
//! D and leaves C alone even though it sits between them. That needs a
//! dependency graph over the units, and this is the half of it that did not
//! exist: what a unit *uses*.
//!
//! Provides was already there, as a unit's typecheck outputs. Uses is
//! `asked_names`, recorded by `TypeContext` as the typechecker resolves names,
//! rather than computed by a separate walk over the AST -- so it cannot miss a
//! form the walk forgot, which is the failure that would keep a stale type.
//!
//! **It over-approximates in one direction on purpose.** A name the unit binds
//! itself is recorded too, because not recording it would mean tracking which
//! binding each lookup found through every site that writes one, and
//! `statement.rs` writes pattern bindings straight into the map while scopes
//! save and restore the whole of it. A name with no earlier provider resolves to
//! no edge, so over-reporting costs nothing but a lookup.
//!
//! See `botdocs/plan-script-reactivity.md`.

use rmx::prelude::*;

use datalove_datafun as datafun;
use datalove_datafun::pipeline::{CompilerOptions, ModuleCompilationPipeline};

/// Compile `units` in order and report each one's provides and uses.
///
/// Uses are reported as the names asked for; provides as the variable and
/// function names bound. Both are sorted, so a case reads as written.
fn graph_of(units: &[&str]) -> Vec<(Vec<String>, Vec<String>)> {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new(CompilerOptions::default());
    let compiled = pipeline.compile_fresh(&db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    let mut compiler = compiled.script_compiler_default(&db).expect("a script compiler");

    for unit in units {
        let result = compiler.compile_fragment(unit);
        assert!(
            matches!(result.typecheck, datafun::pipeline::TypecheckResult::Success { .. }),
            "unit {unit:?} did not typecheck: {:?} / own {:?}", result.typecheck, result.ownership,
        );
    }

    let outputs = compiler.unit_typecheck_outputs();
    outputs.iter()
        .map(|output| {
            let mut provides: Vec<String> = output.new_vars(&db).iter()
                .map(|(name, _, _)| name.as_str(&db).S())
                .chain(output.new_fns(&db).iter().map(|(name, _)| name.as_str(&db).S()))
                .collect();
            provides.sort();
            let uses: Vec<String> = output.asked_names(&db).iter()
                .map(|name| name.as_str(&db).S())
                .collect();
            (provides, uses)
        })
        .collect()
}

/// Compile `units` in order and hand back the last one's result, errors and all.
fn last_result(units: &[&str]) -> datafun::pipeline::ScriptCompilationResult {
    let db = datafun::Database::default();
    let mut pipeline = ModuleCompilationPipeline::new(CompilerOptions::default());
    let compiled = pipeline.compile_fresh(&db);
    let mut compiler = compiled.script_compiler_default(&db).expect("a script compiler");
    let mut result = None;
    for unit in units {
        result = Some(compiler.compile_fragment(unit));
    }
    result.expect("at least one unit")
}

fn provides(graph: &[(Vec<String>, Vec<String>)], unit: usize) -> &[String] {
    &graph[unit].0
}

fn uses(graph: &[(Vec<String>, Vec<String>)], unit: usize) -> &[String] {
    &graph[unit].1
}

/// The case the whole plan is written around.
///
/// A B C D, where C does not depend on B. The graph has to say so: C must not
/// ask for anything B provides, or no amount of precise keying will spare it.
#[test]
fn the_unit_between_two_dependents_uses_neither() {
    let graph = graph_of(&[
        "let a = 1\n",
        "let b = 2\n",
        "let c = 30\n",
        "let d = b + 5\n",
    ]);

    assert_eq!(provides(&graph, 0), ["a"]);
    assert_eq!(provides(&graph, 1), ["b"]);
    assert_eq!(provides(&graph, 2), ["c"]);
    assert_eq!(provides(&graph, 3), ["d"]);

    assert!(!uses(&graph, 2).contains(&"b".S()),
        "C must not use B: it asked for {:?}", uses(&graph, 2));
    assert!(uses(&graph, 3).contains(&"b".S()),
        "D uses B: it asked for {:?}", uses(&graph, 3));
}

/// A unit that reads an earlier variable asks for it.
#[test]
fn reading_an_earlier_variable_is_a_use() {
    let graph = graph_of(&["let x = 7\n", "let y = x + 1\n"]);
    assert!(uses(&graph, 1).contains(&"x".S()), "{:?}", uses(&graph, 1));
}

/// A unit that calls an earlier function asks for it.
#[test]
fn calling_an_earlier_function_is_a_use() {
    let graph = graph_of(&[
        "fun double(n: int): int\n    ret n * 2\nend fun\n",
        "let v = double(4)\n",
    ]);
    assert_eq!(provides(&graph, 0), ["double"]);
    assert!(uses(&graph, 1).contains(&"double".S()), "{:?}", uses(&graph, 1));
}

/// A unit that reads nothing from the environment asks for nothing that resolves.
///
/// It may still ask about its own names -- see the header -- so what is asserted
/// is that it does not reach the earlier unit.
#[test]
fn a_self_contained_unit_reaches_no_earlier_unit() {
    let graph = graph_of(&["let earlier = 1\n", "let later = 2\n"]);
    assert!(!uses(&graph, 1).contains(&"earlier".S()),
        "the second unit reached the first: {:?}", uses(&graph, 1));
}

/// A use inside a function body counts, not just at the top level.
///
/// The recording sits at the lookup rather than in a walk over statements, so
/// nesting costs nothing to support -- which is the reason for doing it that way.
///
/// A const, because that is the one kind of enclosing binding a body may name.
/// `script_const_tests` has what happens when one tries to *lower* that.
#[test]
fn a_use_inside_a_function_body_counts() {
    let graph = graph_of(&[
        "const SCALE = 3\n",
        "fun apply(n: int): int\n    ret n * SCALE\nend fun\n",
    ]);
    assert!(uses(&graph, 1).contains(&"SCALE".S()),
        "a use from inside a body was missed: {:?}", uses(&graph, 1));
}

/// A unit that draws on two earlier units records both.
///
/// The graph is not a chain -- a unit can depend on any earlier unit, and the
/// edges are what decide whether an edit reaches it.
#[test]
fn a_unit_can_use_several_earlier_units() {
    let graph = graph_of(&[
        "let base = 2\n",
        "fun via(n: int): int\n    ret n + 1\nend fun\n",
        "let out = via(base)\n",
    ]);
    assert_eq!(provides(&graph, 1), ["via"]);
    assert!(uses(&graph, 2).contains(&"via".S()), "{:?}", uses(&graph, 2));
    assert!(uses(&graph, 2).contains(&"base".S()), "{:?}", uses(&graph, 2));
    assert!(!uses(&graph, 1).contains(&"base".S()),
        "the function does not touch `base`: {:?}", uses(&graph, 1));
}

/// **A function body is not a closure.** It may not name an enclosing `let`.
///
/// This used to typecheck. A script unit is seeded with every earlier unit's
/// bindings and entering a function body only *saved* that map rather than
/// clearing it, so a body could name a `let` and pass -- and the mistake
/// surfaced from lowering as "binding not available yet", which reads as a
/// phase-ordering problem rather than the scoping error it is. A module has no
/// top-level bindings, so nothing saw it there.
#[test]
fn a_function_body_may_not_name_an_enclosing_let() {
    let result = last_result(&[
        "let scale = 3\n",
        "fun apply(n: int): int\n    ret n * scale\nend fun\n",
    ]);
    match result.typecheck {
        datafun::pipeline::TypecheckResult::Error { ref errors } => assert!(
            errors.iter().any(|e| e.contains("scale")),
            "wrong error for a body naming a `let`: {errors:?}",
        ),
        other => panic!("a body named an enclosing `let` and passed: {other:?}"),
    }
}

