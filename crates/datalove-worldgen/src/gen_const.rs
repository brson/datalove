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
/// The spec says a const holds anything a function can compute. Two things
/// still fall short of that, both written down in
/// `botdocs/reports/report-worldgen-coverage.md` with what they would take.
///
/// - A tensor cannot be read back out of the evaluated value, there being no
///   `ConstValue` that holds one.
/// - A collection nested inside anything -- `(u32, [u32])`, `?[u32]`, a list
///   of lists -- compiles under the interpreter and not under the cranelift
///   AOT, which writes a nested const value to an address and has no way to
///   write a collection there. A collection on its own is fine, having its own
///   path.
///
/// The first of those is why a `data`, an `error` and a result are kept out,
/// which is a weaker reason than it was. Reading those back used to be
/// impossible outright; it works now, and what stops them is only that each
/// carries a payload chosen by datalit's expression generator rather than
/// here, and that payload is sometimes a tensor. `error : [|int, 1|] / ...` is
/// a const the generator would otherwise write.
pub fn a_const_can_hold(ty: &TypeHint<'_>) -> bool {
    match ty {
        TypeHint::Tensor(_) | TypeHint::Data | TypeHint::Error | TypeHint::Result(_) => false,
        // A collection at the top of a const is fine; what it holds may not be
        // another one.
        TypeHint::List(t) => holds_no_collection(&t.element_type),
        TypeHint::Set(t) => holds_no_collection(&t.element_type),
        TypeHint::Map(t) => {
            holds_no_collection(&t.key_type) && holds_no_collection(&t.value_type)
        }
        TypeHint::Option(t) => holds_no_collection(&t.inner_type),
        TypeHint::AnonTuple(t) => t.fields.iter().all(holds_no_collection),
        TypeHint::AnonStruct(t) => t.fields.iter().all(|f| holds_no_collection(&f.type_hint)),
        _ => true,
    }
}

/// Whether a type can sit inside a const without a collection anywhere in it.
fn holds_no_collection(ty: &TypeHint<'_>) -> bool {
    match ty {
        TypeHint::Tensor(_)
        | TypeHint::List(_)
        | TypeHint::Set(_)
        | TypeHint::Map(_)
        | TypeHint::Data
        | TypeHint::Error
        | TypeHint::Result(_) => false,
        TypeHint::Option(t) => holds_no_collection(&t.inner_type),
        TypeHint::AnonTuple(t) => t.fields.iter().all(holds_no_collection),
        TypeHint::AnonStruct(t) => t.fields.iter().all(|f| holds_no_collection(&f.type_hint)),
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
