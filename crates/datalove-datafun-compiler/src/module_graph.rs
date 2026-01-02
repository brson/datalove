//! Module graph abstraction for the core compiler.
//!
//! Re-exports core types from bct and adds datafun-specific typecheck result types.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use bct::text::InternedText;

// Re-export core module graph types from bct.
pub use bct::module_graph::{
    ModuleId,
    Module,
    ModuleGraph,
    ModuleGraphBuilder,
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

// ============================================================================
// Typecheck Result Types (datafun-specific)
// ============================================================================

/// Exported function signatures from a module.
#[salsa::tracked]
pub struct ModuleExports<'db> {
    /// Module this is for.
    pub module_id: ModuleId,

    /// Function signatures as a vector of (name, type) pairs.
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, crate::tycheck::TypeFunction<'db>)>,
}

/// Imported functions for a module.
#[salsa::tracked]
pub struct ModuleImports<'db> {
    /// Module this is for.
    pub module_id: ModuleId,

    /// Imported functions: (local_name, source_module_id, source_name).
    #[returns(ref)]
    pub functions: Vec<(InternedText<'db>, ModuleId, InternedText<'db>)>,
}

/// Result of typechecking a module graph.
#[salsa::tracked]
pub struct ModuleGraphTypecheckResult<'db> {
    /// The module graph that was typechecked.
    pub graph: ModuleGraph,

    /// Type errors encountered, per module.
    #[returns(ref)]
    pub module_errors: BTreeMap<ModuleId, Vec<crate::tycheck::TypeError>>,

    /// Module exports, per module.
    #[returns(ref)]
    pub module_exports: BTreeMap<ModuleId, ModuleExports<'db>>,

    /// Module imports, per module.
    #[returns(ref)]
    pub module_imports: BTreeMap<ModuleId, ModuleImports<'db>>,

    /// Function analysis results for all functions in all modules.
    ///
    /// Indexed by StmtFun salsa ID, contains analysis for all functions
    /// across all modules in the graph.
    #[returns(ref)]
    pub function_analyses: Vec<Option<crate::function_analysis::FunctionAnalysis<'db>>>,

    /// Expression types from all modules, combined.
    ///
    /// Indexed by ExprFun salsa ID, contains types for all expressions
    /// across all modules in the graph.
    #[returns(ref)]
    pub expr_types: Vec<Option<crate::tycheck::TypeAndHeap<'db>>>,

    /// Resolved call targets from all modules, combined.
    ///
    /// Indexed by ExprFunctionCall salsa ID, contains resolved function ASTs
    /// for all function calls across all modules in the graph.
    #[returns(ref)]
    pub call_targets: Vec<Option<crate::tycheck::ResolvedCallTarget<'db>>>,
}

impl<'db> ModuleGraphTypecheckResult<'db> {
    /// Check if typechecking succeeded (no errors).
    pub fn is_ok(&self, db: &'db dyn crate::Db) -> bool {
        self.module_errors(db).values().all(|errors| errors.is_empty())
    }

    /// Get all errors across all modules.
    pub fn all_errors(&self, db: &'db dyn crate::Db) -> Vec<&crate::tycheck::TypeError> {
        self.module_errors(db).values().flatten().collect()
    }
}
