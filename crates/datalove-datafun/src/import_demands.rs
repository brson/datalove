//! Extract import demands from parsed modules.
//!
//! This module bridges datafun-pkg with the compiler's parser: package
//! resolution has to know which modules each module requires, and finding that
//! out means reading its `require` statements.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::module_graph::{Module, ModuleId};
use bct::package_resolve2::{ImportDemand, ImportDemandMap, PackageWorldMap};

use datalove_datafun_parser as parser;
use datalove_datafun_ast::ast;

#[salsa::tracked(returns(copy))]
pub fn import_demands<'db>(
    db: &'db dyn salsa::Database,
    package_world_map: PackageWorldMap<'db>,
) -> ImportDemandMap<'db> {
    let mut map = BTreeMap::new();
    for record in package_world_map.flatten_iter(db) {
        let package_module = record.package_module;

        // The `Module` phase 1 will use, built from the path resolution already
        // knows and the same `Source`. Both are interned, so this is the handle
        // phase 1 gets and not a second one that happens to match -- which is
        // what lets the two share a parse. See `module_import_demands`.
        let path = format!(
            "{}/{}/{}",
            record.import_space, record.package_name, package_module.name(db),
        );
        let module = Module::new(db, ModuleId::new(db, path), package_module.text(db));

        map.insert(package_module, module_import_demands(db, module).C());
    }
    ImportDemandMap::new(db, map)
}

/// The modules one module requires.
///
/// **Keyed on the `Module`, so that this and phase 1 parse it once between
/// them.** It used to take the `Source` and call `parse`, which is
/// `parse_with_module_id` with no module id, where phase 1's `parse_module_full`
/// passes one -- two tracked functions over one body of work, so every module in
/// the world was parsed twice on a cold compile and resolution's half was thrown
/// away but for the `require` lines.
///
/// That was 8.4ms of a 8.6ms package resolution on the system library, and
/// resolution was 28% of a cold compile. `resolve_profile` is where those numbers
/// come from and is how to check this has not come back: its last row parses
/// every module the way phase 1 does, after resolution, and wants to be a memo
/// hit.
#[salsa::tracked(returns(ref))]
fn module_import_demands<'db>(
    db: &'db dyn salsa::Database,
    module: Module<'db>,
) -> Vec<ImportDemand> {
    let parsed = &parser::parse_module_full(db, module).parsed;

    parsed.statements.iter()
        .filter_map(|statement| match statement {
            ast::Statement::Require(ast::StmtRequire::Module(require)) => Some((
                require.import_space.as_str(db).S(),
                require.package_alias.as_str(db).S(),
                require.module_alias.as_str(db).S(),
            )),
            _ => None,
        })
        .collect()
}
