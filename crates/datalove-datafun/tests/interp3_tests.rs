//! IR interpreter tests with script units (interp3).
//!
//! This test suite runs worldfiles that contain module sections along with
//! scriptunit-fragment and scriptunit-expr sections. Tests execute script
//! units sequentially using the new IR-based interpreter.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

/// Analyze a worldfile and produce RON output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Analyze using IR3 analysis.
    let analysis = datafun::worldfile_analysis_ir3::analyze_worldfile_ir3(&db, parsed)
        .map_err(|e| format!("Analysis failed: {}", e))?;

    // Serialize to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    ron::ser::to_string_pretty(&analysis, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("interp3")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
