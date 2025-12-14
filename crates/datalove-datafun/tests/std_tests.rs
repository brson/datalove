use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;

/// Run a script with the std library loaded from sys/ directory.
fn analyze_file(path: &Path) -> Result<String, String> {
    let db = datafun::Database::default();

    // Read the script file.
    let script_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Create script unit.
    let source = bct::input::Source::new(&db, script_text.S());
    let unit = datafun::script::ScriptUnit::new(&db, source);
    let script = datafun::script::Script::new(&db, vec![unit]);

    // Load package world from sys/ directory.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let sys_dir = std::path::PathBuf::from(manifest_dir)
        .parent().unwrap()
        .parent().unwrap()
        .join("sys");

    let config = datafun::package_load::PackageWorldConfig {
        dir_pkglib_system: sys_dir,
        dir_pkglib_local: None,
    };

    let package_world_raw = rmx::futures::executor::block_on(
        datafun::package_load::load_world(config)
    ).map_err(|e| format!("Failed to load package world: {}", e))?;

    let package_world = datafun::package::import_from_loader(&db, package_world_raw);

    // Resolve and convert to ModuleGraph.
    let resolution = datafun::package_resolve::resolve_package_world_with_imports(&db, package_world);
    let pkg_graph = resolution.result(&db)
        .map_err(|e| format!("Package resolution failed: {:?}", e))?;

    // Convert to package-agnostic ModuleGraph and typecheck.
    let module_graph = datafun::to_module_graph(&db, package_world, pkg_graph);
    let typecheck_result = datafun::tycheck::typecheck_module_graph(&db, module_graph);

    // Check for typecheck errors.
    let module_errors = typecheck_result.module_errors(&db);
    let error_count: usize = module_errors.values().map(|v| v.len()).sum();
    if error_count > 0 {
        let mut error_details = Vec::new();
        for (module_id, errors) in module_errors.iter() {
            for err in errors {
                error_details.push(format!("  {}: {:?}", module_id.path(&db), err));
            }
        }
        return Err(format!(
            "Package world has {} typecheck error(s):\n{}",
            error_count,
            error_details.join("\n")
        ));
    }

    // Create interpreter context using ModuleGraph path.
    let mut ctx = datafun::interp::InterpContext::new_with_module_graph(&db, typecheck_result)
        .map_err(|e| format!("Failed to create interpreter context: {:?}", e))?;

    // Populate script-level imports from require/import statements.
    ctx.populate_script_imports(script, module_graph);

    // Execute the script unit.
    datafun::interp::execute_script_unit(&mut ctx, script, 0)
        .map_err(|e| {
            // Clean up any variables before returning error.
            // Only destroy Available variables - Moved ones have been consumed.
            let vars: Vec<_> = ctx.script_scope.variables.drain().collect();
            for (_, var) in vars {
                if var.state == datafun::interp::ScriptVarState::Available {
                    datafun::interp::destroy_value(&mut ctx, var.value);
                }
            }
            format!("Execution error: {:?}", e)
        })?;

    // Extract and pretty-print the output variable.
    let output_name = bct::text::InternedText::new(&db, S("output"));
    let output = match ctx.script_scope.variables.remove(&output_name) {
        Some(var) => {
            let output_str = ctx.pretty_print_value(&var.value)
                .map_err(|e| format!("Failed to pretty-print output: {:?}", e))?;
            datafun::interp::destroy_value(&mut ctx, var.value);
            output_str
        }
        None => {
            // Clean up remaining variables.
            // Only destroy Available variables - Moved ones have been consumed.
            let vars: Vec<_> = ctx.script_scope.variables.drain().collect();
            for (_, var) in vars {
                if var.state == datafun::interp::ScriptVarState::Available {
                    datafun::interp::destroy_value(&mut ctx, var.value);
                }
            }
            return Err("No output variable".to_string());
        }
    };

    // Clean up remaining variables.
    // Only destroy Available variables - Moved ones have been consumed.
    let vars: Vec<_> = ctx.script_scope.variables.drain().collect();
    for (_, var) in vars {
        if var.state == datafun::interp::ScriptVarState::Available {
            datafun::interp::destroy_value(&mut ctx, var.value);
        }
    }

    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("std_tests")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
