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
    let script = datafun::parser::parse_for_diagnostics(&db, source);

    // Type check the script.
    let tycheck_result = datafun::tycheck::type_check(&db, source, script);
    if !tycheck_result.errors(&db).is_empty() {
        return Err(format!("Type check errors: {} error(s)", tycheck_result.errors(&db).len()));
    }

    // Build type table.
    let mut tydesc_table = datafun::datalit::tydesc_table::TyDescTable::new(&db);
    let type_table = datafun::interp_old::type_table::TypeTable::build(&db, script, tycheck_result, &mut tydesc_table)
        .map_err(|e| format!("Failed to build type table: {}", e))?;

    // Create interpreter context.
    let mut ctx = datafun::interp_old::interp::InterpContext::new(&db, type_table);

    // Execute the script.
    ctx.execute(script)
        .map_err(|e| format!("Execution error: {:?}", e))?;

    // Pretty-print the 'output' variable.
    let output_name = bct::text::InternedText::new(&db, S("output"));
    ctx.pretty_print_variable(output_name)
        .map_err(|e| format!("Failed to pretty-print output: {:?}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("interp")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
