//! Differential tests for comptime specialization.
//!
//! This test suite verifies that comptime specialization produces identical
//! runtime behavior to unspecialized execution. For each test fixture,
//! the worldfile is analyzed twice:
//! 1. With specialization enabled (union-branch dispatch)
//! 2. With specialization disabled (original function with comptime args)
//!
//! The debug outputs must match, demonstrating that specialization is
//! semantically correct.

use datalove_datafun_ir::expand_ir_strings;
use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;

/// Analyze a worldfile with differential specialization and produce RON output.
///
/// Runs analysis twice (specialized vs unspecialized) and verifies outputs match.
/// If they differ, the output includes the diff information.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let mut db = datafun::Database::default();

    // Run differential analysis.
    let result = datafun::specialize_differential_analysis::analyze_worldfile_differential_from_bytes(
        &mut db,
        file_bytes.as_slice(),
    ).map_err(|e| format!("Differential analysis failed: {}", e))?;

    // Build output showing both results and comparison.
    let mut output = String::new();

    // Add comparison result.
    if result.outputs_match {
        output.push_str("// PASS: Specialized and unspecialized outputs match\n\n");
    } else {
        output.push_str("// FAIL: Outputs differ between specialized and unspecialized\n");
        output.push_str("// Differences:\n");
        for diff in &result.differences {
            output.push_str(&format!("//   Section {}: {:?}\n", diff.section_index, diff.section_name));
            output.push_str(&format!("//     Specialized: {:?}\n", diff.specialized_output));
            output.push_str(&format!("//     Unspecialized: {:?}\n", diff.unspecialized_output));
        }
        output.push_str("\n");
    }

    // Serialize specialized result to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    let ron_output = ron::ser::to_string_pretty(&result.specialized, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))?;

    output.push_str(&expand_ir_strings(&ron_output));

    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("specialize_differential")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
