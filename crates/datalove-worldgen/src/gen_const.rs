//! Consts, and the reading of them.
//!
//! A const binds a value the compiler works out, and it is the one name in the
//! language that a linear type can be read from twice: a const names a value
//! rather than a place, so each mention produces one of its own and no `@` is
//! wanted. Everything else the generator writes is moved on first use.
//!
//! What a const is bound to is written as a literal here. The rule is that it
//! may only name other consts -- a parameter or a `let` is an error even where
//! a const of that name is in scope -- and it may call functions, but a
//! module-level const that calls a function which itself names a module-level
//! const is a cycle the compiler has to refuse. A literal is clear of all of
//! that.

use rand::Rng;

use datalove_datalit::ast::TypeHint;
use datalove_datalit::ast_gen;

use crate::config::WorldGenConfig;
use crate::context::ConstDef;
use crate::gen_type::gen_type_hint;
use crate::pretty::{pretty_expr, pretty_type_hint};

/// Whether a const may hold this type.
///
/// Everything a const can be evaluated to and written back down as, which is
/// now everything but three, and for a different reason than before.
///
/// A `data` and an `error` carry their own descriptor at run time, and a
/// `ConstValue` does not keep it: `Data(Box<ConstValue>)` holds the value and
/// not its type. So a backend writing one has to work the type out from the
/// value, and a value cannot always say it -- an empty map gives
/// `Map(Unit, Unit)`, which is not a type the program has, and the cranelift
/// AOT has no descriptor for it: "TyDesc not found for inner type
/// Map(Unit, Unit)". A result goes with them, its error side being an `error`.
///
/// The fix is to keep the payload's type in the `ConstValue`, which is written
/// down in `botdocs/reports/report-worldgen-coverage.md`. Reading one back is
/// no longer the problem, and neither is a tensor.
pub fn a_const_can_hold(ty: &TypeHint<'_>) -> bool {
    match ty {
        TypeHint::Data | TypeHint::Error | TypeHint::Result(_) => false,
        TypeHint::List(t) => a_const_can_hold(&t.element_type),
        TypeHint::Set(t) => a_const_can_hold(&t.element_type),
        TypeHint::Map(t) => a_const_can_hold(&t.key_type) && a_const_can_hold(&t.value_type),
        TypeHint::Option(t) => a_const_can_hold(&t.inner_type),
        TypeHint::Tensor(t) => a_const_can_hold(&t.element_type),
        TypeHint::AnonTuple(t) => t.fields.iter().all(a_const_can_hold),
        TypeHint::AnonStruct(t) => t.fields.iter().all(|f| a_const_can_hold(&f.type_hint)),
        _ => true,
    }
}

/// A type a const can hold, asked for until one comes back.
pub fn gen_const_type<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> TypeHint<'db> {
    for _ in 0..8 {
        let candidate = gen_type_hint(db, rng, config);
        if a_const_can_hold(&candidate) {
            return candidate;
        }
    }
    // Gave up asking; a number is always acceptable.
    TypeHint::U32
}

/// Choose the consts a module defines.
///
/// A module-level const is in scope for every function in the module, wherever
/// it is written, which is a different resolution path from a const written
/// inside a body.
pub fn gen_module_consts<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    module_index: usize,
) -> Vec<ConstDef<'db>> {
    let count = rng.gen_range(config.consts_per_module.0..=config.consts_per_module.1);
    (0..count)
        .map(|i| ConstDef {
            name: format!("CONST{}_{}", module_index, i),
            type_hint: gen_const_type(db, rng, config),
        })
        .collect()
}

/// Write a const's declaration, with a literal for its value.
pub fn format_const<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    def: &ConstDef<'db>,
    config: &WorldGenConfig,
) -> String {
    format!(
        "const {}: {} = {}",
        def.name,
        pretty_type_hint(db, def.type_hint.clone()),
        gen_const_value(db, rng, def.type_hint.clone(), config)
    )
}

/// A value a const can be bound to: a literal, naming nothing.
pub fn gen_const_value<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    type_hint: TypeHint<'db>,
    config: &WorldGenConfig,
) -> String {
    let expr = ast_gen::gen_expr_matching_type(db, rng, type_hint.clone(), &config.type_config, 0);
    let written = pretty_expr(db, expr);

    // A fixed-width integer wants its type said, since a bare literal is an
    // `int`. The same rule the expression generator keeps.
    if needs_integer_type_hint(&type_hint) {
        format!(": {} / {}", pretty_type_hint(db, type_hint), written)
    } else {
        written
    }
}

/// Whether an integer literal of this type has to say which type it is.
fn needs_integer_type_hint(type_hint: &TypeHint<'_>) -> bool {
    matches!(
        type_hint,
        TypeHint::U8
            | TypeHint::I8
            | TypeHint::U16
            | TypeHint::I16
            | TypeHint::U32
            | TypeHint::I32
            | TypeHint::U64
            | TypeHint::I64
            | TypeHint::Index
            | TypeHint::Offset
    )
}
