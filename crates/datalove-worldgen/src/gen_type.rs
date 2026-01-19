//! Type hint generation.

use rand::Rng;
use crate::config::WorldGenConfig;
use crate::context::GenContext;

/// Primitive types for simple generation.
/// Note: f64 is excluded because float literals default to f32 without explicit type hints.
const PRIMITIVE_TYPES: &[&str] = &[
    "bool", "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "f32", "string",
];

/// Generate a random primitive type.
pub fn gen_primitive_type<R: Rng>(rng: &mut R) -> String {
    PRIMITIVE_TYPES[rng.gen_range(0..PRIMITIVE_TYPES.len())].to_string()
}

/// Generate a type hint string suitable for function parameters/returns.
///
/// For simplicity, we focus on primitive types to ensure reliable type-checking.
/// Composite types are disabled for now to avoid synthesis issues.
pub fn gen_type_hint<R: Rng>(
    rng: &mut R,
    _config: &WorldGenConfig,
    ctx: &GenContext,
    _depth: usize,
) -> String {
    // Maybe use a type alias if available (but only at top level, not for function signatures
    // since type aliases need to be resolved).
    if !ctx.type_aliases.is_empty() && rng.gen_bool(0.2) {
        let alias = &ctx.type_aliases[rng.gen_range(0..ctx.type_aliases.len())];
        return alias.name.clone();
    }

    // Stick to primitive types for now - they always typecheck correctly.
    gen_primitive_type(rng)
}

/// Generate a type hint with heap annotation.
pub fn gen_type_hint_with_heap<R: Rng>(
    rng: &mut R,
    config: &WorldGenConfig,
    ctx: &GenContext,
    depth: usize,
) -> String {
    let heap = gen_heap(rng);
    let ty = gen_type_hint(rng, config, ctx, depth);
    format!("{}{}", heap, ty)
}

/// Generate a heap annotation.
pub fn gen_heap<R: Rng>(rng: &mut R) -> &'static str {
    // Favor local heap for simplicity.
    let choice = rng.gen_range(0..10);
    match choice {
        0..=6 => "@",  // 70% local
        7..=8 => "#",  // 20% global
        _ => "",       // 10% inferred
    }
}

/// Generate a type alias definition.
///
/// For now, type aliases just wrap primitive types to ensure correct typechecking.
pub fn gen_type_alias<R: Rng>(
    rng: &mut R,
    name: &str,
    _config: &WorldGenConfig,
) -> String {
    // For now, just alias primitive types to ensure reliable typechecking.
    let ty = format!("@{}", gen_primitive_type(rng));
    format!("type {}: {}", name, ty)
}
