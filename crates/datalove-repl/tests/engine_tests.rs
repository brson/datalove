use std::path::Path;
use datalove_repl as repl;

/// Process a REPL fixture file.
///
/// The file contains multiple inputs separated by a line containing only "---".
/// For each input, we record the parse result, eval result, and environment.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datalove_datafun::Database::default();
    let mut engine = repl::Engine::new(&db)
        .map_err(|e| format!("Failed to create engine: {}", e))?;

    let results = engine.run_source(&source_text);

    // Serialize results to pretty JSON.
    rmx::serde_json::to_string_pretty(&results)
        .map_err(|e| format!("Failed to serialize results: {}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("engine")
        .file_extension("repl")
        .run();
}
