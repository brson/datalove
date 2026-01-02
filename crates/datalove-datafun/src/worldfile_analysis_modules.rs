//! Module-only worldfile analysis.
//!
//! This module provides test infrastructure for worldfiles that contain
//! only module sections (no script/scriptunit/expr sections). Tests execute
//! a nullary `main` function from the `local/test/main` module and capture
//! the return value.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use rmx::std::collections::BTreeMap;

use datalove_datafun_pkg::package_load_worldfile::{WorldfileSection, ParsedWorldfile};
use datalove_datafun_pkg::package_load::{Package, PackageModule};
use datalove_datafun_compiler::interp::InterpContext;

/// Result of analyzing a module-only worldfile.
#[derive(Debug, Serialize, Deserialize)]
pub struct ModulesAnalysis {
    /// Typecheck result for all modules.
    pub typecheck: TypecheckResult,
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

/// Analyze a module-only worldfile.
///
/// This function:
/// 1. Validates that there are no script/scriptunit/expr sections
/// 2. Loads all module sections into a PackageWorld
/// 3. Typechecks the modules
/// 4. Calls the nullary `main` function from `local/test/main`
/// 5. Returns the output value
pub fn analyze_modules_worldfile(
    db: &dyn salsa::Database,
    parsed: ParsedWorldfile,
) -> AnyResult<ModulesAnalysis> {
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

    // Extract modules to build PackageWorld.
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

    // Verify local/test/main module exists.
    let local_lib = pkglib_local.get("test")
        .ok_or_else(|| anyhow!("missing local/test package"))?;
    if !local_lib.modules.contains_key("main") {
        bail!("missing local/test/main module");
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
        Err(e) => {
            return Ok(ModulesAnalysis {
                typecheck: TypecheckResult::Error {
                    errors: vec![format!("Package resolution failed: {:?}", e)],
                },
                output: String::new(),
            });
        }
    };

    // Convert to package-agnostic ModuleGraph and parse.
    let module_graph = datalove_datafun_pkg::to_module_graph(db, package_world, pkg_graph);
    let parsed_graph = datalove_datafun_compiler::module_graph::parse_module_graph(db, module_graph);

    // Typecheck using the package-agnostic path.
    let typecheck_result = datalove_datafun_compiler::tycheck::typecheck_module_graph(db, parsed_graph);

    // Check for typecheck errors.
    if !typecheck_result.is_ok(db) {
        let all_errors: Vec<_> = typecheck_result.all_errors(db)
            .into_iter()
            .map(|e| format!("{:?}", e))
            .collect();
        return Ok(ModulesAnalysis {
            typecheck: TypecheckResult::Error { errors: all_errors },
            output: String::new(),
        });
    }

    // Create interpreter context.
    let mut interp_ctx = match InterpContext::new_with_module_graph(db, typecheck_result) {
        Ok(ctx) => ctx,
        Err(datalove_datafun_compiler::interp::InterpError::TypecheckErrors(errors)) => {
            let error_details: Vec<_> = errors.iter().map(|e| format!("{:?}", e)).collect();
            return Ok(ModulesAnalysis {
                typecheck: TypecheckResult::Error { errors: error_details },
                output: String::new(),
            });
        }
        Err(e) => bail!("Failed to create interpreter context: {:?}", e),
    };

    // Find the main module and its main function.
    let main_module_path = "local/test/main";
    let main_module_id = module_graph.iter_modules(db)
        .find(|m| m.id(db).path(db) == main_module_path)
        .map(|m| m.id(db))
        .ok_or_else(|| anyhow!("local/test/main module not found in graph"))?;

    // Get the main function from the module.
    let main_func_name = bct::text::InternedText::new(db, S("main"));
    let main_func = interp_ctx.module_function_graph()
        .get_module_functions(main_module_id)
        .and_then(|funcs| funcs.get(&main_func_name).copied())
        .ok_or_else(|| anyhow!("main function not found in local/test/main module"))?;

    // Verify main is nullary.
    if !main_func.params(db).is_empty() {
        bail!("main function must have no parameters");
    }

    // Verify main has a return type.
    if main_func.return_type(db).is_none() {
        bail!("main function must have a return type");
    }

    // Allocate return destination.
    let ret_type = main_func.return_type(db).unwrap();
    let ret_tydesc = datalove_datafun_compiler::interp::tydesc::type_hint_to_tydesc(&mut interp_ctx, ret_type);
    let ret_ptr = unsafe {
        datalove_rt::c::dtlv_rti_mem_alloc_local(
            interp_ctx.runtime_handle(),
            ret_tydesc,
            1,
        )
    };
    if ret_ptr.is_null() {
        bail!("Failed to allocate return buffer");
    }
    let return_dest = datalove_datafun_compiler::interp::Destination {
        ptr: ret_ptr,
        tydesc: ret_tydesc,
    };

    // Execute main function.
    let result = datalove_datafun_compiler::interp::execute_function_body(
        &mut interp_ctx,
        main_func,
        Some(main_module_id),
        Vec::new(),
        return_dest,
    );

    let output = match result {
        Ok(value) => {
            // Pretty-print the return value.
            let output_str = interp_ctx.pretty_print_value(&value)
                .unwrap_or_else(|e| format!("Error: {:?}", e));

            // Destroy the value.
            unsafe {
                let rt_handle = interp_ctx.runtime_handle();
                datalove_rt::c::dtlv_rti_any_destroy_local(rt_handle, value.ptr, value.tydesc);
                datalove_rt::c::dtlv_rti_mem_free_local(rt_handle, value.tydesc, 1, value.ptr);
            }

            output_str
        }
        Err(e) => {
            // Execution error.
            unsafe {
                datalove_rt::c::dtlv_rti_mem_free_local(
                    interp_ctx.runtime_handle(),
                    ret_tydesc,
                    1,
                    ret_ptr,
                );
            }
            return Ok(ModulesAnalysis {
                typecheck: TypecheckResult::Success,
                output: format!("Error: {:?}", e),
            });
        }
    };

    Ok(ModulesAnalysis {
        typecheck: TypecheckResult::Success,
        output,
    })
}
