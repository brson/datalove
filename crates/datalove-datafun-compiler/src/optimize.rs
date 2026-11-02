//! The passes that finish a function, in the order every pipeline runs them;
//! see `botdocs/plan-ir-passes.md`.
//!
//! Const inlining and specialization come first and are not here: the one
//! because specialization reads const arguments out of what it leaves, the
//! other because it makes functions. What is here is the rest, one function at
//! a time, reading nothing but the function and the bodies of the inline
//! functions it calls.

use std::sync::Arc;

use datalove_datafun_const::{eliminate_dead_code_unit, promote_function_consts};
use datalove_datafun_ir::inline::{calls_inline, Inliner};
use datalove_datafun_ir::list_access::{may_repeat, reuse_list_accesses};
use datalove_datafun_ir::simplify::simplify;
use datalove_datafun_ir::IrCodeUnit;

/// Finish a function: each call to an inline function replaced by its body,
/// list elements it reaches more than once reached once, what that leaves
/// simplified and its dead code removed, and, where consts were inlined, the
/// function's consts promoted to statics.
///
/// Nothing here can fail: a cycle of inline functions, the one thing inlining
/// cannot do, is refused by typecheck (F085).
pub fn finish(func: &mut IrCodeUnit, inliner: &mut Inliner<'_>, promote: bool) {
    let inlined = calls_inline(func, inliner.resolver()) && inliner.inline(func);
    let reused = reuse_list_accesses(func);
    if reused {
        simplify(func);
    }
    if inlined || reused {
        eliminate_dead_code_unit(func);
    }
    if promote && !func.const_values.is_empty() {
        promote_function_consts(func);
    }
}

/// Whether `finish` may change `func`: a condition for each of its passes.
///
/// A pass added to `finish` needs one here too, or `finish_shared` skips it
/// for every shared function. A debug build checks that `finish` leaves as it
/// was a function this rules out.
fn may_change(func: &IrCodeUnit, inliner: &Inliner<'_>, promote: bool) -> bool {
    calls_inline(func, inliner.resolver()) || may_repeat(func) || promote && !func.const_values.is_empty()
}

/// `finish` for a function behind a handle, which stays shared where nothing
/// changes it.
pub fn finish_shared(unit: &mut Arc<IrCodeUnit>, inliner: &mut Inliner<'_>, promote: bool) {
    if may_change(unit, inliner, promote) {
        finish(Arc::make_mut(unit), inliner, promote);
    } else if cfg!(debug_assertions) {
        let mut finished = IrCodeUnit::clone(unit);
        finish(&mut finished, inliner, promote);
        assert!(finished == **unit, "`finish` changed {}, which `may_change` ruled out", unit.name);
    }
}
