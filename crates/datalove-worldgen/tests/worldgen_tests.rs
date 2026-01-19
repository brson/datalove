//! Integration tests for worldfile generation.
//!
//! These tests verify that generated worldfiles parse and typecheck successfully.

use datalove_worldgen::{WorldGenConfig, gen_worldfile_seeded};
use datalove_datafun::{Database, package, package_resolve, package_load_worldfile, to_module_graph, module_graph};
use datalove_datafun_tycheck::typecheck_module_graph;

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
    let parsed_graph = module_graph::parse_module_graph(&db, graph_with_requires.graph, graph_with_requires.resolved_requires);
    let typecheck_result = typecheck_module_graph(&db, parsed_graph);

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
    config.if_probability = 0.0;
    config.loop_probability = 0.0;
    config.function_call_probability = 0.0;

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
