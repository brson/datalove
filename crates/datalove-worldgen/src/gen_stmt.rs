//! Statement generation.

use rand::Rng;
use datalove_datalit::ast::TypeHint;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, Variable, is_linear_type};
use crate::gen_type::gen_type_hint_and_spelling;
use crate::gen_enum::gen_match;
use crate::gen_expr::{gen_expr, gen_bool_expr};
use crate::pretty::pretty_type_hint;

/// Call a generic function, binding what comes back.
///
/// The types are picked before the call is written, so the binding can name
/// the answer: a generic's answer is whatever the call bound its parameters
/// to. See `gen_generic`.
///
/// `None` when there is no generic in reach.
pub fn gen_generic_call_stmt<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> Option<String> {
    if ctx.generic_functions.is_empty() {
        return None;
    }
    let sig = ctx.generic_functions[rng.gen_range(0..ctx.generic_functions.len())].clone();
    let (prelude, call, result) =
        crate::gen_generic::gen_generic_call(db, rng, &sig, config, ctx, var_counter);

    let mut lines: Vec<String> = prelude
        .into_iter()
        .map(|line| format!("{}{}", indent, line))
        .collect();

    match result {
        // Nothing comes back, so the call is a statement on its own.
        None => lines.push(format!("{}{}", indent, call)),
        Some(result_type) => {
            let name = format!("v{}", *var_counter);
            *var_counter += 1;
            let type_str = pretty_type_hint(db, result_type.clone());
            ctx.variables.push(Variable {
                name: name.clone(),
                type_hint: result_type,
                is_mutable: false,
            });
            lines.push(format!("{}let {}: {} = {}", indent, name, type_str, call));
        }
    }
    Some(lines.join("\n"))
}

/// Unwrap something in scope, binding what comes out.
///
/// Driven by what is in scope rather than by a type wanted somewhere: `?` and
/// `!` are worth writing for their own sake, and waiting for a binding of the
/// right shape to be wanted at exactly the right type left `!` unwritten
/// across three hundred worldfiles at a stretch.
///
/// `None` unless the enclosing function returns the matching kind -- a `?`
/// leaves through a `none` and a `!` through an error -- and there is
/// something of that shape to take apart. The binding is moved out of.
fn gen_unwrap_let<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> Option<String> {
    let mark = crate::gen_expr::early_return_mark(&ctx.return_type)?;
    let candidates: Vec<(String, TypeHint<'db>)> = ctx
        .variables
        .iter()
        .filter(|v| !ctx.is_consumed(&v.name) && !ctx.is_loop_protected(&v.name))
        .filter_map(|v| match (&v.type_hint, mark) {
            (TypeHint::Option(o), "?") => Some((v.name.clone(), (*o.inner_type).clone())),
            (TypeHint::Result(r), "!") => Some((v.name.clone(), (*r.inner_type).clone())),
            _ => Option::None,
        })
        .collect();
    if candidates.is_empty() {
        return Option::None;
    }

    let (scrutinee, inner) = candidates[rng.gen_range(0..candidates.len())].clone();
    ctx.consume_variable(&scrutinee);

    let name = format!("v{}", *var_counter);
    *var_counter += 1;
    let written = pretty_type_hint(db, inner.clone());
    ctx.variables.push(Variable {
        name: name.clone(),
        type_hint: inner,
        is_mutable: false,
    });
    Some(format!("{}let {}: {} = {}{}", indent, name, written, scrutinee, mark))
}

/// Bind a value of one of the enums in reach.
///
/// Its type is written as the alias the enum was declared under, because that
/// is the only name it has, and a `match` needs the name to say which variants
/// it is covering.
///
/// `None` when the body has no enum in reach, which is every body outside a
/// module: an enum is declared in the module that defines it.
fn gen_enum_let<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> Option<String> {
    if ctx.enums.is_empty() {
        return Option::None;
    }
    let def = ctx.enums[rng.gen_range(0..ctx.enums.len())].clone();

    // Sometimes the variant on its own rather than widened. What is bound is
    // then of the atom's or the term's own type, which nothing matches over
    // and nothing else reaches for -- it is written to be written.
    if rng.gen_bool(0.3) {
        let (prelude, written_type, value) = crate::gen_enum::gen_bare_variant_let(
            db, rng, &def, config, ctx, var_counter, indent);
        let name = format!("v{}", *var_counter);
        *var_counter += 1;
        let mut lines = prelude;
        lines.push(format!("{}let {}: {} = {}", indent, name, written_type, value));
        return Some(lines.join("\n"));
    }

    let (prelude, value) =
        crate::gen_enum::gen_enum_value(db, rng, &def, config, ctx, var_counter, indent);

    let name = format!("v{}", *var_counter);
    *var_counter += 1;
    ctx.variables.push(Variable {
        name: name.clone(),
        type_hint: TypeHint::Alias(bct::text::InternedText::new(db, def.name.clone())),
        is_mutable: false,
    });

    let mut lines = prelude;
    lines.push(format!("{}let {}: {} = {}", indent, name, def.name, value));
    Some(lines.join("\n"))
}

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

    let (type_hint, type_str) =
        gen_type_hint_and_spelling(db, rng, config, &ctx.type_aliases);

    // Inside a function that returns an option or a result, sometimes bind the
    // matching wrapper over what was picked, so there is something for a `?`
    // or a `!` to unwrap. Left to chance, a binding of the right shape and a
    // function of the right kind hardly ever met.
    let (type_hint, type_str) =
        match crate::gen_expr::wrapper_worth_binding(&ctx.return_type, type_hint.clone()) {
            Some(wrapped) if config.check_probability(rng, config.projection_probability) => {
                let written = pretty_type_hint(db, wrapped.clone());
                (wrapped, written)
            }
            _ => (type_hint, type_str),
        };

    let value = gen_expr(db, rng, type_hint.clone(), config, ctx);

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

    let (type_hint, type_str) =
        gen_type_hint_and_spelling(db, rng, config, &ctx.type_aliases);
    let value = gen_expr(db, rng, type_hint.clone(), config, ctx);

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
    // Filter out consumed variables (debuglog borrows, so can use loop-protected).
    let available: Vec<_> = ctx.variables
        .iter()
        .filter(|v| !ctx.is_consumed(&v.name))
        .collect();
    if available.is_empty() {
        return None;
    }
    let var = available[rng.gen_range(0..available.len())];
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
    insist_on_a_binding: bool,
) -> String {
    // An `if` over an option may bind what is inside it, which is the only way
    // the language has of getting at that payload. The binding
    // moves out of what it destructured -- `if x |value|` leaves `x` moved
    // from on both paths -- so the scrutinee has to be one this body is
    // allowed to move, and it is consumed before the branches are written.
    // Whether this `if` takes something apart or tests a condition. A caller
    // that reached for the destructuring form on purpose says so; otherwise it
    // is rolled for here.
    let binding = if insist_on_a_binding || config.check_probability(rng, config.if_binding_probability) {
        gen_if_binding(rng, ctx, var_counter)
    } else {
        Option::None
    };

    let mut result = match &binding {
        Some(b) => format!("{}if {} |{}|\n", indent, b.scrutinee, b.name),
        Option::None => {
            let condition = gen_bool_expr(db, rng, config, ctx);
            format!("{}if {}\n", indent, condition)
        }
    };

    // Generate then-body.
    let inner_indent = format!("{}  ", indent);
    ctx.control_flow_depth += 1;

    // Save variables and consumed state before entering then branch.
    let saved_variables = ctx.variables.clone();
    let saved_consumed = ctx.consumed_variables.clone();
    let saved_protected = ctx.loop_protected_variables.clone();

    // A branch may not move what was declared outside it. Moving in one branch
    // and not the other is refused, and the generator has no way to promise
    // the other branch will match, so it borrows rather than moves. The same
    // rule a loop body keeps, for the same kind of reason.
    let outer_linear: Vec<String> = ctx.variables
        .iter()
        .filter(|v| is_linear_type(&v.type_hint))
        .map(|v| v.name.clone())
        .collect();
    for name in &outer_linear {
        ctx.loop_protect_variable(name);
    }

    // What the binding named is in scope for the then branch alone, and the
    // restore below is what takes it back out again.
    if let Some(b) = &binding {
        ctx.variables.push(Variable {
            name: b.name.clone(),
            type_hint: b.payload_type.clone(),
            is_mutable: false,
        });
    }

    let then_stmt_count = rng.gen_range(1..=2);
    for _ in 0..then_stmt_count {
        let stmt = gen_simple_statement(db, rng, config, ctx, var_counter, &inner_indent);
        result.push_str(&stmt);
        result.push('\n');
    }

    // Restore variables and consumed state after then branch.
    ctx.variables = saved_variables;
    ctx.consumed_variables = saved_consumed;

    // Maybe generate else-body. A result destructured above has to have one,
    // and it has to bind what went wrong.
    let error_binding = binding.as_ref().and_then(|b| b.error_binding.clone());
    if error_binding.is_some() || rng.gen_bool(0.5) {
        match &error_binding {
            Some(err) => result.push_str(&format!("{}else |{}|\n", indent, err)),
            Option::None => result.push_str(&format!("{}else\n", indent)),
        }

        // Save variables and consumed state before entering else branch.
        let saved_variables = ctx.variables.clone();
        let saved_consumed = ctx.consumed_variables.clone();

        if let Some(err) = &error_binding {
            ctx.variables.push(Variable {
                name: err.clone(),
                type_hint: TypeHint::Error,
                is_mutable: false,
            });
        }

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

    ctx.loop_protected_variables = saved_protected;
    ctx.control_flow_depth -= 1;
    result.push_str(&format!("{}end if", indent));
    result
}

/// Whether anything in scope can be taken apart by an `if`.
fn has_something_to_destructure(ctx: &GenContext<'_>) -> bool {
    ctx.variables.iter().any(|v| {
        !ctx.is_consumed(&v.name)
            && !ctx.is_loop_protected(&v.name)
            && matches!(v.type_hint, TypeHint::Option(_) | TypeHint::Result(_))
    })
}

/// An `if` that takes something apart rather than testing a condition.
pub struct IfBinding<'db> {
    /// What is being destructured, which the `if` moves out of.
    pub scrutinee: String,
    /// What the payload is called in the then branch, and its type.
    pub name: String,
    pub payload_type: TypeHint<'db>,
    /// What the error is called in the else branch, for a result.
    ///
    /// An option's else branch binds nothing and may be left off altogether. A
    /// result's must be there and must bind: the typechecker refuses one
    /// without it, `ResultRequiresErrorBinding`.
    pub error_binding: Option<String>,
}

/// Pick something for an `if` to destructure, if there is anything to pick.
///
/// An option or a result, which are the two the language unwraps this way.
/// The scrutinee is marked consumed here, because the binding moves out of it
/// whichever way the branch goes.
///
/// A variable the body is only borrowing -- one declared outside a loop, say
/// -- cannot be destructured, since that would be a move. Those are the ones
/// already marked loop-protected, and they are left out.
fn gen_if_binding<'db, R: Rng>(
    rng: &mut R,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
) -> Option<IfBinding<'db>> {
    // (name, payload type, whether the else branch has to bind the error).
    let candidates: Vec<(String, TypeHint<'db>, bool)> = ctx
        .variables
        .iter()
        .filter(|v| !ctx.is_consumed(&v.name) && !ctx.is_loop_protected(&v.name))
        .filter_map(|v| match &v.type_hint {
            TypeHint::Option(o) => Some((v.name.clone(), (*o.inner_type).clone(), false)),
            TypeHint::Result(r) => Some((v.name.clone(), (*r.inner_type).clone(), true)),
            _ => Option::None,
        })
        .collect();

    if candidates.is_empty() {
        return Option::None;
    }

    let (scrutinee, payload_type, wants_error) =
        candidates[rng.gen_range(0..candidates.len())].clone();
    ctx.consume_variable(&scrutinee);

    let name = format!("v{}", *var_counter);
    *var_counter += 1;
    let error_binding = if wants_error {
        let err = format!("v{}", *var_counter);
        *var_counter += 1;
        Some(err)
    } else {
        Option::None
    };

    Some(IfBinding { scrutinee, name, payload_type, error_binding })
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
pub(crate) fn gen_simple_statement<'db, R: Rng>(
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

/// Generate a statement suitable for loop body.
///
/// Does NOT generate `continue` statements because they would skip the
/// final `break` that `gen_loop` adds to prevent infinite loops.
fn gen_loop_body_statement<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let choice = rng.gen_range(0..14);
    match choice {
        0..=4 => gen_let(db, rng, config, ctx, var_counter, indent),
        5..=8 => gen_var(db, rng, config, ctx, var_counter, indent),
        9..=11 => gen_set(db, rng, config, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        12 => format!("{}break", indent),
        // Removed: continue - would make final break unreachable.
        13 if !ctx.at_max_depth(config) => gen_if(db, rng, config, ctx, var_counter, indent, false),
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

    let can_generic = !ctx.generic_functions.is_empty()
        && config.check_probability(rng, config.generic_call_probability);

    if can_generic {
        if let Some(stmt) = gen_generic_call_stmt(db, rng, config, ctx, var_counter, indent) {
            return stmt;
        }
    }

    if config.check_probability(rng, config.projection_probability) {
        if let Some(stmt) = gen_unwrap_let(db, rng, config, ctx, var_counter, indent) {
            return stmt;
        }
    }

    // Taking an option or a result apart with an `if` is worth reaching for on
    // its own, rather than waiting for the `if` roll and the binding roll to
    // come up together. Left to those, it was written three times in three
    // hundred worldfiles and the result form once.
    if !ctx.at_max_depth(config)
        && config.check_probability(rng, config.if_binding_probability)
        && has_something_to_destructure(ctx)
    {
        return gen_if(db, rng, config, ctx, var_counter, indent, true);
    }

    // A `match` is the only way to take an enum apart, and something has to
    // have made one first, so the two are reached for together.
    if !ctx.enums.is_empty() && config.check_probability(rng, config.match_probability) {
        if let Some(stmt) = gen_match(db, rng, config, ctx, var_counter, indent) {
            return stmt;
        }
        if let Some(stmt) = gen_enum_let(db, rng, config, ctx, var_counter, indent) {
            return stmt;
        }
    }

    let choice = rng.gen_range(0..12);
    match choice {
        0..=3 => gen_let(db, rng, config, ctx, var_counter, indent),
        4..=5 => gen_var(db, rng, config, ctx, var_counter, indent),
        6..=7 => gen_set(db, rng, config, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        8 if can_if => gen_if(db, rng, config, ctx, var_counter, indent, false),
        9 if can_loop => gen_loop(db, rng, config, ctx, var_counter, indent),
        10..=11 if can_debuglog => gen_debuglog(db, rng, ctx, indent)
            .unwrap_or_else(|| gen_let(db, rng, config, ctx, var_counter, indent)),
        _ => gen_let(db, rng, config, ctx, var_counter, indent),
    }
}
