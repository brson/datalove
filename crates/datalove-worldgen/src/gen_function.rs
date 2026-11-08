//! Function generation.

use rmx::prelude::*;
use bct::random::SeededRng;
use datalove_datalit::ast::{TypeHint, TypeHintOption, TypeHintResult};
use crate::config::WorldGenConfig;
use crate::context::{GenContext, FunctionSig, Param, ParamMode, Variable};
use crate::gen_type::gen_type_hint;
use crate::gen_expr::gen_expr;
use crate::gen_stmt::{gen_body_statement, gen_ret};
use crate::pretty::pretty_type_hint;

/// Pick how a parameter is passed.
///
/// Mostly `in`, which is the one with no marker and the one a call can always
/// satisfy. The other three ask something of the call site -- a `ref` wants a
/// binding rather than an expression, and a `mut` or an `out` wants a `var` --
/// so a function that takes one can only be called from somewhere that has it.
fn gen_param_mode(rng: &mut SeededRng, config: &WorldGenConfig) -> ParamMode {
    if !config.check_probability(rng, config.param_mode_probability) {
        return ParamMode::In;
    }
    match rng.random_range(0..3) {
        0 => ParamMode::Ref,
        1 => ParamMode::Mut,
        _ => ParamMode::Out,
    }
}

/// Generate a function signature.
pub fn gen_function_signature<'db>(
    db: &'db dyn salsa::Database,
    rng: &mut SeededRng,
    name: &str,
    config: &WorldGenConfig,
) -> FunctionSig<'db> {
    // Generate 0-3 parameters.
    let param_count = rng.random_range(0..=3);
    let params: Vec<_> = (0..param_count)
        .map(|i| Param {
            name: format!("p{}", i),
            type_hint: gen_type_hint(db, rng, config),
            mode: gen_param_mode(rng, config),
        })
        .collect();

    // Maybe generate a return type (80% of functions have returns).
    //
    // Sometimes an option or a result over one. What a function returns is
    // what decides whether its body may write anything that early-returns --
    // `?`, `!`, checked arithmetic, a fallible index -- and with plain types
    // only, seventeen expressions in four hundred and sixty were written
    // anywhere that could.
    let return_type = if rng.random_bool(0.8) {
        let inner = gen_type_hint(db, rng, config);
        Some(if config.check_probability(rng, config.fallible_return_probability) {
            if rng.random_bool(0.5) {
                TypeHint::Option(TypeHintOption { inner_type: Box::new(inner) })
            } else {
                TypeHint::Result(TypeHintResult { inner_type: Box::new(inner) })
            }
        } else {
            inner
        })
    } else {
        None
    };

    FunctionSig {
        name: name.S(),
        params,
        return_type,
    }
}

/// Generate a complete function definition.
pub fn gen_function<'db>(
    db: &'db dyn salsa::Database,
    rng: &mut SeededRng,
    sig: &FunctionSig<'db>,
    config: &WorldGenConfig,
    module_ctx: &GenContext<'db>,
) -> String {
    let mut lines = Vec::new();

    // Build signature line.
    let params_str = sig
        .params
        .iter()
        .map(|p| format!("{}{}: {}", p.mode.marker(), p.name, pretty_type_hint(db, p.type_hint.C())))
        .collect::<Vec<_>>()
        .join(", ");

    let sig_line = match &sig.return_type {
        Some(ret_ty) => format!("fun {}({}): {}", sig.name, params_str, pretty_type_hint(db, ret_ty.C())),
        None => format!("fun {}({})", sig.name, params_str),
    };
    lines.push(sig_line);

    // Create function context with parameters.
    let mut ctx = GenContext::new();
    ctx.return_type = sig.return_type.C();
    ctx.type_aliases = module_ctx.type_aliases.C();
    ctx.enums = module_ctx.enums.C();
    ctx.consts = module_ctx.consts.C();
    ctx.functions = module_ctx.functions.C();
    ctx.imported_functions = module_ctx.imported_functions.C();
    ctx.generic_functions = module_ctx.generic_functions.C();
    // Set current function name to prevent self-recursive calls.
    ctx.current_function_name = Some(sig.name.C());
    // And how far down the module the body may call, which is what stops two
    // functions calling each other. The module works this out; the body is
    // where it has to hold, and this context is a fresh one.
    ctx.max_callable_function_index = module_ctx.max_callable_function_index;

    // Add parameters as variables, as far as each mode allows.
    //
    // Only an `in` parameter is the body's to move. The other three belong to
    // the caller, so they are marked the way a loop marks what it may borrow
    // and not move. A `mut` is also a place the body may write, and an `out`
    // is a place it *must* write before it returns, which is what the first
    // statement below does -- until then it holds nothing and cannot be read.
    let mut lead_statements = Vec::new();
    let mut var_counter = 0;
    for param in &sig.params {
        if param.mode == ParamMode::Out {
            // Nothing that can return early, which would leave this parameter
            // and the ones after it unwritten (`OutParamNotInitialized`): with
            // no return type in force, `gen_expr` offers no `?` or `!` form.
            let return_type = ctx.return_type.take();
            let value = gen_expr(db, rng, param.type_hint.C(), config, &mut ctx);
            ctx.return_type = return_type;
            lead_statements.push(format!("  set {} = {}", param.name, value));
        }
        ctx.variables.push(Variable {
            name: param.name.C(),
            type_hint: param.type_hint.C(),
            is_mutable: matches!(param.mode, ParamMode::Mut | ParamMode::Out),
        });
        if param.mode != ParamMode::In {
            ctx.loop_protect_variable(&param.name);
        }
    }
    lines.extend(lead_statements);

    // Generate body statements.
    let stmt_count = rng.random_range(config.statements_per_function.0..=config.statements_per_function.1);

    for _ in 0..stmt_count {
        let stmt = gen_body_statement(db, rng, config, &mut ctx, &mut var_counter, "  ");
        lines.push(stmt);
    }

    // Generate return statement.
    // Void functions can optionally omit the ret statement entirely.
    let include_ret = match sig.return_type {
        Some(_) => true,  // Non-void functions must return a value.
        None => rng.random_bool(0.5),  // Void functions: 50% include bare ret.
    };

    if include_ret {
        let ret_stmt = gen_ret(db, rng, config, &mut ctx, "  ");
        lines.push(ret_stmt);
    }

    lines.push("end fun".S());
    lines.join("\n")
}
