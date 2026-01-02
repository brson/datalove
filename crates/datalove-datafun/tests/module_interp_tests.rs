//! Module-only IR interpreter tests.
//!
//! This test suite runs worldfiles that contain only module sections
//! (no script/scriptunit/expr). Tests execute a nullary `main` function
//! from the `local/test/main` module using the IR-based interpreter
//! and capture the return value.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

/// Analyze a module-only worldfile and produce RON output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Analyze using IR module-only analysis.
    let analysis = datafun::worldfile_analysis_modules::analyze_modules_worldfile(&db, parsed)
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
        .fixture_subdir("module_interp")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
