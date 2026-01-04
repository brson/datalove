//! Tests for standard library functions using the IR3 interpreter.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datafun::pipeline::{ModuleCompilationPipeline, TypecheckResult, LoweringResult};

/// Run a script with the std library loaded from sys/ directory.
fn analyze_file(path: &Path) -> Result<String, String> {
    let db = datafun::Database::default();

    // Read the script file.
    let script_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Load package world from sys/ directory.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let sys_dir = std::path::PathBuf::from(manifest_dir)
        .parent().unwrap()
        .parent().unwrap()
        .join("sys");

    let config = datafun::package_load::PackageWorldConfig {
        dir_pkglib_system: sys_dir,
        dir_pkglib_local: None,
    };

    let package_world_raw = rmx::futures::executor::block_on(
        datafun::package_load::load_world(config)
    ).map_err(|e| format!("Failed to load package world: {}", e))?;

    // Build pipeline with loaded packages.
    let mut pipeline = ModuleCompilationPipeline::new(&db);

    // Add all modules from the loaded package world.
    for (pkg_name, pkg) in &package_world_raw.pkglib_system {
        for (mod_name, pkg_mod) in &pkg.modules {
            pipeline.add_module("sys", pkg_name, mod_name, &pkg_mod.text);
        }
    }

    // Compile modules.
    let compiled = pipeline.compile();

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        return Err(format!("Package resolution error: {}", err));
    }

    // Check for typecheck errors in modules.
    for (path, errors) in &compiled.path_to_errors {
        if !errors.is_empty() {
            return Err(format!("Typecheck errors in {}: {:?}", path, errors));
        }
    }

    // Create script compilation context (module specs are built internally from module graph).
    let mut ctx = compiled.script_context(&db, datafun::DebugOutputMode::Disabled);

    // Run the script as a fragment.
    let result = ctx.eval_fragment(&script_text);

    // Check for errors.
    if let TypecheckResult::Error { errors } = &result.typecheck {
        ctx.destroy_all();
        return Err(format!("Script typecheck errors: {:?}", errors));
    }
    if let LoweringResult::Error { message } = &result.lowering {
        ctx.destroy_all();
        return Err(format!("Script lowering error: {}", message));
    }
    if result.output.starts_with("Error:") {
        ctx.destroy_all();
        return Err(result.output);
    }

    // Evaluate the output variable.
    let output_result = ctx.eval_expr("output");
    ctx.destroy_all();

    // Check for errors.
    if let TypecheckResult::Error { errors } = &output_result.typecheck {
        return Err(format!("Output typecheck errors: {:?}", errors));
    }
    if let LoweringResult::Error { message } = &output_result.lowering {
        return Err(format!("Output lowering error: {}", message));
    }
    if output_result.output.starts_with("Error:") {
        return Err(output_result.output);
    }

    Ok(output_result.output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("std_tests")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
