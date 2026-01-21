//! Statement generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, Variable, is_linear_type};
use crate::gen_type::gen_type_hint;
use crate::gen_expr::{gen_expr, gen_bool_expr};
use crate::pretty::pretty_type_hint;

/// Generate a let statement.
pub fn gen_let<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let name = format!("v{}", *var_counter);
    *var_counter += 1;

    let type_hint = gen_type_hint(db, rng, config);
    let value = gen_expr(db, rng, type_hint.clone(), config, ctx);
    let type_str = pretty_type_hint(db, type_hint.clone());

    ctx.variables.push(Variable {
        name: name.clone(),
        type_hint,
        is_mutable: false,
    });

    format!("{}let {}: {} = {}", indent, name, type_str, value)
}

/// Generate a var statement.
pub fn gen_var<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let name = format!("v{}", *var_counter);
    *var_counter += 1;

    let type_hint = gen_type_hint(db, rng, config);
    let value = gen_expr(db, rng, type_hint.clone(), config, ctx);
    let type_str = pretty_type_hint(db, type_hint.clone());

    ctx.variables.push(Variable {
        name: name.clone(),
        type_hint,
        is_mutable: true,
    });

    format!("{}var {}: {} = {}", indent, name, type_str, value)
}

/// Generate a set statement.
pub fn gen_set<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    indent: &str,
) -> Option<String> {
    let mutable_vars = ctx.mutable_variables();
    if mutable_vars.is_empty() {
        return None;
    }

    let var_idx = rng.gen_range(0..mutable_vars.len());
    let var_name = mutable_vars[var_idx].name.clone();
    let var_type = mutable_vars[var_idx].type_hint.clone();
    let value = gen_expr(db, rng, var_type, config, ctx);

    Some(format!("{}set {} = {}", indent, var_name, value))
}

/// Generate a debuglog statement.
///
/// Prints an existing variable's value for interpreter output comparison.
pub fn gen_debuglog<'db, R: Rng>(
    _db: &'db dyn salsa::Database,
    rng: &mut R,
    ctx: &GenContext<'db>,
    indent: &str,
) -> Option<String> {
    if ctx.variables.is_empty() {
        return None;
    }
    let var = &ctx.variables[rng.gen_range(0..ctx.variables.len())];
    Some(format!("{}debuglog {}", indent, var.name))
}

/// Generate a return statement.
pub fn gen_ret<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    indent: &str,
) -> String {
    match &ctx.return_type {
        Some(return_type) => {
            let value = gen_expr(db, rng, return_type.clone(), config, ctx);
            format!("{}ret {}", indent, value)
        }
        None => format!("{}ret", indent),
    }
}

/// Generate an if statement.
pub fn gen_if<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let condition = gen_bool_expr(db, rng, config, ctx);
    let mut result = format!("{}if {}\n", indent, condition);

    // Generate then-body.
    let inner_indent = format!("{}  ", indent);
    ctx.control_flow_depth += 1;

    // Save variables and consumed state before entering then branch.
    let saved_variables = ctx.variables.clone();
    let saved_consumed = ctx.consumed_variables.clone();

    let then_stmt_count = rng.gen_range(1..=2);
    for _ in 0..then_stmt_count {
        let stmt = gen_simple_statement(db, rng, config, ctx, var_counter, &inner_indent);
        result.push_str(&stmt);
        result.push('\n');
    }

    // Restore variables and consumed state after then branch.
    ctx.variables = saved_variables;
    ctx.consumed_variables = saved_consumed;

    // Maybe generate else-body.
    if rng.gen_bool(0.5) {
        result.push_str(&format!("{}else\n", indent));

        // Save variables and consumed state before entering else branch.
        let saved_variables = ctx.variables.clone();
        let saved_consumed = ctx.consumed_variables.clone();

        let else_stmt_count = rng.gen_range(1..=2);
        for _ in 0..else_stmt_count {
            let stmt = gen_simple_statement(db, rng, config, ctx, var_counter, &inner_indent);
            result.push_str(&stmt);
            result.push('\n');
        }

        // Restore variables and consumed state after else branch.
        ctx.variables = saved_variables;
        ctx.consumed_variables = saved_consumed;
    }

    ctx.control_flow_depth -= 1;
    result.push_str(&format!("{}end if", indent));
    result
}

/// Generate a loop statement.
///
/// Generates either:
/// - `loop while condition` - conditional loop
/// - `loop` - bare infinite loop (must have break)
pub fn gen_loop<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    // 30% chance to generate bare loop, 70% loop while.
    let is_bare_loop = rng.gen_bool(0.3);

    let mut result = if is_bare_loop {
        format!("{}loop\n", indent)
    } else {
        let condition = gen_bool_expr(db, rng, config, ctx);
        format!("{}loop while {}\n", indent, condition)
    };

    let inner_indent = format!("{}  ", indent);
    ctx.control_flow_depth += 1;
    ctx.loop_depth += 1;

    // Save variables, consumed, and loop-protected state before entering loop body.
    let saved_variables = ctx.variables.clone();
    let saved_consumed = ctx.consumed_variables.clone();
    let saved_loop_protected = ctx.loop_protected_variables.clone();

    // Mark all outer-scoped linear variables as loop-protected to prevent MoveInLoop errors.
    // They can still be borrowed (used in operators) but not moved directly.
    let linear_vars: Vec<String> = ctx.variables
        .iter()
        .filter(|v| is_linear_type(&v.type_hint))
        .map(|v| v.name.clone())
        .collect();
    for name in linear_vars {
        ctx.loop_protect_variable(&name);
    }

    let body_stmt_count = rng.gen_range(1..=3);
    for i in 0..body_stmt_count {
        // Always end with break to prevent infinite loops.
        // Even `loop while` conditions can be always-true (e.g., `not false`).
        let is_last = i == body_stmt_count - 1;
        if is_last {
            result.push_str(&format!("{}break\n", inner_indent));
        } else {
            let stmt = gen_loop_body_statement(db, rng, config, ctx, var_counter, &inner_indent);
            result.push_str(&stmt);
            result.push('\n');
        }
    }

    // Restore variables, consumed, and loop-protected state after loop body.
    ctx.variables = saved_variables;
    ctx.consumed_variables = saved_consumed;
    ctx.loop_protected_variables = saved_loop_protected;

    ctx.loop_depth -= 1;
    ctx.control_flow_depth -= 1;
    result.push_str(&format!("{}end loop", indent));
    result
}

/// Generate a simple statement (let, var, set).
fn gen_simple_statement<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let choice = rng.gen_range(0..10);
    match choice {
        0..=3 => gen_let(db, rng, config, ctx, var_counter, indent),
        4..=6 => gen_var(db, rng, config, ctx, var_counter, indent),
        7..=9 => gen_set(db, rng, config, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        _ => gen_let(db, rng, config, ctx, var_counter, indent),
    }
}

/// Generate a statement suitable for loop body (may include break/continue).
fn gen_loop_body_statement<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let choice = rng.gen_range(0..15);
    match choice {
        0..=4 => gen_let(db, rng, config, ctx, var_counter, indent),
        5..=8 => gen_var(db, rng, config, ctx, var_counter, indent),
        9..=11 => gen_set(db, rng, config, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        12 => format!("{}break", indent),
        13 => format!("{}continue", indent),
        14 if !ctx.at_max_depth(config) => gen_if(db, rng, config, ctx, var_counter, indent),
        _ => gen_let(db, rng, config, ctx, var_counter, indent),
    }
}

/// Generate a function body statement.
pub fn gen_body_statement<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    // Check probabilities for control flow and debuglog.
    let can_if = !ctx.at_max_depth(config) && config.check_probability(rng, config.if_probability);
    let can_loop = !ctx.at_max_depth(config) && config.check_probability(rng, config.loop_probability);
    let can_debuglog = config.check_probability(rng, config.debuglog_probability);

    let choice = rng.gen_range(0..12);
    match choice {
        0..=3 => gen_let(db, rng, config, ctx, var_counter, indent),
        4..=5 => gen_var(db, rng, config, ctx, var_counter, indent),
        6..=7 => gen_set(db, rng, config, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        8 if can_if => gen_if(db, rng, config, ctx, var_counter, indent),
        9 if can_loop => gen_loop(db, rng, config, ctx, var_counter, indent),
        10..=11 if can_debuglog => gen_debuglog(db, rng, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        _ => gen_let(db, rng, config, ctx, var_counter, indent),
    }
}
