//! Worldfile analysis using the IR interpreter (interp3).
//!
//! This module provides test infrastructure for worldfiles that may contain
//! module sections along with scriptunit-fragment and scriptunit-expr sections.
//! Tests execute script units sequentially using the new IR-based interpreter.
//!
//! Script units are executed with a shared environment, allowing later units
//! to reference values, slots, and functions from earlier units.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_compiler::ir;
use datalove_datafun_compiler::tycheck::{
    type_check_script_units, UnitTypecheckResultTracked,
    ScriptUnitSpec, ModuleSpec, ScriptBatchSpec, UnitKindTag,
};
use ir::interp::{ScriptEnvironment, UnitCompletion};

use crate::pipeline::{
    ModuleCompilationPipeline, TypecheckResult, LoweringResult, format_module_lowering_result,
};

/// Result of analyzing a worldfile with IR interpreter.
#[derive(Debug, Serialize, Deserialize)]
pub struct Ir3Analysis {
    /// Per-section results.
    pub sections: Vec<SectionResult>,
}

/// Result of analyzing one section.
#[derive(Debug, Serialize, Deserialize)]
pub struct SectionResult {
    /// Section type.
    pub section_type: String,
    /// Section name/identifier (for modules).
    pub name: Option<String>,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value (for expression units) or function call result.
    pub output: String,
}

/// Parsed script unit info for processing.
struct ParsedUnit<'db> {
    #[allow(dead_code)]
    source: bct::input::Source,
    kind: ParsedUnitKind<'db>,
}

enum ParsedUnitKind<'db> {
    Fragment(datalove_datafun_compiler::ast::Script<'db>),
    Expr(datalove_datafun_compiler::ast::ExprFun<'db>),
}

/// Analyze a worldfile using the IR interpreter.
///
/// This function processes sections in order:
/// 1. Module sections: typechecked but not executed
/// 2. scriptunit-fragment sections: typechecked, lowered to IR, executed
/// 3. scriptunit-expr sections: typechecked, lowered to IR, executed, result captured
///
/// Script units share a `ScriptLowerContext` (for cross-unit name resolution during lowering)
/// and a `ScriptEnvironment` (for cross-unit value/function access during execution).
pub fn analyze_worldfile_ir3(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Ir3Analysis> {
    let mut results = Vec::new();

    // Build pipeline and add modules.
    let mut pipeline = ModuleCompilationPipeline::new(db);
    pipeline.add_modules_from_sections(&parsed.sections);

    // Compile modules (typecheck, drop analysis, lower).
    let mut compiled = pipeline.compile();

    // Check for resolution errors.
    if let Some(err) = &compiled.resolution_error {
        // Add a synthetic module result with the resolution error.
        results.push(SectionResult {
            section_type: "resolution".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: vec![err.clone()] },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
        return Ok(Ir3Analysis { sections: results });
    }

    // Build module specs for script unit typechecking.
    let mut module_specs: Vec<ModuleSpec> = Vec::new();
    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            let module_path = format!("{}/{}/{}", library, package, module);
            let src = bct::input::Source::new(db, source.to_string());
            module_specs.push(ModuleSpec::new(db, module_path, src));
        }
    }

    // Build ScriptLowerContext from compiled modules.
    let mut script_ctx = ir::lower::ScriptLowerContext::new();
    for (name, (module_id, func_id)) in &compiled.all_module_functions {
        if let Some(ir_func) = compiled.env.registry.get_module_function(*module_id, *func_id) {
            script_ctx.add_module_function(name.clone(), *module_id, *func_id, ir_func.clone());
        }
    }

    // Collect all script units - build specs for typechecking, parse for execution.
    let mut parsed_units: Vec<ParsedUnit> = Vec::new();
    let mut unit_specs: Vec<ScriptUnitSpec> = Vec::new();

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Modules are handled by pipeline.
            }
            WorldfileSection::ScriptFragment { source } => {
                let src = bct::input::Source::new(db, source.to_string());
                let parse_result = datalove_datafun_compiler::parser::parse(db, src);
                let script = parse_result.script(db);
                parsed_units.push(ParsedUnit {
                    source: src,
                    kind: ParsedUnitKind::Fragment(script),
                });
                unit_specs.push(ScriptUnitSpec::new(db, src, UnitKindTag::Fragment));
            }
            WorldfileSection::ScriptExpr { source } => {
                let src = bct::input::Source::new(db, source.to_string());
                let expr = datalove_datafun_compiler::parser::parse_expr(db, src);
                parsed_units.push(ParsedUnit {
                    source: src,
                    kind: ParsedUnitKind::Expr(expr),
                });
                unit_specs.push(ScriptUnitSpec::new(db, src, UnitKindTag::Expr));
            }
        }
    }

    let mut interp = ir::interp::IrInterpreter::new();

    // Process each section with incremental typechecking.
    // We accumulate unit specs and re-typecheck after adding each unit to simulate
    // REPL behavior where each script unit is typechecked with knowledge of all prior units.
    let mut accumulated_unit_specs: Vec<ScriptUnitSpec> = Vec::new();
    let mut unit_idx = 0;
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { library, package, module, .. } => {
                let module_path = format!("{}/{}/{}", library, package, module);

                // Look up typecheck errors for this module.
                let typecheck = match compiled.path_to_errors.get(&module_path) {
                    Some(errors) if !errors.is_empty() => {
                        TypecheckResult::Error { errors: errors.clone() }
                    }
                    _ => TypecheckResult::Success,
                };

                // Look up drop analysis errors.
                let drop_key = format!("{}", module_path);
                let has_drop_errors = compiled.drop_analysis_errors.keys()
                    .any(|k| k.starts_with(&drop_key));

                // Look up lowering results for this module.
                let has_typecheck_errors = matches!(&typecheck, TypecheckResult::Error { .. });
                let lowering = if has_drop_errors {
                    let errors: Vec<_> = compiled.drop_analysis_errors.iter()
                        .filter(|(k, _)| k.starts_with(&drop_key))
                        .flat_map(|(_, v)| v.iter().cloned())
                        .collect();
                    LoweringResult::Error { message: format!("Drop analysis errors: {}", errors.join("; ")) }
                } else {
                    match compiled.module_lowering_results.get(&module_path) {
                        Some(ir_dumps) => format_module_lowering_result(ir_dumps, has_typecheck_errors),
                        None => LoweringResult::Skipped,
                    }
                };

                results.push(SectionResult {
                    section_type: "module".to_string(),
                    name: Some(module_path),
                    typecheck,
                    lowering,
                    output: String::new(),
                });
            }

            WorldfileSection::ScriptFragment { .. } => {
                let parsed_unit = &parsed_units[unit_idx];

                // Incremental typecheck: add this unit's spec and re-typecheck all accumulated units.
                // Memoization makes this efficient - only the new unit requires typechecking work.
                accumulated_unit_specs.push(unit_specs[unit_idx].clone());
                let batch_spec = ScriptBatchSpec::new(db, accumulated_unit_specs.clone(), module_specs.clone());
                let typecheck_results = type_check_script_units(db, batch_spec);
                let all_results = typecheck_results.results(db);
                let tycheck_result = *all_results.last().unwrap();

                unit_idx += 1;

                let result = process_fragment(
                    db,
                    parsed_unit,
                    tycheck_result,
                    &mut script_ctx,
                    &mut compiled.env,
                    &mut interp,
                );
                results.push(result);
            }

            WorldfileSection::ScriptExpr { .. } => {
                let parsed_unit = &parsed_units[unit_idx];

                // Incremental typecheck: add this unit's spec and re-typecheck all accumulated units.
                accumulated_unit_specs.push(unit_specs[unit_idx].clone());
                let batch_spec = ScriptBatchSpec::new(db, accumulated_unit_specs.clone(), module_specs.clone());
                let typecheck_results = type_check_script_units(db, batch_spec);
                let all_results = typecheck_results.results(db);
                let tycheck_result = *all_results.last().unwrap();

                unit_idx += 1;

                let result = process_expr(
                    db,
                    parsed_unit,
                    tycheck_result,
                    &mut script_ctx,
                    &mut compiled.env,
                    &mut interp,
                );
                results.push(result);
            }
        }
    }

    // Cleanup: destroy all values in frames to prevent memory leaks.
    compiled.env.destroy_all(interp.runtime_handle());

    Ok(Ir3Analysis { sections: results })
}

/// Process a script fragment unit.
fn process_fragment<'db>(
    db: &'db dyn salsa::Database,
    parsed_unit: &ParsedUnit<'db>,
    tycheck_result: UnitTypecheckResultTracked<'db>,
    script_ctx: &mut ir::lower::ScriptLowerContext,
    env: &mut ScriptEnvironment,
    interp: &mut ir::interp::IrInterpreter,
) -> SectionResult {
    // Check for typecheck errors.
    let tycheck_errors: Vec<_> = tycheck_result.errors(db).into_iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();
    if !tycheck_errors.is_empty() {
        return SectionResult {
            section_type: "scriptunit-fragment".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: tycheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        };
    }

    let script = match &parsed_unit.kind {
        ParsedUnitKind::Fragment(s) => *s,
        _ => unreachable!(),
    };

    // Process require/import statements to populate import tracking.
    for statement in script.statements(db) {
        match statement {
            datalove_datafun_compiler::ast::Statement::Require(
                datalove_datafun_compiler::ast::StmtRequire::Module(req)
            ) => {
                let import_space = req.import_space(db).text(db).to_string();
                let package_alias = req.package_alias(db).text(db).to_string();
                let module_alias = req.module_alias(db).text(db).to_string();
                let full_path = format!("{}/{}/{}", import_space, package_alias, module_alias);
                script_ctx.add_module_alias(module_alias, full_path);
            }
            datalove_datafun_compiler::ast::Statement::Import(import) => {
                let item_name = import.item_name(db).text(db).to_string();
                script_ctx.import_module_function(item_name);
            }
            _ => {}
        }
    }

    // Run drop analysis on all functions first.
    let expr_types = tycheck_result.expr_types(db);
    let stmts = script.statements(db).to_vec();
    let func_analyses = match ir::drop_analysis::analyze_script_functions(db, expr_types, &stmts) {
        Ok(analyses) => analyses,
        Err(errors) => {
            let error_msgs: Vec<String> = errors.into_iter()
                .map(|(func_name, errs)| {
                    let errs_str: Vec<String> = errs.iter().map(|e| format!("{:?}", e)).collect();
                    format!("{}: {}", func_name, errs_str.join("; "))
                })
                .collect();
            return SectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: format!("Drop analysis errors: {}", error_msgs.join(", ")) },
                output: String::new(),
            };
        }
    };

    // Lower using the typecheck result's expr_types.
    let ir_unit = match ir::lower::lower_script_fragment_raw(
        db,
        expr_types,
        script_ctx.clone(),
        stmts,
        func_analyses,
    ) {
        Ok(unit) => unit,
        Err(e) => {
            return SectionResult {
                section_type: "scriptunit-fragment".to_string(),
                name: None,
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: format!("{}", e) },
                output: String::new(),
            };
        }
    };

    // Format IR dump.
    let ir_dump = format!("{}", ir_unit);

    // Execute the fragment with shared environment.
    // ret_dest is sized for Result<(), Error> in case of early return.
    let mut tydesc_table = ir::interp::IrTyDescTable::new();
    let ret_type = ir::IrType::Result(Box::new(ir::IrType::Unit));
    let ret_tydesc = tydesc_table.get_or_create(&ret_type);
    let ret_size = unsafe { (*ret_tydesc).size };
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = ir::interp::Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    // Fragments have no expression result, so expr_dest is None.
    let output = match interp.execute_script_unit_in_env(&ir_unit, env, ret_dest, None) {
        Ok(UnitCompletion::Normal) => "(fragment executed)".to_string(),
        Ok(UnitCompletion::EarlyReturn) => {
            // Early return - pretty print the Result<(), Error> value.
            let value = ir::interp::Value {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };
            let output_str = interp.pretty_print_value(&value)
                .unwrap_or_else(|e| format!("Error: {:?}", e));
            let _ = interp.destroy_value(&value);
            output_str
        }
        Err(e) => format!("Error: {:?}", e),
    };

    // Update script context with exports from this unit for subsequent lowering.
    let unit_index = script_ctx.current_unit;
    script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.slot_types);
    script_ctx.current_unit += 1;

    SectionResult {
        section_type: "scriptunit-fragment".to_string(),
        name: None,
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    }
}

/// Process a script expression unit.
fn process_expr<'db>(
    db: &'db dyn salsa::Database,
    parsed_unit: &ParsedUnit<'db>,
    tycheck_result: UnitTypecheckResultTracked<'db>,
    script_ctx: &mut ir::lower::ScriptLowerContext,
    env: &mut ScriptEnvironment,
    interp: &mut ir::interp::IrInterpreter,
) -> SectionResult {
    // Check for typecheck errors.
    let tycheck_errors: Vec<_> = tycheck_result.errors(db).into_iter()
        .map(|e| format!("{:?}", e.error(db)))
        .collect();
    if !tycheck_errors.is_empty() {
        return SectionResult {
            section_type: "scriptunit-expr".to_string(),
            name: None,
            typecheck: TypecheckResult::Error { errors: tycheck_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        };
    }

    let expr = match &parsed_unit.kind {
        ParsedUnitKind::Expr(e) => *e,
        _ => unreachable!(),
    };

    // Lower the expression as a script unit.
    let ir_unit = match ir::lower::lower_script_expr(
        db,
        tycheck_result.expr_types(db),
        script_ctx.clone(),
        expr,
    ) {
        Ok(unit) => unit,
        Err(e) => {
            return SectionResult {
                section_type: "scriptunit-expr".to_string(),
                name: None,
                typecheck: TypecheckResult::Success,
                lowering: LoweringResult::Error { message: format!("{}", e) },
                output: String::new(),
            };
        }
    };

    let ir_dump = format!("{}", ir_unit);

    // Execute the script unit if it has a result.
    let output = if let Some(result_id) = ir_unit.result {
        use datalove_rt::rust::AlignedBuffer;

        // Create tydesc table for both destinations.
        let mut tydesc_table = ir::interp::IrTyDescTable::new();

        // ret_dest is for early returns: always Result<(), Error>.
        let ret_type = ir::IrType::Result(Box::new(ir::IrType::Unit));
        let ret_tydesc = tydesc_table.get_or_create(&ret_type);
        let ret_size = unsafe { (*ret_tydesc).size };
        let ret_align = unsafe { (*ret_tydesc).align };
        let mut ret_buffer = AlignedBuffer::with_align(ret_size as usize, ret_align as usize);
        let ret_dest = ir::interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // expr_dest is for the expression result.
        let expr_type = &ir_unit.value_types[result_id.0 as usize];
        let expr_tydesc = tydesc_table.get_or_create(expr_type);
        let expr_size = unsafe { (*expr_tydesc).size };
        let expr_align = unsafe { (*expr_tydesc).align };
        let mut expr_buffer = AlignedBuffer::with_align(expr_size as usize, expr_align as usize);
        let expr_dest = ir::interp::Destination {
            ptr: expr_buffer.as_mut_ptr(),
            tydesc: expr_tydesc,
        };

        // Execute the script unit with shared environment.
        match interp.execute_script_unit_in_env(&ir_unit, env, ret_dest, Some(expr_dest)) {
            Ok(UnitCompletion::Normal) => {
                // Normal completion - pretty print the expression result.
                let value = ir::interp::Value {
                    ptr: expr_buffer.as_mut_ptr(),
                    tydesc: expr_tydesc,
                };
                let output_str = interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));
                let _ = interp.destroy_value(&value);
                output_str
            }
            Ok(UnitCompletion::EarlyReturn) => {
                // Early return - pretty print the Result<(), Error> value.
                let value = ir::interp::Value {
                    ptr: ret_buffer.as_mut_ptr(),
                    tydesc: ret_tydesc,
                };
                let output_str = interp.pretty_print_value(&value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));
                let _ = interp.destroy_value(&value);
                output_str
            }
            Err(e) => format!("Error: {:?}", e),
        }
    } else {
        "(fragment executed)".to_string()
    };

    // Update script context with exports from this unit.
    let unit_index = script_ctx.current_unit;
    script_ctx.add_exports(unit_index, &ir_unit.exports, &ir_unit.slot_types);
    script_ctx.current_unit += 1;

    SectionResult {
        section_type: "scriptunit-expr".to_string(),
        name: None,
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    }
}
