use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let ast = datalove_datalit::parser::parse_integration_test(&db, source);
    let _resolved = datalove_datalit::resolve::resolve_names(&db, source, ast.clone());

    // Convert AST to serde format.
    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);

    // With named types removed, resolutions are always empty.
    let output = json!({
        "ast": serde_ast,
        "resolutions": [],
        "errors": []
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("resolve")
        .file_extension("dlt")
        .run();
}
