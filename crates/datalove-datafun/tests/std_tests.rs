use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;

/// Run a script with the std library loaded from sys/ directory.
fn analyze_file(path: &Path) -> Result<String, String> {
    let db = datafun::Database::default();

    // Read the script file.
    let script_text = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Parse the script.
    let source = bct::input::Source::new(&db, script_text.S());
    let script = datafun::parser::parse_for_diagnostics(&db, source);

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

    // Load and resolve script with package world.
    let script_world = datafun::script_world::load_script_with_package_world(&db, script, package_world);

    // Check if resolution succeeded.
    let resolution = script_world.resolution(&db);
    if let Err(e) = resolution.result(&db) {
        return Err(format!("Package resolution failed: {:?}", e));
    }

    // Get typecheck result.
    let typecheck_result = script_world.typecheck_result(&db)
        .ok_or_else(|| "Package world typecheck failed".to_string())?;

    // Check for package world typecheck errors.
    let module_errors = typecheck_result.module_errors(&db);
    if !module_errors.is_empty() {
        let error_count: usize = module_errors.values().map(|v| v.len()).sum();
        return Err(format!("Package world has {} typecheck error(s)", error_count));
    }

    // Typecheck the script with package world context.
    let script_typecheck = datafun::tycheck::type_check_with_package_world(
        &db,
        source,
        script,
        package_world,
        typecheck_result,
    );

    // Check for script typecheck errors.
    if !script_typecheck.errors(&db).is_empty() {
        let errors: Vec<_> = script_typecheck.errors(&db).iter()
            .map(|e| format!("{:?}", e.error(&db)))
            .collect();
        return Err(format!("Script has {} typecheck error(s):\n{}", errors.len(), errors.join("\n")));
    }

    // Build type table for the script.
    let mut tydesc_table = datafun::datalit::tydesc_table::TyDescTable::new(&db);
    let type_table = datafun::interp_old::type_table::TypeTable::build(&db, script, script_typecheck, &mut tydesc_table)
        .map_err(|e| format!("Failed to build type table: {}", e))?;

    // Create interpreter context with package world support.
    let mut ctx = datafun::interp_old::interp::InterpContext::with_package_world(
        &db,
        type_table,
        &script,
        package_world,
        &typecheck_result,
    );

    // Execute the script.
    ctx.execute(script)
        .map_err(|e| format!("Execution error: {:?}", e))?;

    // Pretty-print the 'output' variable.
    let output_name = bct::text::InternedText::new(&db, S("output"));
    ctx.pretty_print_variable(output_name)
        .map_err(|e| format!("Failed to pretty-print output: {:?}", e))
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("std_tests")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
