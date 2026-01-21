//! Function generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, FunctionSig, Variable};
use crate::gen_type::gen_type_hint;
use crate::gen_stmt::{gen_body_statement, gen_ret};
use crate::pretty::pretty_type_hint;

/// Generate a function signature.
pub fn gen_function_signature<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    name: &str,
    config: &WorldGenConfig,
) -> FunctionSig<'db> {
    // Generate 0-3 parameters.
    let param_count = rng.gen_range(0..=3);
    let params: Vec<_> = (0..param_count)
        .map(|i| {
            let param_name = format!("p{}", i);
            let param_type = gen_type_hint(db, rng, config);
            (param_name, param_type)
        })
        .collect();

    // Maybe generate a return type (80% of functions have returns).
    let return_type = if rng.gen_bool(0.8) {
        Some(gen_type_hint(db, rng, config))
    } else {
        None
    };

    FunctionSig {
        name: name.to_string(),
        params,
        return_type,
    }
}

/// Generate a complete function definition.
pub fn gen_function<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    sig: &FunctionSig<'db>,
    config: &WorldGenConfig,
    module_ctx: &GenContext<'db>,
) -> String {
    let mut lines = Vec::new();

    // Build signature line.
    let params_str = sig
        .params
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, pretty_type_hint(db, ty.clone())))
        .collect::<Vec<_>>()
        .join(", ");

    let sig_line = match &sig.return_type {
        Some(ret_ty) => format!("fun {}({}): {}", sig.name, params_str, pretty_type_hint(db, ret_ty.clone())),
        None => format!("fun {}({})", sig.name, params_str),
    };
    lines.push(sig_line);

    // Create function context with parameters.
    let mut ctx = GenContext::new();
    ctx.return_type = sig.return_type.clone();
    ctx.type_aliases = module_ctx.type_aliases.clone();
    ctx.functions = module_ctx.functions.clone();
    ctx.imported_functions = module_ctx.imported_functions.clone();
    // Set current function name to prevent self-recursive calls.
    ctx.current_function_name = Some(sig.name.clone());

    // Add parameters as variables.
    for (name, ty) in &sig.params {
        ctx.variables.push(Variable {
            name: name.clone(),
            type_hint: ty.clone(),
            is_mutable: false,
        });
    }

    // Generate body statements.
    let mut var_counter = 0;
    let stmt_count = rng.gen_range(config.statements_per_function.0..=config.statements_per_function.1);

    for _ in 0..stmt_count {
        let stmt = gen_body_statement(db, rng, config, &mut ctx, &mut var_counter, "  ");
        lines.push(stmt);
    }

    // Generate return statement.
    // Void functions can optionally omit the ret statement entirely.
    let include_ret = match sig.return_type {
        Some(_) => true,  // Non-void functions must return a value.
        None => rng.gen_bool(0.5),  // Void functions: 50% include bare ret.
    };

    if include_ret {
        let ret_stmt = gen_ret(db, rng, config, &mut ctx, "  ");
        lines.push(ret_stmt);
    }

    lines.push("end fun".to_string());
    lines.join("\n")
}
