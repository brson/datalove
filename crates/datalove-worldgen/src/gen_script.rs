//! Script fragment generation.

use rmx::prelude::*;
use bct::lookup::{LookupMap, LookupSet};

use rand::Rng;
use datalove_datalit::ast::TypeHint;
use crate::config::WorldGenConfig;
use crate::context::{ConstDef, GenContext, ModuleInfo, Variable, is_linear_type};
use crate::gen_stmt::gen_body_statement;
use crate::gen_expr::{gen_bool_expr, gen_expr, gen_literal};
use crate::gen_type::gen_type_hint_and_spelling;
use crate::pretty::pretty_type_hint;

/// A generated script.
pub struct GenScript<'db> {
    /// Its top-level items, in order.
    pub items: Vec<String>,
    /// The bindings its top level makes.
    pub variables: Vec<Variable<'db>>,
    /// The data files it requires, as `library/package/name` paths and
    /// their text.
    pub data: Vec<(String, String)>,
}

/// Generate a script as its top-level items.
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
) -> GenScript<'db> {
    let mut ctx = GenContext::new();
    let mut data = Vec::new();

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
        let access = config.binding_access_probability > 0
            && config.check_probability(rng, config.binding_access_probability);
        let stmt = match access {
            true => gen_binding_access(db, rng, config, &mut ctx, &mut var_counter, &mut data),
            false => gen_body_statement(db, rng, config, &mut ctx, &mut var_counter, ""),
        };
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

    GenScript { items, variables: ctx.variables, data }
}

/// A statement that makes a binding for later statements to act on, or acts
/// on one an earlier statement made.
///
/// Each writes a literal, which reads nothing, so it compiles wherever the
/// binding it names is in scope:
///
/// - a `var` of a type picked at random, since few other statements make
///   one, and a `var` is what the rest write;
/// - an overwrite, `set v = <literal>`, of a `var`;
/// - an update, the same under an `if`, since a write that may not happen
///   keeps what the binding held;
/// - a take, `let w = v`, of a binding of a type that is moved rather than
///   copied;
/// - a revive, `set v = <literal>`, of a `var` taken before;
/// - a shadow, `let v` or `var v` again, of the same type, which the
///   statements after it find instead;
/// - a divisor, `var dN: int` of a small literal, named so that a test can
///   find it and set it to 0;
/// - a quotient by one, `let v: int = <literal> /! dN`, which returns early
///   from its unit when the divisor is 0, and blocks the units that read
///   what it binds;
/// - a data require, `require data local/gen/xN: <type>` of a simple type,
///   whose file, a literal of the type, goes in `data`, for a test to edit.
fn gen_binding_access<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    data: &mut Vec<(String, String)>,
) -> String {
    let live = |ctx: &GenContext<'db>, v: &Variable<'db>| !ctx.is_consumed(&v.name);
    let writable: Vec<Variable<'db>> = ctx.variables.iter()
        .filter(|v| v.is_mutable && live(ctx, v))
        .cloned()
        .collect();
    let takable: Vec<Variable<'db>> = ctx.variables.iter()
        .filter(|v| is_linear_type(&v.type_hint) && live(ctx, v) && !ctx.is_loop_protected(&v.name))
        .cloned()
        .collect();
    let revivable: Vec<Variable<'db>> = ctx.variables.iter()
        .filter(|v| v.is_mutable && !live(ctx, v))
        .cloned()
        .collect();

    let divisors: Vec<Variable<'db>> = ctx.variables.iter()
        .filter(|v| v.name.starts_with('d') && live(ctx, v))
        .cloned()
        .collect();
    let mut kinds: Vec<u8> = vec![5, 6, 8];
    if !divisors.is_empty() {
        kinds.extend([7, 7]);
    }
    if !writable.is_empty() {
        kinds.extend([0, 1]);
    }
    // Takes and revives twice: a revive needs a take of a `var` before it,
    // which is rare enough that a run of 20 seeds can otherwise make none.
    if !takable.is_empty() {
        kinds.extend([2, 2]);
    }
    if !revivable.is_empty() {
        kinds.extend([3, 3]);
    }
    if !ctx.variables.is_empty() {
        kinds.push(4);
    }
    let pick = |rng: &mut R, from: &[Variable<'db>]| from[rng.gen_range(0..from.len())].C();

    match kinds[rng.gen_range(0..kinds.len())] {
        0 => {
            let v = pick(rng, &writable);
            format!("set {} = {}", v.name, gen_literal(db, rng, v.type_hint, config))
        }
        1 => {
            let v = pick(rng, &writable);
            let condition = gen_bool_expr(db, rng, config, ctx);
            let value = gen_literal(db, rng, v.type_hint, config);
            format!("if {}\n  set {} = {}\nend if", condition, v.name, value)
        }
        2 => {
            let v = pick(rng, &takable);
            let name = format!("v{}", *var_counter);
            *var_counter += 1;
            let ty = pretty_type_hint(db, v.type_hint.C());
            ctx.consume_variable(&v.name);
            ctx.variables.push(Variable { name: name.C(), type_hint: v.type_hint, is_mutable: false });
            format!("let {}: {} = {}", name, ty, v.name)
        }
        3 => {
            let v = pick(rng, &revivable);
            ctx.revive_variable(&v.name);
            format!("set {} = {}", v.name, gen_literal(db, rng, v.type_hint, config))
        }
        6 => {
            let name = format!("d{}", *var_counter);
            *var_counter += 1;
            ctx.variables.push(Variable { name: name.C(), type_hint: TypeHint::Int, is_mutable: true });
            format!("var {}: int = {}", name, rng.gen_range(1..10))
        }
        7 => {
            let d = pick(rng, &divisors);
            let name = format!("v{}", *var_counter);
            *var_counter += 1;
            ctx.variables.push(Variable { name: name.C(), type_hint: TypeHint::Int, is_mutable: false });
            format!("let {}: int = {} /! {}", name, rng.gen_range(0..200), d.name)
        }
        8 => {
            let name = format!("x{}", *var_counter);
            *var_counter += 1;
            let type_hint = match rng.gen_range(0..4) {
                0 => TypeHint::Int,
                1 => TypeHint::I64,
                2 => TypeHint::Bool,
                _ => TypeHint::String,
            };
            let ty = pretty_type_hint(db, type_hint.C());
            let path = format!("local/gen/{}", name);
            data.push((path.C(), gen_literal(db, rng, type_hint.C(), config)));
            // A data binding is a const, read wherever it is named.
            ctx.consts.push(ConstDef { name, type_hint });
            format!("require data {}: {}", path, ty)
        }
        4 => {
            let v = pick(rng, &ctx.variables.C());
            ctx.revive_variable(&v.name);
            let keyword = if v.is_mutable { "var" } else { "let" };
            let ty = pretty_type_hint(db, v.type_hint.C());
            format!("{} {}: {} = {}", keyword, v.name, ty, gen_literal(db, rng, v.type_hint, config))
        }
        _ => {
            let name = format!("v{}", *var_counter);
            *var_counter += 1;
            let (type_hint, ty) = gen_type_hint_and_spelling(db, rng, config, &ctx.type_aliases);
            let value = gen_literal(db, rng, type_hint.C(), config);
            ctx.variables.push(Variable { name: name.C(), type_hint, is_mutable: true });
            format!("var {}: {} = {}", name, ty, value)
        }
    }
}
