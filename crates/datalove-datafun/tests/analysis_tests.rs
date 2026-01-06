//! Analysis test suite.
//!
//! Tests the prototype termination detection and refinement type analysis.
//! Ingests worldfiles and outputs serialized ModuleGraphAnalysis results.

use std::path::Path;

use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile;

/// Analyze a worldfile and produce RON output of analysis results.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Create database and pipeline.
    let db = datafun::Database::default();
    let mut pipeline = datafun::pipeline::ModuleCompilationPipeline::new(&db);

    // Add modules from worldfile sections.
    pipeline.add_modules_from_sections(&parsed.sections);

    // Enable analysis.
    pipeline.enable_analysis(true);

    // Compile.
    let compiled = pipeline.compile();

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        return Err(format!("Resolution error: {}", err));
    }

    // Check for typecheck errors (report but don't fail).
    let mut errors = Vec::new();
    for (path, errs) in &compiled.path_to_errors {
        if !errs.is_empty() {
            errors.push(format!("{}: {}", path, errs.join("; ")));
        }
    }

    // Build output combining errors and analysis.
    let output = AnalysisOutput {
        errors,
        analysis: compiled.analysis,
    };

    // Serialize to RON format.
    let ron_config = ron::ser::PrettyConfig::new()
        .struct_names(true)
        .enumerate_arrays(false)
        .compact_arrays(false);

    ron::ser::to_string_pretty(&output, ron_config)
        .map_err(|e| format!("Failed to serialize to RON: {}", e))
}

/// Combined output for test fixtures.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct AnalysisOutput {
    /// Typecheck errors (if any).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<String>,
    /// Analysis results.
    analysis: datalove_datafun_analysis::ModuleGraphAnalysis,
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("analysis")
        .file_extension("world")
        .allow_errors(false)
        .run();
}
