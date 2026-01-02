//! IR lowering tests.
//!
//! This test suite loads worldfiles, typechecks them as modules, lowers them to IR,
//! and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

use datafun::pipeline::ModuleCompilationPipeline;

/// Analyze a worldfile and produce IR output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Build pipeline and add modules.
    let mut pipeline = ModuleCompilationPipeline::new(&db);
    pipeline.add_modules_from_sections(&parsed.sections);

    // Verify local/test/main module exists.
    let local_lib = pipeline.pkglib_local().get("test")
        .ok_or_else(|| "No local/test package found".to_string())?;
    if !local_lib.modules.contains_key("main") {
        return Err("No local/test/main module found".to_string());
    }

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = pipeline.compile();

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        return Ok(format!("Resolution error: {}\n", err));
    }

    // Check for typecheck errors.
    let all_typecheck_errors: Vec<String> = compiled.path_to_errors.values()
        .flatten()
        .cloned()
        .collect();
    if !all_typecheck_errors.is_empty() {
        return Ok(format!("Typecheck error: {}\n", all_typecheck_errors.join("; ")));
    }

    // Collect IR dump from all modules.
    let output: String = compiled.module_lowering_results.values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    if output.is_empty() {
        Ok("(no functions)\n".to_string())
    } else {
        Ok(output + "\n")
    }
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("ir_lower")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
