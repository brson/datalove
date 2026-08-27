//! Extract import demands from parsed modules.
//!
//! This module bridges datafun-pkg with the compiler's parser.
//! Extracts both module import demands and rider demands.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::input::Source;
use bct::package2::PackageModule;
use bct::package_resolve2::{ImportDemand, ImportDemandMap, PackageWorldMap};

use datalove_datafun_parser as parser;
use datalove_datafun_ast::ast;

/// A rider demand is just the symbolic rider name from `require rider <name>`.
pub type RiderDemand = String;

/// Map from package modules to their rider demands.
#[salsa::tracked]
pub struct RiderDemandMap<'db> {
    #[returns(ref)]
    pub map: BTreeMap<PackageModule<'db>, Vec<RiderDemand>>,
}

#[salsa::tracked(returns(copy))]
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

/// Extract rider demands from all modules in a package world.
#[salsa::tracked(returns(copy))]
pub fn rider_demands<'db>(
    db: &'db dyn salsa::Database,
    package_world_map: PackageWorldMap<'db>,
) -> RiderDemandMap<'db> {
    let mut map = BTreeMap::new();
    for package_world_record in package_world_map.flatten_iter(db) {
        let package_module = package_world_record.package_module;
        let source = package_module.text(db);
        let demands = module_rider_demands(db, source);
        map.insert(package_module, demands.demands(db).C());
    }
    RiderDemandMap::new(db, map)
}

#[salsa::tracked]
struct ModuleImportDemands<'db> {
    #[returns(ref)]
    demands: Vec<ImportDemand>,
}

#[salsa::tracked]
struct ModuleRiderDemands<'db> {
    #[returns(ref)]
    demands: Vec<RiderDemand>,
}

#[salsa::tracked(returns(copy))]
fn module_import_demands<'db>(
    db: &'db dyn salsa::Database,
    source: Source,
) -> ModuleImportDemands<'db> {
    let parsed = &parser::parse(db, source).parsed;

    let mut demands = Vec::new();

    for statement in &parsed.statements {
        if let ast::Statement::Require(ast::StmtRequire::Module(require)) = statement {
            let demand = (
                require.import_space.as_str(db).S(),
                require.package_alias.as_str(db).S(),
                require.module_alias.as_str(db).S(),
            );
            demands.push(demand);
        }
    }

    ModuleImportDemands::new(db, demands)
}

/// Extract rider demands from a single module's source.
#[salsa::tracked(returns(copy))]
fn module_rider_demands<'db>(
    db: &'db dyn salsa::Database,
    source: Source,
) -> ModuleRiderDemands<'db> {
    let parsed = &parser::parse(db, source).parsed;

    let mut demands = Vec::new();

    for statement in &parsed.statements {
        if let ast::Statement::Require(ast::StmtRequire::Rider(rider)) = statement {
            demands.push(rider.name.as_str(db).S());
        }
    }

    ModuleRiderDemands::new(db, demands)
}
