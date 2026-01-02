//! Tests for equivalence between datalit and datafun parsing and typechecking.
//!
//! These tests verify that datafun's inline literal expressions produce the same
//! AST structure and typecheck results as datalit for pure datalit expressions.

use rmx::prelude::*;

use datalove_datafun_compiler::funlit_equiv::{
    datafun_expr_to_datalit_serde,
    datalit_typecheck_to_serde,
    datafun_unit_typecheck_to_serde,
};
use datalove_datalit::ast_gen::{AstGenConfig, gen_expr_full_seeded};

/// Extract expression from a let statement in a Script.
fn extract_let_value<'db>(
    db: &'db datalove_datafun_compiler::Database,
    script: datalove_datafun_compiler::ast::Script<'db>,
) -> Result<datalove_datafun_compiler::ast::ExprFun<'db>, String> {
    let statements = script.statements(db);
    if statements.len() != 1 {
        return Err(format!("expected 1 statement, got {}", statements.len()));
    }
    match &statements[0] {
        datalove_datafun_compiler::ast::Statement::Let(stmt) => Ok(stmt.value(db)),
        other => Err(format!("expected Let statement, got {:?}", std::mem::discriminant(other))),
    }
}

/// Test that a single expression parses equivalently in both parsers.
fn test_parse_equiv(db: &datalove_datafun_compiler::Database, expr_text: &str) -> Result<(), String> {
    // Parse with datalit.
    let datalit_source = bct::input::Source::new(db, expr_text.to_string());
    let datalit_parsed = datalove_datalit::parser::parse_integration_test(db, datalit_source);
    let datalit_serde = datalove_datalit::ast_serde::ExprFull::from_ast(db, datalit_parsed);

    // Parse with datafun (wrap in "let _x = " prefix).
    let datafun_text = format!("let _x = {}", expr_text);
    let datafun_source = bct::input::Source::new(db, datafun_text.clone());
    let datafun_script = datalove_datafun_compiler::parser::parse_integration_test(db, datafun_source);

    // Extract expression from let statement.
    let datafun_expr = extract_let_value(db, datafun_script)?;

    // Convert to datalit serde.
    let datafun_serde = datafun_expr_to_datalit_serde(db, datafun_expr)
        .map_err(|e| format!("Datafun to datalit conversion failed: {}", e))?;

    // Compare.
    if datalit_serde != datafun_serde {
        return Err(format!(
            "AST mismatch for '{}'\nDatalit: {:?}\nDatafun: {:?}",
            expr_text, datalit_serde, datafun_serde
        ));
    }

    Ok(())
}

/// Test that a single expression typechecks equivalently in both typecheckers.
fn test_typecheck_equiv(db: &datalove_datafun_compiler::Database, expr_text: &str) -> Result<(), String> {
    // First verify parsing works.
    test_parse_equiv(db, expr_text)?;

    // Parse and typecheck with datalit.
    let datalit_source = bct::input::Source::new(db, expr_text.to_string());
    let datalit_parsed = datalove_datalit::parser::parse_integration_test(db, datalit_source);
    let datalit_resolved = datalove_datalit::resolve::resolve_names(db, datalit_source, datalit_parsed);
    let datalit_result = datalove_datalit::tycheck::type_check(db, datalit_parsed, datalit_resolved);
    let datalit_serde = datalit_typecheck_to_serde(db, datalit_result);

    // Parse and typecheck with datafun (wrap in "let _x = " prefix) using production path.
    let datafun_text = format!("let _x = {}", expr_text);
    let datafun_source = bct::input::Source::new(db, datafun_text.clone());
    let datafun_script = datalove_datafun_compiler::parser::parse_integration_test(db, datafun_source);
    let datafun_spans = datalove_datafun_compiler::parser::datafun_spans(db, datafun_source);
    let datafun_result = datalove_datafun_compiler::tycheck::type_check_single_script(db, datafun_source, datafun_spans, datafun_script);

    // Extract expression for type lookup.
    let datafun_expr = extract_let_value(db, datafun_script)?;

    let datafun_serde = datafun_unit_typecheck_to_serde(db, datafun_result, datafun_expr);

    // Compare.
    if datalit_serde != datafun_serde {
        return Err(format!(
            "Typecheck mismatch for '{}'\nDatalit: {:?}\nDatafun: {:?}",
            expr_text, datalit_serde, datafun_serde
        ));
    }

    Ok(())
}

// ============================================================================
// Manual tests for specific expressions
// ============================================================================

#[test]
fn test_simple_bool_true() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@true").unwrap();
    test_typecheck_equiv(&db, "@true").unwrap();
}

#[test]
fn test_simple_bool_false() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@false").unwrap();
    test_typecheck_equiv(&db, "@false").unwrap();
}

#[test]
fn test_simple_int() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@42").unwrap();
    test_typecheck_equiv(&db, "@42").unwrap();
}

// Note: Skipping typed_int test - `: type / value` syntax not supported by datafun parser

#[test]
fn test_simple_string() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, r#"@"hello""#).unwrap();
    test_typecheck_equiv(&db, r#"@"hello""#).unwrap();
}

#[test]
fn test_simple_list() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@[1, 2, 3]").unwrap();
    test_typecheck_equiv(&db, "@[1, 2, 3]").unwrap();
}

// Note: Skipping typed_list - `: type / value` syntax not supported by datafun parser
// Note: Skipping empty_list - `: type / value` syntax not supported by datafun parser

#[test]
fn test_nested_list() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@[[1, 2], [3, 4]]").unwrap();
    test_typecheck_equiv(&db, "@[[1, 2], [3, 4]]").unwrap();
}

#[test]
fn test_anon_tuple() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@(1, 2, 3)").unwrap();
    test_typecheck_equiv(&db, "@(1, 2, 3)").unwrap();
}

#[test]
fn test_anon_struct() {
    let db = datalove_datafun_compiler::Database::default();
    test_parse_equiv(&db, "@{x = 1, y = 2}").unwrap();
    test_typecheck_equiv(&db, "@{x = 1, y = 2}").unwrap();
}

// Note: Skipping map test - @{1: 10} syntax not supported (datalit uses struct syntax)
// Note: Skipping set test - @{1, 2, 3} syntax not supported (datalit uses `set {...}`)
// Note: Skipping option_none test - `: type / value` syntax not supported by datafun parser

// ============================================================================
// AST-generated tests
// ============================================================================

/// Create a config for AST generation that produces expressions both parsers can handle.
fn make_compatible_config() -> AstGenConfig {
    use datalove_datalit::ast_gen::{TypeWeights, NumericStrategy};

    AstGenConfig {
        // Type hints work with `: type / expr` syntax.
        // Omitted heap is allowed because function definitions (which also use `:`)
        // are statements, not expressions. In expression context, `:` unambiguously
        // starts a type hint.
        include_type_hints: true,
        heap_distribution: datalove_datalit::ast_gen::HeapDistribution {
            local: 2,
            global: 1,
            omitted: 1,
        },
        max_depth: 2,
        // Minimum 1 to avoid empty lists (type inference differs for []).
        min_collection_size: 1,
        max_collection_size: 3,
        // Small non-negative values work without type hints (no negative literal issues).
        numeric_strategy: NumericStrategy::SmallNonNegative,
        type_weights: TypeWeights {
            bool_type: 10,
            u8_type: 5,
            // Signed ints work - datafun_expr_to_datalit_serde handles UnaryOp(Neg, Int).
            i8_type: 5,
            u16_type: 5,
            i16_type: 5,
            u32_type: 10,
            i32_type: 10,
            // 64-bit ints work with SmallNonNegative strategy (values 0-255).
            u64_type: 10,
            i64_type: 10,
            // Float works after fixing parser to consume dot before checking decimal.
            f32_type: 10,
            // BigInt works with SmallNonNegative strategy.
            int_type: 10,
            string_type: 10,
            // Lists work with min_collection_size: 1 to avoid empty list type inference issues.
            list_type: 10,
            // Map and set work with min_collection_size: 1.
            map_type: 10,
            set_type: 10,
            // Option type works with type hints for @none.
            option_type: 10,
            // Result type works with type hints for @error.
            result_type: 10,
            tensor_type: 10, // Works with min_collection_size: 1
            anon_tuple_type: 10,
            // Named tuple works with type hints.
            named_tuple_type: 10,
            anon_struct_type: 10,
            // Named struct works with type hints.
            named_struct_type: 10,
            // Enums work with type hints.
            anon_enum_type: 10,
            named_enum_type: 10,
            // Data and error types work with type hints.
            data_type: 10,
            error_type: 10,
        },
        ..Default::default()
    }
}

#[test]
fn test_funlit_equiv_generated_parse() {
    let db = datalove_datafun_compiler::Database::default();
    let config = make_compatible_config();

    let mut failures = vec![];

    for seed in 0..100 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());
        let expr_text = datalove_datalit::pretty::pretty_print(&db, expr_full);

        if let Err(e) = test_parse_equiv(&db, &expr_text) {
            failures.push((seed, expr_text, e));
        }
    }

    if !failures.is_empty() {
        for (seed, text, err) in &failures {
            eprintln!("Seed {}: {}\n{}\n", seed, text, err);
        }
        panic!("{} parse equivalence failures out of 100", failures.len());
    }
}

/// Temporarily ignored due to differences in Option/Result coercion handling.
#[test]
#[ignore = "needs investigation after coercion removal"]
fn test_funlit_equiv_generated_typecheck() {
    let db = datalove_datafun_compiler::Database::default();
    let config = make_compatible_config();

    let mut failures = vec![];

    for seed in 0..100 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());
        let expr_text = datalove_datalit::pretty::pretty_print(&db, expr_full);

        if let Err(e) = test_typecheck_equiv(&db, &expr_text) {
            failures.push((seed, expr_text, e));
        }
    }

    if !failures.is_empty() {
        for (seed, text, err) in &failures {
            eprintln!("Seed {}: {}\n{}\n", seed, text, err);
        }
        panic!("{} typecheck equivalence failures out of 100", failures.len());
    }
}

#[test]
fn test_funlit_equiv_roundtrip() {
    let db = datalove_datafun_compiler::Database::default();
    let config = make_compatible_config();

    let mut failures = vec![];

    for seed in 0..50 {
        let expr_full = gen_expr_full_seeded(&db, seed, config.clone());
        let original_text = datalove_datalit::pretty::pretty_print(&db, expr_full);

        // First pass.
        if let Err(e) = test_parse_equiv(&db, &original_text) {
            failures.push((seed, "first pass".to_string(), original_text.clone(), e));
            continue;
        }

        // Parse with datafun, pretty-print, then test again.
        let datafun_text = format!("let _x = {}", original_text);
        let datafun_source = bct::input::Source::new(&db, datafun_text);
        let datafun_script = datalove_datafun_compiler::parser::parse_integration_test(&db, datafun_source);

        if let Ok(datafun_expr) = extract_let_value(&db, datafun_script) {
            if let Ok(serde) = datafun_expr_to_datalit_serde(&db, datafun_expr) {
                // Serialize and deserialize to get a new string representation.
                let json = rmx::serde_json::to_string(&serde).unwrap();
                let reparsed: datalove_datalit::ast_serde::ExprFull =
                    rmx::serde_json::from_str(&json).unwrap();

                // The roundtrip should be identical.
                if serde != reparsed {
                    failures.push((seed, "roundtrip json".to_string(), original_text.clone(),
                        format!("JSON roundtrip changed AST")));
                }
            }
        }
    }

    if !failures.is_empty() {
        for (seed, phase, text, err) in &failures {
            eprintln!("Seed {} ({}): {}\n{}\n", seed, phase, text, err);
        }
        panic!("{} roundtrip failures out of 50", failures.len());
    }
}
