//! IR lowering tests for script units.
//!
//! This test suite loads worldfiles with scriptunit-fragment sections,
//! lowers them to IR, and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::cell::RefCell;
use std::path::Path;
use std::collections::HashMap;
use std::rc::Rc;
use datalove_datafun as datafun;
use datalove_datafun_resolve::resolve_script_names;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_compiler::lower::{self, ScriptLowerContext, evaluate_consts, ScriptLowerOptions};
use datalove_datafun_compiler::const_inline::inline_script_consts;
use datalove_datafun_compiler::ownership_analysis::{self, CallInfo};
use datalove_datafun_compiler::tracked_script_ownership::ScriptAnalysisData;
use datalove_datafun_compiler::ir_ext::IrTypeExt;
use datalove_datafun_ir::{IrType, ConstBindingInfo, ConstBindingGraph};
use datalove_datafun_tycheck::{Type, ResolvedCallTarget};
use datalove_datafun_interp::InterpCtfeEvaluator;
use salsa::plumbing::AsId;
use bct::input::Source;
use datalove_datafun_ast::ast::Statement;

/// Convert tycheck expression types to IR types.
fn convert_expr_types<'db>(
    db: &'db dyn salsa::Database,
    types: &[Option<Type<'db>>],
) -> Vec<Option<IrType>> {
    types.iter()
        .map(|opt| opt.as_ref().map(|ty| IrType::from_tycheck(db, ty)))
        .collect()
}

/// Convert resolved call targets to CallInfo.
fn convert_call_targets<'db>(
    db: &'db dyn salsa::Database,
    targets: &[Option<ResolvedCallTarget<'db>>],
) -> Vec<Option<CallInfo>> {
    targets.iter()
        .map(|opt| opt.as_ref().map(|target| CallInfo {
            param_modes: target.func(db).params(db)
                .iter()
                .map(|p| p.mode)
                .collect()
        }))
        .collect()
}

/// Build a simple const binding graph from statements and expr_types.
/// This is a non-tracked version of collect_const_graph for tests.
fn build_const_graph<'db>(
    db: &'db dyn salsa::Database,
    stmts: &[Statement<'db>],
    expr_types: &[Option<datalove_datafun_tycheck::Type<'db>>],
) -> ConstBindingGraph {
    let mut bindings = Vec::new();

    for stmt in stmts {
        if let Statement::Const(const_stmt) = stmt {
            let expr = const_stmt.value;
            let stmt_id = expr.as_id();
            let name = const_stmt.name.text(db).to_string();
            let expr_id = expr.as_id();

            // Get the type from typechecker using expression ID index.
            let ir_type = expr_types.get(expr_id.index() as usize)
                .cloned()
                .flatten()
                .map(|ty| IrType::from_tycheck(db, &ty))
                .unwrap_or(IrType::Unit);

            bindings.push(ConstBindingInfo {
                stmt_id,
                name,
                expr_id,
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
                let parsed_ast = parse_result.parsed;
                let stmts: Vec<Statement> = parsed_ast.statements.to_vec();

                // Typecheck to get expression types using production path.
                let spans = datalove_datafun_parser::datafun_spans(&db, source_obj);
                let name_resolution = resolve_script_names(&db, source_obj, parsed_ast.clone());
                let tycheck_result = datalove_datafun_tycheck::type_check_single_script(&db, source_obj, spans, parsed_ast, name_resolution);
                let expr_types_raw = tycheck_result.expr_types(&db);
                let call_targets_raw = tycheck_result.call_targets(&db);

                // Convert to IR types for ownership analysis.
                let expr_types_ir = convert_expr_types(&db, expr_types_raw);
                let call_info = convert_call_targets(&db, call_targets_raw);

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
                let func_analyses = match ownership_analysis::analyze_script_functions(&db, &expr_types_ir, &call_info, &stmts, Some(&func_param_types)) {
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
                let script_analysis_raw = ownership_analysis::analyze_script_statements(&db, &expr_types_ir, &call_info, &stmts);

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
                };

                // Script tests don't use modules, so use empty func_id_map.
                let func_id_map = HashMap::new();

                // Evaluate const bindings using CTFE (Phase 2).
                let const_graph = build_const_graph(&db, &stmts, expr_types_raw);
                let resolved_consts = if !const_graph.bindings.is_empty() {
                    let evaluator = Rc::new(RefCell::new(InterpCtfeEvaluator::new()));
                    match evaluate_consts(&db, &const_graph, &stmts, expr_types, call_targets, &func_analyses, evaluator) {
                        Ok(resolved) => Some(resolved),
                        Err(e) => {
                            output.push_str(&format!("CTFE error: {:?}\n\n", e));
                            unit_index += 1;
                            continue;
                        }
                    }
                } else {
                    None
                };

                // Use const_as_let mode, then inline consts after lowering.
                let options = ScriptLowerOptions { const_as_let: true };

                match lower::lower_script_fragment_raw(&db, expr_types, call_targets, &func_id_map, script_ctx.clone(), stmts, func_analyses, script_analysis, Some(&func_param_types), None, options) {
                    Ok(ir_unit) => {
                        // Inline const values into the IR.
                        let const_values_map: HashMap<String, datalove_datafun_ir::ConstValue> = resolved_consts
                            .as_ref()
                            .map(|rc| rc.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
                            .unwrap_or_default();
                        let ir_unit = inline_script_consts(ir_unit, &const_values_map);

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
