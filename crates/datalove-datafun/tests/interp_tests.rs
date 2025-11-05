//! Tests for the new analysis-driven interpreter.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;

/// Run a script and extract the value of the 'output' variable.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    // Parse the script.
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    // Create an empty package world.
    let empty_sys = rmx::std::collections::BTreeMap::new();
    let empty_local = rmx::std::collections::BTreeMap::new();
    let package_world = datafun::package::PackageWorld::new(&db, empty_sys, empty_local);

    // Execute the script.
    let result = datafun::interp::execute_script(&db, script, package_world);

    match result {
        Ok(mut script_result) => {
            // Pretty-print the output variable.
            datafun::interp::pretty_print_value(&mut script_result)
                .map_err(|e| format!("Failed to pretty-print output: {:?}", e))
        }
        Err(datafun::interp::InterpError::NoOutputVariable) => {
            // Return Ok with error message for expected error cases.
            Ok("Error: NoOutputVariable".to_string())
        }
        Err(e) => {
            Err(format!("Execution error: {:?}", e))
        }
    }
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("interp2")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
