use rmx::prelude::*;
use std::path::Path;
use datalove_datafun as datafun;

/// Run a script with package world and extract the value of the 'output' variable.
fn analyze_file(path: &Path) -> Result<String, String> {
    let db = datafun::Database::default();

    // Load worldfile with embedded script.
    let worldfile_bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let worldfile_result = datafun::package_load_worldfile::load_worldfile_with_script(worldfile_bytes.as_slice())
        .map_err(|e| format!("Failed to load worldfile: {}", e))?;

    let package_world = datafun::package::import_from_loader(&db, worldfile_result.package_world);

    // Extract the script from the worldfile.
    let script_text = worldfile_result.script
        .ok_or_else(|| "Worldfile must contain a 'script' section".to_string())?;

    // Parse the script.
    let source = bct::input::Source::new(&db, script_text.S());
    let script = datafun::parser::parse_for_diagnostics(&db, source);

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
    // TODO: Pass actual spans once available
    let script_typecheck = datafun::tycheck::type_check_with_package_world(
        &db,
        script,
        vec![],
        vec![],
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
        .fixture_subdir("interp_with_package")
        .file_extension("world")
        .allow_errors(true)
        .run();
}
