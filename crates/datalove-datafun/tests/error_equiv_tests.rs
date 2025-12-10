//! Tests for error equivalence between datalit and datafun parsers/typecheckers.
//!
//! These tests verify that both systems produce equivalent errors when given
//! erroneous input (generated via mutation-based testing).
//!
//! Note: Some tests reveal genuine differences between datalit and datafun
//! error handling. The infrastructure here is designed to discover these differences.
//! Known discrepancies are documented in the test output.

use rmx::prelude::*;
use rand::SeedableRng;
use rand::rngs::StdRng;

use datalove_datalit::ast_gen::AstGenConfig;
use datalove_datalit::mutation_gen::{Mutation, MutationResult};

/// Extract error codes from datalit typecheck result.
fn get_datalit_errors<'db>(
    db: &'db datalove_datafun::Database,
    source: &str,
) -> (Vec<String>, bool) {
    let src = bct::input::Source::new(db, source.to_string());
    let parsed = datalove_datalit::parser::parse_integration_test(db, src);

    // Check for parse errors first.
    if has_parse_error(db, parsed) {
        return (vec!["PARSE_ERROR".to_string()], true);
    }

    let resolved = datalove_datalit::resolve::resolve_names(db, src, parsed);
    let result = datalove_datalit::tycheck::type_check(db, parsed, resolved);

    let errors: Vec<String> = result
        .errors(db)
        .iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();

    (errors, false)
}

/// Check if expression contains parse error.
fn has_parse_error<'db>(
    db: &'db datalove_datafun::Database,
    expr: datalove_datalit::ast::ExprFull<'db>,
) -> bool {
    use datalove_datalit::ast::Expr;
    match expr.expr(db).expr(db) {
        Expr::ParseError(_) => true,
        Expr::List(l) => l.elements(db).iter().any(|e| has_parse_error(db, *e)),
        Expr::Set(s) => s.elements(db).iter().any(|e| has_parse_error(db, *e)),
        Expr::Map(m) => m.entries(db).iter().any(|e| {
            has_parse_error(db, e.key(db)) || has_parse_error(db, e.value(db))
        }),
        Expr::AnonTuple(t) => t.elements(db).iter().any(|e| has_parse_error(db, *e)),
        Expr::NamedTuple(t) => t.elements(db).iter().any(|e| has_parse_error(db, *e)),
        Expr::AnonStruct(s) => s.fields(db).iter().any(|f| has_parse_error(db, f.value(db))),
        Expr::NamedStruct(s) => s.fields(db).iter().any(|f| has_parse_error(db, f.value(db))),
        Expr::AnonEnum(e) => e.payload(db).map(|p| has_parse_error(db, p)).unwrap_or(false),
        Expr::NamedEnum(e) => e.payload(db).map(|p| has_parse_error(db, p)).unwrap_or(false),
        Expr::Data(d) => has_parse_error(db, d.value(db)),
        Expr::Err(e) => has_parse_error(db, e.value(db)),
        Expr::Tensor(t) => t.elements(db).iter().any(|e| has_parse_error(db, *e)),
        _ => false,
    }
}

/// Extract error codes from datafun typecheck result.
fn get_datafun_errors<'db>(
    db: &'db datalove_datafun::Database,
    source: &str,
) -> (Vec<String>, bool) {
    // Wrap in "let _x = " for datafun parsing.
    let datafun_text = format!("let _x = {}", source);
    let src = bct::input::Source::new(db, datafun_text.clone());
    let script = datalove_datafun::parser::parse_integration_test(db, src);

    // Check for parse errors.
    let statements = script.statements(db);
    if statements.is_empty() {
        return (vec!["PARSE_ERROR".to_string()], true);
    }

    // Extract the let statement value.
    let expr = match &statements[0] {
        datalove_datafun::ast::Statement::Let(stmt) => stmt.value(db),
        _ => return (vec!["PARSE_ERROR".to_string()], true),
    };

    // Check for parse errors in the expression.
    if has_datafun_parse_error(db, expr) {
        return (vec!["PARSE_ERROR".to_string()], true);
    }

    let result = datalove_datafun::tycheck::type_check(db, src, script);

    let errors: Vec<String> = result
        .errors(db)
        .iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();

    (errors, false)
}

/// Check if datafun expression contains parse error.
fn has_datafun_parse_error<'db>(
    db: &'db datalove_datafun::Database,
    expr: datalove_datafun::ast::ExprFun<'db>,
) -> bool {
    use datalove_datafun::ast::ExprFunKind;
    match expr.expr(db) {
        ExprFunKind::ParseError(_) => true,
        ExprFunKind::List(l) => l.elements(db).iter().any(|e| has_datafun_parse_error(db, *e)),
        ExprFunKind::Set(s) => s.elements(db).iter().any(|e| has_datafun_parse_error(db, *e)),
        ExprFunKind::Map(m) => m.entries(db).iter().any(|e| {
            has_datafun_parse_error(db, e.key(db)) || has_datafun_parse_error(db, e.value(db))
        }),
        ExprFunKind::AnonTuple(t) => t.elements(db).iter().any(|e| has_datafun_parse_error(db, *e)),
        ExprFunKind::NamedTuple(t) => t.elements(db).iter().any(|e| has_datafun_parse_error(db, *e)),
        ExprFunKind::AnonStruct(s) => s.fields(db).iter().any(|f| has_datafun_parse_error(db, f.value(db))),
        ExprFunKind::NamedStruct(s) => s.fields(db).iter().any(|f| has_datafun_parse_error(db, f.value(db))),
        ExprFunKind::AnonEnum(e) => e.payload(db).map(|p| has_datafun_parse_error(db, p)).unwrap_or(false),
        ExprFunKind::NamedEnum(e) => e.payload(db).map(|p| has_datafun_parse_error(db, p)).unwrap_or(false),
        ExprFunKind::Data(d) => has_datafun_parse_error(db, d.value(db)),
        ExprFunKind::Err(e) => has_datafun_parse_error(db, e.value(db)),
        ExprFunKind::Tensor(t) => t.elements(db).iter().any(|e| has_datafun_parse_error(db, *e)),
        ExprFunKind::Tuple(t) => t.elements(db).iter().any(|e| has_datafun_parse_error(db, *e)),
        ExprFunKind::UnaryOp(u) => has_datafun_parse_error(db, u.operand(db)),
        ExprFunKind::BinOp(b) => {
            has_datafun_parse_error(db, b.lhs(db)) || has_datafun_parse_error(db, b.rhs(db))
        }
        _ => false,
    }
}

/// Compare error results between datalit and datafun.
fn errors_equivalent(
    datalit_errors: &[String],
    datafun_errors: &[String],
    datalit_has_parse_error: bool,
    datafun_has_parse_error: bool,
) -> bool {
    // If both have parse errors, consider them equivalent.
    if datalit_has_parse_error && datafun_has_parse_error {
        return true;
    }

    // If one has parse error and the other doesn't, not equivalent.
    if datalit_has_parse_error != datafun_has_parse_error {
        return false;
    }

    // For type errors, compare the error lists.
    // Sort both lists for comparison (order may differ).
    let mut datalit_sorted = datalit_errors.to_vec();
    let mut datafun_sorted = datafun_errors.to_vec();
    datalit_sorted.sort();
    datafun_sorted.sort();

    datalit_sorted == datafun_sorted
}

/// Test error equivalence for a specific mutated expression.
fn test_error_equiv(
    db: &datalove_datafun::Database,
    mutation_result: &MutationResult,
) -> Result<(), String> {
    let (datalit_errors, datalit_parse_err) = get_datalit_errors(db, &mutation_result.source);
    let (datafun_errors, datafun_parse_err) = get_datafun_errors(db, &mutation_result.source);

    if !errors_equivalent(&datalit_errors, &datafun_errors, datalit_parse_err, datafun_parse_err) {
        return Err(format!(
            "Error mismatch for '{}'\n  Mutation: {}\n  Datalit errors: {:?} (parse_err={})\n  Datafun errors: {:?} (parse_err={})",
            mutation_result.source,
            mutation_result.description,
            datalit_errors, datalit_parse_err,
            datafun_errors, datafun_parse_err,
        ));
    }

    Ok(())
}

/// Create a config for AST generation suitable for mutation testing.
fn make_mutation_config() -> AstGenConfig {
    use datalove_datalit::ast_gen::{TypeWeights, NumericStrategy};

    AstGenConfig {
        include_type_hints: true,
        heap_distribution: datalove_datalit::ast_gen::HeapDistribution {
            local: 2,
            global: 1,
            omitted: 1,
        },
        max_depth: 2,
        min_collection_size: 1,
        max_collection_size: 3,
        numeric_strategy: NumericStrategy::SmallNonNegative,
        type_weights: TypeWeights {
            bool_type: 10,
            u8_type: 10,
            i8_type: 10,
            u16_type: 5,
            i16_type: 5,
            u32_type: 10,
            i32_type: 10,
            u64_type: 5,
            i64_type: 5,
            f32_type: 5,
            int_type: 5,
            string_type: 10,
            list_type: 15, // Higher weight for list to test collection mutations.
            map_type: 5,
            set_type: 5,
            option_type: 10,
            result_type: 5,
            tensor_type: 3,
            anon_tuple_type: 10, // Higher weight for tuple to test arity mutations.
            named_tuple_type: 5,
            anon_struct_type: 10, // Higher weight for struct to test field mutations.
            named_struct_type: 5,
            anon_enum_type: 10, // Higher weight for enum to test variant mutations.
            named_enum_type: 5,
            data_type: 3,
            error_type: 3,
        },
        ..Default::default()
    }
}

/// Statistics for a mutation type.
struct MutationStats {
    tested: usize,
    passed: usize,
    failed: usize,
}

impl MutationStats {
    fn new() -> Self {
        MutationStats {
            tested: 0,
            passed: 0,
            failed: 0,
        }
    }

    fn pass_rate(&self) -> f64 {
        if self.tested > 0 {
            (self.passed as f64 / self.tested as f64) * 100.0
        } else {
            0.0
        }
    }
}

/// Run a single mutation test, returning Some(true) if passed, Some(false) if failed, None if skipped.
fn run_single_mutation_test(
    mutation: Mutation,
    config: &AstGenConfig,
    seed: u64,
) -> Option<bool> {
    // Create fresh database for each test to avoid salsa state issues.
    let db = datalove_datafun::Database::default();
    let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
    let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

    if let Some(result) = mutation.apply(&db, expr, &mut rng) {
        Some(test_error_equiv(&db, &result).is_ok())
    } else {
        None
    }
}

/// Run mutation tests for a given mutation type and return statistics.
/// Note: Tests that panic are skipped to avoid salsa state corruption.
fn run_mutation_tests(
    mutation: Mutation,
    config: &AstGenConfig,
    num_seeds: u64,
) -> MutationStats {
    let mut stats = MutationStats::new();

    for seed in 0..num_seeds {
        // Run in a separate thread to isolate panics.
        let config_clone = config.clone();
        let result = std::thread::spawn(move || {
            // Set up a panic hook to suppress output.
            let prev_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_single_mutation_test(mutation, &config_clone, seed)
            }));
            std::panic::set_hook(prev_hook);
            result
        }).join();

        match result {
            Ok(Ok(Some(true))) => {
                stats.tested += 1;
                stats.passed += 1;
            }
            Ok(Ok(Some(false))) => {
                stats.tested += 1;
                stats.failed += 1;
            }
            Ok(Ok(None)) => {
                // Mutation not applicable, skip.
            }
            Ok(Err(_)) | Err(_) => {
                // Panic in test - count as failure.
                stats.tested += 1;
                stats.failed += 1;
            }
        }
    }

    stats
}

// ============================================================================
// Main error equivalence test (reports discrepancies, does not fail)
// ============================================================================

/// Test that discovers and reports error equivalence discrepancies.
///
/// This test runs all mutations and reports statistics on equivalence.
/// It does NOT fail on discrepancies - use this to understand the current state.
#[test]
fn test_error_equiv_discovery() {
    let config = make_mutation_config();

    println!("\n=== Error Equivalence Discovery Report ===\n");

    let mut total_tested = 0;
    let mut total_passed = 0;

    // Test each mutation type.
    for mutation in Mutation::all() {
        let stats = run_mutation_tests(*mutation, &config, 50);
        total_tested += stats.tested;
        total_passed += stats.passed;

        if stats.tested > 0 {
            println!(
                "{:?}: {}/{} passed ({:.1}%)",
                mutation,
                stats.passed,
                stats.tested,
                stats.pass_rate()
            );
        }
    }

    println!("\n=== Summary ===");
    let overall_rate = if total_tested > 0 {
        (total_passed as f64 / total_tested as f64) * 100.0
    } else {
        0.0
    };
    println!(
        "Overall: {}/{} passed ({:.1}%)",
        total_passed, total_tested, overall_rate
    );

    // Report known discrepancy categories.
    println!("\n=== Known Discrepancies ===");
    println!("1. OutOfRangeInt: Datafun doesn't check integer literal ranges against type hints");
    println!("2. HeapMismatch: Datafun may handle heap annotation checking differently");
    println!("3. WrongElementType: Collection type mismatch detection may differ");
    println!("4. Source mutations: Parser may panic on malformed input (counted as failures)");
    println!();
}

// ============================================================================
// Individual mutation tests (for detailed investigation)
// ============================================================================

/// Detailed test for OutOfRangeInt mutations.
///
/// Note: This test may fail if datafun and datalit have different
/// integer range checking behavior. Run to investigate discrepancies.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_out_of_range_int_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let mut failures = vec![];

    for seed in 0..200 {
        let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

        if let Some(result) = Mutation::OutOfRangeInt.apply(&db, expr, &mut rng) {
            if let Err(e) = test_error_equiv(&db, &result) {
                failures.push((seed, e));
            }
        }
    }

    if !failures.is_empty() {
        for (seed, err) in &failures[..failures.len().min(10)] {
            eprintln!("Seed {}: {}\n", seed, err);
        }
        if failures.len() > 10 {
            eprintln!("... and {} more failures", failures.len() - 10);
        }
        panic!("{} OutOfRangeInt tests failed", failures.len());
    }
}

/// Detailed test for WrongElementType mutations.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_wrong_element_type_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let mut failures = vec![];

    for seed in 0..200 {
        let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

        if let Some(result) = Mutation::WrongElementType.apply(&db, expr, &mut rng) {
            if let Err(e) = test_error_equiv(&db, &result) {
                failures.push((seed, e));
            }
        }
    }

    if !failures.is_empty() {
        for (seed, err) in &failures[..failures.len().min(10)] {
            eprintln!("Seed {}: {}\n", seed, err);
        }
        if failures.len() > 10 {
            eprintln!("... and {} more failures", failures.len() - 10);
        }
        panic!("{} WrongElementType tests failed", failures.len());
    }
}

/// Detailed test for HeapMismatch mutations.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_heap_mismatch_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let mut failures = vec![];

    for seed in 0..200 {
        let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

        if let Some(result) = Mutation::HeapMismatch.apply(&db, expr, &mut rng) {
            if let Err(e) = test_error_equiv(&db, &result) {
                failures.push((seed, e));
            }
        }
    }

    if !failures.is_empty() {
        for (seed, err) in &failures[..failures.len().min(10)] {
            eprintln!("Seed {}: {}\n", seed, err);
        }
        if failures.len() > 10 {
            eprintln!("... and {} more failures", failures.len() - 10);
        }
        panic!("{} HeapMismatch tests failed", failures.len());
    }
}

/// Detailed test for ArityMismatch mutations.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_arity_mismatch_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let mut failures = vec![];

    for seed in 0..200 {
        let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

        if let Some(result) = Mutation::ArityMismatch.apply(&db, expr, &mut rng) {
            if let Err(e) = test_error_equiv(&db, &result) {
                failures.push((seed, e));
            }
        }
    }

    if !failures.is_empty() {
        for (seed, err) in &failures[..failures.len().min(10)] {
            eprintln!("Seed {}: {}\n", seed, err);
        }
        if failures.len() > 10 {
            eprintln!("... and {} more failures", failures.len() - 10);
        }
        panic!("{} ArityMismatch tests failed", failures.len());
    }
}

/// Detailed test for RemoveTypeHint mutations.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_remove_type_hint_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let mut failures = vec![];

    for seed in 0..200 {
        let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());

        if let Some(result) = Mutation::RemoveTypeHint.apply(&db, expr, &mut StdRng::seed_from_u64(0)) {
            if let Err(e) = test_error_equiv(&db, &result) {
                failures.push((seed, e));
            }
        }
    }

    if !failures.is_empty() {
        for (seed, err) in &failures[..failures.len().min(10)] {
            eprintln!("Seed {}: {}\n", seed, err);
        }
        if failures.len() > 10 {
            eprintln!("... and {} more failures", failures.len() - 10);
        }
        panic!("{} RemoveTypeHint tests failed", failures.len());
    }
}

/// Detailed test for WrongVariant mutations.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_wrong_variant_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let mut failures = vec![];

    for seed in 0..200 {
        let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
        let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

        if let Some(result) = Mutation::WrongVariant.apply(&db, expr, &mut rng) {
            if let Err(e) = test_error_equiv(&db, &result) {
                failures.push((seed, e));
            }
        }
    }

    if !failures.is_empty() {
        for (seed, err) in &failures[..failures.len().min(10)] {
            eprintln!("Seed {}: {}\n", seed, err);
        }
        if failures.len() > 10 {
            eprintln!("... and {} more failures", failures.len() - 10);
        }
        panic!("{} WrongVariant tests failed", failures.len());
    }
}

/// Detailed test for source-level mutations.
#[test]
#[ignore] // Run with --ignored to investigate
fn test_error_equiv_source_mutations_detailed() {
    let db = datalove_datafun::Database::default();
    let config = make_mutation_config();

    let source_mutations = [
        Mutation::DeleteHeapSigil,
        Mutation::DeleteOpeningBracket,
        Mutation::TruncateSource,
        Mutation::DeleteComma,
        Mutation::ExtraClosingBracket,
    ];

    let mut total_failures = vec![];

    for mutation in source_mutations {
        let mut failures = vec![];

        for seed in 0..100 {
            let expr = datalove_datalit::ast_gen::gen_expr_full_seeded(&db, seed, config.clone());
            let mut rng = StdRng::seed_from_u64(seed.wrapping_mul(0xdeadbeef));

            if let Some(result) = mutation.apply(&db, expr, &mut rng) {
                if let Err(e) = test_error_equiv(&db, &result) {
                    failures.push((seed, mutation, e));
                }
            }
        }

        total_failures.extend(failures);
    }

    if !total_failures.is_empty() {
        for (seed, mutation, err) in &total_failures[..total_failures.len().min(10)] {
            eprintln!("Seed {} ({:?}): {}\n", seed, mutation, err);
        }
        if total_failures.len() > 10 {
            eprintln!("... and {} more failures", total_failures.len() - 10);
        }
        panic!("{} source mutation tests failed", total_failures.len());
    }
}
