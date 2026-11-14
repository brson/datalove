//! IR lowering tests.
//!
//! This test suite loads worldfiles, typechecks them as modules, lowers them to IR,
//! and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

use datafun::driver::{ModuleCompiler, CompilerOptions};

/// Analyze a worldfile and produce IR output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Build the module compiler from sections.
    let mut module_compiler = ModuleCompiler::from_sections(
        &db,
        &parsed.sections,
        CompilerOptions { keep_ir_dumps: true, ..CompilerOptions::default() },
    );

    // Verify local/test/main module exists.
    if !module_compiler.contains_module("local", "test", "main") {
        return Err("No local/test/main module found".S());
    }

    // Compile modules (typecheck, drop analysis, lower).
    let compiled = module_compiler.compile(&db);

    // Check for errors using consolidated helper methods.
    // The first phase that failed, as its messages.
    let messages = |by_path: &std::collections::BTreeMap<String, Vec<bct::diagnostic::StoredDiagnostic>>| -> Vec<String> {
        by_path.values().flatten().map(|d| d.message.clone()).collect()
    };
    let parse = messages(&compiled.parse_diagnostics);
    if !parse.is_empty() {
        return Ok(format!("Parse error: {}\n", parse.join("; ")));
    }
    let types = messages(&compiled.type_diagnostics);
    if !types.is_empty() {
        return Ok(format!("Typecheck error: {}\n", types.join("; ")));
    }
    let ownership = messages(&compiled.ownership_diagnostics);
    if !ownership.is_empty() {
        return Ok(format!("Ownership error: {}\n", ownership.join("; ")));
    }
    let lowering = messages(&compiled.lowering_diagnostics);
    if !lowering.is_empty() {
        return Ok(format!("{}\n", lowering.join("\n")));
    }

    // Collect IR dump from all modules.
    let output: String = compiled.module_ir_dumps.values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");

    if output.is_empty() {
        Ok("(no functions)\n".S())
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
