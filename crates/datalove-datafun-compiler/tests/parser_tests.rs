use rmx::prelude::*;
use std::path::Path;

fn analyze_file(path: &Path) -> Result<String, String> {
    let source_text = std::fs::read_to_string(path).X();
    let db = datalove_datafun_compiler::Database::default();
    let source = bct::input::Source::new(&db, source_text.S());

    let script = datalove_datafun_compiler::parser::parse_integration_test(&db, source);
    let serde_ast = datalove_datafun_compiler::ast_serde::Script::from_ast(&db, script);
    Ok(rmx::serde_json::to_string_pretty(&serde_ast).X())
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("parser")
        .file_extension("dfs")
        .run();
}
