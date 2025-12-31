//! IR lowering tests for script units.
//!
//! This test suite loads worldfiles with scriptunit-fragment sections,
//! lowers them to IR, and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_compiler::ir;
use datalove_datafun_compiler::ir::lower::{ScriptLowerContext, ScriptUnitKind};
use bct::input::Source;
use datalove_datafun_compiler::ast::Statement;

/// Analyze a worldfile and produce IR output for script units.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    let mut output = String::new();
    let mut script_ctx = ScriptLowerContext::new();
    let mut unit_index = 0u32;

    for section in &parsed.sections {
        match section {
            WorldfileSection::ScriptFragment { source } => {
                // Parse the fragment to get statements.
                let source_obj = Source::new(&db, source.clone());
                let parse_result = datalove_datafun_compiler::parser::parse(&db, source_obj);
                let script_ast = parse_result.script(&db);
                let stmts: Vec<Statement> = script_ast.statements(&db).to_vec();

                // Typecheck to get expression types.
                let tycheck_result = datalove_datafun_compiler::tycheck::type_check(&db, source_obj, script_ast);
                let expr_types = tycheck_result.expr_types(&db);

                output.push_str(&format!("--- script unit {} (fragment) ---\n", unit_index));

                // Run drop analysis on all functions first.
                let func_analyses = match ir::lower::analyze_script_functions(&db, expr_types, &stmts) {
                    Ok(analyses) => analyses,
                    Err(errors) => {
                        for (func_name, errs) in errors {
                            let error_msgs: Vec<String> = errs.iter()
                                .map(|e| format!("{:?}", e))
                                .collect();
                            output.push_str(&format!("Drop analysis error in {}: {}\n", func_name, error_msgs.join("; ")));
                        }
                        output.push('\n');
                        unit_index += 1;
                        continue;
                    }
                };

                match ir::lower::lower_script_unit(&db, tycheck_result, script_ctx.clone(), ScriptUnitKind::Fragment(stmts), func_analyses) {
                    Ok(ir_unit) => {
                        output.push_str(&format!("{}", ir_unit));
                        // Update context with exports for next unit.
                        script_ctx.add_exports(unit_index, &ir_unit.exports);
                        script_ctx.current_unit = unit_index + 1;
                    }
                    Err(e) => {
                        output.push_str(&format!("Error: {}\n", e));
                    }
                }
                output.push('\n');
                unit_index += 1;
            }
            WorldfileSection::ScriptExpr { source: _ } => {
                // TODO: ScriptExpr requires unified typechecking infrastructure.
                // For now, skip bare expression tests.
                output.push_str(&format!("--- script unit {} (expr) ---\n", unit_index));
                output.push_str("(skipped: expr typechecking not yet implemented)\n");
                output.push('\n');
                unit_index += 1;
            }
            WorldfileSection::Module { .. } => {
                // Skip module sections in script unit tests.
            }
        }
    }

    if output.is_empty() {
        output.push_str("(no script units)\n");
    }

    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("ir_lower_script")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
