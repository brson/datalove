//! Salsa-tracked API for drop analysis.
//!
//! Provides memoized, per-module drop analysis following the same pattern as
//! the typecheck module. Each module is analyzed independently, enabling
//! incremental recompilation and parallel execution.
//!
//! Drop analysis runs after typechecking and before lowering. It computes drop
//! schedules for resource management and detects ownership errors (use-after-move,
//! move-in-loop, etc).

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use bct::module_graph::{Module, ModuleId};
use datalove_ct::query_log::{log_query, QueryPhase};
use datalove_datafun_ast::ast::{ParsedStatements, Statement};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    SingleModuleTypecheckResult,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use crate::ownership_analysis::{self, FunctionDropAnalysis};

/// Result of drop analysis for a single function.
#[salsa::tracked]
pub struct SingleFunctionDropAnalysis<'db> {
    /// Name of the function.
    pub func_name: String,

    /// Drop analysis result (None if analysis failed with errors).
    #[returns(ref)]
    pub analysis: Option<FunctionDropAnalysis>,

    /// Errors from drop analysis.
    #[returns(ref)]
    pub errors: Vec<String>,
}

/// Result of drop analysis for a single module.
#[salsa::tracked]
pub struct SingleModuleDropAnalysis<'db> {
    /// Module that was analyzed.
    pub module_id: ModuleId,

    /// Per-function drop analysis results.
    #[returns(ref)]
    pub function_analyses: BTreeMap<String, SingleFunctionDropAnalysis<'db>>,

    /// Aggregated errors from all functions.
    #[returns(ref)]
    pub errors: Vec<String>,
}

/// Result of drop analysis for an entire module graph.
#[salsa::tracked]
pub struct ModuleGraphDropAnalysis<'db> {
    /// Per-module drop analysis results.
    #[returns(ref)]
    pub module_results: BTreeMap<ModuleId, SingleModuleDropAnalysis<'db>>,

    /// Whether all modules analyzed successfully.
    pub success: bool,
}

impl<'db> ModuleGraphDropAnalysis<'db> {
    /// Check if drop analysis succeeded (no errors).
    pub fn is_ok(&self, db: &'db dyn salsa::Database) -> bool {
        self.success(db)
    }

    /// Get all drop analysis errors across all modules.
    pub fn all_errors(&self, db: &'db dyn salsa::Database) -> Vec<String> {
        self.module_results(db)
            .values()
            .flat_map(|r| r.errors(db).iter().cloned())
            .collect()
    }
}

/// Analyze drops for a single module.
#[salsa::tracked]
pub fn analyze_module_drops<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
    parsed: ParsedStatements<'db>,
    typecheck_result: SingleModuleTypecheckResult<'db>,
) -> SingleModuleDropAnalysis<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("ownership_analysis", module_path, QueryPhase::Start);

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    let mut function_analyses = BTreeMap::new();
    let mut all_errors = Vec::new();

    for statement in &parsed.statements {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();

            // Run drop analysis.
            let analysis = ownership_analysis::analyze_function(db, *func, expr_types, call_targets);

            let (opt_analysis, errors) = if analysis.errors.is_empty() {
                (Some(analysis), Vec::new())
            } else {
                // Format errors same as original: single error string with comma-separated errors.
                let error_msgs: Vec<String> = analysis.errors.iter()
                    .map(|e| format!("{:?}", e))
                    .collect();
                let formatted = format!(
                    "Drop analysis error in {}: {}",
                    func_name,
                    error_msgs.join(", ")
                );
                all_errors.push(formatted.clone());
                (None, vec![formatted])
            };

            let single_analysis = SingleFunctionDropAnalysis::new(
                db,
                func_name.clone(),
                opt_analysis,
                errors,
            );
            function_analyses.insert(func_name, single_analysis);
        }
    }

    log_query("ownership_analysis", module_path, QueryPhase::End);

    SingleModuleDropAnalysis::new(db, module_id, function_analyses, all_errors)
}

/// Analyze drops for an entire module graph.
#[salsa::tracked]
pub fn analyze_module_graph_drops<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> ModuleGraphDropAnalysis<'db> {
    let graph = parsed_graph.graph(db);

    // Build module lookup.
    let module_map: HashMap<ModuleId, Module> = graph.iter_modules(db)
        .map(|m| (m.id(db), m))
        .collect();

    // Get per-module typecheck results.
    let typecheck_errors = typecheck_result.module_errors(db);
    let typecheck_module_results = typecheck_result.module_results(db);

    // Analyze each module.
    let mut module_results = BTreeMap::new();
    let mut all_success = true;

    for (module_id, parsed) in parsed_graph.statements_only(db).iter() {
        // Skip modules with typecheck errors.
        if typecheck_errors.get(module_id).map_or(false, |e| !e.is_empty()) {
            continue;
        }

        let module = *module_map.get(module_id).expect("module should exist");

        // Get the tracked per-module typecheck result.
        let single_typecheck = *typecheck_module_results.get(module_id)
            .expect("module should have typecheck result");

        let result = analyze_module_drops(db, module, parsed.clone(), single_typecheck);

        if !result.errors(db).is_empty() {
            all_success = false;
        }

        module_results.insert(*module_id, result);
    }

    ModuleGraphDropAnalysis::new(db, module_results, all_success)
}

/// Analyze drops for a module graph using parallel execution.
pub fn analyze_module_graph_drops_parallel<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> ModuleGraphDropAnalysis<'db> {
    use rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);

    // Build module lookup.
    let module_map: HashMap<ModuleId, Module> = graph.iter_modules(db_salsa)
        .map(|m| (m.id(db_salsa), m))
        .collect();

    let typecheck_errors = typecheck_result.module_errors(db_salsa);
    let typecheck_module_results = typecheck_result.module_results(db_salsa);

    // Prepare work items for parallel execution.
    let work: Vec<_> = parsed_graph.statements_only(db_salsa)
        .iter()
        .filter_map(|(module_id, parsed)| {
            // Skip modules with typecheck errors.
            if typecheck_errors.get(module_id).map_or(false, |e| !e.is_empty()) {
                return None;
            }

            let module = *module_map.get(module_id)?;
            let single_typecheck = *typecheck_module_results.get(module_id)?;

            Some((db.dyn_clone(), module, parsed.clone(), single_typecheck))
        })
        .collect();

    // Analyze modules in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module, parsed, single_typecheck)| {
        let db_s = db_clone.as_salsa_db();
        let _ = analyze_module_drops(db_s, module, parsed, single_typecheck);
    });

    // Delegate to tracked function which aggregates results.
    analyze_module_graph_drops(db_salsa, parsed_graph, typecheck_result)
}

/// Analyze module graph drops with configurable parallelism.
pub fn analyze_module_graph_drops_with_mode<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    mode: ParallelMode,
) -> ModuleGraphDropAnalysis<'db> {
    match mode {
        ParallelMode::Sequential => analyze_module_graph_drops(db.as_salsa_db(), parsed_graph, typecheck_result),
        ParallelMode::Parallel => analyze_module_graph_drops_parallel(db, parsed_graph, typecheck_result),
    }
}
