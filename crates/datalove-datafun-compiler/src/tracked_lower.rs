//! Salsa-tracked API for IR lowering.
//!
//! Provides memoized, per-module IR lowering following the same pattern as
//! the typecheck module. Each module is lowered independently, enabling
//! incremental recompilation and parallel execution.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use bct::module_graph::{Module, ModuleId};
use datalove_ct::query_log::{log_query, QueryPhase};
use datalove_datafun_ast::ast::{ParsedStatements, Statement};
use datalove_datafun_ir::{IrFunction, FuncId, IrModuleId};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    SingleModuleTypecheckResult,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use crate::drop_analysis;
use crate::lower;

/// Function ID map for cross-module call resolution.
///
/// This tracked struct wraps the mapping from (ModuleId, func_name) to
/// (IrModuleId, FuncId). Being a tracked struct makes it hashable and
/// usable as a parameter to other tracked functions.
#[salsa::tracked]
pub struct FuncIdMap<'db> {
    /// Mapping entries as a sorted vector for deterministic hashing.
    #[returns(ref)]
    pub entries: Vec<((ModuleId, String), (IrModuleId, FuncId))>,
}

impl<'db> FuncIdMap<'db> {
    /// Look up a function's IR location by module and name.
    pub fn get(&self, db: &'db dyn salsa::Database, module_id: ModuleId, func_name: &str) -> Option<(IrModuleId, FuncId)> {
        self.entries(db)
            .iter()
            .find(|((mid, name), _)| *mid == module_id && name == func_name)
            .map(|(_, loc)| *loc)
    }

    /// Convert to a HashMap for efficient repeated lookups.
    pub fn to_hashmap(&self, db: &'db dyn salsa::Database) -> HashMap<(ModuleId, String), (IrModuleId, FuncId)> {
        self.entries(db).iter().cloned().collect()
    }
}

/// Compute the function ID map from a parsed module graph.
///
/// This assigns module-local FuncIds (0, 1, 2, ...) to each function in each
/// module. The result is cached based on the parsed graph structure.
#[salsa::tracked]
pub fn compute_func_id_map<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
) -> FuncIdMap<'db> {
    let mut entries = Vec::new();

    for (ir_module_idx, (module_id, parsed)) in parsed_graph.statements_only(db).iter().enumerate() {
        let ir_module_id = IrModuleId(ir_module_idx as u32);
        let mut next_func_id: u32 = 0;

        for statement in &parsed.statements {
            if let Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).S();
                let func_id = FuncId(next_func_id);
                next_func_id += 1;
                entries.push(((*module_id, func_name), (ir_module_id, func_id)));
            }
        }
    }

    // Sort for deterministic ordering.
    entries.sort_by(|a, b| {
        let ((mod_a, name_a), _) = a;
        let ((mod_b, name_b), _) = b;
        mod_a.cmp(mod_b).then_with(|| name_a.cmp(name_b))
    });

    FuncIdMap::new(db, entries)
}

/// Result of lowering a single module to IR.
#[salsa::tracked]
pub struct SingleModuleLoweringResult<'db> {
    /// Module that was lowered.
    pub module_id: ModuleId,

    /// IR module index (0-based position in graph).
    pub ir_module_id: IrModuleId,

    /// Successfully lowered functions.
    #[returns(ref)]
    pub functions: Vec<IrFunction>,

    /// Lowering errors (from drop analysis or IR generation).
    #[returns(ref)]
    pub errors: Vec<String>,

    /// Function name to FuncId mapping for this module.
    #[returns(ref)]
    pub func_ids: Vec<(String, FuncId)>,
}

/// Result of lowering an entire module graph to IR.
#[salsa::tracked]
pub struct ModuleGraphLoweringResult<'db> {
    /// Per-module lowering results (module_id -> result).
    #[returns(ref)]
    pub module_results: BTreeMap<ModuleId, SingleModuleLoweringResult<'db>>,

    /// Function ID map for cross-module calls.
    pub func_id_map: FuncIdMap<'db>,

    /// Whether all modules lowered successfully.
    pub success: bool,
}

impl<'db> ModuleGraphLoweringResult<'db> {
    /// Check if lowering succeeded (no errors).
    pub fn is_ok(&self, db: &'db dyn salsa::Database) -> bool {
        self.success(db)
    }

    /// Get all lowering errors across all modules.
    pub fn all_errors(&self, db: &'db dyn salsa::Database) -> Vec<String> {
        self.module_results(db)
            .values()
            .flat_map(|r| r.errors(db).iter().cloned())
            .collect()
    }

    /// Get all successfully lowered functions.
    pub fn all_functions(&self, db: &'db dyn salsa::Database) -> Vec<(IrModuleId, FuncId, IrFunction)> {
        self.module_results(db)
            .values()
            .flat_map(|r| {
                let ir_mod = r.ir_module_id(db);
                r.functions(db).iter().map(move |f| (ir_mod, f.id, f.clone()))
            })
            .collect()
    }
}

/// Lower a single module to IR.
///
/// This is a tracked function enabling per-module memoization. The `module`
/// parameter serves as the primary cache key.
///
/// Drop analysis is performed as part of lowering. If drop analysis fails
/// for a function, that function is skipped and an error is recorded.
#[salsa::tracked]
pub fn lower_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
    ir_module_id: IrModuleId,
    parsed: ParsedStatements<'db>,
    typecheck_result: SingleModuleTypecheckResult<'db>,
    func_id_map: FuncIdMap<'db>,
) -> SingleModuleLoweringResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("lower", module_path, QueryPhase::Start);

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    // Convert FuncIdMap to HashMap for efficient lookup during lowering.
    let func_id_hashmap = func_id_map.to_hashmap(db);

    let mut functions = Vec::new();
    let mut errors = Vec::new();
    let mut func_ids = Vec::new();

    // Assign module-local FuncIds (0, 1, 2, ...).
    let mut next_func_id: u32 = 0;
    for statement in &parsed.statements {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();
            let func_id = FuncId(next_func_id);
            next_func_id += 1;
            func_ids.push((func_name, func_id));
        }
    }

    // Process each function: drop analysis then lowering.
    let mut func_idx = 0;
    for statement in &parsed.statements {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();
            let func_id = func_ids[func_idx].1;
            func_idx += 1;

            // Run drop analysis.
            let analysis = drop_analysis::analyze_function(db, *func, expr_types, call_targets);

            if !analysis.errors.is_empty() {
                let error_msgs: Vec<String> = analysis.errors.iter()
                    .map(|e| format!("{:?}", e))
                    .collect();
                errors.push(format!(
                    "Drop analysis error in {}: {}",
                    func_name,
                    error_msgs.join(", ")
                ));
                continue;
            }

            // Lower to IR.
            match lower::lower_function_for_module(
                db,
                expr_types,
                call_targets,
                &func_id_hashmap,
                *func,
                func_id,
                analysis,
            ) {
                Ok(ir_func) => {
                    functions.push(ir_func);
                }
                Err(e) => {
                    errors.push(format!("Lowering error in {}: {}", func_name, e));
                }
            }
        }
    }

    log_query("lower", module_path, QueryPhase::End);

    SingleModuleLoweringResult::new(
        db,
        module_id,
        ir_module_id,
        functions,
        errors,
        func_ids,
    )
}

/// Lower an entire module graph to IR.
///
/// Processes modules in dependency order, calling the per-module `lower_module`
/// function for each. Each per-module result is cached independently.
#[salsa::tracked]
pub fn lower_module_graph<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> ModuleGraphLoweringResult<'db> {
    let graph = parsed_graph.graph(db);

    // Compute func_id_map (tracked, cached based on parsed_graph).
    let func_id_map = compute_func_id_map(db, parsed_graph);

    // Build module lookup.
    let module_map: HashMap<ModuleId, Module> = graph.iter_modules(db)
        .map(|m| (m.id(db), m))
        .collect();

    // Get per-module typecheck results.
    let typecheck_errors = typecheck_result.module_errors(db);
    let typecheck_module_results = typecheck_result.module_results(db);

    // Lower each module.
    let mut module_results = BTreeMap::new();
    let mut all_success = true;

    for (ir_module_idx, (module_id, parsed)) in parsed_graph.statements_only(db).iter().enumerate() {
        let ir_module_id = IrModuleId(ir_module_idx as u32);

        // Skip modules with typecheck errors.
        if typecheck_errors.get(module_id).map_or(false, |e| !e.is_empty()) {
            continue;
        }

        let module = *module_map.get(module_id).expect("module should exist");

        // Get the tracked per-module typecheck result (preserves salsa ID for memoization).
        let single_typecheck = *typecheck_module_results.get(module_id)
            .expect("module should have typecheck result");

        let result = lower_module(
            db,
            module,
            ir_module_id,
            parsed.clone(),
            single_typecheck,
            func_id_map,
        );

        if !result.errors(db).is_empty() {
            all_success = false;
        }

        module_results.insert(*module_id, result);
    }

    ModuleGraphLoweringResult::new(db, module_results, func_id_map, all_success)
}

/// Lower a module graph using parallel execution.
///
/// Lowers modules in parallel using rayon to warm salsa's memoization cache,
/// then delegates to the tracked `lower_module_graph` function which will
/// hit the warmed cache.
pub fn lower_module_graph_parallel<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
) -> ModuleGraphLoweringResult<'db> {
    use rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);

    // Compute func_id_map once (will be cached).
    let func_id_map = compute_func_id_map(db_salsa, parsed_graph);

    // Build module lookup and data for parallel phase.
    let module_map: HashMap<ModuleId, Module> = graph.iter_modules(db_salsa)
        .map(|m| (m.id(db_salsa), m))
        .collect();

    let typecheck_errors = typecheck_result.module_errors(db_salsa);
    let typecheck_module_results = typecheck_result.module_results(db_salsa);

    // Prepare work items for parallel execution.
    let work: Vec<_> = parsed_graph.statements_only(db_salsa)
        .iter()
        .enumerate()
        .filter_map(|(ir_module_idx, (module_id, parsed))| {
            // Skip modules with typecheck errors.
            if typecheck_errors.get(module_id).map_or(false, |e| !e.is_empty()) {
                return None;
            }

            let module = *module_map.get(module_id)?;

            // Get the tracked per-module typecheck result.
            let single_typecheck = *typecheck_module_results.get(module_id)?;

            Some((
                db.dyn_clone(),
                module,
                IrModuleId(ir_module_idx as u32),
                parsed.clone(),
                single_typecheck,
            ))
        })
        .collect();

    // Lower modules in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module, ir_module_id, parsed, single_typecheck)| {
        let db_s = db_clone.as_salsa_db();

        // This populates the cache.
        let _ = lower_module(
            db_s,
            module,
            ir_module_id,
            parsed,
            single_typecheck,
            func_id_map,
        );
    });

    // Delegate to tracked function which aggregates results.
    // All lower_module calls will be cache hits from the parallel phase.
    lower_module_graph(db_salsa, parsed_graph, typecheck_result)
}

/// Lower module graph with configurable parallelism.
pub fn lower_module_graph_with_mode<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    mode: ParallelMode,
) -> ModuleGraphLoweringResult<'db> {
    match mode {
        ParallelMode::Sequential => lower_module_graph(db.as_salsa_db(), parsed_graph, typecheck_result),
        ParallelMode::Parallel => lower_module_graph_parallel(db, parsed_graph, typecheck_result),
    }
}
