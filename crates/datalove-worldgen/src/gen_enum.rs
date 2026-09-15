//! Enum generation, and the `match` that takes one apart.
//!
//! An enum is a closed union of atoms and terms, and `match` is the only way
//! to get at which one a value holds. Both are written per module, as a type
//! alias with a name, because a `match` has to name the variants of the type
//! it is matching and can only do that where the type is known.
//!
//! The variants are kept plain -- an atom, or a term over one generated type
//! -- because what the match has to get right is the discriminant and the
//! payload it names, not how elaborate the payload is.

use rand::Rng;

use datalove_datalit::ast::TypeHint;

use crate::config::WorldGenConfig;
use crate::context::{EnumDef, EnumVariant, GenContext, Variable};
use crate::gen_expr::gen_expr;
use crate::gen_type::gen_type_hint;
use crate::pretty::pretty_type_hint;

/// Choose the enums a module defines.
///
/// Named by module as well as by index, under the same rule as the generics:
/// a module may import one and define its own, and two of a name is an error
/// rather than a shadowing.
pub fn gen_module_enums<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    module_index: usize,
) -> Vec<EnumDef<'db>> {
    let count = rng.gen_range(config.enums_per_module.0..=config.enums_per_module.1);
    (0..count)
        .map(|i| {
            let name = format!("Enum{}_{}", module_index, i);
            let variant_count = rng.gen_range(2..=4);
            let variants = (0..variant_count)
                .map(|v| {
                    let variant_name = format!("{}V{}", name, v);
                    // A term as often as not, so both arms of a match are
                    // written and both ways of laying a variant out are built.
                    let payload = if rng.gen_bool(0.5) {
                        Some(gen_type_hint(db, rng, config))
                    } else {
                        Option::None
                    };
                    EnumVariant { name: variant_name, payload }
                })
                .collect();
            EnumDef { name, variants }
        })
        .collect()
}

/// Write an enum's declaration.
pub fn format_enum<'db>(db: &'db dyn salsa::Database, def: &EnumDef<'db>) -> String {
    let variants: Vec<String> = def
        .variants
        .iter()
        .map(|v| match &v.payload {
            Some(ty) => format!("term {} {}", v.name, pretty_type_hint(db, ty.clone())),
            Option::None => format!("atom {}", v.name),
        })
        .collect();
    format!("type {}: enum {{ {} }}", def.name, variants.join(", "))
}

/// Write a value of an enum type, and whatever has to be written before it.
///
/// There are two ways to say it and both are written. `(atom Red)@` widens the
/// variant into the enum, which is how the spec puts it. `enum { atom Red }`
/// says the same thing by naming the enum and letting the variant be checked
/// against it, and takes a different path through the compiler: the coercion
/// builds the variant, while the literal is the variant, checked.
///
/// A term's payload is bound to a name carrying its type first, for the same
/// reason a generic's arguments are: the payload has to be exactly the type
/// the variant declares, and an expression left to say its own type often says
/// a different one. An unadorned `1.5` is an `f64` where the variant wanted an
/// `f32`, and an empty `%{}` says nothing about what it holds at all.
pub fn gen_enum_value<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    def: &EnumDef<'db>,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> (Vec<String>, String) {
    let variant = &def.variants[rng.gen_range(0..def.variants.len())];
    let as_literal = rng.gen_bool(0.5);

    match &variant.payload {
        Some(ty) => {
            let name = format!("a{}", *var_counter);
            *var_counter += 1;
            let value = gen_expr(db, rng, ty.clone(), config, ctx);
            let bind = format!(
                "{}let {}: {} = {}",
                indent,
                name,
                pretty_type_hint(db, ty.clone()),
                value
            );
            let written = if as_literal {
                format!("enum {{ term {} {} }}", variant.name, name)
            } else {
                // Bracketed, since `term Name a -? b` reads the term as the
                // left side of the operator. Inside the braces of a literal
                // the payload is delimited already.
                format!("(term {} {})@", variant.name, name)
            };
            (vec![bind], written)
        }
        Option::None => {
            let written = if as_literal {
                format!("enum {{ atom {} }}", variant.name)
            } else {
                format!("(atom {})@", variant.name)
            };
            (Vec::new(), written)
        }
    }
}

/// Bind an atom or a term on its own, rather than widened into an enum.
///
/// An atom is both a type and a value, and a term is a name over a payload;
/// neither needs an enum to be written. Two atoms of the same name are the
/// same type, so the variant names are reused here -- what is bound is the
/// variant standing on its own.
pub fn gen_bare_variant_let<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    def: &EnumDef<'db>,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> (Vec<String>, String, String) {
    let variant = &def.variants[rng.gen_range(0..def.variants.len())];
    match &variant.payload {
        Some(ty) => {
            let written = pretty_type_hint(db, ty.clone());
            let name = format!("a{}", *var_counter);
            *var_counter += 1;
            let value = gen_expr(db, rng, ty.clone(), config, ctx);
            let bind = format!("{}let {}: {} = {}", indent, name, written, value);
            (
                vec![bind],
                format!("term {} {}", variant.name, written),
                format!("term {} {}", variant.name, name),
            )
        }
        Option::None => (
            Vec::new(),
            format!("atom {}", variant.name),
            format!("atom {}", variant.name),
        ),
    }
}

/// Write a `match` over something of an enum type in scope.
///
/// `None` when there is nothing to match on. The input is moved by the match,
/// so it has to be something this body is allowed to move, and it is consumed
/// before the arms are written.
pub fn gen_match<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> Option<String> {
    let candidates: Vec<(String, EnumDef<'db>)> = ctx
        .variables
        .iter()
        .filter(|v| !ctx.is_consumed(&v.name) && !ctx.is_loop_protected(&v.name))
        .filter_map(|v| match &v.type_hint {
            TypeHint::Alias(name) => ctx
                .enums
                .iter()
                .find(|e| e.name.as_str() == name.name.text(db))
                .map(|e| (v.name.clone(), e.clone())),
            _ => Option::None,
        })
        .collect();

    if candidates.is_empty() {
        return Option::None;
    }

    let (scrutinee, def) = candidates[rng.gen_range(0..candidates.len())].clone();
    ctx.consume_variable(&scrutinee);

    let mut lines = vec![format!("{}match {}", indent, scrutinee)];
    let inner_indent = format!("{}  ", indent);

    // An arm is a branch, and a branch may not move what was declared outside
    // it -- moving in one and not another is refused. The same rule an `if`
    // keeps, kept the same way.
    let saved_variables = ctx.variables.clone();
    let saved_consumed = ctx.consumed_variables.clone();
    let saved_protected = ctx.loop_protected_variables.clone();
    let outer_linear: Vec<String> = ctx
        .variables
        .iter()
        .filter(|v| crate::context::is_linear_type(&v.type_hint))
        .map(|v| v.name.clone())
        .collect();
    for name in &outer_linear {
        ctx.loop_protect_variable(name);
    }

    ctx.control_flow_depth += 1;

    // Either every variant, or some of them and a default. Without a default
    // the match has to be exhaustive, and a default with every variant already
    // written is refused as unreachable.
    let with_default = rng.gen_bool(0.3) && def.variants.len() > 1;
    let covered = if with_default {
        rng.gen_range(1..def.variants.len())
    } else {
        def.variants.len()
    };

    for variant in def.variants.iter().take(covered) {
        let branch_variables = ctx.variables.clone();
        let branch_consumed = ctx.consumed_variables.clone();

        match &variant.payload {
            Some(ty) => {
                let bound = format!("v{}", *var_counter);
                *var_counter += 1;
                lines.push(format!("{}case term {} {}", indent, variant.name, bound));
                ctx.variables.push(Variable {
                    name: bound,
                    type_hint: ty.clone(),
                    is_mutable: false,
                });
            }
            Option::None => lines.push(format!("{}case atom {}", indent, variant.name)),
        }

        lines.push(gen_arm_body(db, rng, config, ctx, var_counter, &inner_indent));

        ctx.variables = branch_variables;
        ctx.consumed_variables = branch_consumed;
    }

    if with_default {
        lines.push(format!("{}case default", indent));
        lines.push(gen_arm_body(db, rng, config, ctx, var_counter, &inner_indent));
    }

    ctx.control_flow_depth -= 1;
    ctx.variables = saved_variables;
    ctx.consumed_variables = saved_consumed;
    ctx.loop_protected_variables = saved_protected;

    lines.push(format!("{}end match", indent));
    Some(lines.join("\n"))
}

/// One or two statements for an arm, which must not be empty.
fn gen_arm_body<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &mut GenContext<'db>,
    var_counter: &mut usize,
    indent: &str,
) -> String {
    let count = rng.gen_range(1..=2);
    (0..count)
        .map(|_| crate::gen_stmt::gen_simple_statement(db, rng, config, ctx, var_counter, indent))
        .collect::<Vec<_>>()
        .join("\n")
}
