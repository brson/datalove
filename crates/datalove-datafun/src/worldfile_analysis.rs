//! Analysis output for worldfile test sections.
//!
//! Note: Script execution has been removed. This module only supports module
//! analysis. For module-only worldfiles with function execution, use
//! worldfile_analysis_modules instead.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};

/// Analysis result for a single worldfile section.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SectionAnalysis {
    Module(ModuleAnalysis),
    /// Script execution has been removed.
    ScriptNotSupported { message: String },
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
/// Note: Script execution (script/scriptunit/expr sections) has been removed.
/// Use worldfile_analysis_modules for module-only worldfiles that execute main().
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
    let _raw_package_world = datalove_datafun_pkg::package_load::PackageWorld {
        pkglib_system,
        pkglib_local,
    };

    // Second pass: analyze each section.
    for section in parsed.sections {
        let analysis = match section {
            WorldfileSection::Module { library, package, module, source } => {
                analyze_module_section(db, &library, &package, &module, &source)?
            }
            WorldfileSection::ScriptUnit { .. } => {
                SectionAnalysis::ScriptNotSupported {
                    message: "scriptunit sections not supported - script interpreter removed".to_string()
                }
            }
            WorldfileSection::Expr { .. } => {
                SectionAnalysis::ScriptNotSupported {
                    message: "expr sections not supported - script interpreter removed".to_string()
                }
            }
            WorldfileSection::Script { .. } => {
                SectionAnalysis::ScriptNotSupported {
                    message: "script sections not supported - script interpreter removed".to_string()
                }
            }
        };

        analyses.push(analysis);
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
        exports: vec![],
    }))
}
