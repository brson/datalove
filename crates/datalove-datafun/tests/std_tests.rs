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

    // Resolve and typecheck the package world.
    let resolution = datafun::package_resolve::resolve_package_world_with_imports(&db, package_world);
    let graph = resolution.result(&db)
        .map_err(|e| format!("Package resolution failed: {:?}", e))?;

    let typecheck_result = datafun::tycheck::typecheck_package_world(&db, graph);

    // Check for package world typecheck errors.
    let module_errors = typecheck_result.module_errors(&db);
    if !module_errors.is_empty() {
        let error_count: usize = module_errors.values().map(|v| v.len()).sum();
        return Err(format!("Package world has {} typecheck error(s)", error_count));
    }

    // Execute the script with the new interpreter.
    let mut result = datafun::interp::execute_script(&db, script, package_world, typecheck_result)
        .map_err(|e| format!("Execution error: {:?}", e))?;

    // Create RAII guard for automatic cleanup.
    let _guard = unsafe {
        datalove_rt::rust::ValueGuard::from_raw(
            result.runtime.handle(),
            result.value.tydesc,
            result.value.ptr,
        )
    };

    // Pretty-print the output.
    let output = datafun::interp::pretty_print_value(&mut result)
        .map_err(|e| format!("Failed to pretty-print output: {:?}", e))?;

    // Guard cleans up automatically on drop.
    Ok(output)
}

fn main() {
    datalove_exampletest::ExampleTestRunner::new(env!("CARGO_MANIFEST_DIR"), analyze_file)
        .fixture_subdir("std_tests")
        .file_extension("dfs")
        .allow_errors(true)
        .run();
}
