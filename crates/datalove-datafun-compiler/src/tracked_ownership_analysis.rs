//! Salsa-tracked API for ownership analysis.
//!
//! Provides memoized, per-module ownership analysis following the same pattern
//! as the typecheck module. Each module is analyzed independently, enabling
//! incremental recompilation and parallel execution.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use bct::module_graph::{Module, ModuleId};
use datalove_ct::query_log::{log_query, QueryPhase};
use datalove_datafun_ast::ast::Statement;
use datalove_datafun_tycheck::{
    SingleModuleTypecheckResult,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};
use datalove_datafun_ownership::{self as ownership_analysis, FunctionAnalysis, AutoAdaptMode};


/// Result of ownership analysis for a single function.
#[salsa::tracked]
pub struct SingleFunctionAnalysis<'db> {
    /// Name of the function.
    #[returns(clone)]
    pub func_name: String,

    /// Analysis result (None if analysis failed with errors).
    #[returns(ref)]
    pub analysis: Option<FunctionAnalysis<'db>>,

    /// Errors from analysis.
    #[returns(ref)]
    pub errors: Vec<String>,
}

/// Result of ownership analysis for a single module.
#[salsa::tracked]
pub struct SingleModuleAnalysis<'db> {
    /// Module that was analyzed.
    #[returns(copy)]
    pub module_id: ModuleId<'db>,

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
    pub module_results: BTreeMap<ModuleId<'db>, SingleModuleAnalysis<'db>>,

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

/// Analyze a single module for ownership errors.
#[salsa::tracked(returns(copy))]
pub fn analyze_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module<'db>,
    typecheck_result: SingleModuleTypecheckResult<'db>,
    auto_adapt_mode: AutoAdaptMode,
) -> SingleModuleAnalysis<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("ownership_analysis", module_path, QueryPhase::Start);

    let expr_types = typecheck_result.expr_types(db);
    let parsed = datalove_datafun_parser::parse_module_ast(db, module);

    // Build map of function name -> resolved param types from exports.
    // This is needed to resolve type aliases in function parameters.
    let mut func_param_types: HashMap<String, Vec<datalove_datafun_tycheck::Type<'db>>> = HashMap::new();
    for (name, func_type) in typecheck_result.exports(db) {
        func_param_types.insert(name.text(db).S(), func_type.param_types(db).C());
    }

    let mut function_analyses = BTreeMap::new();
    let mut all_errors = Vec::new();

    for statement in parsed.statements(db).iter() {
        if let Statement::Fun(func) = statement {
            let func_name = func.name().text(db).S();

            // Get resolved param types for this function.
            let resolved_params = func_param_types.get(&func_name).map(|v| v.as_slice());

            // Run ownership analysis with auto-adapt mode.
            let analysis = ownership_analysis::analyze_function_with_mode(
                db, func, expr_types, typecheck_result.call_targets(db), resolved_params, auto_adapt_mode
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
    let mut module_results = BTreeMap::new();
    let mut all_success = true;

    for (module, single_typecheck) in analysis_work(db, parsed_graph, typecheck_result) {
        let result = analyze_module(db, module, single_typecheck, auto_adapt_mode);

        if !result.errors(db).is_empty() {
            all_success = false;
        }

        module_results.insert(module.id(db), result);
    }

    ModuleGraphAnalysis::new(db, module_results, all_success)
}

/// The modules `analyze_module_graph` analyzes, with the typecheck results it
/// keys `analyze_module` on: every module but those with type errors.
///
/// The parallel compile warms `analyze_module` over exactly these
/// (`datalove_datafun_common::warm`), so that what it computes ahead is what
/// the graph's analysis then asks for.
pub fn analysis_work<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> Vec<(Module<'db>, SingleModuleTypecheckResult<'db>)> {
    let typecheck_errors = typecheck_result.module_errors(db);
    let typecheck_module_results = typecheck_result.module_results(db);
    parsed_graph.graph(db).iter_modules(db)
        .filter(|module| typecheck_errors.get(&module.id(db)).is_none_or(|e| e.is_empty()))
        .map(|module| {
            let single_typecheck = *typecheck_module_results.get(&module.id(db))
                .expect("module should have typecheck result");
            (module, single_typecheck)
        })
        .collect()
}

