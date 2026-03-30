//! Tests for standard library functions using the IR3 interpreter.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datafun::pipeline::{WorkspaceDescriptor, CompilerOptions, TypecheckResult, LoweringResult};

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

    // Build workspace descriptor from loaded packages.
    let descriptor = WorkspaceDescriptor::from_package_world(
        &package_world_raw,
        CompilerOptions::default(),
    );

    // Create pipeline from descriptor and compile.
    let mut pipeline = descriptor.to_pipeline(&db);
    let compiled = pipeline.compile_fresh(&db);

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

    // Create script compiler and executor (module specs are built internally from module graph).
    let Some(mut compiler) = compiled.script_compiler_default(&db) else {
        return Err("Module compilation failed".to_string());
    };
    let Some(mut executor) = compiled.script_executor(datafun::DebugOutputMode::Disabled, None) else {
        return Err("Module compilation failed".to_string());
    };

    // Build unified native component and load it.
    let mut _loaded_riders = Vec::new();
    let rider_crate_dirs = descriptor.rider_crate_dirs();
    if !rider_crate_dirs.is_empty() {
        let build_result = datafun::pipeline::rider_build::build_native_component(&rider_crate_dirs)
            .map_err(|e| format!("rider build error: {}", e))?;
        let native_symbols = compiled.native_symbols();
        if !native_symbols.is_empty() {
            let loaded = datafun::pipeline::rider_load::load_rider_library(
                &build_result.cdylib_path,
                "native-component",
                &native_symbols,
                executor.native_table_mut(),
            ).map_err(|e| format!("rider load error: {}", e))?;
            _loaded_riders.push(loaded);
        }
    }

    // Compile the script as a fragment.
    let result = compiler.compile_fragment(&script_text);

    // Check for errors.
    if let TypecheckResult::Error { errors } = &result.typecheck {
        executor.destroy_live_values();
        return Err(format!("Script typecheck errors: {:?}", errors));
    }
    if let LoweringResult::Error { message } = &result.lowering {
        executor.destroy_live_values();
        return Err(format!("Script lowering error: {}", message));
    }

    // Execute if compilation succeeded.
    if let Some(ir_unit) = &result.ir_unit {
        let output = executor.execute_fragment(ir_unit);
        if output.starts_with("Error:") {
            executor.destroy_live_values();
            return Err(output);
        }
    }

    // Compile and evaluate the output variable.
    let output_compiled = compiler.compile_expr("output");

    // Check for errors.
    if let TypecheckResult::Error { errors } = &output_compiled.typecheck {
        executor.destroy_live_values();
        return Err(format!("Output typecheck errors: {:?}", errors));
    }
    if let LoweringResult::Error { message } = &output_compiled.lowering {
        executor.destroy_live_values();
        return Err(format!("Output lowering error: {}", message));
    }

    // Execute the output expression.
    let output = if let Some(ir_unit) = &output_compiled.ir_unit {
        let (_, value) = executor.execute_expr(ir_unit);
        value
    } else {
        String::new()
    };
    executor.destroy_live_values();

    if output.starts_with("Error:") {
        return Err(output);
    }

    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("std_tests")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
