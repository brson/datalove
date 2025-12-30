//! Module-only worldfile analysis using the IR interpreter (interp3).
//!
//! This module provides test infrastructure for worldfiles that contain
//! only module sections (no script/scriptunit/expr sections). Tests execute
//! a nullary `main` function from the `local/test/main` module using the
//! new IR-based interpreter.
//!
//! Functions defined in any module can call other functions from the same or
//! other modules via the `ScriptEnvironment`.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::ir;
use datalove_datafun_compiler::tycheck::typecheck_module_graph;
use datalove_datafun_compiler::module_graph::ModuleGraphBuilder;
use ir::interp::ScriptEnvironment;

/// Result of analyzing a module-only worldfile with IR interpreter.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModulesIr3Analysis {
    /// Typecheck result for all modules.
    pub typecheck: TypecheckResult,
    /// Lowering result.
    pub lowering: LoweringResult,
    /// Output value from calling main().
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
}

/// Lowering result summary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success {
        /// IR dump of the main function.
        ir: String,
    },
    Error {
        message: String,
    },
    Skipped,
}

/// Analyze a module-only worldfile using the IR interpreter.
///
/// This function:
/// 1. Validates that there are no script/scriptunit/expr sections
/// 2. Builds a module graph from all module sections
/// 3. Typechecks all modules together (handles inter-module imports)
/// 4. Lowers all functions from all modules to IR
/// 5. Executes the `main` function (with access to all other functions)
/// 6. Returns the output value
pub fn analyze_modules_worldfile_ir3(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<ModulesIr3Analysis> {
    // Validate: only module sections allowed.
    for section in &parsed.sections {
        match section {
            WorldfileSection::Module { .. } => {}
            WorldfileSection::ScriptFragment { .. } => {
                bail!("scriptunit-fragment section not allowed in module-only worldfile");
            }
            WorldfileSection::ScriptExpr { .. } => {
                bail!("scriptunit-expr section not allowed in module-only worldfile");
            }
        }
    }

    // Extract modules to build PackageWorld (for validation) and module specs (for typechecking).
    let mut pkglib_local = BTreeMap::<String, Package>::new();
    let mut module_specs = Vec::new();

    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            // Build module path.
            let module_path = format!("{}/{}/{}", library, package, module);

            // Create source for module graph.
            let src = bct::input::Source::new(db, source.to_string());
            module_specs.push((module_path.clone(), src));

            // Also track in package structure for validation.
            if library == "local" {
                let pkg = pkglib_local.entry(package.C())
                    .or_insert_with(|| Package {
                        name: package.C(),
                        modules: BTreeMap::new(),
                    });

                let pkg_module = PackageModule {
                    name: module.C(),
                    path: module_path.C().into(),
                    text: source.C(),
                };

                pkg.modules.insert(module.C(), pkg_module);
            }
        }
    }

    // Verify local/test/main module exists.
    let local_lib = pkglib_local.get("test")
        .ok_or_else(|| anyhow!("missing local/test package"))?;
    if !local_lib.modules.contains_key("main") {
        bail!("missing local/test/main module");
    }

    // Build ModuleGraph for typechecking.
    let mut builder = ModuleGraphBuilder::new(db);
    for (path, source) in &module_specs {
        builder.add_module(path.clone(), *source);
    }
    let module_graph = builder.build();

    // Typecheck all modules together (handles inter-module imports).
    let graph_typecheck = typecheck_module_graph(db, module_graph.clone());
    let combined_expr_types = graph_typecheck.expr_types(db);

    // Check for typecheck errors.
    let module_errors = graph_typecheck.module_errors(db);
    let mut all_errors = Vec::new();
    for (module_id, errors) in module_errors {
        for e in errors {
            all_errors.push(format!("{}: {:?}", module_id.path(db), e));
        }
    }
    if !all_errors.is_empty() {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Error { errors: all_errors },
            lowering: LoweringResult::Skipped,
            output: String::new(),
        });
    }

    // First pass: collect all function names from all modules.
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

    // Second pass: lower all functions and build the execution environment.
    let mut env = ScriptEnvironment::new();
    let mut main_ir: Option<ir::IrFunction> = None;
    let mut ir_dumps = Vec::new();
    let mut lowering_errors = Vec::new();

    for module in module_graph.iter_modules(db) {
        let module_id = module.id(db);
        let module_path = module_id.path(db).clone();
        let module_source = module.source(db);
        let parse_result = datalove_datafun_compiler::parser::parse(db, module_source);
        let script = parse_result.script(db);

        for statement in script.statements(db) {
            if let datalove_datafun_compiler::ast::Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).to_string();

                match ir::lower::lower_function_for_module(
                    db, combined_expr_types, &all_module_functions, *func
                ) {
                    Ok(ir_func) => {
                        ir_dumps.push(format!("{}", ir_func));

                        // Track main function separately.
                        if module_path == "local/test/main" && func_name == "main" {
                            main_ir = Some(ir_func.clone());
                        }

                        // Add to environment for cross-function calls.
                        env.add_module_function(func_name, ir_func);
                    }
                    Err(e) => {
                        lowering_errors.push(format!("Error lowering {}/{}: {}", module_path, func_name, e));
                    }
                }
            }
        }
    }

    if !lowering_errors.is_empty() {
        return Ok(ModulesIr3Analysis {
            typecheck: TypecheckResult::Success,
            lowering: LoweringResult::Error { message: lowering_errors.join("\n") },
            output: String::new(),
        });
    }

    let main_func = main_ir.ok_or_else(|| anyhow!("main function not found in local/test/main module"))?;

    // Verify main is nullary.
    if !main_func.params.is_empty() {
        bail!("main function must have no parameters");
    }

    // Capture IR dump (all functions).
    let ir_dump = ir_dumps.join("\n");

    // Create IR interpreter and execute main.
    let mut interp = ir::interp::IrInterpreter::new();
    let mut tydesc_table = ir::interp::IrTyDescTable::new();

    // Infer return type from the IR function.
    let ret_ir_type = main_func.infer_return_type();
    let ret_tydesc = tydesc_table.get_or_create(&ret_ir_type);
    let ret_size = unsafe { (*ret_tydesc).size };

    // Allocate return buffer.
    let mut ret_buffer = vec![0u8; ret_size as usize];
    let ret_dest = ir::interp::Destination {
        ptr: ret_buffer.as_mut_ptr(),
        tydesc: ret_tydesc,
    };

    // Execute main with the environment (so it can call other functions).
    let output = match interp.call_with_env(&main_func, Vec::new(), ret_dest, &env) {
        Ok(()) => {
            // Pretty-print the return value.
            let value = ir::interp::Value {
                ptr: ret_buffer.as_mut_ptr(),
                tydesc: ret_tydesc,
            };
            let output_str = interp.pretty_print_value(&value)
                .unwrap_or_else(|e| format!("Error: {:?}", e));
            // Destroy the value to free any allocations.
            let _ = interp.destroy_value(&value);
            output_str
        }
        Err(e) => format!("Error: {:?}", e),
    };

    // Cleanup: destroy all values in frames to prevent memory leaks.
    env.destroy_all(interp.runtime_handle());

    Ok(ModulesIr3Analysis {
        typecheck: TypecheckResult::Success,
        lowering: LoweringResult::Success { ir: ir_dump },
        output,
    })
}
