use rmx::prelude::*;

use bct::package_resolve2::{
    PackageWorldMap,
    ImportDemandMap,
    PackageWorldModuleGraphWithErrors,
    resolve_package_world,
};

use crate::package::PackageWorld;

#[salsa::tracked]
pub fn resolve_package_world_with_imports<'db>(
    db: &'db dyn crate::Db,
    package_world: PackageWorld,
) -> PackageWorldModuleGraphWithErrors<'db> {
    let package_world_map = crate::package::package_world_map(db, package_world);
    let import_demand_map = crate::import_demands::import_demands(db, package_world_map);
    resolve_package_world(db, package_world_map, import_demand_map)
}

#[cfg(test)]
mod tests {
    use super::*;
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

        // Verify sys library loaded
        assert!(!package_world.pkglib_system(db).is_empty());

        // Try to resolve imports
        let resolution = resolve_package_world_with_imports(db, package_world);
        let result = resolution.result(db);

        // Should succeed if sys modules can be loaded
        match result {
            Ok(_) => {
                // Success!
            }
            Err(e) => {
                panic!("Resolution failed: {:?}", e);
            }
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
}
