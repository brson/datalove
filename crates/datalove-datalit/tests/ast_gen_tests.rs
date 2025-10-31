use datalove_datalit::{ast_gen::*, Database};

#[test]
fn test_nested_named_types_should_resolve() {
    // Test that named types nested in type hints should be resolvable
    let db = Database::default();

    let source_text = r#"
: @tuple Outer (
  @struct Inner {
    x: @u32
  }
) / @tuple Outer (
  @struct Inner {
    x = @1
  }
)
"#;

    let source = bct::input::Source::new(&db, source_text.into());
    let parsed = datalove_datalit::parser::parse_integration_test(&db, source);
    let resolved = datalove_datalit::resolve::resolve_names(&db, parsed, vec![]);
    let typechecked = datalove_datalit::tycheck::type_check(&db, parsed, resolved);

    let has_errors = !typechecked.errors(&db).is_empty();
    let has_type = typechecked.root_type(&db).is_some();

    if has_errors {
        println!("\nErrors:");
        for error_entry in typechecked.errors(&db) {
            println!("  {:?}", error_entry.error(&db));
        }
    }

    assert!(!has_errors, "Should not have resolution/typecheck errors");
    assert!(has_type, "Should have a type");
}

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

#[test]
fn test_generated_exprs_typecheck() {
    let db = Database::default();
    let config = AstGenConfig::default();

    for seed in 0..100 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());

        let resolved = datalove_datalit::resolve::resolve_names(&db, expr_full, vec![]);
        let typechecked = datalove_datalit::tycheck::type_check(&db, expr_full, resolved);

        let has_errors = !typechecked.errors(&db).is_empty();
        let has_type = typechecked.root_type(&db).is_some();

        assert!(!has_errors && has_type,
            "Generated expression failed typecheck for seed {}: has_type={}, has_errors={}",
            seed, has_type, has_errors);
    }
}
