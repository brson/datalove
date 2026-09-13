//! Integration tests for worldfile generation.
//!
//! These tests verify that generated worldfiles parse and typecheck successfully.

use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};
use datalove_datafun::{Database, package, package_resolve, package_load_worldfile, to_module_graph, module_graph};
use datalove_datafun_tycheck::{typecheck_module_graph, AutoAdaptMode};
use datalove_datafun_resolve::{resolve_all_names, resolve_all_exports, build_all_function_ast_maps};

/// Parse and typecheck a worldfile, returning the number of errors.
fn typecheck_worldfile(source: &str) -> Vec<String> {
    let db = Database::default();

    // Load the worldfile.
    let package_world_raw = match package_load_worldfile::load_world_from_worldfile(source.as_bytes()) {
        Ok(pw) => pw,
        Err(e) => return vec![format!("Parse error: {}", e)],
    };
    let package_world = package::import_from_loader(&db, package_world_raw);

    // Resolve imports.
    let resolution = package_resolve::resolve_package_world_with_imports(&db, package_world);
    let result = resolution.result(&db);

    if let Err(e) = result {
        return vec![format!("Resolution error: {:?}", e)];
    }

    let pkg_graph = result.ok().unwrap();

    // Convert to ModuleGraph, parse, and typecheck.
    let graph_with_requires = to_module_graph(&db, package_world, pkg_graph);
    let parsed_graph = module_graph::parse_module_graph(&db, graph_with_requires.graph, graph_with_requires.resolved_requires, Vec::new());
    let all_names = resolve_all_names(&db, parsed_graph);
    let all_exports = resolve_all_exports(&db, parsed_graph);
    let all_function_asts = build_all_function_ast_maps(&db, parsed_graph);
    let typecheck_result = typecheck_module_graph(&db, parsed_graph, all_names, all_exports, all_function_asts, AutoAdaptMode::Disabled);

    // Collect all errors from all modules.
    let mut errors = Vec::new();
    let module_errors = typecheck_result.module_errors(&db);

    for (module_id, errs) in module_errors.iter() {
        for err in errs {
            errors.push(format!("{}: {:?}", module_id.path(&db), err));
        }
    }

    errors
}

#[test]
fn test_generated_worldfile_typechecks() {
    let config = WorldGenConfig::default();
    let wf = gen_worldfile_seeded(12345, config);

    let errors = typecheck_worldfile(&wf);

    if !errors.is_empty() {
        eprintln!("Generated worldfile:");
        eprintln!("{}", wf);
        eprintln!("\nTypecheck errors:");
        for err in &errors {
            eprintln!("  {}", err);
        }
        panic!("Generated worldfile has {} typecheck errors", errors.len());
    }
}

#[test]
fn test_multiple_seeds_typecheck() {
    let config = WorldGenConfig::default();

    // Test a range of seeds.
    let mut failed_seeds = Vec::new();

    for seed in 0..50 {
        let wf = gen_worldfile_seeded(seed, config.clone());
        let errors = typecheck_worldfile(&wf);

        if !errors.is_empty() {
            failed_seeds.push((seed, errors));
        }
    }

    if !failed_seeds.is_empty() {
        eprintln!("Failed seeds:");
        for (seed, errors) in &failed_seeds {
            eprintln!("\nSeed {}:", seed);
            let wf = gen_worldfile_seeded(*seed, config.clone());
            eprintln!("{}", wf);
            eprintln!("\nErrors:");
            for err in errors {
                eprintln!("  {}", err);
            }
        }
        panic!("{} out of 50 seeds failed to typecheck", failed_seeds.len());
    }
}

#[test]
fn test_minimal_config_typechecks() {
    // Test with minimal configuration.
    let mut config = WorldGenConfig::default();
    config.module_count = (1, 1);
    config.functions_per_module = (1, 1);
    config.statements_per_function = (1, 1);
    config.type_aliases_per_module = (0, 0);
    config.script_statements = (1, 1);
    config.if_probability = 0;
    config.loop_probability = 0;
    config.function_call_probability = 0;

    for seed in 0..20 {
        let wf = gen_worldfile_seeded(seed, config.clone());
        let errors = typecheck_worldfile(&wf);

        if !errors.is_empty() {
            eprintln!("Seed {}: Generated worldfile:", seed);
            eprintln!("{}", wf);
            eprintln!("\nTypecheck errors:");
            for err in &errors {
                eprintln!("  {}", err);
            }
            panic!("Minimal config seed {} has typecheck errors", seed);
        }
    }
}

#[test]
fn test_worldfile_has_expected_constructs() {
    let config = WorldGenConfig::default();
    let wf = gen_worldfile_seeded(42, config);

    // Check for expected constructs.
    assert!(wf.contains("module local/"), "Should have module declaration");
    assert!(wf.contains("fun "), "Should have function declaration");
    assert!(wf.contains("end fun"), "Should have function end");
    assert!(wf.contains("ret ") || wf.contains("ret\n"), "Should have return statement");
    assert!(wf.contains("let ") || wf.contains("var "), "Should have variable declaration");
    assert!(wf.contains("script"), "Should have script section");
}

#[test]
fn test_print_example_worldfile() {
    // Print an example worldfile for inspection.
    let config = WorldGenConfig::default();
    let wf = gen_worldfile_seeded(42, config);

    println!("Example generated worldfile (seed 42):");
    println!("========================================");
    println!("{}", wf);
    println!("========================================");
}

/// Test 1000 seeds for comprehensive coverage.
///
/// This test is ignored by default since it takes longer.
/// Run with: cargo test -p datalove-worldgen test_1000_seeds -- --ignored
#[test]
#[ignore]
fn test_1000_seeds_typecheck() {
    let config = WorldGenConfig::default();
    let mut failed_seeds = Vec::new();

    for seed in 0..1000 {
        let wf = gen_worldfile_seeded(seed, config.clone());
        let errors = typecheck_worldfile(&wf);

        if !errors.is_empty() {
            failed_seeds.push((seed, errors));
        }
    }

    if !failed_seeds.is_empty() {
        eprintln!("Failed {} out of 1000 seeds:", failed_seeds.len());
        for (seed, errors) in failed_seeds.iter().take(5) {
            eprintln!("\nSeed {}:", seed);
            for err in errors {
                eprintln!("  {}", err);
            }
        }
        if failed_seeds.len() > 5 {
            eprintln!("... and {} more", failed_seeds.len() - 5);
        }
        panic!("{} out of 1000 seeds failed to typecheck", failed_seeds.len());
    }
}


// ============================================================================
// Tests for type generation
// ============================================================================

/// Verify that type hints are generated in output.
#[test]
fn test_types_in_output() {
    let config = WorldGenConfig::default();

    let mut found_types = false;

    for seed in 0..100 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        if wf.contains("u32") || wf.contains("i32") || wf.contains("bool")
            || wf.contains("u64") || wf.contains("string")
        {
            found_types = true;
            break;
        }
    }

    assert!(found_types, "Should generate types in worldfile");
}

/// Verify that generated expressions match their declared types.
#[test]
fn test_type_consistency() {
    let config = WorldGenConfig::default();

    for seed in 0..20 {
        let wf = gen_worldfile_seeded(seed, config.clone());
        let errors = typecheck_worldfile(&wf);

        if !errors.is_empty() {
            eprintln!("Seed {}: Type consistency errors:", seed);
            for err in &errors {
                eprintln!("  {}", err);
            }
            panic!("Type consistency check failed for seed {}", seed);
        }
    }
}

/// Verify that type alias definitions are formatted correctly.
#[test]
fn test_type_alias_format() {
    let mut config = WorldGenConfig::default();
    config.type_aliases_per_module = (2, 3);

    let wf = gen_worldfile_seeded(42, config);

    // Type aliases should be in format "type Name: type".
    for line in wf.lines() {
        if line.starts_with("type Type") {
            assert!(
                line.contains(": "),
                "Type alias should have type: {}",
                line
            );
        }
    }
}

/// Verify that function signatures have proper type annotations.
#[test]
fn test_function_signature_types() {
    let config = WorldGenConfig::default();
    let wf = gen_worldfile_seeded(42, config);

    // Find function definitions.
    for line in wf.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("fun ") {
            // Parameters should have type annotations.
            if trimmed.contains("(") && trimmed.contains(")") {
                let params_start = trimmed.find('(').unwrap();
                let params_end = trimmed.find(')').unwrap();
                let params = &trimmed[params_start + 1..params_end];

                if !params.is_empty() {
                    // Each parameter should have a colon for type annotation.
                    assert!(
                        params.contains(':'),
                        "Parameters should have type annotations: {}",
                        trimmed
                    );
                }
            }

            // Return type (if any) should be complete.
            if trimmed.ends_with(":") {
                // This shouldn't happen - types should be complete.
                panic!("Function signature ends with incomplete type: {}", trimmed);
            }
        }
    }
}

/// Test that variable declarations have proper type and value.
#[test]
fn test_variable_declaration_format() {
    let config = WorldGenConfig::default();
    let wf = gen_worldfile_seeded(42, config);

    for line in wf.lines() {
        let trimmed = line.trim();

        // Check let declarations.
        if trimmed.starts_with("let ") {
            assert!(
                trimmed.contains(": ") && trimmed.contains(" = "),
                "let should have type and value: {}",
                trimmed
            );
        }

        // Check var declarations.
        if trimmed.starts_with("var ") {
            assert!(
                trimmed.contains(": ") && trimmed.contains(" = "),
                "var should have type and value: {}",
                trimmed
            );
        }
    }
}

/// Verify primitive type variety in generated code.
#[test]
fn test_primitive_type_variety() {
    let config = WorldGenConfig::default();

    let mut type_counts = std::collections::HashMap::new();

    // Use more seeds to ensure less common types appear.
    for seed in 0..200 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        // Count occurrences of each primitive type (no sigils).
        for ty in ["bool", "u8", "i8", "u16", "i16", "u32", "i32",
                   "u64", "i64", "index", "offset", "f32", "f64", "string"] {
            let count = wf.matches(ty).count();
            *type_counts.entry(ty).or_insert(0) += count;
        }
    }

    // Should generate at least a few of the common types.
    let common_types = ["u32", "i32", "bool", "string"];
    for ty in common_types {
        let count = type_counts.get(ty).copied().unwrap_or(0);
        assert!(
            count > 0,
            "Should generate {} at least once across 200 seeds, found {}",
            ty,
            count
        );
    }

    // usize/isize should also be generated (they have weight 5 in leaf_only).
    for ty in ["index", "offset"] {
        let count = type_counts.get(ty).copied().unwrap_or(0);
        assert!(
            count > 0,
            "Should generate {} at least once across 200 seeds, found {}",
            ty,
            count
        );
    }
}

/// Test that comparison operators in bool expressions work correctly.
#[test]
fn test_comparison_operators() {
    let config = WorldGenConfig::default();

    let mut found_ops = std::collections::HashSet::new();

    for seed in 0..100 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        // Check for comparison operators.
        if wf.contains(".<") { found_ops.insert(".<"); }
        if wf.contains(".>") { found_ops.insert(".>"); }
        if wf.contains("<=") { found_ops.insert("<="); }
        if wf.contains(">=") { found_ops.insert(">="); }
        if wf.contains("==") { found_ops.insert("=="); }
        if wf.contains("!=") { found_ops.insert("!="); }

        if found_ops.len() >= 3 {
            break;
        }
    }

    assert!(
        found_ops.len() >= 2,
        "Should generate at least 2 different comparison operators, found: {:?}",
        found_ops
    );
}

/// Verify that salsa tracked types are deterministic across runs.
#[test]
fn test_salsa_type_determinism() {
    let config = WorldGenConfig::default();

    // Generate the same seed multiple times and verify identical output.
    for seed in [42, 123, 456] {
        let wf1 = gen_worldfile_seeded(seed, config.clone());
        let wf2 = gen_worldfile_seeded(seed, config.clone());
        let wf3 = gen_worldfile_seeded(seed, config.clone());

        assert_eq!(wf1, wf2, "Seed {} should be deterministic", seed);
        assert_eq!(wf2, wf3, "Seed {} should be deterministic", seed);
    }
}

/// Verify that arithmetic expressions are generated for bigints and floats.
#[test]
fn test_arithmetic_expressions() {
    let config = WorldGenConfig::default();

    // Track bigint operators.
    let mut int_add = false;
    let mut int_sub = false;
    let mut int_mul = false;

    // Track float operators.
    let mut float_add = false;
    let mut float_sub = false;
    let mut float_mul = false;
    let mut float_div = false;

    for seed in 0..300 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        for line in wf.lines() {
            // Check for bigint arithmetic (simple format: 123 + 456).
            if line.contains("int") {
                if line.contains(" + ") { int_add = true; }
                if line.contains(" - ") { int_sub = true; }
                if line.contains(" * ") { int_mul = true; }
            }

            // Check for float arithmetic (type-hinted format: (: f32 / val) op (: f32 / val)).
            if line.contains("(: f32") || line.contains("(: f64") {
                if line.contains(") + (") { float_add = true; }
                if line.contains(") - (") { float_sub = true; }
                if line.contains(") * (") { float_mul = true; }
                if line.contains(") / (") { float_div = true; }
            }
        }

        // Break early if we found all operators.
        if int_add && int_sub && int_mul && float_add && float_sub && float_mul && float_div {
            break;
        }
    }

    // Assert bigint arithmetic.
    assert!(int_add, "Should generate addition (+) on bigints");
    assert!(int_sub, "Should generate subtraction (-) on bigints");
    assert!(int_mul, "Should generate multiplication (*) on bigints");

    // Assert float arithmetic.
    assert!(float_add, "Should generate addition (+) on floats");
    assert!(float_sub, "Should generate subtraction (-) on floats");
    assert!(float_mul, "Should generate multiplication (*) on floats");
    assert!(float_div, "Should generate division (/) on floats");
}

/// Verify that logical operators are generated and typecheck correctly.
#[test]
fn test_logical_operators() {
    let config = WorldGenConfig::default();

    let mut found_not = false;
    let mut found_and = false;
    let mut found_or = false;
    let mut found_xor = false;

    for seed in 0..300 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        if wf.contains("not ") { found_not = true; }
        if wf.contains(" and ") { found_and = true; }
        if wf.contains(" or ") { found_or = true; }
        if wf.contains(" xor ") { found_xor = true; }

        // Verify worldfiles with logical operators typecheck.
        if found_not || found_and || found_or || found_xor {
            let errors = typecheck_worldfile(&wf);
            if !errors.is_empty() {
                eprintln!("Seed {} failed with logical operators:", seed);
                eprintln!("{}", wf);
                for err in &errors {
                    eprintln!("  {}", err);
                }
                panic!("Logical operator worldfile failed to typecheck");
            }
        }

        if found_not && found_and && found_or && found_xor {
            break;
        }
    }

    assert!(found_not, "Should generate 'not' expressions");
    assert!(found_and, "Should generate 'and' expressions");
    assert!(found_or, "Should generate 'or' expressions");
    assert!(found_xor, "Should generate 'xor' expressions");
}

/// Verify that unary negation is generated and typechecks correctly.
#[test]
fn test_unary_negation() {
    let config = WorldGenConfig::default();

    let mut found_int_neg = false;
    let mut found_float_neg = false;

    for seed in 0..300 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        // Look for unary negation patterns: -(: type / value)
        if wf.contains("-(: int /") { found_int_neg = true; }
        if wf.contains("-(: f32 /") || wf.contains("-(: f64 /") {
            found_float_neg = true;
        }

        // Verify worldfiles with negation typecheck.
        if found_int_neg || found_float_neg {
            let errors = typecheck_worldfile(&wf);
            if !errors.is_empty() {
                eprintln!("Seed {} failed with unary negation:", seed);
                eprintln!("{}", wf);
                for err in &errors {
                    eprintln!("  {}", err);
                }
                panic!("Unary negation worldfile failed to typecheck");
            }
        }

        if found_int_neg && found_float_neg {
            break;
        }
    }

    assert!(found_int_neg, "Should generate unary negation on bigints");
    assert!(found_float_neg, "Should generate unary negation on floats");
}

/// Verify that void functions are generated and typecheck correctly.
#[test]
fn test_void_functions() {
    let config = WorldGenConfig::default();

    let mut found_void_with_ret = false;
    let mut found_void_without_ret = false;

    for seed in 0..200 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        // Look for void function patterns.
        let lines: Vec<&str> = wf.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            // Void function signature: "fun name(...)" without ": type"
            if trimmed.starts_with("fun ") && trimmed.ends_with(")") {
                // Check if the function has a ret statement before end fun.
                let mut has_ret = false;
                for j in (i + 1)..lines.len() {
                    let inner = lines[j].trim();
                    if inner == "end fun" {
                        break;
                    }
                    if inner == "ret" {
                        has_ret = true;
                    }
                }

                if has_ret {
                    found_void_with_ret = true;
                } else {
                    found_void_without_ret = true;
                }
            }
        }

        // Verify worldfiles typecheck.
        if found_void_with_ret || found_void_without_ret {
            let errors = typecheck_worldfile(&wf);
            if !errors.is_empty() {
                eprintln!("Seed {} failed with void functions:", seed);
                eprintln!("{}", wf);
                for err in &errors {
                    eprintln!("  {}", err);
                }
                panic!("Void function worldfile failed to typecheck");
            }
        }

        if found_void_with_ret && found_void_without_ret {
            break;
        }
    }

    assert!(found_void_with_ret, "Should generate void functions with bare ret");
    assert!(found_void_without_ret, "Should generate void functions without ret");
}

/// Verify that bare loops are generated and typecheck correctly.
#[test]
fn test_bare_loops() {
    let config = WorldGenConfig::default();

    let mut found_bare_loop = false;
    let mut found_while_loop = false;

    for seed in 0..200 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        for line in wf.lines() {
            let trimmed = line.trim();
            if trimmed == "loop" {
                found_bare_loop = true;
            }
            if trimmed.starts_with("loop while ") {
                found_while_loop = true;
            }
        }

        // Verify worldfiles typecheck.
        let errors = typecheck_worldfile(&wf);
        if !errors.is_empty() {
            eprintln!("Seed {} failed:", seed);
            eprintln!("{}", wf);
            for err in &errors {
                eprintln!("  {}", err);
            }
            panic!("Bare loop worldfile failed to typecheck");
        }

        if found_bare_loop && found_while_loop {
            break;
        }
    }

    assert!(found_bare_loop, "Should generate bare loops");
    assert!(found_while_loop, "Should generate while loops");
}

/// Verify that modules with cross-module function calls typecheck correctly.
#[test]
fn test_cross_module_calls_typecheck() {
    let config = WorldGenConfig::default();

    // Find seeds that generate cross-module calls and verify they typecheck.
    let mut found_cross_module_call = false;

    for seed in 0..500 {
        let wf = gen_worldfile_seeded(seed, config.clone());

        // Check if this worldfile has a cross-module call.
        let has_cross_call = has_cross_module_call(&wf);

        if has_cross_call {
            found_cross_module_call = true;

            // Verify it typechecks.
            let errors = typecheck_worldfile(&wf);
            if !errors.is_empty() {
                eprintln!("Seed {} has cross-module call but failed typecheck:", seed);
                eprintln!("{}", wf);
                eprintln!("\nErrors:");
                for err in &errors {
                    eprintln!("  {}", err);
                }
                panic!("Cross-module call in seed {} failed to typecheck", seed);
            }
        }
    }

    assert!(
        found_cross_module_call,
        "Should find at least one cross-module function call in 500 seeds"
    );
}

/// Check if a worldfile contains a cross-module function call.
fn has_cross_module_call(wf: &str) -> bool {
    let parts: Vec<&str> = wf.split("----------").collect();

    let mut i = 1;
    while i + 1 < parts.len() {
        let header = parts[i].trim();
        let content = parts[i + 1].trim();

        // Check if this is a module section (not script) with imports.
        if header.starts_with("module local/gen/") && content.contains("require module") {
            // Extract imported function names.
            let mut imported: Vec<String> = Vec::new();
            for line in content.lines() {
                if line.starts_with("import ") {
                    if let Some(name) = line.split('.').last() {
                        imported.push(name.trim().to_string());
                    }
                }
            }

            // Check for calls to imported functions in function bodies.
            let mut in_fun = false;
            for line in content.lines() {
                let l = line.trim();
                if l.starts_with("fun ") {
                    in_fun = true;
                    continue;
                }
                if l == "end fun" {
                    in_fun = false;
                    continue;
                }
                if in_fun {
                    for imp in &imported {
                        let pattern = format!("{}(", imp);
                        if l.contains(&pattern) {
                            return true;
                        }
                    }
                }
            }
        }
        i += 2;
    }

    false
}

