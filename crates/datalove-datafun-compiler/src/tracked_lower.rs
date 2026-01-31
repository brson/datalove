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
use std::sync::Arc;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, IrFunction, IrType, FuncId, IrModuleId, ModuleFunctionRegistry};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    SingleModuleTypecheckResult,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use datalove_datafun_const::{inline_module_functions, PreparedConst, evaluate_prepared_const};
use crate::IrTypeExt;
use crate::lower;
use crate::specialize::{build_specialization_plan, transform_function, rewrite_comptime_calls};
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
/// If `pre_resolved_consts` is provided, those values are used for function-level
/// const inlining (when `skip_const_inlining` is false).
///
/// If `lowered_functions` is provided, those functions are reused instead of
/// being re-lowered from the AST.
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
    skip_const_inlining: bool,
    lowered_functions: Option<ModuleLoweredFunctions>,
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

    // Build maps of function name -> resolved types from exports.
    // This is needed to resolve type aliases in function parameters and return types.
    let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
    let mut func_return_types: HashMap<String, IrType> = HashMap::new();
    for (name, func_type) in typecheck_result.exports(db) {
        let param_types: Vec<IrType> = func_type.param_types(db)
            .iter()
            .map(|ty| IrType::from_tycheck(db, ty))
            .collect();
        func_param_types.insert(name.text(db).S(), param_types);

        let return_type = IrType::from_tycheck(db, &func_type.return_type(db));
        func_return_types.insert(name.text(db).S(), return_type);
    }

    let mut functions;
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

    // If lowered functions are provided, use them directly.
    if let Some(ref lf) = lowered_functions {
        functions = lf.functions.clone();
        // func_ids was already computed above, and should match lf.func_name_to_id.
    } else {
        // Lower functions from scratch.
        functions = Vec::new();
        let mut func_idx = 0;
        for statement in &parsed.statements {
            if let Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).S();
                let func_id = func_ids[func_idx].1;
                func_idx += 1;

                // Get resolved types for this function.
                let resolved_params = func_param_types.get(&func_name).map(|v| v.as_slice());
                let resolved_return = func_return_types.get(&func_name).cloned();

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
                // Function-level consts are pre-evaluated via evaluate_all_module_consts() when
                // using lower_module_graph_with_evaluator(), supporting full CTFE expressions.
                match lower::lower_function_for_module(
                    db,
                    expr_types,
                    call_targets,
                    &func_id_hashmap,
                    *func,
                    func_id,
                    analysis,
                    resolved_params,
                    resolved_return,
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
    }

    // Inline evaluated const values into the lowered functions.
    // Skip inlining in skip_const_inlining test mode (when no pre_resolved_consts provided
    // and skip_const_inlining flag is true, we want consts evaluated at runtime).
    if !skip_const_inlining {
        if let Some(ref pre_resolved) = pre_resolved_consts {
            // Include any CTFE errors from const evaluation.
            errors.extend(pre_resolved.errors.iter().cloned());

            // Build const values map from pre-resolved consts.
            // Pre-resolved consts have qualified names like "func_name::const_name".
            let const_values: HashMap<String, ConstValue> = pre_resolved.consts
                .iter()
                .map(|(name, _ir_type, value)| (name.clone(), value.clone()))
                .collect();
            inline_module_functions(&mut functions, &const_values);
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

/// Lowered functions for a single module.
///
/// Functions are lowered once, then reused for const evaluation
/// and final module assembly.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ModuleLoweredFunctions {
    /// The lowered IR functions for this module.
    pub functions: Vec<IrFunction>,
    /// Map from function name to FuncId.
    /// Sorted vector for deterministic hashing.
    pub func_name_to_id: Vec<(String, FuncId)>,
}

/// Build a module function registry from lowered functions.
///
/// This creates a temporary registry for CTFE to use when evaluating
/// const expressions that call functions from other modules.
fn build_module_registry_from_lowered(
    lowered_functions: &HashMap<ModuleId, ModuleLoweredFunctions>,
    func_id_map: &HashMap<(ModuleId, String), (IrModuleId, FuncId)>,
) -> Arc<ModuleFunctionRegistry> {
    let mut registry = ModuleFunctionRegistry::new();

    // Iterate over the func_id_map to get the IrModuleId for each function.
    for ((module_id, _func_name), (ir_module_id, func_id)) in func_id_map {
        // Find the corresponding lowered function.
        if let Some(lowered) = lowered_functions.get(module_id) {
            if let Some(func) = lowered.functions.iter().find(|f| f.id == *func_id) {
                registry.add_module_function(*ir_module_id, *func_id, func.clone());
            }
        }
    }

    Arc::new(registry)
}

/// Lower all module functions.
///
/// This lowers all functions across all modules. The lowered functions are
/// reused for const evaluation and final module assembly.
///
/// Returns a map of module_id -> lowered functions.
pub fn lower_all_module_functions<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    func_id_map: FuncIdMap<'db>,
) -> HashMap<ModuleId, ModuleLoweredFunctions> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let ownership_analysis_results = ownership_analysis.module_results(db);
    let func_id_hashmap = func_id_map.to_hashmap(db);

    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let Some(single_ownership) = ownership_analysis_results.get(module_id) else {
            continue;
        };

        let expr_types = single_typecheck.expr_types(db);
        let call_targets = single_typecheck.call_targets(db);
        let function_analyses = single_ownership.function_analyses(db);

        // Build maps of function name -> resolved types from exports.
        let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
        let mut func_return_types: HashMap<String, IrType> = HashMap::new();
        for (name, func_type) in single_typecheck.exports(db) {
            let param_types: Vec<IrType> = func_type.param_types(db)
                .iter()
                .map(|ty| IrType::from_tycheck(db, ty))
                .collect();
            func_param_types.insert(name.text(db).S(), param_types);
            let return_type = IrType::from_tycheck(db, &func_type.return_type(db));
            func_return_types.insert(name.text(db).S(), return_type);
        }

        let mut functions = Vec::new();
        let mut func_name_to_id = Vec::new();

        // Assign FuncIds and lower each function.
        let mut next_func_id: u32 = 0;
        for statement in &parsed.statements {
            if let Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).S();
                let func_id = FuncId(next_func_id);
                next_func_id += 1;

                func_name_to_id.push((func_name.clone(), func_id));

                // Get resolved param and return types for this function.
                let resolved_params = func_param_types.get(&func_name).map(|v| v.as_slice());
                let resolved_return = func_return_types.get(&func_name).cloned();

                // Get pre-computed ownership analysis for this function.
                let Some(single_analysis) = function_analyses.get(&func_name) else {
                    continue;
                };

                // Skip functions that had ownership analysis errors.
                let Some(analysis) = single_analysis.analysis(db).clone() else {
                    continue;
                };

                match lower::lower_function_for_module(
                    db,
                    expr_types,
                    call_targets,
                    &func_id_hashmap,
                    *func,
                    func_id,
                    analysis,
                    resolved_params,
                    resolved_return,
                ) {
                    Ok(ir_func) => {
                        functions.push(ir_func);
                    }
                    Err(_) => {
                        // Errors will be reported during the main lowering phase.
                        continue;
                    }
                }
            }
        }

        if !functions.is_empty() {
            result.insert(*module_id, ModuleLoweredFunctions {
                functions,
                func_name_to_id,
            });
        }
    }

    result
}

/// Evaluate module-level and function-level const bindings using the CTFE evaluator.
///
/// Evaluates all const bindings across all modules. This includes both
/// module-level consts and consts inside function bodies.
///
/// Returns a map of module_id -> resolved consts.
///
/// The `lowered_functions` are used when const expressions call functions,
/// avoiding re-lowering.
///
/// This function accesses tracked struct fields but does NOT create tracked structs,
/// so it can be called outside of tracked function context.
///
/// The `func_id_map` enables cross-module function calls in const expressions.
pub fn evaluate_all_module_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    lowered_functions: &HashMap<ModuleId, ModuleLoweredFunctions>,
    func_id_map: FuncIdMap<'db>,
) -> HashMap<ModuleId, ModulePreResolvedConsts> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let expr_types = single_typecheck.expr_types(db);
        let call_targets = single_typecheck.call_targets(db);

        // Get lowered functions for this module (if any).
        let (funcs, func_map): (&[IrFunction], HashMap<String, FuncId>) = match lowered_functions.get(module_id) {
            Some(lf) => (lf.functions.as_slice(), lf.func_name_to_id.iter().cloned().collect()),
            None => (&[], HashMap::new()),
        };

        let mut consts = Vec::new();
        let mut errors = Vec::new();

        // Evaluate function-level consts.
        // Module-level consts are not allowed (rejected by typechecker).
        for statement in &parsed.statements {
            if let Statement::Fun(func_stmt) = statement {
                let func_name = func_stmt.name(db).text(db);
                // Get function's return type for try operators in const expressions.
                let func_return_type = func_stmt.return_type(db)
                    .map(|ty| IrType::from_type_hint(db, &ty));
                // Track local consts for this function so later consts can reference earlier ones.
                let mut func_local_consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();

                for func_body_stmt in func_stmt.body(db).iter() {
                    if let Statement::Const(const_stmt) = func_body_stmt {
                        match evaluate_single_const(
                            db, const_stmt, expr_types, call_targets, &func_local_consts, &evaluator,
                            funcs, &func_map, func_return_type.clone(), func_id_map,
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
///
/// The `lowered_functions` are used when const expressions call functions.
/// The `func_return_type` is needed for try operators (`?` and `!`) in const expressions.
/// The `func_id_map` enables cross-module function calls in const expressions.
fn evaluate_single_const<'db>(
    db: &'db dyn salsa::Database,
    const_stmt: &datalove_datafun_ast::ast::StmtConst<'db>,
    expr_types: &'db [Option<datalove_datafun_tycheck::Type<'db>>],
    call_targets: &'db [Option<datalove_datafun_tycheck::ResolvedCallTarget<'db>>],
    resolved_so_far: &HashMap<String, (IrType, ConstValue)>,
    evaluator: &Rc<RefCell<dyn CtfeEvaluator>>,
    lowered_functions: &[IrFunction],
    func_name_to_id: &HashMap<String, FuncId>,
    func_return_type: Option<IrType>,
    func_id_map: FuncIdMap<'db>,
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

    // Lower the const binding using the "lower then evaluate" pattern.
    // Convert the tracked FuncIdMap to a HashMap for lowering.
    let func_id_map_hashmap = func_id_map.to_hashmap(db);
    let (unit_opt, value_opt) = lower::lower_const_binding(
        db,
        init_expr,
        &ir_type,
        expr_types,
        call_targets,
        resolved_so_far,
        func_return_type,
        lowered_functions,
        func_name_to_id,
        Some(&func_id_map_hashmap),
    ).map_err(|e| format!("const '{}': lowering error: {}", name, e))?;

    // Evaluate to get the const value.
    let value = match (unit_opt, value_opt) {
        (None, Some(v)) => v,
        (Some(unit), None) => {
            let prepared = PreparedConst::Unit(unit);
            evaluate_prepared_const(&prepared, &ir_type, evaluator)
                .map_err(|e| format!("const '{}': CTFE error: {}", name, e))?
        }
        _ => unreachable!("lower_const_binding returns exactly one of unit or value"),
    };

    Ok((name, ir_type, value))
}

/// Lower module graph with CTFE evaluator for complex const expressions.
///
/// This is the entry point that supports full const expression evaluation.
///
/// Pipeline phases:
/// 1. Lower functions (5a)
/// 2. Const evaluation (5b) - uses lowered functions for CTFE calls
/// 3. Comptime specialization (5c) - transforms functions with const params
/// 4. Module assembly (5d) - reuses lowered functions
/// 5. Const inlining - replaces const bindings with evaluated values
///
/// CTFE errors are collected and included in the lowering result, causing
/// the overall lowering to fail.
///
/// # Options
///
/// - `skip_const_inlining`: Skip const inlining phase (for testing)
/// - `skip_specialization`: Skip comptime specialization (for differential testing)
pub fn lower_module_graph_with_evaluator<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    mode: ParallelMode,
    evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    skip_const_inlining: bool,
    skip_specialization: bool,
) -> ModuleGraphLoweringResult<'db> {
    let db_salsa = db.as_salsa_db();

    // Compute func_id_map first (needed for lowering).
    let func_id_map = compute_func_id_map(db_salsa, parsed_graph);

    // Phase 5a: Always lower all module functions first.
    let lowered_functions = lower_all_module_functions(
        db_salsa, parsed_graph, typecheck_result, ownership_analysis, func_id_map
    );

    // Phase 5b: Evaluate consts (skip if skip_const_inlining is enabled).
    let resolved_consts = if skip_const_inlining {
        HashMap::new()
    } else {
        // Build a module registry from lowered functions for cross-module CTFE calls.
        let func_id_hashmap = func_id_map.to_hashmap(db_salsa);
        let module_registry = build_module_registry_from_lowered(&lowered_functions, &func_id_hashmap);
        evaluator.borrow_mut().set_module_registry(module_registry);

        evaluate_all_module_consts(db_salsa, parsed_graph, typecheck_result, evaluator, &lowered_functions, func_id_map)
    };

    // Phase 5c: Specialize comptime functions (union-branch transformation).
    // This transforms functions with const parameters and rewrites call sites.
    let lowered_functions = if skip_specialization {
        lowered_functions
    } else {
        specialize_comptime_functions(
            db_salsa,
            typecheck_result,
            &resolved_consts,
            lowered_functions,
        )
    };

    // Phase 5d: Assemble modules with lowered functions, then inline consts.
    // CTFE errors from resolved_consts are included in lower_module via pre_resolved_consts.errors.
    match mode {
        ParallelMode::Sequential => {
            assemble_module_graph(db_salsa, parsed_graph, typecheck_result, ownership_analysis, &resolved_consts, &lowered_functions, skip_const_inlining)
        }
        ParallelMode::Parallel => {
            assemble_module_graph_parallel(db, parsed_graph, typecheck_result, ownership_analysis, &resolved_consts, &lowered_functions, skip_const_inlining)
        }
    }
}

/// Specialize functions with comptime parameters using union-branch transformation.
///
/// This performs two transformations:
/// 1. Callee transformation: Functions with comptime params get transformed to
///    dispatch on a discriminant (union-branch form).
/// 2. Call site transformation: ComptimeCall instructions get transformed to
///    Const(discriminant) + Call with modified args.
fn specialize_comptime_functions<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    resolved_consts: &HashMap<ModuleId, ModulePreResolvedConsts>,
    mut lowered_functions: HashMap<ModuleId, ModuleLoweredFunctions>,
) -> HashMap<ModuleId, ModuleLoweredFunctions> {
    // Get the combined comptime registry.
    let registry = typecheck_result.comptime_registry(db);
    if registry.is_empty() {
        return lowered_functions;
    }

    // Build a flattened map of const names to values for lookup.
    // Include both qualified names (func_name::const_name) and unqualified names.
    let mut const_values_map: HashMap<String, (IrType, ConstValue)> = HashMap::new();
    for pre_resolved in resolved_consts.values() {
        for (qualified_name, ir_type, value) in &pre_resolved.consts {
            const_values_map.insert(qualified_name.clone(), (ir_type.clone(), value.clone()));

            // Also add unqualified name for direct lookup.
            if let Some(unqualified) = qualified_name.rsplit("::").next() {
                if unqualified != qualified_name {
                    // Only add if it's actually qualified.
                    const_values_map.entry(unqualified.to_string())
                        .or_insert_with(|| (ir_type.clone(), value.clone()));
                }
            }
        }
    }

    // Build the specialization plan.
    let spec_result = build_specialization_plan(db, &registry, &const_values_map);
    if spec_result.is_empty() {
        return lowered_functions;
    }

    // Phase 1: Transform callee functions (functions with comptime params).
    for (_module_id, module_funcs) in lowered_functions.iter_mut() {
        let mut new_functions = Vec::new();

        for func in &module_funcs.functions {
            let func_name = &func.name;
            if let Some(spec) = spec_result.specialized_funcs.get(func_name) {
                // Transform this function to union-branch form.
                let transformed = transform_function(func, spec);
                new_functions.push(transformed);
            } else {
                new_functions.push(func.clone());
            }
        }

        module_funcs.functions = new_functions;
    }

    // Build ModuleId -> IrModuleId mapping from the graph.
    let graph = typecheck_result.graph(db);
    let module_id_to_ir: HashMap<ModuleId, IrModuleId> = graph.iter_modules(db)
        .enumerate()
        .map(|(idx, module)| (module.id(db), IrModuleId(idx as u32)))
        .collect();

    // Build a global (IrModuleId, FuncId) -> name map for resolving FuncRef during call rewriting.
    // This is module-aware because FuncId is only unique within a module.
    let mut func_id_to_name: HashMap<(IrModuleId, FuncId), String> = HashMap::new();
    for (module_id, module_funcs) in lowered_functions.iter() {
        if let Some(&ir_module_id) = module_id_to_ir.get(module_id) {
            for (name, func_id) in &module_funcs.func_name_to_id {
                func_id_to_name.insert((ir_module_id, *func_id), name.clone());
            }
        }
    }

    // Phase 2: Rewrite call sites (ComptimeCall -> Const + Call).
    for (module_id, module_funcs) in lowered_functions.iter_mut() {
        // Get the IrModuleId for this module.
        let Some(&ir_module_id) = module_id_to_ir.get(module_id) else {
            continue;
        };

        let mut new_functions = Vec::new();

        for func in &module_funcs.functions {
            // Track value allocation for new Const instructions.
            let mut value_types = func.value_types.clone();
            let mut next_value = func.value_count;

            let transformed = rewrite_comptime_calls(func, &spec_result, &func_id_to_name, ir_module_id, &mut value_types, &mut next_value);
            new_functions.push(transformed);
        }

        module_funcs.functions = new_functions;
    }

    lowered_functions
}

/// Assemble module graph sequentially.
///
/// Combines lowered functions with module-level code, then applies const inlining.
fn assemble_module_graph<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    resolved_consts: &HashMap<ModuleId, ModulePreResolvedConsts>,
    lowered_functions: &HashMap<ModuleId, ModuleLoweredFunctions>,
    skip_const_inlining: bool,
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

        // Get resolved consts for this module.
        let module_consts = resolved_consts.get(module_id).cloned();

        // Get lowered functions for this module.
        let module_funcs = lowered_functions.get(module_id).cloned();

        let result = lower_module(
            db,
            module,
            ir_module_id,
            parsed.clone(),
            single_typecheck,
            single_ownership_analysis,
            func_id_map,
            module_consts,
            skip_const_inlining,
            module_funcs,
        );

        if !result.errors(db).is_empty() {
            all_success = false;
        }

        results_vec.push((*module_id, result));
    }

    // Use tracked wrapper to create the result (requires tracked function context).
    create_module_graph_lowering_result(db, results_vec, func_id_map, all_success)
}

/// Assemble module graph in parallel.
///
/// Warms salsa's memoization cache by assembling modules in parallel,
/// then delegates to the tracked function for final aggregation.
fn assemble_module_graph_parallel<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    resolved_consts: &HashMap<ModuleId, ModulePreResolvedConsts>,
    lowered_functions: &HashMap<ModuleId, ModuleLoweredFunctions>,
    skip_const_inlining: bool,
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

            // Get resolved consts for this module.
            let module_consts = resolved_consts.get(module_id).cloned();

            // Get lowered functions for this module.
            let module_funcs = lowered_functions.get(module_id).cloned();

            Some((
                db.dyn_clone(),
                module,
                IrModuleId(ir_module_idx as u32),
                parsed.clone(),
                single_typecheck,
                single_ownership_analysis,
                module_consts,
                skip_const_inlining,
                module_funcs,
            ))
        })
        .collect();

    // Assemble modules in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module, ir_module_id, parsed, single_typecheck, single_ownership_analysis, module_consts, skip_const_inlining, module_funcs)| {
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
            module_consts,
            skip_const_inlining,
            module_funcs,
        );
    });

    // Delegate to sequential function which aggregates results.
    // All lower_module calls will be cache hits from the parallel phase.
    assemble_module_graph(db_salsa, parsed_graph, typecheck_result, ownership_analysis, resolved_consts, lowered_functions, skip_const_inlining)
}
