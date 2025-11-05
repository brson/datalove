//! Integration layer for scripts with package world support.

use rmx::prelude::*;
use crate::ast::Script;
use crate::package::PackageWorld;
use crate::package_resolve::resolve_package_world_with_imports;
use crate::tycheck::{typecheck_package_world, PackageWorldTypecheckResult, TypecheckResult};
use crate::interp_old::type_table::TypeTable;
use crate::interp_old::interp::InterpContext;
use bct::package_resolve2::PackageWorldModuleGraphWithErrors;

/// Result of loading and resolving a script with package world.
#[salsa::tracked]
pub struct ScriptWithPackageWorld<'db> {
    /// The parsed script.
    pub script: Script<'db>,

    /// The package world.
    pub package_world: PackageWorld,

    /// Package world resolution result.
    pub resolution: PackageWorldModuleGraphWithErrors<'db>,

    /// Package world typecheck result (if resolution succeeded).
    #[returns(ref)]
    pub typecheck_result: Option<PackageWorldTypecheckResult<'db>>,
}

/// Load and resolve a script with package world support.
///
/// This function:
/// 1. Resolves the package world imports
/// 2. Typechecks the package world modules
/// 3. Returns a combined result ready for script typechecking and execution
#[salsa::tracked]
pub fn load_script_with_package_world<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    package_world: PackageWorld,
) -> ScriptWithPackageWorld<'db> {
    // Resolve package world imports.
    let resolution = resolve_package_world_with_imports(db, package_world);

    // Typecheck package world if resolution succeeded.
    let typecheck_result = match resolution.result(db) {
        Ok(graph) => Some(typecheck_package_world(db, graph)),
        Err(_) => None,
    };

    ScriptWithPackageWorld::new(
        db,
        script,
        package_world,
        resolution,
        typecheck_result,
    )
}

/// Execute a script with package world support.
///
/// This is a high-level API that:
/// 1. Loads and resolves the package world
/// 2. Typechecks the script with package world context
/// 3. Creates an interpreter with package world support
/// 4. Executes the script
///
/// Returns an InterpContext that can be used to inspect results.
pub fn execute_script_with_package_world<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    package_world: PackageWorld,
) -> Result<InterpContext<'db>, String> {
    // Load and resolve script with package world.
    let script_world = load_script_with_package_world(db, script, package_world);

    // Check if resolution succeeded.
    let resolution = script_world.resolution(db);
    if let Err(e) = resolution.result(db) {
        return Err(format!("Package resolution failed: {:?}", e));
    }

    // Get typecheck result.
    let typecheck_result = script_world.typecheck_result(db)
        .ok_or_else(|| "Package world typecheck failed".to_string())?;

    // Check for package world typecheck errors.
    let module_errors = typecheck_result.module_errors(db);
    if !module_errors.is_empty() {
        let error_count: usize = module_errors.values().map(|v| v.len()).sum();
        return Err(format!("Package world has {} typecheck error(s)", error_count));
    }

    // Typecheck the script with package world context.
    let dummy_source = bct::input::Source::new(db, String::new());
    let script_typecheck = crate::tycheck::type_check_with_package_world(
        db,
        dummy_source,
        script,
        package_world,
        typecheck_result,
    );

    // Check for script typecheck errors.
    if !script_typecheck.errors(db).is_empty() {
        return Err(format!("Script has {} typecheck error(s)", script_typecheck.errors(db).len()));
    }

    // Build type table for the script.
    let mut tydesc_table = crate::datalit::tydesc_table::TyDescTable::new(db);
    let type_table = TypeTable::build(db, script, script_typecheck, &mut tydesc_table)
        .map_err(|e| format!("Failed to build type table: {}", e))?;

    // Create interpreter context with package world support.
    let mut ctx = InterpContext::with_package_world(
        db,
        type_table,
        &script,
        package_world,
        &typecheck_result,
    );

    // Execute the script.
    ctx.execute(script)
        .map_err(|e| format!("Execution error: {:?}", e))?;

    Ok(ctx)
}
