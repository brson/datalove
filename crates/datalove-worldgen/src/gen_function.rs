//! Function generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, FunctionSig, Variable};
use crate::gen_type::gen_type_hint_with_heap;
use crate::gen_stmt::{gen_body_statement, gen_ret};

/// Generate a function signature.
pub fn gen_function_signature<R: Rng>(
    rng: &mut R,
    name: &str,
    config: &WorldGenConfig,
    ctx: &GenContext,
) -> FunctionSig {
    // Generate 0-3 parameters.
    let param_count = rng.gen_range(0..=3);
    let params: Vec<(String, String)> = (0..param_count)
        .map(|i| {
            let param_name = format!("p{}", i);
            let param_type = gen_type_hint_with_heap(rng, config, ctx, 0);
            (param_name, param_type)
        })
        .collect();

    // Maybe generate a return type (80% of functions have returns).
    let return_type = if rng.gen_bool(0.8) {
        Some(gen_type_hint_with_heap(rng, config, ctx, 0))
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
pub fn gen_function<R: Rng>(
    rng: &mut R,
    sig: &FunctionSig,
    config: &WorldGenConfig,
    module_ctx: &GenContext,
) -> String {
    let mut lines = Vec::new();

    // Build signature line.
    let params_str = sig
        .params
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, ty))
        .collect::<Vec<_>>()
        .join(", ");

    let sig_line = match &sig.return_type {
        Some(ret_ty) => format!("fun {}({}): {}", sig.name, params_str, ret_ty),
        None => format!("fun {}({})", sig.name, params_str),
    };
    lines.push(sig_line);

    // Create function context with parameters.
    let mut ctx = GenContext::new();
    ctx.return_type = sig.return_type.clone();
    ctx.type_aliases = module_ctx.type_aliases.clone();
    ctx.functions = module_ctx.functions.clone();
    ctx.imported_functions = module_ctx.imported_functions.clone();

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
        let stmt = gen_body_statement(rng, config, &mut ctx, &mut var_counter, "  ");
        lines.push(stmt);
    }

    // Generate return statement.
    let ret_stmt = gen_ret(rng, config, &ctx, "  ");
    lines.push(ret_stmt);

    lines.push("end fun".to_string());
    lines.join("\n")
}

