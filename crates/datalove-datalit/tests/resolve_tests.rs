use rmx::prelude::*;
use std::path::Path;
use rmx::serde_json::json;

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let ast = datalove_datalit::parser::parse_integration_test(&db, source);
    let resolved = datalove_datalit::resolve::resolve_names(&db, ast);

    // Convert AST to serde format.
    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);

    // Convert resolutions to JSON-serializable format.
    let resolutions: Vec<_> = resolved.resolutions(&db)
        .iter()
        .map(|entry| {
            let name = entry.name(&db).as_str(&db);
            let resolution = entry.resolution(&db);
            json!({
                "name": name,
                "binding_id": resolution.binding_id(&db).0,
                "scope_depth": resolution.scope_depth(&db)
            })
        })
        .collect();

    // Convert errors to JSON-serializable format.
    let errors: Vec<_> = resolved.errors(&db)
        .iter()
        .map(|entry| {
            let name = entry.name(&db).as_str(&db);
            let error = entry.error(&db);
            let error_str = match error {
                datalove_datalit::resolve::ResolutionError::UnboundName => "UnboundName",
            };
            json!({
                "name": name,
                "error": error_str
            })
        })
        .collect();

    let output = json!({
        "ast": serde_ast,
        "resolutions": resolutions,
        "errors": errors
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("resolve")
        .file_extension("dlt")
        .run();
}
