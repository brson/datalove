//! Which script units' analysis salsa ran, for saying what a step compiled.
//!
//! A unit counts as compiled when salsa ran one of the queries that analyze
//! it: its parse, its typecheck, or its ownership analysis. Those queries
//! record themselves here, at the top of their bodies, which salsa only runs
//! when it executes the query; a memoized answer records nothing. Name
//! resolution is not recorded apart, because it runs again only when the
//! parse does. Lowering and what follows it are not analysis and are not
//! recorded.
//!
//! **A query is recorded only if running it means the pass ran.** The parse
//! is recorded in the parser's `parse` and `parse_expr`, which salsa runs
//! when the unit's text changed, and not in the typechecker's `unit_parses`,
//! which it runs again on every edit to any unit, since it reads the parse's
//! accumulated diagnostics.
//!
//! **This is a thread-local, and cheaper than [`crate::query_events`].** No
//! salsa event callback is installed, so queries that are not recorded cost
//! nothing, and a recorded one costs a thread-local read when nothing is
//! collecting. It sees only what runs on the thread collecting, which is
//! enough because script units are analyzed on the thread compiling them;
//! the parallel compile is of modules. The reactive tests check both: that
//! this agrees with what salsa reports it ran, and that no script unit's
//! analysis runs on a worker.

use bct::input::Source;
use rmx::std::cell::{Cell, RefCell};

/// An analysis pass a script unit goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Parse,
    Typecheck,
    Ownership,
}

thread_local! {
    static RAN: RefCell<Option<Vec<(Pass, Source)>>> = const { RefCell::new(None) };
    static COMPILING: Cell<Option<Source>> = const { Cell::new(None) };
}

/// Record that `pass` ran for the unit with this source.
pub fn record(pass: Pass, source: Source) {
    RAN.with_borrow_mut(|ran| {
        if let Some(ran) = ran {
            ran.push((pass, source));
        }
    });
}

/// Record that `pass` ran for the unit being compiled; see [`compiling`].
///
/// For a pass whose query is keyed on nothing that names its unit.
pub fn record_compiling(pass: Pass) {
    RAN.with_borrow_mut(|ran| {
        if let Some(ran) = ran {
            let source = COMPILING.get()
                .unwrap_or_else(|| panic!("{pass:?} analysis ran outside a unit's compile"));
            ran.push((pass, source));
        }
    });
}

/// Run `f` as the compile of the unit with this source.
pub fn compiling<R>(source: Source, f: impl FnOnce() -> R) -> R {
    let outer = COMPILING.replace(Some(source));
    let _restore = Restore(Some(move || COMPILING.set(outer)));
    f()
}

/// Run `f`, and say which units' analysis ran while it did, in the order it
/// ran. A unit is there once for every pass that ran for it.
pub fn collect<R>(f: impl FnOnce() -> R) -> (R, Vec<(Pass, Source)>) {
    let outer = RAN.replace(Some(Vec::new()));
    let _restore = Restore(Some(move || RAN.set(outer)));
    let result = f();
    let ran = RAN.with_borrow_mut(|ran| ran.as_mut().map(std::mem::take)).expect("collecting");
    (result, ran)
}

/// Runs its closure when dropped, so a panic out of `f` leaves the
/// thread-locals as they were.
struct Restore<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for Restore<F> {
    fn drop(&mut self) {
        if let Some(restore) = self.0.take() {
            restore();
        }
    }
}
