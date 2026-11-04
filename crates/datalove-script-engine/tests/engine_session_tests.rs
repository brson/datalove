//! Several sessions in one engine: they share the modules, and nothing else.

use rmx::prelude::*;

use datalove_datafun::pipeline::{
    ModuleDescriptor, PackageDescriptor, PackageLibrary, WorkspaceDescriptor,
};
use datalove_script_engine::{Engine, Eval, SessionId, Unit};

const M: &str = "fun f(x: int): int\n    ret x\nend fun\n";

/// A function that overflows the stack, which is a runtime error.
const DOWN: &str = "fun down(s: string): u32\n  let t = s@\n  ret icall add_wrapping_u32(down(t), : u32 / 1)\nend fun";

/// An engine over the system library and `local/test/m`.
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

fn eval(engine: &mut Engine, session: SessionId, source: &str) -> Eval {
    engine.session(session).eval(Unit::Fragment(source.S()))
}

fn value_of(engine: &mut Engine, session: SessionId, name: &str) -> Option<String> {
    engine.session(session).get_environment().into_iter()
        .find(|binding| binding.name == name)
        .map(|binding| binding.value.expect("a value not given away"))
}

/// Each session has its own bindings and numbers its own units.
#[test]
fn sessions_keep_their_own_bindings() {
    let mut engine = engine_with_module(M);
    let first = engine.first_session();
    let second = engine.open_session();
    assert_ne!(first, second);

    eval(&mut engine, first, "let a = 1");
    eval(&mut engine, second, "let a = 2");
    eval(&mut engine, second, "let b = a + 10");

    assert_eq!(value_of(&mut engine, first, "a").as_deref(), Some("1"));
    assert_eq!(value_of(&mut engine, second, "a").as_deref(), Some("2"));
    assert_eq!(value_of(&mut engine, second, "b").as_deref(), Some("12"));
    assert_eq!(value_of(&mut engine, first, "b"), None);
    assert_eq!(engine.session(first).unit_ids(), vec![1]);
    assert_eq!(engine.session(second).unit_ids(), vec![1, 2]);

    let reports = engine.session(second).edit_unit(1, "let a = 3");
    assert_eq!(reports.iter().filter(|r| r.eval.is_some()).map(|r| r.unit).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(value_of(&mut engine, second, "b").as_deref(), Some("13"));
    assert_eq!(value_of(&mut engine, first, "a").as_deref(), Some("1"), "the first session is untouched");
    let first_step: Vec<u32> = engine.last_step().iter().map(|report| report.unit).collect();
    assert_eq!(first_step, vec![1], "the first session's last step is still its append");
}

/// **A runtime error empties its own session**, and the others keep their
/// units and their values.
#[test]
fn a_runtime_error_resets_only_its_session() {
    let mut engine = engine_with_module(M);
    let first = engine.first_session();
    let second = engine.open_session();
    eval(&mut engine, first, "let a = 1");
    eval(&mut engine, second, DOWN);

    let failed = eval(&mut engine, second, "let x = down(\"abc\")");
    assert!(matches!(failed, Eval::RuntimeError(_)), "{failed:?}");

    assert_eq!(engine.session(second).unit_count(), 0);
    assert_eq!(engine.session(first).unit_ids(), vec![1]);
    assert_eq!(value_of(&mut engine, first, "a").as_deref(), Some("1"));
    eval(&mut engine, second, "let c = 3");
    assert_eq!(value_of(&mut engine, second, "c").as_deref(), Some("3"));
}

/// A module one session brought into the compile is there for another that
/// requires it later, calling it as it is.
///
/// The second session's units moved no roots, so it is not that which puts
/// the module in front of its executor.
#[test]
fn a_session_requiring_a_module_another_brought_in_calls_it() {
    let mut engine = engine_with_module(M);
    let first = engine.first_session();
    let second = engine.open_session();
    eval(&mut engine, second, "let z = 0");

    eval(&mut engine, first, "require module local/test/m\nimport m.f\nlet a = f(1)");
    let eval_b = eval(&mut engine, second, "require module local/test/m\nimport m.f\nlet b = f(2)");
    assert!(matches!(eval_b, Eval::Ran(_)), "{eval_b:?}");
    assert_eq!(value_of(&mut engine, second, "b").as_deref(), Some("2"));
}

/// **A module edit re-derives what it reaches in every session**, and each
/// session's report is its own.
#[test]
fn a_module_edit_reaches_every_session() {
    let mut engine = engine_with_module(M);
    let first = engine.first_session();
    let second = engine.open_session();
    let third = engine.open_session();
    eval(&mut engine, first, "require module local/test/m\nimport m.f\nlet a = f(1)");
    eval(&mut engine, second, "let y = 5");
    eval(&mut engine, second, "require module local/test/m\nimport m.f\nlet b = f(2)");
    eval(&mut engine, third, "let c = 3");

    let reports = engine.edit_module("local", "test", "m", "fun f(x: int): int\n    ret x + 100\nend fun\n");

    let rederived = |id: SessionId| -> Vec<u32> {
        reports[&id].iter().filter(|report| report.eval.is_some()).map(|report| report.unit).collect()
    };
    assert_eq!(rederived(first), vec![1]);
    assert_eq!(rederived(second), vec![2]);
    assert_eq!(rederived(third), Vec::<u32>::new());
    assert_eq!(value_of(&mut engine, first, "a").as_deref(), Some("101"));
    assert_eq!(value_of(&mut engine, second, "b").as_deref(), Some("102"));
    assert_eq!(value_of(&mut engine, third, "c").as_deref(), Some("3"));
    let last: Vec<u32> = engine.session(second).last_step().iter().map(|report| report.unit).collect();
    assert_eq!(last, reports[&second].iter().map(|report| report.unit).collect::<Vec<_>>());
}

/// Closing the one session requiring a broken module takes the module's
/// errors with it.
#[test]
fn closing_a_session_lets_go_of_the_modules_only_it_required() {
    let mut engine = engine_with_module(M);
    let first = engine.first_session();
    let second = engine.open_session();
    eval(&mut engine, first, "let a = 1");
    eval(&mut engine, second, "require module local/test/m\nimport m.f\nlet b = f(2)");

    engine.edit_module("local", "test", "m", "fun f(x: int): int\n    ret \"nope\"\nend fun\n");
    assert!(engine.module_errors().broken.contains_key("local/test/m"));

    engine.close_session(second);
    assert!(engine.module_errors().is_empty(), "{:?}", engine.module_errors().broken);
    assert_eq!(engine.session_ids(), vec![first]);
    assert_eq!(value_of(&mut engine, first, "a").as_deref(), Some("1"));
}

/// A module one session brings into the compile costs another session's
/// units nothing: its next step compiles only the unit it appends.
#[test]
fn a_module_another_session_brings_in_recompiles_nothing_here() {
    let mut engine = engine_with_module(M);
    let first = engine.first_session();
    let second = engine.open_session();
    eval(&mut engine, second, "let y = 5");
    eval(&mut engine, second, "let z = y + 1");

    eval(&mut engine, first, "require module local/test/m\nimport m.f\nlet a = f(1)");
    eval(&mut engine, second, "let w = 2");

    let compiled: Vec<u32> = engine.session(second).last_step().iter()
        .filter(|report| report.compiled.any())
        .map(|report| report.unit)
        .collect();
    assert_eq!(compiled, vec![3]);
}
