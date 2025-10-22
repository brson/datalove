use rmx::prelude::*;
use std::path::Path;

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datalit::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let parse_result = datalove_datalit::parser::parse(&db, source);
    let ast = parse_result.expr;
    let serde_ast = datalove_datalit::ast_serde::ExprFull::from_ast(&db, ast);
    Ok(rmx::serde_json::to_string_pretty(&serde_ast).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("parser")
        .file_extension("dlt")
        .run();
}
