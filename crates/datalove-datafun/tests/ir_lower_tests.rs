//! IR lowering tests.
//!
//! This test suite loads worldfiles, typechecks them as modules, lowers them to IR,
//! and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

use datafun::pipeline::{ModuleCompilationPipeline, ConstInlining};

/// Analyze a worldfile and produce IR output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Build pipeline from sections.
    let mut pipeline = ModuleCompilationPipeline::from_sections(&db, &parsed.sections, ConstInlining::Enabled);

    // Verify local/test/main module exists.
    if !pipeline.contains_module("local", "test", "main") {
        return Err("No local/test/main module found".to_string());
    }

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = pipeline.compile_fresh(&db);

    // Check for errors using consolidated helper methods.
    if let Some(err) = &compiled.resolution_error {
        return Ok(format!("Resolution error: {}\n", err));
    }

    let all_parse_errors = compiled.all_parse_errors();
    if !all_parse_errors.is_empty() {
        return Ok(format!("Parse error: {}\n", all_parse_errors.join("; ")));
    }

    let all_typecheck_errors = compiled.all_typecheck_errors();
    if !all_typecheck_errors.is_empty() {
        return Ok(format!("Typecheck error: {}\n", all_typecheck_errors.join("; ")));
    }

    // Check for lowering errors.
    let all_lowering_errors = compiled.all_lowering_errors();
    if !all_lowering_errors.is_empty() {
        return Ok(format!("{}\n", all_lowering_errors.join("\n")));
    }

    // Collect IR dump from all modules.
    let output: String = compiled.module_ir_dumps.values()
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
