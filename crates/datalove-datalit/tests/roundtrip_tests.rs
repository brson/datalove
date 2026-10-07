use rmx::prelude::*;
use std::path::Path;
use datalove_datalit as datalit;

/// Round-trip test: parse -> pretty-print (AST) -> parse -> pretty-print (AST).
///
/// Both pretty-prints should be identical.
/// Note: This uses AST-level pretty-printing for now, not runtime pretty-printing,
/// since runtime evaluation isn't fully implemented for all types yet.
fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Step 1: Parse the original datalit.
    let db = datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());
    let ast = datalit::parser::parse_integration_test(&db, source);

    // Step 2: Pretty-print the AST.
    let pretty1 = datalit::pretty::pretty_print(&db, &ast);

    // Step 3: Parse the pretty-printed output.
    let source2 = bct::input::Source::new(&db, pretty1.S());
    let ast2 = datalit::parser::parse_integration_test(&db, source2);

    // Step 4: Pretty-print again.
    let pretty2 = datalit::pretty::pretty_print(&db, &ast2);

    // Step 5: Check that both pretty-prints are identical.
    if pretty1 != pretty2 {
        return Err(format!(
            "Pretty-prints differ:\nFirst:  {}\nSecond: {}",
            pretty1, pretty2
        ));
    }

    Ok(pretty1)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("roundtrip")
        .file_extension("dlt")
        .allow_errors(true)
        .run();
}
