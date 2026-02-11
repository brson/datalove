//! Module memoization analysis tests.
//!
//! Tests Salsa memoization behavior by processing worldfile modules
//! incrementally and verifying parse/typecheck/hash behavior.

use std::path::Path;
use datalove_datafun::memo_analysis;

/// Analyze memoization behavior for a worldfile.
fn analyze_file(path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let analysis = memo_analysis::analyze_memo_worldfile(&content)
        .map_err(|e| format!("Analysis failed: {}", e))?;

    rmx::serde_json::to_string_pretty(&analysis)
        .map_err(|e| format!("Failed to serialize to JSON: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("module_memo")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
