//! Statement generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, Variable};
use crate::gen_type::gen_type_hint_with_heap;
use crate::gen_expr::{gen_expr, gen_bool_expr};
use crate::pretty::pretty_type_hint_and_heap;

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

    let type_hint = gen_type_hint_with_heap(db, rng, config);
    let value = gen_expr(db, rng, type_hint, config, ctx);
    let type_str = pretty_type_hint_and_heap(db, type_hint);

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

    let type_hint = gen_type_hint_with_heap(db, rng, config);
    let value = gen_expr(db, rng, type_hint, config, ctx);
    let type_str = pretty_type_hint_and_heap(db, type_hint);

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
    ctx: &GenContext<'db>,
    indent: &str,
) -> Option<String> {
    let mutable_vars = ctx.mutable_variables();
    if mutable_vars.is_empty() {
        return None;
    }

    let var = mutable_vars[rng.gen_range(0..mutable_vars.len())];
    let value = gen_expr(db, rng, var.type_hint, config, ctx);

    Some(format!("{}set {} = {}", indent, var.name, value))
}

/// Generate a return statement.
pub fn gen_ret<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext<'db>,
    indent: &str,
) -> String {
    match ctx.return_type {
        Some(return_type) => {
            let value = gen_expr(db, rng, return_type, config, ctx);
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

    let then_stmt_count = rng.gen_range(1..=2);
    for _ in 0..then_stmt_count {
        let stmt = gen_simple_statement(db, rng, config, ctx, var_counter, &inner_indent);
        result.push_str(&stmt);
        result.push('\n');
    }

    // Maybe generate else-body.
    if rng.gen_bool(0.5) {
        result.push_str(&format!("{}else\n", indent));
        let else_stmt_count = rng.gen_range(1..=2);
        for _ in 0..else_stmt_count {
            let stmt = gen_simple_statement(db, rng, config, ctx, var_counter, &inner_indent);
            result.push_str(&stmt);
            result.push('\n');
        }
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

    let body_stmt_count = rng.gen_range(1..=3);
    for i in 0..body_stmt_count {
        // For bare loops, always end with break to prevent infinite loop.
        // For while loops, last statement might be a break.
        let is_last = i == body_stmt_count - 1;
        if is_last && (is_bare_loop || rng.gen_bool(0.5)) {
            result.push_str(&format!("{}break\n", inner_indent));
        } else {
            let stmt = gen_loop_body_statement(db, rng, config, ctx, var_counter, &inner_indent);
            result.push_str(&stmt);
            result.push('\n');
        }
    }

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
    // Check probabilities for control flow.
    let can_if = !ctx.at_max_depth(config) && config.check_probability(rng, config.if_probability);
    let can_loop = !ctx.at_max_depth(config) && config.check_probability(rng, config.loop_probability);

    let choice = rng.gen_range(0..10);
    match choice {
        0..=3 => gen_let(db, rng, config, ctx, var_counter, indent),
        4..=5 => gen_var(db, rng, config, ctx, var_counter, indent),
        6..=7 => gen_set(db, rng, config, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        8 if can_if => gen_if(db, rng, config, ctx, var_counter, indent),
        9 if can_loop => gen_loop(db, rng, config, ctx, var_counter, indent),
        _ => gen_let(db, rng, config, ctx, var_counter, indent),
    }
}
