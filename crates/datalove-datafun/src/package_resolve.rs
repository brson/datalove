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
}
