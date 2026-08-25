//! Salsa-tracked API for ownership analysis.
//!
//! Provides memoized, per-module ownership analysis following the same pattern
//! as the typecheck module. Each module is analyzed independently, enabling
//! incremental recompilation and parallel execution.

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
    Type,
};
use datalove_datafun_ir::IrType;
use datalove_datafun_ownership::{self as ownership_analysis, FunctionAnalysis, AutoAdaptMode};

use crate::IrTypeExt;

/// Result of ownership analysis for a single function.
#[salsa::tracked]
pub struct SingleFunctionAnalysis<'db> {
    /// Name of the function.
    #[returns(clone)]
    pub func_name: String,

    /// Analysis result (None if analysis failed with errors).
    #[returns(ref)]
    pub analysis: Option<FunctionAnalysis>,

    /// Errors from analysis.
    #[returns(ref)]
    pub errors: Vec<String>,
}

/// Result of ownership analysis for a single module.
#[salsa::tracked]
pub struct SingleModuleAnalysis<'db> {
    /// Module that was analyzed.
    #[returns(copy)]
    pub module_id: ModuleId,

    /// Per-function analysis results.
    #[returns(ref)]
    pub function_analyses: BTreeMap<String, SingleFunctionAnalysis<'db>>,

    /// Aggregated errors from all functions.
    #[returns(ref)]
    pub errors: Vec<String>,
}

/// Result of ownership analysis for an entire module graph.
#[salsa::tracked]
pub struct ModuleGraphAnalysis<'db> {
    /// Per-module analysis results.
    #[returns(ref)]
    pub module_results: BTreeMap<ModuleId, SingleModuleAnalysis<'db>>,

    /// Whether all modules analyzed successfully.
    #[returns(copy)]
    pub success: bool,
}

impl<'db> ModuleGraphAnalysis<'db> {
    /// Check if analysis succeeded (no errors).
    pub fn is_ok(&self, db: &'db dyn salsa::Database) -> bool {
        self.success(db)
    }

    /// Get all analysis errors across all modules.
    pub fn all_errors(&self, db: &'db dyn salsa::Database) -> Vec<String> {
        self.module_results(db)
            .values()
            .flat_map(|r| r.errors(db).iter().cloned())
            .collect()
    }
}

// ============================================================================
// Type Conversion Helpers
// ============================================================================

/// Convert tycheck expression types to IR types.
fn convert_expr_types<'db>(
    db: &'db dyn salsa::Database,
    types: &[Option<Type<'db>>],
) -> Vec<Option<IrType>> {
    types.iter()
        .map(|opt| opt.as_ref().map(|ty| IrType::from_tycheck(db, ty)))
        .collect()
}

/// Analyze a single module for ownership errors.
#[salsa::tracked(returns(copy))]
pub fn analyze_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
    parsed: ParsedStatements<'db>,
    typecheck_result: SingleModuleTypecheckResult<'db>,
    auto_adapt_mode: AutoAdaptMode,
) -> SingleModuleAnalysis<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("ownership_analysis", module_path, QueryPhase::Start);

    // Convert tycheck types to IR types.
    let expr_types = convert_expr_types(db, typecheck_result.expr_types(db));

    // Build map of function name -> resolved param types from exports.
    // This is needed to resolve type aliases in function parameters.
    let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
    for (name, func_type) in typecheck_result.exports(db) {
        let param_types: Vec<IrType> = func_type.param_types(db)
            .iter()
            .map(|ty| IrType::from_tycheck(db, ty))
            .collect();
        func_param_types.insert(name.text(db).S(), param_types);
    }

    let mut function_analyses = BTreeMap::new();
    let mut all_errors = Vec::new();

    for statement in &parsed.statements {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();

            // Get resolved param types for this function.
            let resolved_params = func_param_types.get(&func_name).map(|v| v.as_slice());

            // Run ownership analysis with auto-adapt mode.
            let analysis = ownership_analysis::analyze_function_with_mode(
                db, *func, &expr_types, resolved_params, auto_adapt_mode
            );

            let (opt_analysis, errors) = if analysis.errors.is_empty() {
                (Some(analysis), Vec::new())
            } else {
                // Format errors same as original: single error string with comma-separated errors.
                let error_msgs: Vec<String> = analysis.errors.iter()
                    .map(|e| format!("{:?}", e))
                    .collect();
                let formatted = format!(
                    "Ownership error in {}: {}",
                    func_name,
                    error_msgs.join(", ")
                );
                all_errors.push(formatted.clone());
                (None, vec![formatted])
            };

            let single_analysis = SingleFunctionAnalysis::new(
                db,
                func_name.clone(),
                opt_analysis,
                errors,
            );
            function_analyses.insert(func_name, single_analysis);
        }
    }

    log_query("ownership_analysis", module_path, QueryPhase::End);

    SingleModuleAnalysis::new(db, module_id, function_analyses, all_errors)
}

/// Analyze an entire module graph for ownership errors.
#[salsa::tracked(returns(copy))]
pub fn analyze_module_graph<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    auto_adapt_mode: AutoAdaptMode,
) -> ModuleGraphAnalysis<'db> {
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

        let result = analyze_module(db, module, parsed.clone(), single_typecheck, auto_adapt_mode);

        if !result.errors(db).is_empty() {
            all_success = false;
        }

        module_results.insert(*module_id, result);
    }

    ModuleGraphAnalysis::new(db, module_results, all_success)
}

/// Analyze module graph for ownership errors using parallel execution.
pub fn analyze_module_graph_parallel<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    auto_adapt_mode: AutoAdaptMode,
) -> ModuleGraphAnalysis<'db> {
    use rmx::rayon::prelude::*;

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
        let _ = analyze_module(db_s, module, parsed, single_typecheck, auto_adapt_mode);
    });

    // Delegate to tracked function which aggregates results.
    analyze_module_graph(db_salsa, parsed_graph, typecheck_result, auto_adapt_mode)
}

/// Analyze module graph for ownership errors with configurable parallelism.
pub fn analyze_module_graph_with_mode<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    mode: ParallelMode,
    auto_adapt_mode: AutoAdaptMode,
) -> ModuleGraphAnalysis<'db> {
    match mode {
        ParallelMode::Sequential => analyze_module_graph(db.as_salsa_db(), parsed_graph, typecheck_result, auto_adapt_mode),
        ParallelMode::Parallel => analyze_module_graph_parallel(db, parsed_graph, typecheck_result, auto_adapt_mode),
    }
}
