//! IR interpreter tests with skip_const_inlining mode enabled.
//!
//! This test suite is similar to interp_tests but uses skip_const_inlining mode,
//! where const bindings in functions are lowered as let bindings instead
//! of being evaluated at compile time.
//!
//! This tests the runtime behavior of const-like values.

use datalove_datafun_ir::expand_ir_strings;
use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

/// Analyze a worldfile with skip_const_inlining mode and produce RON output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let mut db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Analyze using skip_const_inlining mode.
    let analysis = datafun::constlet_worldfile_analysis::analyze_worldfile_constlet(&mut db, parsed)
        .map_err(|e| format!("Analysis failed: {}", e))?;

    // Serialize to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    let ron_output = ron::ser::to_string_pretty(&analysis, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))?;

    Ok(expand_ir_strings(&ron_output))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("interp_constlet")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
