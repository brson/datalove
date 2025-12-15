//! Extract import demands from parsed modules.
//!
//! This module bridges datafun-pkg with the compiler's parser.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
use bct::package_resolve2::{ImportDemand, ImportDemandMap, PackageWorldMap};

use datalove_datafun_compiler::parser;
use datalove_datafun_compiler::ast;

#[salsa::tracked]
pub fn import_demands<'db>(
    db: &'db dyn salsa::Database,
    package_world_map: PackageWorldMap<'db>,
) -> ImportDemandMap<'db> {
    let mut map = BTreeMap::new();
    for package_world_record in package_world_map.flatten_iter(db) {
        let package_module = package_world_record.package_module;
        let source = package_module.text(db);
        let module_import_demands = module_import_demands(db, source);
        map.insert(package_module, module_import_demands.demands(db).C());
    }
    ImportDemandMap::new(db, map)
}

#[salsa::tracked]
struct ModuleImportDemands<'db> {
    #[returns(ref)]
    demands: Vec<ImportDemand>,
}

#[salsa::tracked]
fn module_import_demands<'db>(
    db: &'db dyn salsa::Database,
    source: Source,
) -> ModuleImportDemands<'db> {
    let ast = parser::parse(db, source).script(db);

    let mut demands = Vec::new();

    for statement in ast.statements(db) {
        match statement {
            ast::Statement::Require(ast::StmtRequire::Module(require)) => {
                let demand = (
                    require.import_space(db).as_str(db).S(),
                    require.package_alias(db).as_str(db).S(),
                    require.module_alias(db).as_str(db).S(),
                );
                demands.push(demand);
            }
            _ => { /* pass */ },
        }
    }

    ModuleImportDemands::new(db, demands)
}
