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
    ParsedModuleGraph,
};

/// Parse all modules in a graph.
///
/// Returns a ParsedModuleGraph containing the original graph plus pre-parsed statements and spans.
#[salsa::tracked]
pub fn parse_module_graph<'db>(
    db: &'db dyn salsa::Database,
    graph: ModuleGraph,
) -> ParsedModuleGraph<'db> {
    let mut parsed_statements = Vec::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let source = module.source(db);
        let parse_result = datalove_datafun_parser::parse(db, source);
        let spans = datalove_datafun_parser::datafun_spans(db, source);
        parsed_statements.push((module_id, parse_result.parsed(db), spans));
    }
    ParsedModuleGraph::new(db, graph, parsed_statements)
}
