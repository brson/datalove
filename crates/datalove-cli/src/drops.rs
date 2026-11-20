//! `datalove script-drops`: a function's drop schedule as text.
//!
//! The formal model's drop schedule is held to this (`drops_agree` in
//! `datalove-formal-tests`), so the format is a contract with that test: a
//! `fun` line, then the statements in the order the analysis numbered them,
//! then a line to each point that drops something, its bindings by name in
//! the order the analysis listed them.

use rmx::prelude::*;
use datalove::datafun_sema::{BindingId, BindingInfo, DropSchedule};
use std::collections::BTreeMap;

/// The schedule of the function `name`, whose bindings are `bindings`.
pub fn render(name: &str, schedule: &DropSchedule, bindings: &[BindingInfo]) -> String {
    let names = |ids: &[BindingId]| -> String {
        ids.iter().map(|id| bindings[id.0 as usize].name.as_str()).collect::<Vec<_>>().join(" ")
    };
    let mut out = fmt!("fun {name}\n");
    #[cfg(debug_assertions)]
    {
        let kinds: Vec<&str> = schedule.stmt_order.iter().map(|k| k.kind_name()).collect();
        out.push_str(&fmt!("  stmts {}\n", kinds.join(" ")));
    }
    let mut keyed = |what: &str, map: &BTreeMap<usize, Vec<BindingId>>| {
        for (stmt, ids) in map {
            out.push_str(&fmt!("  {what} {stmt}: {}\n", names(ids)));
        }
    };
    keyed("then_branch_exit", &schedule.then_branch_exit);
    keyed("else_branch_exit", &schedule.else_branch_exit);
    keyed("before_return", &schedule.before_return);
    keyed("before_try_return", &schedule.before_try_return);
    keyed("before_set_target_early_return", &schedule.before_set_target_early_return);
    keyed("loop_body_end", &schedule.loop_body_end);
    keyed("before_break", &schedule.before_break);
    keyed("before_continue", &schedule.before_continue);
    for ((stmt, arm), ids) in &schedule.match_arm_exit {
        out.push_str(&fmt!("  match_arm_exit {stmt}.{arm}: {}\n", names(ids)));
    }
    for (stmt, moved) in &schedule.try_return_not_yet_moved {
        let parts: Vec<String> = moved.iter()
            .map(|(expr, id)| fmt!("{}@{expr}", bindings[id.0 as usize].name))
            .collect();
        out.push_str(&fmt!("  try_return_not_yet_moved {stmt}: {}\n", parts.join(" ")));
    }
    if !schedule.at_function_exit.is_empty() {
        out.push_str(&fmt!("  at_function_exit: {}\n", names(&schedule.at_function_exit)));
    }
    out
}
