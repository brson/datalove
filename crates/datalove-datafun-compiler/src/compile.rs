//! Module compilation from pre-resolved module graphs.
//!
//! This module provides the core compilation pipeline that accepts pre-resolved
//! module graphs (output of package resolution) and produces compilation artifacts.
//! It does not depend on package resolution or the interpreter.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;

use bct::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    typecheck_module_graph_with_mode,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use crate::module_graph::{parse_module_graph_with_mode, parse_module_full};
use crate::tracked_ownership_analysis::{analyze_module_graph_with_mode, ModuleGraphAnalysis};

/// Input for module compilation - the output of package resolution.
pub struct ModuleCompilationInput {
    /// The module graph with modules in dependency order.
    pub graph: ModuleGraph,
    /// Resolved require aliases per module: (alias, target_module_id).
    pub resolved_requires: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
}

/// Output of module compilation - analysis results without lowering.
///
/// Contains parsing, typechecking, and ownership analysis results.
/// Callers who need IR lowering should call `lower_module_graph_with_evaluator`
/// after checking for errors.
pub struct ModuleCompilationOutput<'db> {
    /// The input module graph.
    pub module_graph: ModuleGraph,
    /// Parsed module graph with AST.
    pub parsed_graph: ParsedModuleGraph<'db>,
    /// Typecheck results per module.
    pub typecheck_result: ModuleGraphTypecheckResult<'db>,
    /// Ownership analysis results per module.
    pub ownership_analysis: ModuleGraphAnalysis<'db>,
    /// Parse errors by module path.
    pub parse_errors: BTreeMap<String, Vec<String>>,
    /// Typecheck errors by module path.
    pub typecheck_errors: BTreeMap<String, Vec<String>>,
    /// Ownership analysis errors by module path.
    pub ownership_errors: BTreeMap<String, Vec<String>>,
}

impl<'db> ModuleCompilationOutput<'db> {
    /// Check if analysis succeeded (no errors).
    pub fn is_successful(&self) -> bool {
        self.parse_errors.values().all(|e| e.is_empty())
            && self.typecheck_errors.values().all(|e| e.is_empty())
            && self.ownership_errors.values().all(|e| e.is_empty())
    }
}

/// Analyze modules from pre-resolved input.
///
/// Runs parsing, typechecking, and ownership analysis. Does NOT run IR lowering.
/// Callers who need IR should call `lower_module_graph_with_evaluator` after checking
/// that `is_successful()` returns true.
///
/// This separation allows callers to skip lowering entirely when analysis fails,
/// and gives them control over when/if lowering happens.
pub fn compile_modules<'db>(
    db: &'db dyn DbClone,
    input: ModuleCompilationInput,
    mode: ParallelMode,
) -> ModuleCompilationOutput<'db> {
    let parsed_graph = parse_module_graph_with_mode(
        db,
        input.graph.clone(),
        input.resolved_requires,
        mode,
    );
    let typecheck_result = typecheck_module_graph_with_mode(db, parsed_graph, mode);
    let ownership_analysis = analyze_module_graph_with_mode(db, parsed_graph, typecheck_result, mode);

    collect_results(db, input.graph, parsed_graph, typecheck_result, ownership_analysis)
}

/// Collect all analysis results into the output structure.
fn collect_results<'db>(
    db: &'db dyn DbClone,
    module_graph: ModuleGraph,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
) -> ModuleCompilationOutput<'db> {
    // Collect parse errors from accumulated diagnostics.
    let mut parse_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for module in module_graph.iter_modules(db.as_salsa_db()) {
        let diags = parse_module_full::accumulated::<datalove_diagnostic::ParseDiagnostic>(db.as_salsa_db(), module);
        if !diags.is_empty() {
            let path = module.id(db.as_salsa_db()).path(db.as_salsa_db()).clone();
            let error_strings: Vec<String> = diags.iter()
                .map(|d| {
                    let diag = d.to_diagnostic(db.as_salsa_db());
                    diag.message.as_str(db.as_salsa_db()).S()
                })
                .collect();
            parse_errors.insert(path, error_strings);
        }
    }

    // Collect typecheck errors with location info from pending diagnostics.
    let module_results = typecheck_result.module_results(db.as_salsa_db());
    let mut typecheck_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (module_id, result) in module_results {
        let errors = result.errors(db.as_salsa_db());
        if errors.is_empty() {
            continue;
        }
        let path = module_id.path(db.as_salsa_db()).clone();

        // Format pending diagnostics with location info.
        let pending = result.pending_diagnostics(db.as_salsa_db());
        let span_lookup = datalove_datafun_tycheck::ModuleGraphSpanLookup::new(&parsed_graph, *module_id);
        let formatted = datalove_datafun_tycheck::format_pending_diagnostics(db.as_salsa_db(), pending, &span_lookup);

        // Use formatted diagnostics if available, otherwise fall back to raw error format.
        let error_strings = if !formatted.is_empty() {
            formatted
        } else {
            errors.iter()
                .map(|e| format!("{}: {:?}", path, e))
                .collect()
        };
        typecheck_errors.insert(path, error_strings);
    }

    // Collect ownership analysis errors.
    let mut ownership_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (module_id, result) in ownership_analysis.module_results(db.as_salsa_db()) {
        let errors = result.errors(db.as_salsa_db());
        if !errors.is_empty() {
            let path = module_id.path(db.as_salsa_db()).clone();
            ownership_errors.insert(path, errors.clone());
        }
    }

    ModuleCompilationOutput {
        module_graph,
        parsed_graph,
        typecheck_result,
        ownership_analysis,
        parse_errors,
        typecheck_errors,
        ownership_errors,
    }
}
