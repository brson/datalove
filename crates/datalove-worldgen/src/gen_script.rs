//! Script fragment generation.

use rmx::prelude::*;
use bct::lookup::{LookupMap, LookupSet};

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
    gen_script_items(db, rng, config, modules).join("\n")
}

/// Generate a script as its top-level items, in order.
///
/// An item is a statement, a `require` or an `import`. Each function a
/// statement calls is imported somewhere before that statement and after the
/// `require` of its module, which goes somewhere before the import, so they
/// stand among the statements rather than all at the top. Cut into script
/// units, a `require`, its imports and their callers then fall into units of
/// their own as often as not.
pub fn gen_script_items<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    modules: &[ModuleInfo<'db>],
) -> Vec<String> {
    let mut ctx = GenContext::new();

    // What may be imported, and from where.
    //
    // A name is imported once: every module names its functions `fn0`, `fn1`
    // and so on, so two of them export the same name, and a second import
    // under a name already bound is an error rather than a shadowing. The
    // module generator has kept to this; the script did not.
    let mut sources: LookupMap<String, &ModuleInfo<'db>> = LookupMap::default();
    for module in modules {
        for func in &module.functions {
            if !sources.contains_key(&func.name) {
                sources.insert(func.name.C(), module);
                ctx.imported_functions.push(func.C());
            }
        }
        // The generics too, under the same one-name-once rule.
        for sig in &module.generics {
            if !sources.contains_key(&sig.name) {
                sources.insert(sig.name.C(), module);
                ctx.generic_functions.push(sig.C());
            }
        }
    }

    let mut items: Vec<String> = Vec::new();
    let mut imported: LookupSet<String> = LookupSet::default();
    let mut push = |rng: &mut R, items: &mut Vec<String>, ctx: &mut GenContext<'db>, stmt: String| {
        let mut called: Vec<String> = ctx.called.drain_unordered()
            .filter(|name| !imported.contains(name))
            .collect();
        // In the order they were first called would be as good, but a set
        // has no order, and the output has to follow from the seed.
        called.sort();
        for name in called {
            imported.insert(name.C());
            let module = sources[&name];
            let require = format!("require module {}", module.path());
            let required = match items.iter().position(|item| *item == require) {
                Some(at) => at,
                None => {
                    let at = rng.gen_range(0..=items.len());
                    items.insert(at, require);
                    at
                }
            };
            let at = rng.gen_range(required + 1..=items.len());
            items.insert(at, format!("import {}.{}", module.alias(), name));
        }
        items.push(stmt);
    };

    // Generate script statements.
    let mut var_counter = 0;
    let stmt_count = rng.gen_range(config.script_statements.0..=config.script_statements.1);

    for _ in 0..stmt_count {
        let stmt = gen_body_statement(db, rng, config, &mut ctx, &mut var_counter, "");
        push(rng, &mut items, &mut ctx, stmt);
    }

    // Generate a final expression using a variable or function call.
    // Filter to only variables that haven't been consumed (moved).
    let live_vars: Vec<_> = ctx.variables.iter()
        .filter(|v| !ctx.is_consumed(&v.name))
        .collect();
    if !live_vars.is_empty() && rng.gen_bool(0.7) {
        let var = live_vars[rng.gen_range(0..live_vars.len())];
        // Use debuglog to output the value.
        let stmt = format!("debuglog {}", var.name);
        push(rng, &mut items, &mut ctx, stmt);
    } else if !ctx.imported_functions.is_empty() {
        let func = ctx.imported_functions[rng.gen_range(0..ctx.imported_functions.len())].C();
        if let Some(ret_ty) = func.return_type {
            // Bound to a name carrying the type before it is printed. Some
            // expressions cannot say their own type -- `er error 100` does not
            // know what its ok side is -- and the binding is what tells them.
            let call = gen_expr(db, rng, ret_ty.C(), config, &mut ctx);
            let name = format!("v{}", var_counter);
            let stmt = format!("let {}: {} = {}\ndebuglog {}", name, pretty_type_hint(db, ret_ty), call, name);
            push(rng, &mut items, &mut ctx, stmt);
        }
    }

    items
}
