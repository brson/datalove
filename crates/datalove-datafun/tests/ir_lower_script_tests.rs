//! IR lowering tests for script units.
//!
//! This test suite loads worldfiles with scriptunit-fragment sections,
//! lowers them to IR, and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use std::collections::HashMap;
use std::sync::Arc;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_compiler::lower::{self, ScriptLowerContext, lower_script_functions, lower_const_binding};
use datalove_datafun_const::{inline_script_consts, evaluate_const_unit};
use datalove_datafun_compiler::ownership_analysis;
use datalove_datafun_compiler::tracked_script_ownership::ScriptAnalysisData;
use datalove_datafun_compiler::IrTypeExt;
use datalove_datafun_ir::{IrType, ConstBindingInfo, ConstBindingGraph, ConstValue, ResolvedConsts};
use datalove_datafun_interp::InterpCtfeEvaluator;
use bct::input::Source;
use datalove_datafun_ast::ast::Statement;

/// Convert tycheck expression types to IR types.
fn convert_expr_types<'db>(
    db: &'db dyn salsa::Database,
    types: &datalove_datafun_tycheck::ExprTypes<'db>,
) -> datalove_datafun_tycheck::ExprIrTypes<'db> {
    types.iter()
        .map(|(key, ty)| (*key, IrType::from_tycheck(db, ty)))
        .collect()
}

/// Build a simple const binding graph from statements and expr_types.
/// This is a non-tracked version of collect_const_graph for tests.
fn build_const_graph<'db>(
    db: &'db dyn salsa::Database,
    stmts: &[Statement<'db>],
    expr_types: &datalove_datafun_tycheck::ExprTypes<'db>,
) -> ConstBindingGraph {
    let mut bindings = Vec::new();

    for stmt in stmts {
        if let Statement::Const(const_stmt) = stmt {
            let expr = const_stmt.value;
            let stmt_id = datalove_datafun_ir::ConstStmtId(bindings.len() as u32);
            let name = const_stmt.name.text(db).to_string();
            // Get the type from the typechecker's table.
            let ir_type = expr_types
                .get(&datalove_datafun_ast::ast::ExprKey::of(db, expr))
                .map(|ty| IrType::from_tycheck(db, ty))
                .unwrap_or(IrType::Unit);

            bindings.push(ConstBindingInfo {
                stmt_id,
                name,
                ir_type,
                depends_on: Vec::new(), // Simple test case: no dependencies.
            });
        }
    }

    ConstBindingGraph::new(bindings)
}

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
                let parsed_ast = &parse_result.parsed;
                let stmts: Vec<Statement> = parsed_ast.statements.to_vec();

                // Typecheck to get expression types using production path.
                let tycheck_result = datalove_datafun_tycheck::type_check_single_script(&db, source_obj);

                // Check for type errors - if any, skip lowering.
                let tycheck_errors = tycheck_result.errors(&db);
                if !tycheck_errors.is_empty() {
                    output.push_str(&format!("--- script unit {} (fragment) ---\n", unit_index));
                    for err in &tycheck_errors {
                        output.push_str(&format!("Type error: {:?}\n", err.error(&db)));
                    }
                    output.push('\n');
                    unit_index += 1;
                    continue;
                }

                let expr_types_raw = tycheck_result.expr_types(&db);
                let call_targets_raw = tycheck_result.call_targets(&db);

                // Convert to IR types for ownership analysis.
                let expr_types_ir = convert_expr_types(&db, expr_types_raw);

                output.push_str(&format!("--- script unit {} (fragment) ---\n", unit_index));

                // Build map of function name -> resolved param/return types for type alias support.
                let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
                let mut func_return_types: HashMap<String, IrType> = HashMap::new();
                for (name, func_type) in tycheck_result.function_types(&db) {
                    let param_types: Vec<IrType> = func_type.param_types(&db)
                        .iter()
                        .map(|ty| IrType::from_tycheck(&db, ty))
                        .collect();
                    func_param_types.insert(name.text(&db).S(), param_types);
                    let return_type = IrType::from_tycheck(&db, &func_type.return_type(&db));
                    func_return_types.insert(name.text(&db).S(), return_type);
                }

                // Run drop analysis on all functions first.
                let func_analyses = match ownership_analysis::analyze_script_functions(&db, &expr_types_ir, call_targets_raw, &stmts, Some(&func_param_types)) {
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
                let script_analysis_raw = ownership_analysis::analyze_script_statements(&db, &expr_types_ir, call_targets_raw, &stmts);

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
                    tracking: script_analysis_raw.tracking,
                    unit_end: script_analysis_raw.unit_end,
                    adapt_sites: script_analysis_raw.adapt_sites,
                    dead_exports: script_analysis_raw.dead_exports,
                    revived_exports: script_analysis_raw.revived_exports,
                };

                // Script tests don't use modules, so use empty func_id_map.
                let func_id_map = HashMap::new();

                // Lower functions so CTFE can reuse them.
                // Use empty ScriptLowerContext since tests don't accumulate across units.
                // Use empty func_id_map since these tests don't use modules.
                // These fixtures declare no script-level const a body names, so
                // there is nothing to seed and nothing to defer.
                // These fixtures require no data.
                let no_data = lower::DataFiles::new();
                let lowered = lower_script_functions(
                    &db, expr_types_raw, call_targets_raw, &stmts, &func_analyses, None, Some(&func_return_types), &func_id_map, &no_data, ScriptLowerContext::new(),
                    &HashMap::new(), false,
                ).expect("function lowering failed");
                let (lowered_functions, func_name_to_id) = (lowered.functions, lowered.func_name_to_id);

                // Evaluate const bindings using CTFE with "lower then evaluate" pattern.
                let const_graph = build_const_graph(&db, &stmts, expr_types_raw);
                let resolved_consts = if !const_graph.bindings.is_empty() {
                    let mut evaluator = InterpCtfeEvaluator::new();
                    let mut resolved = ResolvedConsts::new();
                    let mut resolved_consts_map: HashMap<String, (IrType, Arc<ConstValue>)> = HashMap::new();
                    let mut ctfe_error = None;

                    // A binding's id is its position among the const statements.
                    let const_exprs: Vec<_> = stmts.iter()
                        .filter_map(|s| match s {
                            Statement::Const(c) => Some(c.value),
                            _ => None,
                        })
                        .collect();

                    for binding in &const_graph.bindings {
                        let expr = const_exprs[binding.stmt_id.0 as usize];

                        // Lower the const binding.
                        let lower_result = lower_const_binding(
                            &db,
                            expr,
                            &binding.ir_type,
                            expr_types_raw,
                            call_targets_raw,
                            &resolved_consts_map,
                            None,
                            &lowered_functions.iter().cloned().map(std::sync::Arc::new).collect::<Vec<_>>(),
                            &func_name_to_id,
                            None, // No module functions for script tests
                            &no_data,
                        );

                        let value = match lower_result {
                            Ok((None, Some(v))) => v,
                            Ok((Some(unit), None)) => {
                                match evaluate_const_unit(&unit, &binding.ir_type, &mut evaluator) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        ctfe_error = Some(format!("CTFE error: {}", e));
                                        break;
                                    }
                                }
                            }
                            Ok(_) => unreachable!(),
                            Err(e) => {
                                ctfe_error = Some(format!("Lowering error: {}", e));
                                break;
                            }
                        };

                        resolved.insert(binding.stmt_id, binding.name.clone(), value.clone());
                        resolved_consts_map.insert(binding.name.clone(), (binding.ir_type.clone(), value));
                    }

                    if let Some(err) = ctfe_error {
                        output.push_str(&format!("{}\n\n", err));
                        unit_index += 1;
                        continue;
                    }

                    Some(resolved)
                } else {
                    None
                };

                // Pass lowered functions to avoid re-lowering them.
                let lowered_funcs_arg = if lowered_functions.is_empty() {
                    None
                } else {
                    Some((lowered_functions, func_name_to_id))
                };

                match lower::lower_script_fragment_raw(&db, expr_types_raw, call_targets_raw, &func_id_map, &no_data, script_ctx.clone(), stmts, func_analyses, script_analysis, Some(&func_param_types), Some(&func_return_types), lowered_funcs_arg) {
                    Ok(ir_code_unit) => {
                        // Inline const values into the IR.
                        let const_values_map: HashMap<String, Arc<datalove_datafun_ir::ConstValue>> = resolved_consts
                            .as_ref()
                            .map(|rc| rc.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
                            .unwrap_or_default();
                        let ir_unit = inline_script_consts(ir_code_unit, &const_values_map);

                        output.push_str(&format!("{}", ir_unit));
                        // Update context with exports for next unit.
                        let exports = ir_unit.script_context().map(|c| &c.exports).expect("script context required");
                        script_ctx.add_exports(unit_index, exports, &ir_unit.value_types, &ir_unit.slot_types);
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
            | WorldfileSection::ModuleChangeTy { .. }
            | WorldfileSection::Data { .. }
            | WorldfileSection::Rider { .. } => {
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
