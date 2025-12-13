use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};

use bct::package_resolve2::{
    PackageWorldMap,
    ImportDemandMap,
    PackageWorldModuleGraphWithErrors,
    PackageWorldModuleGraph,
    ResolvedPackageModule,
    resolve_package_world,
};

use crate::package::PackageWorld;
use crate::module_graph::{ModuleGraph, ModuleGraphBuilder, ModuleId, ResolvedImport};

#[salsa::tracked]
pub fn resolve_package_world_with_imports<'db>(
    db: &'db dyn crate::Db,
    package_world: PackageWorld,
) -> PackageWorldModuleGraphWithErrors<'db> {
    let package_world_map = crate::package::package_world_map(db, package_world);
    let import_demand_map = crate::import_demands::import_demands(db, package_world_map);
    resolve_package_world(db, package_world_map, import_demand_map)
}

/// Convert a PackageWorldModuleGraph to a ModuleGraph.
///
/// This bridges the package system with the core compiler's module abstraction.
/// The resulting ModuleGraph can be used with `typecheck_module_graph`.
pub fn to_module_graph<'db>(
    db: &'db dyn crate::Db,
    package_world: PackageWorld,
    graph: PackageWorldModuleGraph<'db>,
) -> ModuleGraph {
    // Build a mapping from PackageModule to its module path string.
    let mut pkg_module_to_path: HashMap<bct::package2::PackageModule, String> = HashMap::new();

    // Traverse the package world to build paths.
    let world_map = crate::package::package_world_map(db, package_world);
    for (import_space, packages) in world_map.map(db) {
        for (package_name, package) in packages {
            for (module_name, package_module) in package.modules(db) {
                let path = format!("{}/{}/{}", import_space, package_name, module_name);
                pkg_module_to_path.insert(*package_module, path);
            }
        }
    }

    // Sort modules in topological order.
    let sorted_modules = crate::tycheck::topological_sort_modules(db, graph)
        .unwrap_or_else(|_| graph.map(db).keys().copied().collect());

    // Build ModuleGraph.
    let mut builder = ModuleGraphBuilder::new(db);

    // Map from PackageModule to ModuleId for import resolution.
    let mut pkg_to_module_id: HashMap<bct::package2::PackageModule, ModuleId> = HashMap::new();

    // First pass: add all modules.
    for pkg_module in &sorted_modules {
        let path = pkg_module_to_path.get(pkg_module)
            .cloned()
            .unwrap_or_else(|| pkg_module.name(db).to_string());
        let source = pkg_module.text(db);
        let module_id = builder.add_module(path, source);
        pkg_to_module_id.insert(*pkg_module, module_id);
    }

    // Second pass: add imports.
    // Note: The ModuleGraph uses pre-resolved imports, but we need to
    // map import demands to actual function imports. For now, we'll
    // rely on the typecheck phase to resolve function names.
    // Here we just record module-level dependencies.

    // Actually, looking at the structure, the PackageWorldModuleGraph only
    // tracks module dependencies, not individual function imports.
    // The function imports are resolved during typechecking.
    // For the ModuleGraph, we need to track the function-level imports.

    // The current design has resolved imports at the function level,
    // but PackageWorldModuleGraph only has module-level deps.
    // We'll need to build the function-level imports from the AST.

    builder.build()
}

/// Convert a PackageWorldModuleGraph to a ModuleGraph with resolved imports.
///
/// This version parses each module to extract import statements and
/// resolves them to function-level imports.
pub fn to_module_graph_with_imports<'db>(
    db: &'db dyn crate::Db,
    package_world: PackageWorld,
    graph: PackageWorldModuleGraph<'db>,
) -> ModuleGraph {
    use crate::ast::Statement;

    // Build a mapping from PackageModule to its module path string.
    let mut pkg_module_to_path: HashMap<bct::package2::PackageModule, String> = HashMap::new();

    // Traverse the package world to build paths.
    let world_map = crate::package::package_world_map(db, package_world);
    for (import_space, packages) in world_map.map(db) {
        for (package_name, package) in packages {
            for (module_name, package_module) in package.modules(db) {
                let path = format!("{}/{}/{}", import_space, package_name, module_name);
                pkg_module_to_path.insert(*package_module, path);
            }
        }
    }

    // Sort modules in topological order.
    let sorted_modules = crate::tycheck::topological_sort_modules(db, graph)
        .unwrap_or_else(|_| graph.map(db).keys().copied().collect());

    // Build ModuleGraph.
    let mut builder = ModuleGraphBuilder::new(db);

    // Map from PackageModule to ModuleId for import resolution.
    let mut pkg_to_module_id: HashMap<bct::package2::PackageModule, ModuleId> = HashMap::new();

    // First pass: add all modules.
    for pkg_module in &sorted_modules {
        let path = pkg_module_to_path.get(pkg_module)
            .cloned()
            .unwrap_or_else(|| pkg_module.name(db).to_string());
        let source = pkg_module.text(db);
        let module_id = builder.add_module(path, source);
        pkg_to_module_id.insert(*pkg_module, module_id);
    }

    // Build alias maps for each module (require statement -> PackageModule).
    let graph_map = graph.map(db);

    // Second pass: resolve imports for each module.
    for pkg_module in &sorted_modules {
        let module_id = *pkg_to_module_id.get(pkg_module).unwrap();

        // Parse the module to get import statements.
        let source = pkg_module.text(db);
        let parse_result = crate::parser::parse(db, source);
        let script = parse_result.script(db);

        // Build alias map from require statements.
        let mut alias_map: HashMap<bct::text::InternedText, bct::package2::PackageModule> = HashMap::new();
        if let Some(deps) = graph_map.get(pkg_module) {
            let mut demand_to_module = HashMap::new();
            for (demand, resolved) in deps {
                if let ResolvedPackageModule::Resolved(resolved_module) = resolved {
                    demand_to_module.insert(demand, *resolved_module);
                }
            }

            for statement in script.statements(db) {
                if let Statement::Require(crate::ast::StmtRequire::Module(req)) = statement {
                    let import_space = req.import_space(db);
                    let package_alias = req.package_alias(db);
                    let module_alias = req.module_alias(db);

                    let demand = (
                        import_space.as_str(db).S(),
                        package_alias.as_str(db).S(),
                        module_alias.as_str(db).S(),
                    );

                    if let Some(&resolved_module) = demand_to_module.get(&demand) {
                        alias_map.insert(module_alias, resolved_module);
                    }
                }
            }
        }

        // Process import statements.
        for statement in script.statements(db) {
            if let Statement::Import(import) = statement {
                let module_name = import.module_name(db);
                let item_name = import.item_name(db);

                if let Some(&source_pkg_module) = alias_map.get(&module_name) {
                    if let Some(&source_module_id) = pkg_to_module_id.get(&source_pkg_module) {
                        // Add the resolved import.
                        let local_name = item_name.as_str(db).to_string();
                        let export_name = item_name.as_str(db).to_string();
                        builder.add_import(module_id, local_name, source_module_id, export_name);
                    }
                }
            }
        }
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_module_graph_with_imports() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/u32
----------

fun negate(x: @u32): @u32
  ret @0
end fun

fun add(x: @u32, y: @u32): @u32
  ret @0
end fun

----------
module sys/util/math
----------

require module sys/std/u32

import u32.negate
import u32.add

fun double_negate(n: @u32): @u32
  ret negate(n)
end fun

fun add_three(a: @u32, b: @u32, c: @u32): @u32
  let sum_ab: @u32 = add(a, b)
  ret add(sum_ab, c)
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Resolve imports.
        let resolution = resolve_package_world_with_imports(db, package_world);
        let pkg_graph = resolution.result(db).expect("resolution should succeed");

        // Convert to ModuleGraph.
        let module_graph = to_module_graph_with_imports(db, package_world, pkg_graph);

        // Verify structure.
        let modules = module_graph.modules(db);
        assert_eq!(modules.len(), 2, "should have 2 modules");

        // Verify paths.
        let paths: Vec<_> = modules.iter()
            .map(|m| m.id(db).path(db).clone())
            .collect();
        assert!(paths.contains(&"sys/std/u32".to_string()));
        assert!(paths.contains(&"sys/util/math".to_string()));

        // Find the math module and check its imports.
        let math_module = modules.iter()
            .find(|m| m.id(db).path(db).contains("math"))
            .expect("should have math module");
        let math_imports = module_graph.get_imports(db, math_module.id(db));
        assert_eq!(math_imports.len(), 2, "math should have 2 imports");

        // Check import names.
        let import_names: Vec<_> = math_imports.iter()
            .map(|i| i.local_name.clone())
            .collect();
        assert!(import_names.contains(&"negate".to_string()));
        assert!(import_names.contains(&"add".to_string()));
    }

    #[test]
    fn test_typecheck_module_graph() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/u32
----------

fun negate(x: @u32): @u32
  ret @0
end fun

fun add(x: @u32, y: @u32): @u32
  ret @0
end fun

----------
module sys/util/math
----------

require module sys/std/u32

import u32.negate
import u32.add

fun double_negate(n: @u32): @u32
  ret negate(n)
end fun

fun add_three(a: @u32, b: @u32, c: @u32): @u32
  let sum_ab: @u32 = add(a, b)
  ret add(sum_ab, c)
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Resolve imports.
        let resolution = resolve_package_world_with_imports(db, package_world);
        let pkg_graph = resolution.result(db).expect("resolution should succeed");

        // Convert to ModuleGraph.
        let module_graph = to_module_graph_with_imports(db, package_world, pkg_graph);

        // Typecheck using the new function.
        let typecheck_result = crate::tycheck::typecheck_module_graph(db, module_graph);

        // Check that there are no errors.
        assert!(typecheck_result.is_ok(db), "typecheck should succeed");

        // Verify exports.
        let module_exports = typecheck_result.module_exports(db);

        // Find u32 module.
        let u32_module_id = module_graph.modules(db).iter()
            .find(|m| m.id(db).path(db).contains("u32"))
            .map(|m| m.id(db))
            .expect("should have u32 module");

        let u32_exports = module_exports.get(&u32_module_id).expect("should have u32 exports");
        assert_eq!(u32_exports.functions(db).len(), 2);

        // Find math module.
        let math_module_id = module_graph.modules(db).iter()
            .find(|m| m.id(db).path(db).contains("math"))
            .map(|m| m.id(db))
            .expect("should have math module");

        let math_exports = module_exports.get(&math_module_id).expect("should have math exports");
        assert_eq!(math_exports.functions(db).len(), 2);
    }
    use rmx::std::path::PathBuf;
    use rmx::futures::executor::block_on;

    #[test]
    fn test_load_sys_modules() {
        let ref db = crate::Database::default();

        // sys directory is at project root (../../sys from this crate)
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let sys_dir = manifest_dir.join("../../sys");

        let config = crate::package_load::PackageWorldConfig {
            dir_pkglib_system: sys_dir,
            dir_pkglib_local: None,
        };

        let package_world_raw = block_on(crate::package_load::load_world(config)).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Verify sys library loaded.
        assert!(!package_world.pkglib_system(db).is_empty());

        // Resolve imports.
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Resolution should succeed.
        let graph = match result {
            Ok(graph) => graph,
            Err(e) => {
                panic!("Resolution failed: {:?}", e);
            }
        };

        // Typecheck the package world.
        let typecheck_result = crate::tycheck::typecheck_package_world(db, graph);

        // Check for typecheck errors.
        let module_errors = typecheck_result.module_errors(db);
        if !module_errors.is_empty() {
            eprintln!("\nTypecheck errors found in sys/ modules:");
            for (module, errors) in module_errors {
                eprintln!("\nModule: {}", module.name(db));
                for error in errors {
                    eprintln!("  - {:?}", error);
                }
            }
            panic!("Package world has typecheck errors");
        }
    }

    #[test]
    fn test_resolve_valid_module_imports() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/bool
----------

fun is_true()
end fun

----------
module sys/std/int
----------

require module sys/std/bool

fun add()
end fun

----------
module sys/util/helpers
----------

require module sys/std/int
require module sys/std/bool

fun helper()
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Verify packages loaded correctly
        assert_eq!(package_world.pkglib_system(db).len(), 2);

        // Try to resolve imports
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Should succeed - all required modules exist
        match result {
            Ok(graph) => {
                // Success! The graph should have all 3 modules
                assert_eq!(graph.map(db).len(), 3);
            }
            Err(e) => {
                panic!("Resolution should succeed but failed: {:?}", e);
            }
        }
    }

    #[test]
    fn test_resolve_missing_module_import() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/bool
----------

fun is_true()
end fun

----------
module sys/std/int
----------

require module sys/std/missing_module

fun add()
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Try to resolve imports
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Should fail - sys/std/missing_module doesn't exist
        match result {
            Ok(graph) => {
                // The graph may still be built, but should contain unresolved dependencies
                // Check that the graph contains the module with unresolved imports
                let map = graph.map(db);
                let has_unresolved = map.values().any(|deps| {
                    deps.iter().any(|(_, resolved)| {
                        matches!(resolved, bct::package_resolve2::ResolvedPackageModule::Unresolved)
                    })
                });
                assert!(has_unresolved, "Expected unresolved module dependencies");
            }
            Err(_error) => {
                // Also acceptable - validation might fail
            }
        }
    }

    #[test]
    fn test_resolve_circular_imports() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/a
----------

require module sys/std/b

fun func_a()
end fun

----------
module sys/std/b
----------

require module sys/std/a

fun func_b()
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Try to resolve imports
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Circular imports should be detected
        match result {
            Ok(_) => {
                panic!("Resolution should fail for circular imports");
            }
            Err(error) => {
                // Expected error for circular dependency
                use bct::package_resolve2::ValidationError;
                assert!(matches!(error, ValidationError::CycleDetected));
            }
        }
    }

    #[test]
    fn test_resolve_local_imports() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/bool
----------

fun is_true()
end fun

----------
module local/app/main
----------

require module sys/std/bool

fun app_main()
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Verify both libraries loaded
        assert_eq!(package_world.pkglib_system(db).len(), 1);
        assert_eq!(package_world.pkglib_local(db).len(), 1);

        // Try to resolve imports
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Should succeed - local can import from sys
        match result {
            Ok(graph) => {
                // Success! Both modules should be in the graph
                assert_eq!(graph.map(db).len(), 2);
            }
            Err(e) => {
                panic!("Resolution should succeed but failed: {:?}", e);
            }
        }
    }

    #[test]
    fn test_typecheck_package_world_with_imports() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/u32
----------

fun negate(x: @u32): @u32
  ret @0
end fun

fun add(x: @u32, y: @u32): @u32
  ret @0
end fun

----------
module sys/util/math
----------

require module sys/std/u32

import u32.negate
import u32.add

fun double_negate(n: @u32): @u32
  ret negate(n)
end fun

fun add_three(a: @u32, b: @u32, c: @u32): @u32
  let sum_ab: @u32 = add(a, b)
  ret add(sum_ab, c)
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Resolve imports.
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Should succeed - all required modules exist.
        let graph = match result {
            Ok(graph) => graph,
            Err(e) => {
                panic!("Resolution should succeed but failed: {:?}", e);
            }
        };

        // Typecheck the package world.
        let typecheck_result = crate::tycheck::typecheck_package_world(db, graph);

        // Check that there are no errors.
        let module_errors = typecheck_result.module_errors(db);
        if !module_errors.is_empty() {
            for (module, errors) in module_errors {
                eprintln!("Module {} has errors:", module.name(db));
                for error in errors {
                    eprintln!("  {:?}", error);
                }
            }
            panic!("Typecheck should succeed but found errors");
        }

        // Verify exports from sys/std/u32 module.
        let module_exports = typecheck_result.module_exports(db);
        let u32_module = graph.map(db).keys()
            .find(|m| m.name(db) == "u32")
            .expect("should have u32 module");

        let u32_exports = module_exports.get(u32_module).expect("should have exports for u32");
        let u32_functions = u32_exports.functions(db);

        assert_eq!(u32_functions.len(), 2, "u32 should export 2 functions");
        assert!(u32_functions.iter().any(|(name, _)| name.as_str(db) == "negate"));
        assert!(u32_functions.iter().any(|(name, _)| name.as_str(db) == "add"));

        // Verify exports from sys/util/math module.
        let math_module = graph.map(db).keys()
            .find(|m| m.name(db) == "math")
            .expect("should have math module");

        let math_exports = module_exports.get(math_module).expect("should have exports for math");
        let math_functions = math_exports.functions(db);

        assert_eq!(math_functions.len(), 2, "math should export 2 functions");
        assert!(math_functions.iter().any(|(name, _)| name.as_str(db) == "double_negate"));
        assert!(math_functions.iter().any(|(name, _)| name.as_str(db) == "add_three"));
    }

    #[test]
    fn test_typecheck_void_functions() {
        let ref db = crate::Database::default();

        let worldfile = r#"
----------
module sys/std/u32
----------

fun print_number(x: @u32)
end fun

----------
module sys/util/debug
----------

require module sys/std/u32

import u32.print_number

fun debug_value(n: @u32)
  let _ = print_number(n)
end fun
"#;

        let package_world_raw = crate::package_load_worldfile::load_world_from_worldfile(worldfile.as_bytes()).X();
        let package_world = crate::package::import_from_loader(db, package_world_raw);

        // Resolve imports.
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Should succeed - all required modules exist.
        let graph = match result {
            Ok(graph) => graph,
            Err(e) => {
                panic!("Resolution should succeed but failed: {:?}", e);
            }
        };

        // Typecheck the package world.
        let typecheck_result = crate::tycheck::typecheck_package_world(db, graph);

        // Check that there are no errors.
        let module_errors = typecheck_result.module_errors(db);
        if !module_errors.is_empty() {
            for (module, errors) in module_errors {
                eprintln!("Module {} has errors:", module.name(db));
                for error in errors {
                    eprintln!("  {:?}", error);
                }
            }
            panic!("Typecheck should succeed but found errors");
        }

        // Verify exports from sys/std/u32 module.
        let module_exports = typecheck_result.module_exports(db);
        let u32_module = graph.map(db).keys()
            .find(|m| m.name(db) == "u32")
            .expect("should have u32 module");

        let u32_exports = module_exports.get(u32_module).expect("should have exports for u32");
        let u32_functions = u32_exports.functions(db);

        // Void function should be exported.
        assert_eq!(u32_functions.len(), 1, "u32 should export 1 function");
        let (name, func_type) = &u32_functions[0];
        assert_eq!(name.as_str(db), "print_number");

        // Check that return type is void.
        let return_type = func_type.return_type(db);
        assert!(matches!(return_type.ty(db), crate::tycheck::Type::Void), "print_number should return void");

        // Verify debug module also exports void function.
        let debug_module = graph.map(db).keys()
            .find(|m| m.name(db) == "debug")
            .expect("should have debug module");

        let debug_exports = module_exports.get(debug_module).expect("should have exports for debug");
        let debug_functions = debug_exports.functions(db);

        assert_eq!(debug_functions.len(), 1, "debug should export 1 function");
        assert!(debug_functions.iter().any(|(name, _)| name.as_str(db) == "debug_value"));
    }
}
