use rmx::prelude::*;
use rmx::serde_json::json;
use std::path::Path;

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let ast = datalove_datalit::parser::parse_integration_test(&db, source);

    // Collect accumulated parse diagnostics with spans.
    let parse_diagnostics = datalove_datalit::parser::parse_integration_test::accumulated::<datalove_diagnostic::ParseDiagnostic>(&db, source);
    let diagnostics: Vec<_> = parse_diagnostics
        .iter()
        .map(|d| {
            let diag = d.to_diagnostic(&db);
            let code = diag.code.map(|c| c.as_str(&db).to_string());
            let labels: Vec<_> = diag.labels.iter().map(|label| {
                json!({
                    "span": [label.span.start, label.span.end],
                    "text": source_text[label.span.clone()].to_string(),
                    "message": label.message.map(|m| m.as_str(&db).to_string())
                })
            }).collect();
            json!({
                "code": code,
                "message": diag.message.as_str(&db),
                "labels": labels
            })
        })
        .collect();

    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);

    let output = json!({
        "ast": serde_ast,
        "diagnostics": diagnostics
    });

    Ok(rmx::serde_json::to_string_pretty(&output).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("parser")
        .file_extension("dlt")
        .run();
}
