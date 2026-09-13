//! Type hint generation using datalit's type system.

use rand::Rng;
use datalove_datalit::ast::TypeHint;
use datalove_datalit::ast_gen;
use crate::config::WorldGenConfig;
use crate::pretty::pretty_type_hint;

/// Generate a type hint.
///
/// Uses datalit's type generation with a config suitable for worldgen.
pub fn gen_type_hint<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> TypeHint<'db> {
    // Whatever the config asks for. This forced leaf-only types once, to work
    // around a one-element tuple printed as `(x)` rather than `(x,)`, which
    // reads as a bracketed expression and did not match its own type. Two
    // shapes of tensor were held back after that, until the stack slots they
    // wanted stopped being declared with an alignment four gigabytes wide.
    // Nothing is held back now.
    ast_gen::gen_type_hint(db, rng, &config.type_config, 0)
}

/// Pick a type, and how to write it.
///
/// A type alias is another name for a type, and the generator declared plenty
/// of them and never once wrote one down -- `type_alias_usage_probability` was
/// in the config and read by nothing. So a declaration meant nothing and the
/// resolver's work on them was never reached.
///
/// The type that comes back is the structural one, because that is what the
/// value has to be built from and what later statements match against. Only
/// the way it is written changes.
pub fn gen_type_hint_and_spelling<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
    aliases: &[crate::context::TypeAlias<'db>],
) -> (TypeHint<'db>, String) {
    if !aliases.is_empty() && config.check_probability(rng, config.type_alias_usage_probability) {
        let alias = &aliases[rng.gen_range(0..aliases.len())];
        return (alias.type_hint.clone(), alias.name.clone());
    }
    let type_hint = gen_type_hint(db, rng, config);
    let spelling = pretty_type_hint(db, type_hint.clone());
    (type_hint, spelling)
}

/// Generate a type alias definition.
///
/// Returns the TypeAlias name and its underlying type.
pub fn gen_type_alias<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    name: &str,
    _config: &WorldGenConfig,
) -> (String, TypeHint<'db>) {
    let type_config = ast_gen::AstGenConfig::default();
    let type_hint = ast_gen::gen_type_hint(db, rng, &type_config, 0);
    (name.to_string(), type_hint)
}

/// Format a type alias definition for output.
pub fn format_type_alias<'db>(
    db: &'db dyn salsa::Database,
    name: &str,
    type_hint: TypeHint<'db>,
) -> String {
    format!("type {}: {}", name, pretty_type_hint(db, type_hint))
}

/// Generate a bool type.
pub fn gen_bool_type<'db>(_db: &'db dyn salsa::Database) -> TypeHint<'db> {
    TypeHint::Bool
}

/// Generate a u32 type.
pub fn gen_u32_type<'db>(_db: &'db dyn salsa::Database) -> TypeHint<'db> {
    TypeHint::U32
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datalit::Database;
    use rand::SeedableRng;

    #[test]
    fn test_gen_bool_type() {
        let db = Database::default();
        let ty = gen_bool_type(&db);
        assert!(matches!(ty, TypeHint::Bool));
    }

    #[test]
    fn test_gen_u32_type() {
        let db = Database::default();
        let ty = gen_u32_type(&db);
        assert!(matches!(ty, TypeHint::U32));
    }

    #[test]
    fn test_gen_type_hint_follows_the_weights() {
        let db = Database::default();
        test_gen_type_hint_weights_inner(&db);
    }

    /// The weights are what decide, rather than anything forced here.
    ///
    /// This asked for leaf types once, because the generator pinned them
    /// whatever the config said. A signature holding a list or an option is
    /// the interesting case, so what is checked now is that both answers are
    /// reachable: leaf-only weights give leaf types, and the default weights
    /// give compound ones too.
    #[salsa::tracked(returns(copy))]
    fn test_gen_type_hint_weights_inner<'db>(db: &'db dyn salsa::Database) {
        fn is_leaf(ty: &TypeHint) -> bool {
            matches!(
                ty,
                TypeHint::Bool
                    | TypeHint::U8
                    | TypeHint::I8
                    | TypeHint::U16
                    | TypeHint::I16
                    | TypeHint::U32
                    | TypeHint::I32
                    | TypeHint::U64
                    | TypeHint::I64
                    | TypeHint::Index
                    | TypeHint::Offset
                    | TypeHint::F32
                    | TypeHint::F64
                    | TypeHint::Int
                    | TypeHint::String
            )
        }

        let mut leaf_config = WorldGenConfig::default();
        leaf_config.type_config.type_weights =
            datalove_datalit::ast_gen::TypeWeights::leaf_only();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        for _ in 0..100 {
            let type_hint = gen_type_hint(db, &mut rng, &leaf_config);
            assert!(is_leaf(&type_hint), "leaf-only weights gave a compound type");
        }

        let config = WorldGenConfig::default();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let compound = (0..100)
            .filter(|_| !is_leaf(&gen_type_hint(db, &mut rng, &config)))
            .count();
        assert!(compound > 0, "the default weights gave no compound type in 100");
    }

    #[test]
    fn test_gen_type_alias() {
        let db = Database::default();
        test_gen_type_alias_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_type_alias_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let (name, type_hint) = gen_type_alias(db, &mut rng, "MyType", &config);
        assert_eq!(name, "MyType");
        // Type should be a leaf type.
        assert!(matches!(
            type_hint,
            TypeHint::Bool
                | TypeHint::U8
                | TypeHint::I8
                | TypeHint::U16
                | TypeHint::I16
                | TypeHint::U32
                | TypeHint::I32
                | TypeHint::U64
                | TypeHint::I64
                | TypeHint::F32
                | TypeHint::F64
                | TypeHint::Int
                | TypeHint::String
        ));
    }

    #[test]
    fn test_format_type_alias() {
        let db = Database::default();
        test_format_type_alias_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_format_type_alias_inner<'db>(db: &'db dyn salsa::Database) {
        let ty = TypeHint::U32;
        let formatted = format_type_alias(db, "Counter", ty);
        assert_eq!(formatted, "type Counter: u32");
    }

    #[test]
    fn test_gen_type_deterministic() {
        let db = Database::default();
        test_gen_type_deterministic_inner(&db);
    }

    #[salsa::tracked(returns(copy))]
    fn test_gen_type_deterministic_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();

        // Same seed should produce structurally identical types.
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(42);
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..10 {
            let ty1 = gen_type_hint(db, &mut rng1, &config);
            let ty2 = gen_type_hint(db, &mut rng2, &config);
            // Compare pretty-printed output for determinism.
            let s1 = pretty_type_hint(db, ty1);
            let s2 = pretty_type_hint(db, ty2);
            assert_eq!(s1, s2, "Same seed should produce same types");
        }
    }
}
