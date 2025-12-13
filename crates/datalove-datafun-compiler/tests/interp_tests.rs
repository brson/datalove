//! Tests for the new analysis-driven interpreter using worldfile format.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun_compiler as datafun;

/// Analyze a worldfile and produce RON output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = datafun::package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Analyze all sections.
    let analyses = datafun::worldfile_analysis::analyze_worldfile(&db, parsed)
        .map_err(|e| format!("Analysis failed: {}", e))?;

    // Serialize to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    ron::ser::to_string_pretty(&analyses, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("interp")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
