//! Salsa-tracked API for IR lowering.
//!
//! Provides memoized, per-module IR lowering following the same pattern as
//! the typecheck module. Each module is lowered independently, enabling
//! incremental recompilation and parallel execution.

use rmx::prelude::*;
use rmx::std::collections::{BTreeMap, HashMap};
use salsa::plumbing::AsId;
use bct::module_graph::{Module, ModuleId};
use datalove_ct::query_log::{log_query, QueryPhase};
use datalove_datafun_ast::ast::{ParsedStatements, Statement};
use std::cell::RefCell;
use std::rc::Rc;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, IrFunction, IrType, FuncId, IrModuleId};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    SingleModuleTypecheckResult,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use crate::ir_ext::IrTypeExt;
use crate::lower;
use crate::tracked_ownership_analysis::{SingleModuleAnalysis, ModuleGraphAnalysis};

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

/// Pre-resolved const bindings for a module.
///
/// This is computed outside tracked functions (using the CTFE evaluator),
/// then passed to tracked lowering functions as plain hashable data.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ModulePreResolvedConsts {
    /// Module these consts belong to.
    pub module_id: ModuleId,

    /// Evaluated const bindings: name -> (type, value).
    pub consts: Vec<(String, IrType, ConstValue)>,

    /// Errors encountered during const evaluation.
    pub errors: Vec<String>,
}

impl ModulePreResolvedConsts {
    /// Create a new pre-resolved consts container.
    pub fn new(module_id: ModuleId, consts: Vec<(String, IrType, ConstValue)>) -> Self {
        Self { module_id, consts, errors: Vec::new() }
    }

    /// Create with errors.
    pub fn with_errors(module_id: ModuleId, consts: Vec<(String, IrType, ConstValue)>, errors: Vec<String>) -> Self {
        Self { module_id, consts, errors }
    }

    /// Convert to a HashMap for efficient lookup.
    pub fn to_hashmap(&self) -> HashMap<String, (IrType, ConstValue)> {
        self.consts
            .iter()
            .map(|(name, ty, val)| (name.clone(), (ty.clone(), val.clone())))
            .collect()
    }
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

    /// Lowering errors (IR generation only, not ownership analysis).
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

/// Create a ModuleGraphLoweringResult from pre-computed module results.
///
/// This is a tracked function that allows creating the result struct
/// from outside of the main `lower_module_graph` tracked function.
#[salsa::tracked]
pub fn create_module_graph_lowering_result<'db>(
    db: &'db dyn salsa::Database,
    results_vec: Vec<(ModuleId, SingleModuleLoweringResult<'db>)>,
    func_id_map: FuncIdMap<'db>,
    all_success: bool,
) -> ModuleGraphLoweringResult<'db> {
    let module_results: BTreeMap<_, _> = results_vec.into_iter().collect();
    ModuleGraphLoweringResult::new(db, module_results, func_id_map, all_success)
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
/// If `pre_resolved_consts` is provided, those values are used for module-level
/// const bindings. Otherwise, only simple literals are supported.
///
/// Requires pre-computed ownership analysis results. Functions with ownership analysis
/// errors are skipped (but those errors are already captured in the ownership
/// analysis result, not here).
#[salsa::tracked]
pub fn lower_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module,
    ir_module_id: IrModuleId,
    parsed: ParsedStatements<'db>,
    typecheck_result: SingleModuleTypecheckResult<'db>,
    ownership_analysis: SingleModuleAnalysis<'db>,
    func_id_map: FuncIdMap<'db>,
    pre_resolved_consts: Option<ModulePreResolvedConsts>,
) -> SingleModuleLoweringResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("lower", module_path, QueryPhase::Start);

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    // Convert FuncIdMap to HashMap for efficient lookup during lowering.
    let func_id_hashmap = func_id_map.to_hashmap(db);

    // Get pre-computed ownership analysis results.
    let function_analyses = ownership_analysis.function_analyses(db);

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

    let mut functions = Vec::new();
    let mut errors = Vec::new();
    let mut func_ids = Vec::new();

    // Get module-level const bindings.
    // If pre-resolved consts are provided (from CTFE evaluation), use those.
    // Otherwise, fall back to simple literal evaluation.
    let module_consts: HashMap<String, (IrType, ConstValue)> = if let Some(pre_resolved) = pre_resolved_consts {
        pre_resolved.to_hashmap()
    } else {
        // Fallback: evaluate simple literals only (no CTFE evaluator available).
        let mut consts = HashMap::new();
        for statement in &parsed.statements {
            if let Statement::Const(const_stmt) = statement {
                let name = const_stmt.name.text(db).S();
                let init_expr = const_stmt.value;

                // Get the type from the typechecker.
                let expr_id = init_expr.as_id();
                let index = expr_id.index() as usize;
                let ir_type = match expr_types.get(index).cloned().flatten() {
                    Some(ty) => IrType::from_tycheck(db, &ty),
                    None => {
                        errors.push(format!("Const '{}': missing type information", name));
                        continue;
                    }
                };

                // Evaluate the const expression (simple literals only).
                match lower::const_expr::eval_const_expr_simple(db, init_expr, &ir_type, &consts) {
                    Ok(value) => {
                        consts.insert(name, (ir_type, value));
                    }
                    Err(e) => {
                        errors.push(format!("Const '{}': {}", name, e));
                    }
                }
            }
        }
        consts
    };

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

    // Lower functions with module-level consts available.
    let module_consts_ref = if module_consts.is_empty() { None } else { Some(&module_consts) };
    let mut func_idx = 0;
    for statement in &parsed.statements {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();
            let func_id = func_ids[func_idx].1;
            func_idx += 1;

            // Get resolved param types for this function.
            let resolved_params = func_param_types.get(&func_name).map(|v| v.as_slice());

            // Get pre-computed ownership analysis for this function.
            let Some(single_analysis) = function_analyses.get(&func_name) else {
                errors.push(format!("Lowering error in {}: missing ownership analysis", func_name));
                continue;
            };

            // Skip functions that had ownership analysis errors.
            let Some(analysis) = single_analysis.analysis(db).clone() else {
                // Drop analysis errors are already captured separately.
                continue;
            };

            // Lower to IR with module-level consts.
            // Note: ctfe_evaluator is None because tracked functions can't take trait objects.
            // Function-level consts in modules only support simple literals.
            match lower::lower_function_for_module(
                db,
                expr_types,
                call_targets,
                &func_id_hashmap,
                *func,
                func_id,
                analysis,
                resolved_params,
                None, // ctfe_evaluator
                module_consts_ref,
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

/// Evaluate module-level and function-level const bindings using the CTFE evaluator.
///
/// Pre-evaluates all const bindings across all modules before lowering.
/// This includes both module-level consts and consts inside function bodies.
/// Returns a map of module_id -> pre-resolved consts.
///
/// This function accesses tracked struct fields but does NOT create tracked structs,
/// so it can be called outside of tracked function context.
pub fn evaluate_all_module_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> HashMap<ModuleId, ModulePreResolvedConsts> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let expr_types = single_typecheck.expr_types(db);

        let mut consts = Vec::new();
        let mut errors = Vec::new();
        let mut resolved_so_far: HashMap<String, (IrType, ConstValue)> = HashMap::new();

        // First pass: evaluate module-level consts.
        for statement in &parsed.statements {
            if let Statement::Const(const_stmt) = statement {
                match evaluate_single_const(
                    db, const_stmt, expr_types, &resolved_so_far, &evaluator
                ) {
                    Ok((name, ir_type, value)) => {
                        resolved_so_far.insert(name.clone(), (ir_type.clone(), value.clone()));
                        consts.push((name, ir_type, value));
                    }
                    Err(e) => errors.push(e),
                }
            }
        }

        // Second pass: evaluate function-level consts.
        // Function-level consts can reference module-level consts and other
        // consts within the same function.
        for statement in &parsed.statements {
            if let Statement::Fun(func_stmt) = statement {
                let func_name = func_stmt.name(db).text(db);
                // Track local consts for this function so later consts can reference earlier ones.
                let mut func_local_consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();

                for func_body_stmt in func_stmt.body(db).iter() {
                    if let Statement::Const(const_stmt) = func_body_stmt {
                        // Merge function-local consts with module-level for lookup.
                        let mut lookup_map = resolved_so_far.clone();
                        for (name, (ty, val)) in &func_local_consts {
                            lookup_map.insert(name.clone(), (ty.clone(), val.clone()));
                        }

                        match evaluate_single_const(
                            db, const_stmt, expr_types, &lookup_map, &evaluator
                        ) {
                            Ok((name, ir_type, value)) => {
                                // Store locally for other consts in this function.
                                func_local_consts.insert(name.clone(), (ir_type.clone(), value.clone()));

                                // Use qualified name for storage: func_name::const_name
                                let qualified_name = format!("{}::{}", func_name, name);
                                consts.push((qualified_name, ir_type, value));
                            }
                            Err(e) => errors.push(format!("{}::{}", func_name, e)),
                        }
                    }
                }
            }
        }

        if !consts.is_empty() || !errors.is_empty() {
            result.insert(*module_id, ModulePreResolvedConsts::with_errors(*module_id, consts, errors));
        }
    }

    result
}

/// Evaluate a single const statement.
///
/// Returns the evaluated const or an error message describing what went wrong.
fn evaluate_single_const<'db>(
    db: &'db dyn salsa::Database,
    const_stmt: &datalove_datafun_ast::ast::StmtConst<'db>,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    resolved_so_far: &HashMap<String, (IrType, ConstValue)>,
    evaluator: &Rc<RefCell<dyn CtfeEvaluator>>,
) -> Result<(String, IrType, ConstValue), String> {
    let name = const_stmt.name.text(db).S();
    let init_expr = const_stmt.value;

    // Get the type from the typechecker.
    let expr_id = init_expr.as_id();
    let index = expr_id.index() as usize;
    let ir_type = match expr_types.get(index).cloned().flatten() {
        Some(ty) => IrType::from_tycheck(db, &ty),
        None => return Err(format!("const '{}': missing type information", name)),
    };

    // Try simple evaluation first.
    let value = match lower::const_expr::eval_const_expr_simple(db, init_expr, &ir_type, resolved_so_far) {
        Ok(v) => v,
        Err(simple_err) => {
            // Fall back to CTFE evaluator for complex expressions.
            match lower::const_expr::eval_const_expr_with_evaluator(
                db,
                init_expr,
                &ir_type,
                expr_types,
                resolved_so_far,
                evaluator.clone(),
            ) {
                Ok(v) => v,
                Err(ctfe_err) => {
                    return Err(format!("const '{}': {}", name, ctfe_err));
                }
            }
        }
    };

    Ok((name, ir_type, value))
}

/// Lower module graph with CTFE evaluator for complex const expressions.
///
/// This is the entry point that supports full const expression evaluation.
/// It pre-evaluates all module consts using the CTFE evaluator, then lowers
/// modules (optionally in parallel) with the pre-resolved const values.
///
/// CTFE errors are collected and included in the lowering result, causing
/// the overall lowering to fail.
pub fn lower_module_graph_with_evaluator<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    mode: ParallelMode,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
) -> ModuleGraphLoweringResult<'db> {
    let db_salsa = db.as_salsa_db();

    // Phase 1: Pre-evaluate all module consts using the CTFE evaluator.
    let pre_resolved_consts = evaluate_all_module_consts(db_salsa, parsed_graph, typecheck_result, evaluator);

    // Collect CTFE errors from all modules.
    let ctfe_errors: Vec<(ModuleId, Vec<String>)> = pre_resolved_consts.iter()
        .filter(|(_, v)| !v.errors.is_empty())
        .map(|(k, v)| (*k, v.errors.clone()))
        .collect();

    // Phase 2: Lower modules with pre-resolved consts.
    let mut result = match mode {
        ParallelMode::Sequential => {
            lower_module_graph_with_pre_resolved(db_salsa, parsed_graph, typecheck_result, ownership_analysis, &pre_resolved_consts)
        }
        ParallelMode::Parallel => {
            lower_module_graph_parallel_with_pre_resolved(db, parsed_graph, typecheck_result, ownership_analysis, &pre_resolved_consts)
        }
    };

    // If there were CTFE errors, create a new result with those errors included.
    if !ctfe_errors.is_empty() {
        let mut module_results = result.module_results(db_salsa).clone();

        for (module_id, errors) in ctfe_errors {
            if let Some(existing) = module_results.get(&module_id) {
                // Prepend CTFE errors to existing module errors.
                let mut all_errors = errors;
                all_errors.extend(existing.errors(db_salsa).iter().cloned());

                // Create updated result with CTFE errors.
                let updated = SingleModuleLoweringResult::new(
                    db_salsa,
                    module_id,
                    existing.ir_module_id(db_salsa),
                    existing.functions(db_salsa).clone(),
                    all_errors,
                    existing.func_ids(db_salsa).clone(),
                );
                module_results.insert(module_id, updated);
            }
        }

        result = ModuleGraphLoweringResult::new(
            db_salsa,
            module_results,
            result.func_id_map(db_salsa),
            false, // all_success = false due to CTFE errors
        );
    }

    result
}

/// Lower module graph sequentially with pre-resolved const bindings.
fn lower_module_graph_with_pre_resolved<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    pre_resolved_consts: &HashMap<ModuleId, ModulePreResolvedConsts>,
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

    // Get per-module ownership analysis results.
    let ownership_analysis_results = ownership_analysis.module_results(db);

    // Lower each module with its pre-resolved consts.
    let mut results_vec = Vec::new();
    let mut all_success = true;

    for (ir_module_idx, (module_id, parsed)) in parsed_graph.statements_only(db).iter().enumerate() {
        let ir_module_id = IrModuleId(ir_module_idx as u32);

        // Skip modules with typecheck errors.
        if typecheck_errors.get(module_id).map_or(false, |e| !e.is_empty()) {
            continue;
        }

        let module = *module_map.get(module_id).expect("module should exist");

        // Get the tracked per-module typecheck result.
        let single_typecheck = *typecheck_module_results.get(module_id)
            .expect("module should have typecheck result");

        // Get the tracked per-module ownership analysis result.
        let single_ownership_analysis = *ownership_analysis_results.get(module_id)
            .expect("module should have ownership analysis result");

        // Get pre-resolved consts for this module.
        let pre_resolved = pre_resolved_consts.get(module_id).cloned();

        let result = lower_module(
            db,
            module,
            ir_module_id,
            parsed.clone(),
            single_typecheck,
            single_ownership_analysis,
            func_id_map,
            pre_resolved,
        );

        if !result.errors(db).is_empty() {
            all_success = false;
        }

        results_vec.push((*module_id, result));
    }

    // Use tracked wrapper to create the result (requires tracked function context).
    create_module_graph_lowering_result(db, results_vec, func_id_map, all_success)
}

/// Lower module graph in parallel with pre-resolved const bindings.
///
/// Warms salsa's memoization cache by lowering modules in parallel,
/// then delegates to the tracked function for final aggregation.
fn lower_module_graph_parallel_with_pre_resolved<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    pre_resolved_consts: &HashMap<ModuleId, ModulePreResolvedConsts>,
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
    let ownership_analysis_results = ownership_analysis.module_results(db_salsa);

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

            // Get the tracked per-module ownership analysis result.
            let single_ownership_analysis = *ownership_analysis_results.get(module_id)?;

            // Get pre-resolved consts for this module.
            let pre_resolved = pre_resolved_consts.get(module_id).cloned();

            Some((
                db.dyn_clone(),
                module,
                IrModuleId(ir_module_idx as u32),
                parsed.clone(),
                single_typecheck,
                single_ownership_analysis,
                pre_resolved,
            ))
        })
        .collect();

    // Lower modules in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module, ir_module_id, parsed, single_typecheck, single_ownership_analysis, pre_resolved)| {
        let db_s = db_clone.as_salsa_db();

        // This populates the cache.
        let _ = lower_module(
            db_s,
            module,
            ir_module_id,
            parsed,
            single_typecheck,
            single_ownership_analysis,
            func_id_map,
            pre_resolved,
        );
    });

    // Delegate to sequential function which aggregates results.
    // All lower_module calls will be cache hits from the parallel phase.
    lower_module_graph_with_pre_resolved(db_salsa, parsed_graph, typecheck_result, ownership_analysis, pre_resolved_consts)
}
