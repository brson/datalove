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
use datalove_datafun_ir::IrCodeUnit;

/// Finish a function: each call to an inline function replaced by its body,
/// what that leaves simplified and its dead code removed, and, where consts
/// were inlined, the function's consts promoted to statics.
///
/// An error is a cycle of inline functions.
pub fn finish(func: &mut IrCodeUnit, inliner: &mut Inliner<'_>, promote: bool) -> Result<(), String> {
    if calls_inline(func, inliner.resolver()) && inliner.inline(func).map_err(|e| e.to_string())? {
        eliminate_dead_code_unit(func);
    }
    if promote && !func.const_values.is_empty() {
        promote_function_consts(func);
    }
    Ok(())
}

/// `finish` for a function behind a handle, which stays shared where nothing
/// changes it.
pub fn finish_shared(unit: &mut Arc<IrCodeUnit>, inliner: &mut Inliner<'_>, promote: bool) -> Result<(), String> {
    if calls_inline(unit, inliner.resolver()) || promote && !unit.const_values.is_empty() {
        finish(Arc::make_mut(unit), inliner, promote)?;
    }
    Ok(())
}
