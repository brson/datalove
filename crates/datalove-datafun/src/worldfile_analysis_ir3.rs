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
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::ir;
use datalove_datafun_compiler::tycheck::{
    ScriptUnitInput, ScriptUnitKind, ScriptUnitBatch, type_check_script_units,
    UnitTypecheckResultTracked, ModuleInfo, typecheck_module_graph,
};
use datalove_datafun_compiler::module_graph::ModuleGraphBuilder;
use ir::interp::{ScriptEnvironment, UnitCompletion};

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

/// Typecheck result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    Error {
        errors: Vec<String>,
    },
    Skipped,
}

/// Lowering result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success {
        /// IR dump.
        ir: String,
    },
    Error {
        message: String,
    },
    Skipped,
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

    // First pass: collect all modules for context.
    let mut pkglib_local = BTreeMap::<String, Package>::new();

    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            if library != "local" {
                continue;  // Skip non-local modules for now.
            }

            let pkg = pkglib_local.entry(package.C())
                .or_insert_with(|| Package {
                    name: package.C(),
                    modules: BTreeMap::new(),
                });

            let module_path_str = format!("{}/{}/{}", library, package, module);

            let pkg_module = PackageModule {
                name: module.C(),
                path: module_path_str.C().into(),
                text: source.C(),
            };

            pkg.modules.insert(module.C(), pkg_module);
        }
    }

    // Parse all modules for import resolution.
    let mut module_infos: Vec<ModuleInfo> = Vec::new();
    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            let module_path = format!("{}/{}/{}", library, package, module);
            let src = bct::input::Source::new(db, source.to_string());
            let parse_result = datalove_datafun_compiler::parser::parse(db, src);
            let script = parse_result.script(db);
            module_infos.push(ModuleInfo {
                path: module_path,
                script,
                source: src,
            });
        }
    }

    // Collect and parse all script units.
    let mut parsed_units: Vec<ParsedUnit> = Vec::new();
    let mut unit_inputs: Vec<ScriptUnitInput> = Vec::new();

    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {
                // Modules are handled above.
            }
            WorldfileSection::ScriptFragment { source } => {
                let src = bct::input::Source::new(db, source.to_string());
                let parse_result = datalove_datafun_compiler::parser::parse(db, src);
                let script = parse_result.script(db);
                parsed_units.push(ParsedUnit {
                    source: src,
                    kind: ParsedUnitKind::Fragment(script),
                });
                unit_inputs.push(ScriptUnitInput::new(db, src, ScriptUnitKind::Fragment(script)));
            }
            WorldfileSection::ScriptExpr { source } => {
                let src = bct::input::Source::new(db, source.to_string());
                let expr = datalove_datafun_compiler::parser::parse_expr(db, src);
                parsed_units.push(ParsedUnit {
                    source: src,
                    kind: ParsedUnitKind::Expr(expr),
                });
                unit_inputs.push(ScriptUnitInput::new(db, src, ScriptUnitKind::Expr(expr)));
            }
        }
    }

    // Typecheck all units together (bindings shared across units).
    let batch = ScriptUnitBatch::new(db, unit_inputs, module_infos.clone());
    let typecheck_results = type_check_script_units(db, batch);
    let unit_results = typecheck_results.results(db);

    // Shared state for lowering and execution.
    let mut script_ctx = ir::lower::ScriptLowerContext::new();
    let mut env = ScriptEnvironment::new();
    let mut interp = ir::interp::IrInterpreter::new();

    // Build ModuleGraph and typecheck all modules together.
    // This handles module-to-module imports properly.
    let mut builder = ModuleGraphBuilder::new(db);
    for module_info in &module_infos {
        builder.add_module(module_info.path.clone(), module_info.source);
    }
    let module_graph = builder.build();

    // Typecheck all modules together (handles inter-module imports).
    let graph_typecheck = typecheck_module_graph(db, module_graph.clone());
    let combined_expr_types = graph_typecheck.expr_types(db);

    // Build map from module path to typecheck errors.
    let module_errors = graph_typecheck.module_errors(db);
    let mut path_to_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (module_id, errors) in module_errors {
        let path = module_id.path(db).clone();
        let error_strings: Vec<String> = errors.iter()
            .map(|e| format!("{:?}", e))
            .collect();
        path_to_errors.insert(path, error_strings);
    }

    // First pass: collect all module function names.
    // These will be available when lowering any module function.
    let mut all_module_functions: Vec<String> = Vec::new();
    for module in module_graph.iter_modules(db) {
        let module_source = module.source(db);
        let parse_result = datalove_datafun_compiler::parser::parse(db, module_source);
        let script = parse_result.script(db);
        for statement in script.statements(db) {
            if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
                all_module_functions.push(func.name(db).text(db).to_string());
            }
        }
    }

    // Second pass: lower module functions with all function names available.
    // Track lowering results per module path.
    let mut module_lowering_results: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for module in module_graph.iter_modules(db) {
        let module_id = module.id(db);
        let module_path = module_id.path(db).clone();

        // Skip lowering if module has typecheck errors.
        if path_to_errors.get(&module_path).map_or(false, |e| !e.is_empty()) {
            continue;
        }

        let module_source = module.source(db);
        let parse_result = datalove_datafun_compiler::parser::parse(db, module_source);
        let script = parse_result.script(db);

        let mut ir_dumps = Vec::new();

        // Lower each function in the module.
        for statement in script.statements(db) {
            if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).to_string();

                // Lower the function with all module functions available.
                match ir::lower::lower_function_for_module(
                    db, combined_expr_types, &all_module_functions, *func
                ) {
                    Ok(ir_func) => {
                        ir_dumps.push(format!("{}", ir_func));
                        script_ctx.add_module_function(func_name.clone(), ir_func.clone());
                        env.add_module_function(func_name, ir_func);
                    }
                    Err(e) => {
                        ir_dumps.push(format!("Error lowering {}: {}", func_name, e));
                    }
                }
            }
        }
        module_lowering_results.insert(module_path, ir_dumps);
    }

    // Process each section, using the pre-computed typecheck results.
    let mut unit_idx = 0;
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { library, package, module, .. } => {
                let module_path = format!("{}/{}/{}", library, package, module);

                // Look up typecheck errors for this module.
                let typecheck = match path_to_errors.get(&module_path) {
                    Some(errors) if !errors.is_empty() => {
                        TypecheckResult::Error { errors: errors.clone() }
                    }
                    _ => TypecheckResult::Success,
                };

                // Look up lowering results for this module.
                let lowering = match &typecheck {
                    TypecheckResult::Error { .. } => LoweringResult::Skipped,
                    _ => match module_lowering_results.get(&module_path) {
                        Some(ir_dumps) => {
                            let has_errors = ir_dumps.iter().any(|s| s.starts_with("Error"));
                            if has_errors {
                                let errors: Vec<_> = ir_dumps.iter()
                                    .filter(|s| s.starts_with("Error"))
                                    .cloned()
                                    .collect();
                                LoweringResult::Error { message: errors.join("\n") }
                            } else {
                                LoweringResult::Success { ir: ir_dumps.join("\n") }
                            }
                        }
                        None => LoweringResult::Skipped,
                    },
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
                let tycheck_result = unit_results[unit_idx];
                unit_idx += 1;

                let result = process_fragment(
                    db,
                    parsed_unit,
                    tycheck_result,
                    &mut script_ctx,
                    &mut env,
                    &mut interp,
                );
                results.push(result);
            }

            WorldfileSection::ScriptExpr { .. } => {
                let parsed_unit = &parsed_units[unit_idx];
                let tycheck_result = unit_results[unit_idx];
                unit_idx += 1;

                let result = process_expr(
                    db,
                    parsed_unit,
                    tycheck_result,
                    &mut script_ctx,
                    &mut env,
                    &mut interp,
                );
                results.push(result);
            }
        }
    }

    // Cleanup: destroy all values in frames to prevent memory leaks.
    env.destroy_all(interp.runtime_handle());

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

    // Lower using the typecheck result's expr_types.
    let ir_unit = match ir::lower::lower_script_fragment_raw(
        db,
        tycheck_result.expr_types(db),
        script_ctx.clone(),
        script.statements(db).to_vec(),
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
    script_ctx.add_exports(unit_index, &ir_unit.exports);
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
        // Create tydesc table for both destinations.
        let mut tydesc_table = ir::interp::IrTyDescTable::new();

        // ret_dest is for early returns: always Result<(), Error>.
        let ret_type = ir::IrType::Result(Box::new(ir::IrType::Unit));
        let ret_tydesc = tydesc_table.get_or_create(&ret_type);
        let ret_size = unsafe { (*ret_tydesc).size };
        let mut ret_buffer = vec![0u8; ret_size as usize];
        let ret_dest = ir::interp::Destination {
            ptr: ret_buffer.as_mut_ptr(),
            tydesc: ret_tydesc,
        };

        // expr_dest is for the expression result.
        let expr_type = &ir_unit.value_types[result_id.0 as usize];
        let expr_tydesc = tydesc_table.get_or_create(expr_type);
        let expr_size = unsafe { (*expr_tydesc).size };
        let mut expr_buffer = vec![0u8; expr_size as usize];
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
    script_ctx.add_exports(unit_index, &ir_unit.exports);
    script_ctx.current_unit += 1;

    SectionResult {
        section_type: "scriptunit-expr".to_string(),
        name: None,
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    }
}
