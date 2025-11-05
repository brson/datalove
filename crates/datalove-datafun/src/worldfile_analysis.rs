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
    /// AST dump.
    pub ast: String,
    /// Typecheck result.
    pub typecheck: TypecheckResult,
    /// Exported function names.
    pub exports: Vec<String>,
}

/// Analysis of a scriptunit section.
#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptUnitAnalysis {
    /// AST dump.
    pub ast: String,
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
    /// AST dump.
    pub ast: String,
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
    let _parsed_module = crate::parser::parse_for_diagnostics(db, source_obj);

    // For now, skip AST dump (requires Debug trait).
    // TODO: Add proper AST serialization and export extraction.
    let ast_dump = format!("<module {}>", module_path);

    Ok(SectionAnalysis::Module(ModuleAnalysis {
        path: module_path,
        ast: ast_dump,
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

    // Skip AST dump for now (requires Debug trait).
    let _parsed = crate::parser::parse_script_unit(db, script, 0);
    let ast_dump = String::from("<scriptunit>");

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
                ast: ast_dump,
                typecheck: TypecheckResult::Success,
                state_changes,
            }))
        }
        Err(e) => {
            Ok(SectionAnalysis::ScriptUnit(ScriptUnitAnalysis {
                ast: ast_dump,
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
            // Extract the __temp variable.
            let temp_name = bct::text::InternedText::new(db, S("__temp"));
            if let Some(var) = ctx.script_scope.variables.get(&temp_name) {
                // Pretty-print the value.
                // TODO: Implement proper pretty-printing and type inference.
                let value_str = format!("{:?}", var.value);

                // Remove __temp from scope.
                ctx.script_scope.variables.remove(&temp_name);

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

    // Skip AST dump for now (requires Debug trait).
    let _parsed = crate::parser::parse_script_unit(db, script, 0);
    let ast_dump = String::from("<script>");

    // Execute the script.
    match crate::interp::execute_script(db, script, package_world) {
        Ok(mut result) => {
            // Pretty-print the output.
            let output = crate::interp::pretty_print_value(&mut result)
                .unwrap_or_else(|e| format!("Error: {:?}", e));

            Ok(SectionAnalysis::Script(ScriptAnalysis {
                ast: ast_dump,
                typecheck: TypecheckResult::Success,
                output,
            }))
        }
        Err(e) => {
            Ok(SectionAnalysis::Script(ScriptAnalysis {
                ast: ast_dump,
                typecheck: TypecheckResult::Error {
                    errors: vec![format!("{:?}", e)],
                },
                output: String::new(),
            }))
        }
    }
}
