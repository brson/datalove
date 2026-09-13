//! Script fragment generation.

use std::collections::HashSet;

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::{GenContext, ModuleInfo};
use crate::gen_stmt::gen_body_statement;
use crate::gen_expr::gen_expr;
use crate::pretty::pretty_type_hint;

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
    //
    // A name is imported once: every module names its functions `fn0`, `fn1`
    // and so on, so two of them export the same name, and a second import
    // under a name already bound is an error rather than a shadowing. The
    // module generator has kept to this; the script did not.
    let mut imported_names: HashSet<String> = HashSet::new();
    for module in modules {
        if !module.functions.is_empty() || !module.generics.is_empty() {
            let mut required = false;
            for func in &module.functions {
                if !imported_names.insert(func.name.clone()) {
                    continue;
                }
                if !required {
                    lines.push(format!("require module {}", module.path()));
                    required = true;
                }
                lines.push(format!("import {}.{}", module.alias(), func.name));
                ctx.imported_functions.push(func.clone());
            }
            // The generics too, under the same one-name-once rule.
            for sig in &module.generics {
                if !imported_names.insert(sig.name.clone()) {
                    continue;
                }
                if !required {
                    lines.push(format!("require module {}", module.path()));
                    required = true;
                }
                lines.push(format!("import {}.{}", module.alias(), sig.name));
                ctx.generic_functions.push(sig.clone());
            }
            if required {
                lines.push(String::new());
            }
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
    // Filter to only variables that haven't been consumed (moved).
    let live_vars: Vec<_> = ctx.variables.iter()
        .filter(|v| !ctx.is_consumed(&v.name))
        .collect();
    if !live_vars.is_empty() && rng.gen_bool(0.7) {
        let var = live_vars[rng.gen_range(0..live_vars.len())];
        lines.push(String::new());
        // Use debuglog to output the value.
        lines.push(format!("debuglog {}", var.name));
    } else if !ctx.imported_functions.is_empty() {
        let func = ctx.imported_functions[rng.gen_range(0..ctx.imported_functions.len())].clone();
        if let Some(ret_ty) = func.return_type {
            // Bound to a name carrying the type before it is printed. Some
            // expressions cannot say their own type -- `er error 100` does not
            // know what its ok side is -- and the binding is what tells them.
            let call = gen_expr(db, rng, ret_ty.clone(), config, &mut ctx);
            let name = format!("v{}", var_counter);
            lines.push(String::new());
            lines.push(format!(
                "let {}: {} = {}", name, pretty_type_hint(db, ret_ty), call));
            lines.push(format!("debuglog {}", name));
        }
    }

    lines.join("\n")
}
