//! A module keeps its `IrModuleId` while the module set changes around it.
//!
//! A lowered script unit calls a module function by `CodeRef::Module`, which
//! names the module by number, and nothing re-lowers a unit whose own text did
//! not change. When the numbers were positions in the graph, a module joining
//! the graph ahead of another moved that one's number, and a unit lowered
//! before the move called whichever module had the number now. The REPL
//! re-narrowing its roots as units require more of the library is that, every
//! time a unit requires something new.
//!
//! Each case lowers `g` against one module, brings the other in, and asks `g`
//! again. Both orders, because only a module that sorts ahead of the first one
//! moved it; the other order passed before the fix too.

use rmx::prelude::*;
use rmx::std::collections::BTreeSet;

use datalove_datafun as datafun;
use datafun::incremental::Roots;

mod scriptsession;
use scriptsession::{Module, Session};

const A: Module<'static> = Module {
    library: "local",
    package: "test",
    module: "a",
    source: "fun fa(): int\n    ret 1\nend fun\n",
};

const B: Module<'static> = Module {
    library: "local",
    package: "test",
    module: "b",
    source: "fun fb(): int\n    ret 2\nend fun\n",
};

/// The one function a fixture module defines.
fn func(module: &Module<'_>) -> String {
    fmt!("f{}", module.module)
}

fn require_and_import(module: &Module<'_>) -> String {
    fmt!("require module {}\nimport {}.{}\n", module.path(), module.module, func(module))
}

/// Compile the modules again and put the result in front of the executor.
///
/// What a session has to do whenever its module set changes, so a unit can
/// call into a module the executor's registry did not have.
fn recompile(session: &mut Session) {
    let compiled = session.pipeline.compile(&session.db);
    assert!(compiled.is_successful(), "{:?}", compiled.all_errors());
    session.executor.set_module_registry(compiled.shared.module_registry.clone());
}

/// `g` is lowered against `first`, `second` comes in, and `g` still calls
/// `first`.
fn check(session: &mut Session, first: &Module<'_>, second: &Module<'_>, bring_in_second: impl FnOnce(&mut Session)) {
    session.append(&fmt!(
        "{}fun g(): int\n    ret {}()\nend fun\nlet x = g()",
        require_and_import(first), func(first)));
    let expected = session.binding("x");

    bring_in_second(session);
    recompile(session);
    session.append(&fmt!("{}let y = {}()", require_and_import(second), func(second)));
    session.append("let z = g()");

    assert_eq!(session.binding("z"), expected, "`g` still calls `{}`", func(first));
    assert_ne!(session.binding("y"), expected, "`y` called the other module");
}

/// Roots that start at `first` and are widened to take in `second`, which is
/// what the REPL does when a unit requires a module nothing required before.
fn widening_roots(first: &Module<'_>, second: &Module<'_>) {
    let mut session = Session::with_modules(&[A, B]);
    session.pipeline.set_roots(Roots::From(BTreeSet::from([first.path()])));
    recompile(&mut session);
    let both = BTreeSet::from([first.path(), second.path()]);
    check(&mut session, first, second, |session| session.pipeline.set_roots(Roots::From(both)));
}

/// A module added to the world the session started with.
fn adding_a_module(first: &Module<'_>, second: &Module<'_>) {
    let mut session = Session::with_modules(&[*first]);
    check(&mut session, first, second, |session| session.add_module(second));
}

#[test]
fn widening_the_roots_ahead_of_a_module_keeps_its_number() {
    widening_roots(&B, &A);
}

#[test]
fn widening_the_roots_behind_a_module_keeps_its_number() {
    widening_roots(&A, &B);
}

#[test]
fn adding_a_module_ahead_of_another_keeps_its_number() {
    adding_a_module(&B, &A);
}

#[test]
fn adding_a_module_behind_another_keeps_its_number() {
    adding_a_module(&A, &B);
}
