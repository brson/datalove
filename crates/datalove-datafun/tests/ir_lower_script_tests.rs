//! IR lowering tests for script units.
//!
//! This test suite loads worldfiles with scriptunit-fragment sections,
//! lowers them to IR, and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use std::collections::HashMap;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_compiler::lower::{self, ScriptLowerContext};
use datalove_datafun_compiler::ownership_analysis;
use datalove_datafun_compiler::tracked_script_ownership::ScriptAnalysisData;
use datalove_datafun_compiler::ir_ext::IrTypeExt;
use datalove_datafun_ir::IrType;
use bct::input::Source;
use datalove_datafun_ast::ast::Statement;

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
                let parse_result = datalove_datafun_parser::parse(&db, source_obj);
                let parsed_ast = parse_result.parsed;
                let stmts: Vec<Statement> = parsed_ast.statements.to_vec();

                // Typecheck to get expression types using production path.
                let spans = datalove_datafun_parser::datafun_spans(&db, source_obj);
                let tycheck_result = datalove_datafun_tycheck::type_check_single_script(&db, source_obj, spans, parsed_ast);
                let expr_types = tycheck_result.expr_types(&db);
                let call_targets = tycheck_result.call_targets(&db);

                output.push_str(&format!("--- script unit {} (fragment) ---\n", unit_index));

                // Build map of function name -> resolved param types for type alias support.
                let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
                for (name, func_type) in tycheck_result.function_types(&db) {
                    let param_types: Vec<IrType> = func_type.param_types(&db)
                        .iter()
                        .map(|ty| IrType::from_tycheck(&db, ty))
                        .collect();
                    func_param_types.insert(name.text(&db).S(), param_types);
                }

                // Run drop analysis on all functions first.
                let func_analyses = match ownership_analysis::analyze_script_functions(&db, expr_types, call_targets, &stmts, Some(&func_param_types)) {
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

                // Run script-level ownership analysis.
                // Use for_aot=false since these tests verify REPL behavior with persistent bindings.
                let script_analysis_raw = ownership_analysis::analyze_script_statements(&db, expr_types, call_targets, &stmts, false);

                // Check for script analysis errors.
                if !script_analysis_raw.errors.is_empty() {
                    let error_msgs = ownership_analysis::format_analysis_errors(&script_analysis_raw.errors);
                    output.push_str(&format!("Drop analysis error: {}\n\n", error_msgs));
                    unit_index += 1;
                    continue;
                }

                // Convert to ScriptAnalysisData for lowering.
                let script_analysis = ScriptAnalysisData {
                    schedule: script_analysis_raw.schedule,
                    bindings: script_analysis_raw.bindings,
                    unit_end: script_analysis_raw.unit_end,
                };

                // Script tests don't use modules, so use empty func_id_map.
                let func_id_map = HashMap::new();
                match lower::lower_script_fragment_raw(&db, expr_types, call_targets, &func_id_map, script_ctx.clone(), stmts, func_analyses, script_analysis, Some(&func_param_types)) {
                    Ok(ir_unit) => {
                        output.push_str(&format!("{}", ir_unit));
                        // Update context with exports for next unit.
                        script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.value_types, &ir_unit.slot_types);
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
            WorldfileSection::Module { .. }
            | WorldfileSection::ModuleAdd { .. }
            | WorldfileSection::ModuleRemove { .. }
            | WorldfileSection::ModuleChangeWs { .. }
            | WorldfileSection::ModuleChangeAst { .. }
            | WorldfileSection::ModuleChangeTy { .. } => {
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
