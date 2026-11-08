//! Script fragment generation.

use rmx::prelude::*;
use bct::lookup::{LookupMap, LookupSet};

use bct::random::SeededRng;
use datalove_datalit::ast::TypeHint;
use crate::config::WorldGenConfig;
use crate::context::{ConstDef, FunctionSig, GenContext, ModuleInfo, Variable, is_linear_type};
use crate::gen_function::{gen_function, gen_function_signature};
use crate::gen_stmt::gen_body_statement;
use crate::gen_expr::{can_call, gen_bool_expr, gen_expr, gen_function_call, gen_literal};
use crate::gen_type::gen_type_hint_and_spelling;
use crate::pretty::pretty_type_hint;

/// A top-level item of a generated script.
pub struct ScriptItem {
    pub text: String,
    /// Whether it is an expression, which only a unit of its own can be.
    pub expression: bool,
}

/// A generated script.
pub struct GenScript<'db> {
    /// Its top-level items, in order.
    pub items: Vec<ScriptItem>,
    /// The bindings its top level makes.
    pub variables: Vec<Variable<'db>>,
    /// The data files it requires, as `library/package/name` paths and
    /// their text.
    pub data: Vec<(String, String)>,
}

/// Generate a script as its top-level items.
///
/// An item is a statement, a `require`, an `import`, or an expression for a
/// unit of its own. Each function a
/// statement calls is imported somewhere before that statement and after the
/// `require` of its module, which goes somewhere before the import, so they
/// stand among the statements rather than all at the top. Cut into script
/// units, a `require`, its imports and their callers then fall into units of
/// their own as often as not.
pub fn gen_script_items<'db>(
    db: &'db dyn salsa::Database,
    rng: &mut SeededRng,
    config: &WorldGenConfig,
    modules: &[ModuleInfo<'db>],
) -> GenScript<'db> {
    let mut ctx = GenContext::new();
    let mut data = Vec::new();
    let mut quotients = Vec::new();

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

    let mut items: Vec<ScriptItem> = Vec::new();
    let mut imported: LookupSet<String> = LookupSet::default();
    let fragment = |text: String| ScriptItem { text, expression: false };
    let mut push = |rng: &mut SeededRng, items: &mut Vec<ScriptItem>, ctx: &mut GenContext<'db>, item: ScriptItem| {
        // A module's function, not yet imported; a function the script
        // declares is called by name.
        let mut called: Vec<String> = ctx.called.drain_unordered()
            .filter(|name| sources.contains_key(name) && !imported.contains(name))
            .collect();
        // In the order they were first called would be as good, but a set
        // has no order, and the output has to follow from the seed.
        called.sort();
        for name in called {
            imported.insert(name.C());
            let module = sources[&name];
            let require = format!("require module {}", module.path());
            let required = match items.iter().position(|item| item.text == require) {
                Some(at) => at,
                None => {
                    let at = rng.random_range(0..=items.len());
                    items.insert(at, fragment(require));
                    at
                }
            };
            let at = rng.random_range(required + 1..=items.len());
            items.insert(at, fragment(format!("import {}.{}", module.alias(), name)));
        }
        items.push(item);
    };

    // Generate script statements.
    let mut var_counter = 0;
    let stmt_count = rng.random_range(config.script_statements.0..=config.script_statements.1);

    for _ in 0..stmt_count {
        if config.expression_unit_probability > 0
            && config.check_probability(rng, config.expression_unit_probability)
        {
            let text = gen_expression_unit(db, rng, config, &mut ctx);
            push(rng, &mut items, &mut ctx, ScriptItem { text, expression: true });
            continue;
        }
        let access = config.binding_access_probability > 0
            && config.check_probability(rng, config.binding_access_probability);
        let stmt = match access {
            true => gen_binding_access(db, rng, config, &mut ctx, &mut var_counter, &mut data, &mut quotients),
            false => gen_body_statement(db, rng, config, &mut ctx, &mut var_counter, ""),
        };
        push(rng, &mut items, &mut ctx, fragment(stmt));
    }

    // Generate a final expression using a variable or function call.
    // Filter to only variables that haven't been consumed (moved).
    let live_vars: Vec<_> = ctx.variables.iter()
        .filter(|v| !ctx.is_consumed(&v.name))
        .collect();
    if !live_vars.is_empty() && rng.random_bool(0.7) {
        let var = live_vars[rng.random_range(0..live_vars.len())];
        // Use debuglog to output the value.
        let stmt = format!("debuglog {}", var.name);
        push(rng, &mut items, &mut ctx, fragment(stmt));
    } else if !ctx.imported_functions.is_empty() {
        let func = ctx.imported_functions[rng.random_range(0..ctx.imported_functions.len())].C();
        if let Some(ret_ty) = func.return_type {
            // Bound to a name carrying the type before it is printed. Some
            // expressions cannot say their own type -- `er error 100` does not
            // know what its ok side is -- and the binding is what tells them.
            let call = gen_expr(db, rng, ret_ty.C(), config, &mut ctx);
            let name = format!("v{}", var_counter);
            let stmt = format!("let {}: {} = {}\ndebuglog {}", name, pretty_type_hint(db, ret_ty), call, name);
            push(rng, &mut items, &mut ctx, fragment(stmt));
        }
    }

    GenScript { items, variables: ctx.variables, data }
}

/// An expression unit's code: the name of a live binding, which the unit
/// shows, read outside its code, a const's name cloned, since a const is
/// borrowed wherever it is named and the unit's value is moved out, a call
/// of a function the script declared, which an expression unit, a unit of
/// its own, always makes from another unit, or an expression of a simple
/// type, which may read some.
fn gen_expression_unit<'db>(
    db: &'db dyn salsa::Database,
    rng: &mut SeededRng,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
) -> String {
    let named: Vec<String> = ctx.variables.iter()
        .filter(|v| !ctx.is_consumed(&v.name))
        .map(|v| v.name.C())
        .chain(ctx.consts.iter().map(|c| format!("{}@", c.name)))
        .collect();
    if !named.is_empty() && rng.random_bool(0.5) {
        return named[rng.random_range(0..named.len())].C();
    }
    let callable: Vec<FunctionSig<'db>> = ctx.functions.iter()
        .filter(|func| func.return_type.is_some() && can_call(db, func, ctx))
        .cloned()
        .collect();
    if !callable.is_empty() && rng.random_bool(0.5) {
        let func = &callable[rng.random_range(0..callable.len())];
        return gen_function_call(db, rng, func, config, ctx);
    }
    let type_hint = match rng.random_range(0..3) {
        0 => TypeHint::Int,
        1 => TypeHint::I64,
        _ => TypeHint::Bool,
    };
    gen_expr(db, rng, type_hint, config, ctx)
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
/// - a read of such a quotient, `let v: int = q + <literal>`, which a unit
///   of its own is blocked by when the quotient's unit returns early;
/// - a function, `fun sfN(...)`, whose body may read consts and call the
///   functions declared before it, and a call of one, which a later unit
///   compiles against;
/// - a data require, `require data local/gen/xN: <type>` of a simple type,
///   whose file, a literal of the type, goes in `data`, for a test to edit.
fn gen_binding_access<'db>(
    db: &'db dyn salsa::Database,
    rng: &mut SeededRng,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    data: &mut Vec<(String, String)>,
    quotients: &mut Vec<String>,
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
    let live_quotients: Vec<String> = quotients.iter().filter(|name| !ctx.is_consumed(name)).map(|name| name.C()).collect();
    if !live_quotients.is_empty() {
        kinds.extend([9, 9]);
    }
    if !writable.is_empty() {
        kinds.extend([0, 1]);
    }
    // Takes twice and revives three times, and a take is of a `var` half
    // the time it can be: a revive needs a take of a `var` before it, which
    // is rare enough that a run of 20 seeds can otherwise make none.
    if !takable.is_empty() {
        kinds.extend([2, 2]);
    }
    if !revivable.is_empty() {
        kinds.extend([3, 3, 3]);
    }
    if !ctx.variables.is_empty() {
        kinds.push(4);
    }
    kinds.extend([10, 10]);
    let callable: Vec<FunctionSig<'db>> = ctx.functions.iter()
        .filter(|func| can_call(db, func, ctx))
        .cloned()
        .collect();
    if !callable.is_empty() {
        kinds.extend([11, 11, 11]);
    }
    let pick = |rng: &mut SeededRng, from: &[Variable<'db>]| from[rng.random_range(0..from.len())].C();

    match kinds[rng.random_range(0..kinds.len())] {
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
            let vars: Vec<Variable<'db>> = takable.iter().filter(|v| v.is_mutable).cloned().collect();
            let v = match !vars.is_empty() && rng.random_bool(0.5) {
                true => pick(rng, &vars),
                false => pick(rng, &takable),
            };
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
            format!("var {}: int = {}", name, rng.random_range(1..10))
        }
        7 => {
            let d = pick(rng, &divisors);
            let name = format!("v{}", *var_counter);
            *var_counter += 1;
            ctx.variables.push(Variable { name: name.C(), type_hint: TypeHint::Int, is_mutable: false });
            quotients.push(name.C());
            format!("let {}: int = {} /! {}", name, rng.random_range(0..200), d.name)
        }
        9 => {
            let q = live_quotients[rng.random_range(0..live_quotients.len())].C();
            let name = format!("v{}", *var_counter);
            *var_counter += 1;
            ctx.variables.push(Variable { name: name.C(), type_hint: TypeHint::Int, is_mutable: false });
            format!("let {}: int = {} + {}", name, q, rng.random_range(0..10))
        }
        8 => {
            let name = format!("x{}", *var_counter);
            *var_counter += 1;
            let type_hint = match rng.random_range(0..4) {
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
        10 => {
            // Its body may name the consts so far and call the functions the
            // script declared so far, not a module's, whose import the body
            // would not bring along. Not the script's own consts, `CN`: the
            // body numbers its consts from `C0`, which would shadow them.
            let name = format!("sf{}", ctx.functions.len());
            let sig = gen_function_signature(db, rng, &name, config);
            let mut scope = GenContext::new();
            scope.consts = ctx.consts.iter().filter(|c| !c.name.starts_with('C')).cloned().collect();
            scope.functions = ctx.functions.C();
            let source = gen_function(db, rng, &sig, config, &scope);
            ctx.functions.push(sig);
            source
        }
        11 => {
            let func = callable[rng.random_range(0..callable.len())].C();
            let call = gen_function_call(db, rng, &func, config, ctx);
            match func.return_type {
                Some(ret) => {
                    let name = format!("v{}", *var_counter);
                    *var_counter += 1;
                    let ty = pretty_type_hint(db, ret.C());
                    ctx.variables.push(Variable { name: name.C(), type_hint: ret, is_mutable: false });
                    format!("let {}: {} = {}", name, ty, call)
                }
                None => format!("call {}", call),
            }
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
