//! Analysis output for worldfile test sections.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::interp::InterpContext;

/// Analysis result for a single worldfile section.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SectionAnalysis {
    Module(ModuleAnalysis),
    ScriptUnit(ScriptUnitAnalysis),
    Expr(ExprAnalysis),
    Script(ScriptAnalysis),
}

/// Analysis of a module section.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleAnalysis {
    /// Module path (e.g., "sys/std/u32").
    pub path: String,
    /// AST.
    pub ast: datalove_datafun_compiler::ast_serde::Script,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Exported function names.
    pub exports: Vec<String>,
}

/// Analysis of a scriptunit section.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptUnitAnalysis {
    /// AST.
    pub ast: datalove_datafun_compiler::ast_serde::Script,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Variables and functions added to interpreter state.
    pub state_changes: Vec<String>,
}

/// Analysis of an expr section.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExprAnalysis {
    /// Inferred type (e.g., "Int", "u32", "String").
    #[serde(rename = "type")]
    pub type_: String,
    /// Pretty-printed value (e.g., "@42", "true", "\"hello\"").
    pub value: String,
}

/// Analysis of a script section.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptAnalysis {
    /// AST.
    pub ast: datalove_datafun_compiler::ast_serde::Script,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Final output value.
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

/// Analyze a worldfile and produce analysis for all sections.
///
/// This function processes sections sequentially:
/// - Module sections are loaded into a PackageWorld
/// - ScriptUnit sections are executed incrementally against shared InterpContext
/// - Expr sections evaluate expressions and emit type + value
/// - Script sections execute complete scripts with output variable
pub fn analyze_worldfile(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<Vec<SectionAnalysis>> {
    let mut analyses = Vec::new();

    // First pass: extract modules to build PackageWorld.
    let mut pkglib_system = BTreeMap::new();
    let mut pkglib_local = BTreeMap::new();

    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            let library_map = match library.as_str() {
                "sys" => &mut pkglib_system,
                "local" => &mut pkglib_local,
                other => bail!("unknown library '{other}' (must be 'sys' or 'local')"),
            };

            let pkg = library_map.entry(package.C())
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

    // Convert raw package world to Salsa type.
    let raw_package_world = datalove_datafun_pkg::package_load::PackageWorld {
        pkglib_system,
        pkglib_local,
    };
    let package_world = datalove_datafun_pkg::import_from_loader(db, raw_package_world);

    // Resolve module dependencies and convert to ModuleGraph.
    let resolution = crate::package_resolve::resolve_package_world_with_imports(db, package_world);
    let pkg_graph = match resolution.result(db) {
        Ok(graph) => graph,
        Err(e) => bail!("Package world resolution failed: {:?}", e),
    };

    // Convert to package-agnostic ModuleGraph.
    let module_graph = datalove_datafun_pkg::to_module_graph(db, package_world, pkg_graph);

    // Typecheck using the package-agnostic path.
    let typecheck_result = datalove_datafun_compiler::tycheck::typecheck_module_graph(db, module_graph);

    // Create persistent interpreter context for incremental scriptunit execution.
    let mut interp_ctx = match InterpContext::new_with_module_graph(db, typecheck_result) {
        Ok(ctx) => ctx,
        Err(datalove_datafun_compiler::interp::InterpError::TypecheckErrors(errors)) => {
            // Provide detailed error info for typecheck failures.
            let error_details: Vec<_> = errors.iter().map(|e| format!("{:?}", e)).collect();
            bail!("Package world has {} typecheck errors: {}", errors.len(), error_details.join("; "));
        }
        Err(e) => bail!("Failed to create interpreter context: {:?}", e),
    };

    // Second pass: analyze each section in order.
    for section in parsed.sections {
        let analysis = match section {
            WorldfileSection::Module { library, package, module, source } => {
                analyze_module_section(db, &library, &package, &module, &source)?
            }
            WorldfileSection::ScriptUnit { source } => {
                analyze_scriptunit_section(db, &mut interp_ctx, &source)?
            }
            WorldfileSection::Expr { source } => {
                analyze_expr_section(db, &mut interp_ctx, &source)?
            }
            WorldfileSection::Script { source } => {
                analyze_script_section(db, typecheck_result, &source)?
            }
        };

        analyses.push(analysis);
    }

    // Clean up any remaining variables in the interp_ctx before dropping.
    // Only destroy Available variables - Moved ones have been consumed.
    let remaining_vars: Vec<_> = interp_ctx.script_scope.variables.drain().collect();
    for (_, var) in remaining_vars {
        if var.state == datalove_datafun_compiler::interp::ScriptVarState::Available {
            datalove_datafun_compiler::interp::destroy_value(&mut interp_ctx, var.value);
        }
    }

    Ok(analyses)
}

/// Analyze a module section.
fn analyze_module_section(
    db: &dyn salsa::Database,
    library: &str,
    package: &str,
    module: &str,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    let module_path = format!("{}/{}/{}", library, package, module);

    // Parse the module source.
    let source_obj = bct::input::Source::new(db, source.S());
    let parsed_ast = datalove_datafun_compiler::parser::parse_for_diagnostics(db, source_obj);

    // Convert to serializable AST.
    let serde_ast = datalove_datafun_compiler::ast_serde::Script::from_ast(db, parsed_ast);

    Ok(SectionAnalysis::Module(ModuleAnalysis {
        path: module_path,
        ast: serde_ast,
        typecheck: TypecheckResult::Success,
        exports: vec![],  // TODO: Extract exports from parsed module.
    }))
}

/// Analyze a scriptunit section by executing against incremental context.
fn analyze_scriptunit_section<'db>(
    db: &'db dyn salsa::Database,
    ctx: &mut InterpContext<'db>,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    // Parse the scriptunit source.
    let source_obj = bct::input::Source::new(db, source.S());
    let unit = datalove_datafun_compiler::script::ScriptUnit::new(db, source_obj);
    let script = datalove_datafun_compiler::script::Script::new(db, vec![unit]);

    // Convert AST to serializable format.
    let parsed_ast = datalove_datafun_compiler::parser::parse_script_unit(db, script, 0);
    let serde_ast = datalove_datafun_compiler::ast_serde::Script::from_ast(db, parsed_ast);

    // Execute the scriptunit against the context.
    // This will update ctx.script_scope with new variables and functions.
    let before_vars: Vec<String> = ctx.script_scope.variables.keys()
        .map(|k| k.text(db).to_string())
        .collect();
    let before_funs: Vec<String> = ctx.script_scope.functions.keys()
        .map(|k| k.text(db).to_string())
        .collect();

    match datalove_datafun_compiler::interp::execute_script_unit(ctx, script, 0) {
        Ok(_) => {
            // Track what changed.
            let after_vars: Vec<String> = ctx.script_scope.variables.keys()
                .map(|k| k.text(db).to_string())
                .collect();
            let after_funs: Vec<String> = ctx.script_scope.functions.keys()
                .map(|k| k.text(db).to_string())
                .collect();

            let mut state_changes = Vec::new();
            for var in &after_vars {
                if !before_vars.contains(var) {
                    state_changes.push(format!("var:{}", var));
                }
            }
            for fun in &after_funs {
                if !before_funs.contains(fun) {
                    state_changes.push(format!("fun:{}", fun));
                }
            }

            Ok(SectionAnalysis::ScriptUnit(ScriptUnitAnalysis {
                ast: serde_ast,
                typecheck: TypecheckResult::Success,
                state_changes,
            }))
        }
        Err(e) => {
            Ok(SectionAnalysis::ScriptUnit(ScriptUnitAnalysis {
                ast: serde_ast,
                typecheck: TypecheckResult::Error {
                    errors: vec![format!("{:?}", e)],
                },
                state_changes: vec![],
            }))
        }
    }
}

/// Analyze an expr section by evaluating the expression.
fn analyze_expr_section<'db>(
    db: &'db dyn salsa::Database,
    ctx: &mut InterpContext<'db>,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    // For now, wrap the expression in a temporary let statement to evaluate it.
    let wrapped_source = format!("let __temp = {}", source.trim());
    let source_obj = bct::input::Source::new(db, wrapped_source.S());
    let unit = datalove_datafun_compiler::script::ScriptUnit::new(db, source_obj);
    let script = datalove_datafun_compiler::script::Script::new(db, vec![unit]);

    // Execute to get the value.
    match datalove_datafun_compiler::interp::execute_script_unit(ctx, script, 0) {
        Ok(_) => {
            // Extract and remove the __temp variable.
            let temp_name = bct::text::InternedText::new(db, S("__temp"));
            if let Some(var) = ctx.script_scope.variables.remove(&temp_name) {
                // Pretty-print the value using the context's runtime.
                let value_str = ctx.pretty_print_value(&var.value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));

                // Destroy the value to avoid leaks.
                datalove_datafun_compiler::interp::destroy_value(ctx, var.value);

                Ok(SectionAnalysis::Expr(ExprAnalysis {
                    type_: "Unknown".to_string(),  // TODO: Infer type.
                    value: value_str,
                }))
            } else {
                bail!("Failed to evaluate expression");
            }
        }
        Err(e) => {
            bail!("Expression evaluation failed: {:?}", e);
        }
    }
}

/// Analyze a script section by executing the complete script.
fn analyze_script_section<'db>(
    db: &'db dyn salsa::Database,
    graph_typecheck: datalove_datafun_compiler::module_graph::ModuleGraphTypecheckResult<'db>,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    // Parse the script source.
    let source_obj = bct::input::Source::new(db, source.S());
    let unit = datalove_datafun_compiler::script::ScriptUnit::new(db, source_obj);
    let script = datalove_datafun_compiler::script::Script::new(db, vec![unit]);

    // Convert AST to serializable format.
    let parsed_ast = datalove_datafun_compiler::parser::parse_script_unit(db, script, 0);
    let serde_ast = datalove_datafun_compiler::ast_serde::Script::from_ast(db, parsed_ast);

    // Execute the script using ModuleGraph path.
    match datalove_datafun_compiler::interp::execute_script_with_module_graph(db, script, graph_typecheck) {
        Ok(mut result) => {
            // Pretty-print the output.
            let output = datalove_datafun_compiler::interp::pretty_print_value(&mut result)
                .unwrap_or_else(|e| format!("Error: {:?}", e));

            // Destroy the value using the runtime's destroy function.
            unsafe {
                let rt_handle = result.runtime.handle();
                datalove_rt::c::dtlv_rti_any_destroy_local(
                    rt_handle,
                    result.value.ptr,
                    result.value.tydesc,
                );
                datalove_rt::c::dtlv_rti_mem_free_local(
                    rt_handle,
                    result.value.tydesc,
                    1,
                    result.value.ptr,
                );
            }

            Ok(SectionAnalysis::Script(ScriptAnalysis {
                ast: serde_ast,
                typecheck: TypecheckResult::Success,
                output,
            }))
        }
        Err(e) => {
            Ok(SectionAnalysis::Script(ScriptAnalysis {
                ast: serde_ast,
                typecheck: TypecheckResult::Error {
                    errors: vec![format!("{:?}", e)],
                },
                output: String::new(),
            }))
        }
    }
}
