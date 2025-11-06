//! Analysis output for worldfile test sections.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use crate::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use crate::package_load::{Package, PackageModule};
use crate::interp::InterpContext;
use rmx::std::collections::BTreeMap;

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
    pub ast: crate::ast_serde::Script,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Exported function names.
    pub exports: Vec<String>,
}

/// Analysis of a scriptunit section.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptUnitAnalysis {
    /// AST.
    pub ast: crate::ast_serde::Script,
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
    pub ast: crate::ast_serde::Script,
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
    db: &dyn crate::Db,
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
    let raw_package_world = crate::package_load::PackageWorld {
        pkglib_system,
        pkglib_local,
    };
    let package_world = crate::package::import_from_loader(db, raw_package_world);

    // Create persistent interpreter context for incremental scriptunit execution.
    let mut interp_ctx = InterpContext::new(db, package_world, None);

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
                analyze_script_section(db, package_world, &source)?
            }
        };

        analyses.push(analysis);
    }

    // Clean up any remaining variables in the interp_ctx before dropping.
    let remaining_vars: Vec<_> = interp_ctx.script_scope.variables.drain().map(|(_, var)| var.value).collect();
    for value in remaining_vars {
        crate::interp::destroy_value(&mut interp_ctx, value);
    }

    Ok(analyses)
}

/// Analyze a module section.
fn analyze_module_section(
    db: &dyn crate::Db,
    library: &str,
    package: &str,
    module: &str,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    let module_path = format!("{}/{}/{}", library, package, module);

    // Parse the module source.
    let source_obj = bct::input::Source::new(db, source.S());
    let parsed_ast = crate::parser::parse_for_diagnostics(db, source_obj);

    // Convert to serializable AST.
    let serde_ast = crate::ast_serde::Script::from_ast(db, parsed_ast);

    Ok(SectionAnalysis::Module(ModuleAnalysis {
        path: module_path,
        ast: serde_ast,
        typecheck: TypecheckResult::Success,
        exports: vec![],  // TODO: Extract exports from parsed module.
    }))
}

/// Analyze a scriptunit section by executing against incremental context.
fn analyze_scriptunit_section<'db>(
    db: &'db dyn crate::Db,
    ctx: &mut InterpContext<'db>,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    // Parse the scriptunit source.
    let source_obj = bct::input::Source::new(db, source.S());
    let unit = crate::script::ScriptUnit::new(db, source_obj);
    let script = crate::script::Script::new(db, vec![unit]);

    // Convert AST to serializable format.
    let parsed_ast = crate::parser::parse_script_unit(db, script, 0);
    let serde_ast = crate::ast_serde::Script::from_ast(db, parsed_ast);

    // Execute the scriptunit against the context.
    // This will update ctx.script_scope with new variables and functions.
    let before_vars: Vec<String> = ctx.script_scope.variables.keys()
        .map(|k| k.text(db).to_string())
        .collect();
    let before_funs: Vec<String> = ctx.script_scope.functions.keys()
        .map(|k| k.text(db).to_string())
        .collect();

    match crate::interp::execute_script_unit(ctx, script, 0) {
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
    db: &'db dyn crate::Db,
    ctx: &mut InterpContext<'db>,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    // For now, wrap the expression in a temporary let statement to evaluate it.
    let wrapped_source = format!("let __temp = {}", source.trim());
    let source_obj = bct::input::Source::new(db, wrapped_source.S());
    let unit = crate::script::ScriptUnit::new(db, source_obj);
    let script = crate::script::Script::new(db, vec![unit]);

    // Execute to get the value.
    match crate::interp::execute_script_unit(ctx, script, 0) {
        Ok(_) => {
            // Extract and remove the __temp variable.
            let temp_name = bct::text::InternedText::new(db, S("__temp"));
            if let Some(var) = ctx.script_scope.variables.remove(&temp_name) {
                // Pretty-print the value using the context's runtime.
                let value_str = ctx.pretty_print_value(&var.value)
                    .unwrap_or_else(|e| format!("Error: {:?}", e));

                // Destroy the value to avoid leaks.
                crate::interp::destroy_value(ctx, var.value);

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
fn analyze_script_section(
    db: &dyn crate::Db,
    package_world: crate::package::PackageWorld,
    source: &str,
) -> AnyResult<SectionAnalysis> {
    // Parse the script source.
    let source_obj = bct::input::Source::new(db, source.S());
    let unit = crate::script::ScriptUnit::new(db, source_obj);
    let script = crate::script::Script::new(db, vec![unit]);

    // Convert AST to serializable format.
    let parsed_ast = crate::parser::parse_script_unit(db, script, 0);
    let serde_ast = crate::ast_serde::Script::from_ast(db, parsed_ast);

    // Execute the script.
    match crate::interp::execute_script(db, script, package_world) {
        Ok(mut result) => {
            // Pretty-print the output.
            let output = crate::interp::pretty_print_value(&mut result)
                .unwrap_or_else(|e| format!("Error: {:?}", e));

            // Manually destroy the value before dropping the result.
            // For Int and String types, we need to manually free internal data.
            unsafe {
                let rt_handle = result.runtime.handle();
                let tydesc = result.value.tydesc;
                let type_tag = (*tydesc).type_tag;

                // Check if this is an Int type.
                if type_tag == datalove_rt::rtdt::TyTag::Int {
                    // Manually free the limbs array.
                    let int_ptr = result.value.ptr as *mut datalove_rt::rtdt::Int;
                    if !(*int_ptr).data.is_null() {
                        // Get u32 tydesc for freeing limbs.
                        let u32_tydesc = result.tydesc_table.get_or_create(&crate::datalit::tycheck::Type::U32);
                        let capacity = (*int_ptr).capacity;
                        // Free the limbs array.
                        datalove_rt::c::dtlv_rti_mem_free_local(
                            rt_handle,
                            u32_tydesc,
                            capacity as u32,
                            (*int_ptr).data as *mut u8,
                        );
                    }
                } else if type_tag == datalove_rt::rtdt::TyTag::String {
                    // Manually free the string data buffer.
                    let string_ptr = result.value.ptr as *mut datalove_rt::rtdt::String;
                    if !(*string_ptr).data.is_null() {
                        // Get u8 tydesc for freeing string data.
                        // String data is stored as bytes.
                        let capacity = (*string_ptr).capacity;
                        if capacity > 0 {
                            // Allocate a dummy u8 tydesc for freeing the buffer.
                            // The buffer was allocated as raw bytes.
                            let buffer_tydesc = std::mem::MaybeUninit::<datalove_rt::rtdt::TyDesc>::uninit();
                            let mut buffer_tydesc = buffer_tydesc.assume_init();
                            buffer_tydesc.size = 1;
                            buffer_tydesc.align = 1;
                            buffer_tydesc.type_tag = datalove_rt::rtdt::TyTag::U8;

                            datalove_rt::c::dtlv_rti_mem_free_local(
                                rt_handle,
                                &buffer_tydesc as *const _,
                                capacity as u32,
                                (*string_ptr).data as *mut u8,
                            );
                        }
                    }
                }

                // Free the value structure itself.
                datalove_rt::c::dtlv_rti_mem_free_local(
                    rt_handle,
                    tydesc,
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
