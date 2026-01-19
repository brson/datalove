//! Module compilation from pre-resolved module graphs.
//!
//! This module provides the core compilation pipeline that accepts pre-resolved
//! module graphs (output of package resolution) and produces compilation artifacts.
//! It does not depend on package resolution or the interpreter.

use rmx::prelude::*;
use rmx::std::collections::BTreeMap;
use serde::{Serialize, Deserialize};

use bct::module_graph::{ModuleGraph, ModuleId};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    typecheck_module_graph_with_mode,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use crate::module_graph::parse_module_graph_with_mode;
use crate::tracked_ownership_analysis::analyze_module_graph_with_mode;
use crate::tracked_lower::{lower_module_graph_with_mode, ModuleGraphLoweringResult};

/// Input for module compilation - the output of package resolution.
pub struct ModuleCompilationInput {
    /// The module graph with modules in dependency order.
    pub graph: ModuleGraph,
    /// Resolved require aliases per module: (alias, target_module_id).
    pub resolved_requires: BTreeMap<ModuleId, Vec<(String, ModuleId)>>,
}

/// Output of module compilation - before interpreter integration.
///
/// Contains all compilation artifacts needed to build interpreter structures.
pub struct ModuleCompilationOutput<'db> {
    /// The input module graph.
    pub module_graph: ModuleGraph,
    /// Parsed module graph with AST.
    pub parsed_graph: ParsedModuleGraph<'db>,
    /// Typecheck results per module.
    pub typecheck_result: ModuleGraphTypecheckResult<'db>,
    /// IR lowering results per module.
    pub lowering_result: ModuleGraphLoweringResult<'db>,
    /// Typecheck errors by module path.
    pub typecheck_errors: BTreeMap<String, Vec<String>>,
    /// Drop analysis errors by module path.
    pub drop_analysis_errors: BTreeMap<String, Vec<String>>,
    /// IR lowering errors by module path (distinct from drop analysis).
    pub lowering_errors: BTreeMap<String, Vec<String>>,
    /// IR dumps by module path (for debugging/display, no errors mixed in).
    pub module_ir_dumps: BTreeMap<String, Vec<String>>,
}

impl<'db> ModuleCompilationOutput<'db> {
    /// Check if compilation succeeded (no errors).
    pub fn is_successful(&self) -> bool {
        self.typecheck_errors.values().all(|e| e.is_empty())
            && self.drop_analysis_errors.values().all(|e| e.is_empty())
            && self.lowering_errors.values().all(|e| e.is_empty())
    }
}

/// Compile modules from pre-resolved input.
///
/// This is the main entry point for compilation. It:
/// 1. Parses all modules
/// 2. Typechecks all modules
/// 3. Performs drop analysis (ownership checking)
/// 4. Lowers to IR
///
/// The result contains all compilation artifacts. Interpreter integration
/// (building ModuleFunctionRegistry, etc.) is handled by the caller.
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

    lower_and_collect_results(db, input.graph, parsed_graph, typecheck_result, mode)
}

/// Lower modules and collect all compilation results.
fn lower_and_collect_results<'db>(
    db: &'db dyn DbClone,
    module_graph: ModuleGraph,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    mode: ParallelMode,
) -> ModuleCompilationOutput<'db> {
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

    // Drop analysis pass (runs before lowering).
    let drop_analysis = analyze_module_graph_with_mode(db, parsed_graph, typecheck_result, mode);

    // Collect drop analysis errors.
    let mut drop_analysis_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (module_id, result) in drop_analysis.module_results(db.as_salsa_db()) {
        let errors = result.errors(db.as_salsa_db());
        if !errors.is_empty() {
            let path = module_id.path(db.as_salsa_db()).clone();
            drop_analysis_errors.insert(path, errors.clone());
        }
    }

    // Lower to IR (consumes drop analysis).
    let lowering_result = lower_module_graph_with_mode(db, parsed_graph, typecheck_result, drop_analysis, mode);

    // Collect lowering errors and IR dumps.
    let mut module_ir_dumps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut lowering_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (module_id, result) in lowering_result.module_results(db.as_salsa_db()) {
        let module_path = module_id.path(db.as_salsa_db()).clone();

        // Lowering errors (no drop analysis errors mixed in).
        let errors = result.errors(db.as_salsa_db());
        if !errors.is_empty() {
            lowering_errors.insert(module_path.clone(), errors.clone());
        }

        // Collect IR dumps.
        let ir_dumps: Vec<String> = result.functions(db.as_salsa_db())
            .iter()
            .map(|ir_func| format!("{}", ir_func))
            .collect();

        module_ir_dumps.insert(module_path, ir_dumps);
    }

    ModuleCompilationOutput {
        module_graph,
        parsed_graph,
        typecheck_result,
        lowering_result,
        typecheck_errors,
        drop_analysis_errors,
        lowering_errors,
        module_ir_dumps,
    }
}

// ============================================================================
// Result types (serializable, no interpreter deps)
// ============================================================================

/// Typecheck result summary (serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum TypecheckResult {
    Success,
    ParseError { errors: Vec<String> },
    Error { errors: Vec<String> },
    Skipped,
}

/// Lowering result summary (serializable).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum LoweringResult {
    Success { ir: String },
    Error { message: String },
    Skipped,
}

// ============================================================================
// Helper functions
// ============================================================================

/// Format lowering result for display.
///
/// Takes IR dumps and errors (both drop analysis and lowering errors).
pub fn format_lowering_result(
    ir_dumps: &[String],
    drop_analysis_errors: &[String],
    lowering_errors: &[String],
    has_typecheck_errors: bool,
) -> LoweringResult {
    if has_typecheck_errors {
        return LoweringResult::Skipped;
    }

    // Combine all errors.
    let all_errors: Vec<_> = drop_analysis_errors.iter()
        .chain(lowering_errors.iter())
        .cloned()
        .collect();

    if !all_errors.is_empty() {
        LoweringResult::Error { message: all_errors.join("\n") }
    } else if ir_dumps.is_empty() {
        LoweringResult::Skipped
    } else {
        LoweringResult::Success { ir: ir_dumps.join("\n") }
    }
}
