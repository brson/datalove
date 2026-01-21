//! Script fragment generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, ModuleInfo};
use crate::gen_stmt::gen_body_statement;
use crate::gen_expr::gen_expr;

/// Generate a script fragment that uses module functions.
pub fn gen_script<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    modules: &[ModuleInfo<'db>],
) -> String {
    let mut lines = Vec::new();

    // Build script context with imports.
    let mut ctx = GenContext::new();

    // Import functions from modules.
    for module in modules {
        if !module.functions.is_empty() {
            lines.push(format!("require module {}", module.path()));

            for func in &module.functions {
                lines.push(format!("import {}.{}", module.alias(), func.name));
                ctx.imported_functions.push(func.clone());
            }
            lines.push(String::new());
        }
    }

    // Generate script statements.
    let mut var_counter = 0;
    let stmt_count = rng.gen_range(config.script_statements.0..=config.script_statements.1);

    for _ in 0..stmt_count {
        let stmt = gen_body_statement(db, rng, config, &mut ctx, &mut var_counter, "");
        lines.push(stmt);
    }

    // Generate a final expression using a variable or function call.
    if !ctx.variables.is_empty() && rng.gen_bool(0.7) {
        let var = &ctx.variables[rng.gen_range(0..ctx.variables.len())];
        lines.push(String::new());
        // Use debuglog to output the value.
        lines.push(format!("debuglog {}", var.name));
    } else if !ctx.imported_functions.is_empty() {
        let func = &ctx.imported_functions[rng.gen_range(0..ctx.imported_functions.len())];
        if let Some(ret_ty) = &func.return_type {
            let call = gen_expr(db, rng, ret_ty.clone(), config, &ctx);
            lines.push(String::new());
            lines.push(format!("debuglog {}", call));
        }
    }

    lines.join("\n")
}
