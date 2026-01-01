//! IR lowering tests.
//!
//! This test suite loads worldfiles, parses functions, lowers them to IR,
//! and outputs the serialized IR for snapshot testing.

use rmx::prelude::*;
use std::path::Path;
use std::collections::HashMap;
use datalove_datafun as datafun;
use datalove_datafun_pkg::package_load_worldfile::{self, WorldfileSection};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::ir;
use rmx::std::collections::BTreeMap;
use bct::input::Source;

/// Analyze a worldfile and produce IR output.
fn analyze_file(path: &Path) -> Result<String, String> {
    let file_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let db = datafun::Database::default();

    // Parse the worldfile into sections.
    let parsed = package_load_worldfile::parse_worldfile_sections(file_bytes.as_slice())
        .map_err(|e| format!("Failed to parse worldfile: {}", e))?;

    // Build package world from module sections.
    let mut pkglib_local = BTreeMap::new();

    for section in &parsed.sections {
        if let WorldfileSection::Module { library, package, module, source } = section {
            if library != "local" {
                continue;
            }

            let pkg = pkglib_local.entry(package.clone())
                .or_insert_with(|| Package {
                    name: package.clone(),
                    modules: BTreeMap::new(),
                });

            let module_path_str = format!("{}/{}/{}", library, package, module);

            let pkg_module = PackageModule {
                name: module.clone(),
                path: module_path_str.into(),
                text: source.clone(),
            };

            pkg.modules.insert(module.clone(), pkg_module);
        }
    }

    // Get the main module's source.
    let main_pkg = pkglib_local.get("test")
        .ok_or_else(|| "No local/test package found".to_string())?;
    let main_module = main_pkg.modules.get("main")
        .ok_or_else(|| "No local/test/main module found".to_string())?;

    // Parse the module source to get AST.
    let source = Source::new(&db, main_module.text.clone());
    let parse_result = datalove_datafun_compiler::parser::parse(&db, source);
    let script_ast = parse_result.script(&db);

    // Typecheck the script to get expression types.
    let tycheck_result = datalove_datafun_compiler::tycheck::type_check(&db, source, script_ast);

    // Lower each function to IR.
    let mut output = String::new();
    let expr_types = tycheck_result.expr_types(&db);

    for stmt in script_ast.statements(&db) {
        if let datalove_datafun_compiler::ast::Statement::Fun(func) = stmt {
            // Run drop analysis first.
            let analysis = ir::drop_analysis::analyze_function(&db, *func, expr_types);
            if !analysis.errors.is_empty() {
                let error_msgs: Vec<String> = analysis.errors.iter()
                    .map(|e| format!("{:?}", e))
                    .collect();
                output.push_str(&format!("Drop analysis error in {}: {}\n",
                    func.name(&db).text(&db), error_msgs.join("; ")));
                continue;
            }

            let empty_funcs: HashMap<String, (ir::IrModuleId, ir::FuncId)> = HashMap::new();
            match ir::lower::lower_function_for_module(&db, expr_types, &empty_funcs, *func, analysis) {
                Ok(ir_func) => {
                    output.push_str(&format!("{}", ir_func));
                    output.push('\n');
                }
                Err(e) => {
                    output.push_str(&format!("Error lowering {}: {}\n",
                        func.name(&db).text(&db), e));
                }
            }
        }
    }

    if output.is_empty() {
        output.push_str("(no functions)\n");
    }

    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("ir_lower")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
