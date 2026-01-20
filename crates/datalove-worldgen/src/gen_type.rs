//! Type hint generation using datalit's type system.

use rand::Rng;
use datalove_datalit::ast::{Heap, TypeHint, TypeHintAndHeap};
use datalove_datalit::ast_gen::{self, AstGenConfig, HeapDistribution, TypeWeights};
use crate::config::WorldGenConfig;
use crate::pretty::pretty_type_hint_and_heap;

/// Generate a type hint with heap annotation.
///
/// Uses datalit's type generation with a config suitable for worldgen.
pub fn gen_type_hint_with_heap<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    config: &WorldGenConfig,
) -> TypeHintAndHeap<'db> {
    // Use leaf-only types for function signatures to ensure reliable typechecking.
    let mut type_config = config.type_config.clone();
    type_config.type_weights = TypeWeights::leaf_only();
    // Favor local heap for simplicity.
    type_config.heap_distribution = HeapDistribution {
        local: 7,
        global: 2,
        omitted: 1,
    };

    ast_gen::gen_type_hint_and_heap(db, rng, &type_config, 0)
}

/// Generate a type alias definition.
///
/// Returns the TypeAlias name and its underlying type.
pub fn gen_type_alias<'db, R: Rng>(
    db: &'db dyn salsa::Database,
    rng: &mut R,
    name: &str,
    _config: &WorldGenConfig,
) -> (String, TypeHintAndHeap<'db>) {
    // Generate a primitive type for the alias to ensure reliable typechecking.
    let mut type_config = AstGenConfig::default();
    type_config.type_weights = TypeWeights::leaf_only();
    type_config.heap_distribution = HeapDistribution {
        local: 10,
        global: 0,
        omitted: 0,
    };

    let type_hint = ast_gen::gen_type_hint_and_heap(db, rng, &type_config, 0);
    (name.to_string(), type_hint)
}

/// Format a type alias definition for output.
pub fn format_type_alias<'db>(
    db: &'db dyn salsa::Database,
    name: &str,
    type_hint: TypeHintAndHeap<'db>,
) -> String {
    format!("type {}: {}", name, pretty_type_hint_and_heap(db, type_hint))
}

/// Generate a bool type with local heap.
pub fn gen_bool_type<'db>(db: &'db dyn salsa::Database) -> TypeHintAndHeap<'db> {
    TypeHintAndHeap::new(db, Heap::Local, TypeHint::Bool)
}

/// Generate a u32 type with local heap.
pub fn gen_u32_type<'db>(db: &'db dyn salsa::Database) -> TypeHintAndHeap<'db> {
    TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use datalove_datalit::Database;
    use rand::SeedableRng;

    #[test]
    fn test_gen_bool_type() {
        let db = Database::default();
        // Must call within tracked function context.
        test_gen_bool_type_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_bool_type_inner<'db>(db: &'db dyn salsa::Database) {
        let ty = gen_bool_type(db);
        assert_eq!(ty.heap(db), Heap::Local);
        assert!(matches!(ty.type_hint(db), TypeHint::Bool));
    }

    #[test]
    fn test_gen_u32_type() {
        let db = Database::default();
        test_gen_u32_type_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_u32_type_inner<'db>(db: &'db dyn salsa::Database) {
        let ty = gen_u32_type(db);
        assert_eq!(ty.heap(db), Heap::Local);
        assert!(matches!(ty.type_hint(db), TypeHint::U32));
    }

    #[test]
    fn test_gen_type_hint_with_heap_produces_leaf_types() {
        let db = Database::default();
        test_gen_type_hint_leaf_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_type_hint_leaf_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // Generate 100 types and verify they're all leaf types.
        for _ in 0..100 {
            let ty = gen_type_hint_with_heap(db, &mut rng, &config);
            let type_hint = ty.type_hint(db);

            // Verify it's a leaf type (no containers).
            let is_leaf = matches!(
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
                    | TypeHint::Usize
                    | TypeHint::Isize
                    | TypeHint::F32
                    | TypeHint::F64
                    | TypeHint::Int
                    | TypeHint::String
            );
            assert!(is_leaf, "Expected leaf type");
        }
    }

    #[test]
    fn test_gen_type_hint_heap_distribution() {
        let db = Database::default();
        test_gen_type_hint_heap_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_type_hint_heap_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut rng = rand::rngs::StdRng::seed_from_u64(123);

        let mut local_count = 0;
        let mut global_count = 0;
        let mut omitted_count = 0;

        // Generate 1000 types and check heap distribution.
        for _ in 0..1000 {
            let ty = gen_type_hint_with_heap(db, &mut rng, &config);
            match ty.heap(db) {
                Heap::Local => local_count += 1,
                Heap::Global => global_count += 1,
                Heap::Omitted => omitted_count += 1,
            }
        }

        // Local should be most common (70% target).
        assert!(local_count > global_count, "Local heap should be more common than global");
        assert!(local_count > omitted_count, "Local heap should be more common than omitted");
        // Global should be second (20% target).
        assert!(global_count > omitted_count / 2, "Global heap should be reasonably common");
    }

    #[test]
    fn test_gen_type_alias() {
        let db = Database::default();
        test_gen_type_alias_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_type_alias_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        let (name, type_hint) = gen_type_alias(db, &mut rng, "MyType", &config);
        assert_eq!(name, "MyType");
        // Type alias should have local heap.
        assert_eq!(type_hint.heap(db), Heap::Local);
        // Type should be a leaf type.
        assert!(matches!(
            type_hint.type_hint(db),
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

    #[salsa::tracked]
    fn test_format_type_alias_inner<'db>(db: &'db dyn salsa::Database) {
        let ty = TypeHintAndHeap::new(db, Heap::Local, TypeHint::U32);
        let formatted = format_type_alias(db, "Counter", ty);
        assert_eq!(formatted, "type Counter: u32");
    }

    #[test]
    fn test_gen_type_deterministic() {
        let db = Database::default();
        test_gen_type_deterministic_inner(&db);
    }

    #[salsa::tracked]
    fn test_gen_type_deterministic_inner<'db>(db: &'db dyn salsa::Database) {
        let config = WorldGenConfig::default();

        // Same seed should produce structurally identical types.
        let mut rng1 = rand::rngs::StdRng::seed_from_u64(42);
        let mut rng2 = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..10 {
            let ty1 = gen_type_hint_with_heap(db, &mut rng1, &config);
            let ty2 = gen_type_hint_with_heap(db, &mut rng2, &config);
            // Compare pretty-printed output for determinism.
            let s1 = pretty_type_hint_and_heap(db, ty1);
            let s2 = pretty_type_hint_and_heap(db, ty2);
            assert_eq!(s1, s2, "Same seed should produce same types");
        }
    }
}
