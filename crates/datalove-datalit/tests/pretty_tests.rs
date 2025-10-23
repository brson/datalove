use rmx::prelude::*;
use std::path::Path;

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let ast = datalove_datalit::parser::parse_integration_test(&db, source);
    Ok(datalove_datalit::pretty::pretty_print(&db, ast))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("pretty")
        .file_extension("dlt")
        .run();
}
