//! Module graph abstraction for the core compiler.
//!
//! Re-exports core types from bct and adds datafun-specific parsing/orchestration.

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

// Re-export typecheck result types from tycheck crate.
pub use datalove_datafun_tycheck::{
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

/// Parse all modules in a graph with resolved requires.
///
/// Returns a ParsedModuleGraph containing the original graph, pre-parsed statements,
/// spans, and resolved require aliases from package resolution.
#[salsa::tracked]
pub fn parse_module_graph<'db>(
    db: &'db dyn salsa::Database,
    graph: ModuleGraph,
    resolved_requires_str: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
) -> ParsedModuleGraph<'db> {
    let mut parsed_statements = Vec::new();
    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let source = module.source(db);
        let parse_result = datalove_datafun_parser::parse(db, source);
        let spans = datalove_datafun_parser::datafun_spans(db, source);
        parsed_statements.push((module_id, parse_result.parsed(db), spans));
    }

    // Convert String aliases to InternedText.
    let resolved_requires: BTreeMap<ModuleId, Vec<(InternedText<'db>, ModuleId)>> =
        resolved_requires_str.into_iter()
            .map(|(module_id, requires)| {
                let interned_requires: Vec<(InternedText<'db>, ModuleId)> = requires.into_iter()
                    .map(|(alias, target)| (InternedText::new(db, alias), target))
                    .collect();
                (module_id, interned_requires)
            })
            .collect();

    ParsedModuleGraph::new(db, graph, parsed_statements, resolved_requires)
}
