//! Module graph abstraction for the core compiler.
//!
//! Re-exports core types from bct and adds datafun-specific parsing/orchestration.

use rmx::prelude::*;

// Re-export core module graph types from bct.
pub use bct::module_graph::{
    ModuleId,
    Module,
    ModuleGraph,
    ModuleGraphBuilder,
};

// Re-export typecheck result types from tycheck crate.
pub use datalove_datafun_tycheck::{
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
};

/// A module graph paired with pre-parsed scripts for each module.
#[salsa::tracked]
pub struct ParsedModuleGraph<'db> {
    /// The underlying module graph.
    pub graph: ModuleGraph,

    /// Pre-parsed scripts, as (ModuleId, Script) pairs.
    /// Order matches graph.iter_modules() order.
    #[returns(ref)]
    pub scripts: Vec<(ModuleId, crate::ast::Script<'db>)>,
}

impl<'db> ParsedModuleGraph<'db> {
    /// Get the script for a module by its ID.
    pub fn get_script(&self, db: &'db dyn crate::Db, module_id: ModuleId) -> Option<crate::ast::Script<'db>> {
        self.scripts(db).iter()
            .find(|(id, _)| *id == module_id)
            .map(|(_, script)| *script)
    }
}

/// Parse all modules in a graph.
///
/// Returns a ParsedModuleGraph containing the original graph plus pre-parsed scripts.
#[salsa::tracked]
pub fn parse_module_graph<'db>(
    db: &'db dyn crate::Db,
    graph: ModuleGraph,
) -> ParsedModuleGraph<'db> {
    let mut scripts = Vec::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let source = module.source(db);
        let parse_result = crate::parser::parse(db, source);
        scripts.push((module_id, parse_result.script(db)));
    }
    ParsedModuleGraph::new(db, graph, scripts)
}
