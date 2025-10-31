use datalove_datalit::{ast_gen::*, Database};

#[test]
fn test_gen_expr_full_seeded() {
    let db = Database::default();
    let config = AstGenConfig::default();

    let _expr = gen_expr_full_seeded(&db, 42, config);
}

#[test]
fn test_gen_multiple_exprs() {
    let db = Database::default();
    let config = AstGenConfig::default();

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_corner_cases_strategy() {
    let db = Database::default();
    let config = AstGenConfig {
        numeric_strategy: NumericStrategy::CornerCases,
        ..Default::default()
    };

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_leaf_only_types() {
    let db = Database::default();
    let config = AstGenConfig {
        max_depth: 0,
        ..Default::default()
    };

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}

#[test]
fn test_random_strategy() {
    let db = Database::default();
    let config = AstGenConfig {
        numeric_strategy: NumericStrategy::Random,
        ..Default::default()
    };

    for seed in 0..10 {
        let _expr = gen_expr_full_seeded(&db, seed, config.clone());
    }
}
