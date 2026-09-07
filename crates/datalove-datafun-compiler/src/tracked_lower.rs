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
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use datalove_datafun_ir::{CodeUnitId, ConstValue, CtfeEvaluator, IrCodeUnit, IrType, FuncId, IrModuleId, ModuleFunctionRegistry};
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
/// This tracked struct wraps the mapping from (ModuleId<'db>, func_name) to
/// (IrModuleId, FuncId). Being a tracked struct makes it hashable and
/// usable as a parameter to other tracked functions.
#[salsa::tracked]
pub struct FuncIdMap<'db> {
    /// Mapping entries as a sorted vector for deterministic hashing.
    #[returns(ref)]
    pub entries: Vec<((ModuleId<'db>, String), (IrModuleId, FuncId))>,
}

impl<'db> FuncIdMap<'db> {
    /// Look up a function's IR location by module and name.
    pub fn get(&self, db: &'db dyn salsa::Database, module_id: ModuleId<'db>, func_name: &str) -> Option<(IrModuleId, FuncId)> {
        self.entries(db)
            .iter()
            .find(|((mid, name), _)| *mid == module_id && name == func_name)
            .map(|(_, loc)| *loc)
    }

    /// Convert to a HashMap for efficient repeated lookups.
    pub fn to_hashmap(&self, db: &'db dyn salsa::Database) -> HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)> {
        self.entries(db).iter().cloned().collect()
    }
}

/// The entries of `func_id_map` that lowering `module_id` can reach.
///
/// The full map names every function in the world, so passing it whole makes
/// every module's lowering depend on every other module: add one module
/// anywhere and they all lower again. A module can only call into itself, the
/// modules it transitively requires, and the riders, so it is given those
/// entries and nothing else. Adding an unrelated module then leaves this
/// argument equal and the lowering stays cached.
fn reachable_func_ids<'db>(
    db: &'db dyn salsa::Database,
    graph: bct::module_graph::ModuleGraph<'db>,
    func_id_map: FuncIdMap<'db>,
    module_id: ModuleId<'db>,
) -> Vec<((ModuleId<'db>, String), (IrModuleId, FuncId))> {
    let dependencies = graph.dependencies(db);

    // Transitive requires of this module.
    let mut reachable: std::collections::BTreeSet<ModuleId<'db>> = std::collections::BTreeSet::new();
    let mut queue = vec![module_id];
    while let Some(current) = queue.pop() {
        if !reachable.insert(current) {
            continue;
        }
        if let Some(deps) = dependencies.get(&current) {
            queue.extend(deps.iter().copied());
        }
    }

    // Riders live under synthetic module ids that are not in the graph, so
    // they never appear in `dependencies`; keep every entry the graph does
    // not account for rather than work out which rider belongs to whom.
    let in_graph: std::collections::BTreeSet<ModuleId<'db>> =
        graph.iter_modules(db).map(|m| m.id(db)).collect();

    func_id_map.entries(db)
        .iter()
        .filter(|((mid, _), _)| reachable.contains(mid) || !in_graph.contains(mid))
        .cloned()
        .collect()
}

/// Compute the function ID map from a parsed module graph.
///
/// This assigns module-local FuncIds (0, 1, 2, ...) to each function in each
/// module. The result is cached based on the parsed graph structure.
#[salsa::tracked(returns(copy))]
pub fn compute_func_id_map<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
) -> FuncIdMap<'db> {
    let mut entries = Vec::new();
    let regular_module_count = parsed_graph.statements_only(db).len();

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

    // Add rider functions. Each unique rider alias gets its own IrModuleId.
    let mut rider_module_idx = regular_module_count;
    let mut seen_riders: std::collections::BTreeMap<String, IrModuleId> = std::collections::BTreeMap::new();

    for (_module_id, riders) in parsed_graph.resolved_riders(db).iter() {
        for (alias, rider) in riders {
            let rider_path = format!("@rider/{}", alias.text(db));
            let rider_ir_module_id = *seen_riders.entry(rider_path.clone()).or_insert_with(|| {
                let id = IrModuleId(rider_module_idx as u32);
                rider_module_idx += 1;
                id
            });

            let synthetic_module_id = rider.module_id;
            for (func_idx, (func_name, _func_type)) in rider.functions.iter().enumerate() {
                let name = func_name.text(db).S();
                let key = (synthetic_module_id, name);
                // Avoid duplicates if the same rider is required by multiple modules.
                if !entries.iter().any(|(k, _)| *k == key) {
                    entries.push((key, (rider_ir_module_id, FuncId(func_idx as u32))));
                }
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
#[derive(salsa::SalsaValue)]
pub struct ModulePreResolvedConsts<'db> {
    /// Module these consts belong to.
    pub module_id: ModuleId<'db>,

    /// Evaluated const bindings: name -> (type, value).
    pub consts: Vec<(String, IrType, ConstValue)>,

    /// Errors encountered during const evaluation.
    pub errors: Vec<String>,
}

impl<'db> ModulePreResolvedConsts<'db> {
    /// Create a new pre-resolved consts container.
    pub fn new(module_id: ModuleId<'db>, consts: Vec<(String, IrType, ConstValue)>) -> Self {
        Self { module_id, consts, errors: Vec::new() }
    }

    /// Create with errors.
    pub fn with_errors(module_id: ModuleId<'db>, consts: Vec<(String, IrType, ConstValue)>, errors: Vec<String>) -> Self {
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
    #[returns(copy)]
    pub module_id: ModuleId<'db>,

    /// IR module index (0-based position in graph).
    #[returns(copy)]
    pub ir_module_id: IrModuleId,

    /// Successfully lowered code units (functions).
    #[returns(ref)]
    pub functions: Vec<IrCodeUnit>,

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
    pub module_results: BTreeMap<ModuleId<'db>, SingleModuleLoweringResult<'db>>,

    /// Function ID map for cross-module calls.
    #[returns(copy)]
    pub func_id_map: FuncIdMap<'db>,

    /// Whether all modules lowered successfully.
    #[returns(copy)]
    pub success: bool,
}

/// Create a ModuleGraphLoweringResult from pre-computed module results.
///
/// This is a tracked function that allows creating the result struct
/// from outside of the main `lower_module_graph` tracked function.
#[salsa::tracked(returns(copy))]
pub fn create_module_graph_lowering_result<'db>(
    db: &'db dyn salsa::Database,
    results_vec: Vec<(ModuleId<'db>, SingleModuleLoweringResult<'db>)>,
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

    /// Get all successfully lowered code units.
    pub fn all_functions(&self, db: &'db dyn salsa::Database) -> Vec<(IrModuleId, FuncId, IrCodeUnit)> {
        self.module_results(db)
            .values()
            .flat_map(|r| {
                let ir_mod = r.ir_module_id(db);
                r.functions(db).iter().map(move |f| (ir_mod, FuncId(f.id.0), f.clone()))
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
#[salsa::tracked(returns(copy))]
pub fn lower_module<'db>(
    db: &'db dyn salsa::Database,
    module: Module<'db>,
    ir_module_id: IrModuleId,
    parsed: ParsedStatements<'db>,
    typecheck_result: SingleModuleTypecheckResult<'db>,
    ownership_analysis: SingleModuleAnalysis<'db>,
    func_ids: Vec<((ModuleId<'db>, String), (IrModuleId, FuncId))>,
    pre_resolved_consts: Option<ModulePreResolvedConsts<'db>>,
    skip_const_inlining: bool,
    lowered_functions: Option<ModuleLoweredFunctions>,
) -> SingleModuleLoweringResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("lower", module_path, QueryPhase::Start);

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    let func_id_hashmap: HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)> =
        func_ids.iter().cloned().collect();

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

    // Module-level consts, for the from-scratch path below. They are stored
    // under a bare name; a function-level one is qualified with its function.
    let module_level_consts: HashMap<String, (IrType, ConstValue)> = pre_resolved_consts
        .as_ref()
        .map(|c| c.consts.iter()
            .filter(|(name, _, _)| !name.contains("::"))
            .map(|(name, ty, value)| (name.clone(), (ty.clone(), value.clone())))
            .collect())
        .unwrap_or_default();

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
                    &module_level_consts,
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
            // Inline const values directly into the functions.
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
    /// The lowered IR code units (functions) for this module.
    pub functions: Vec<IrCodeUnit>,
    /// Map from function name to FuncId.
    /// Sorted vector for deterministic hashing.
    pub func_name_to_id: Vec<(String, FuncId)>,
}

/// Build a module function registry from lowered functions.
///
/// This creates a temporary registry for CTFE to use when evaluating
/// const expressions that call functions from other modules.
fn build_module_registry_from_lowered<'db>(
    lowered_functions: &HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
    func_id_map: &HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>,
) -> Arc<ModuleFunctionRegistry> {
    let mut registry = ModuleFunctionRegistry::new();

    // Iterate over the func_id_map to get the IrModuleId for each function.
    for ((module_id, _func_name), (ir_module_id, func_id)) in func_id_map {
        // Find the corresponding lowered function (code unit).
        if let Some(lowered) = lowered_functions.get(module_id) {
            if let Some(unit) = lowered.functions.iter().find(|f| f.id.0 == func_id.0) {
                registry.add_module_code_unit(*ir_module_id, CodeUnitId(func_id.0), unit.clone());
            }
        }
    }

    Arc::new(registry)
}

/// Give each module function the descriptor shapes its callees need of it.
///
/// Lowering knows what a function's own body builds; this adds what it carries
/// on a callee's behalf. See `close_shapes` for why the iteration settles and
/// what it refuses. Done before anything reads a signature, because the shapes
/// are part of one: they say what trailing arguments a call has to pass.
fn close_shapes_over_calls<'db>(
    db: &'db dyn salsa::Database,
    func_id_map: FuncIdMap<'db>,
    lowered_functions: &mut HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
    errors: &mut HashMap<ModuleId<'db>, Vec<String>>,
) {
    use datalove_datafun_ir::{CodeRef, DescriptorShape, Instruction};

    type Key = (IrModuleId, FuncId);

    // Where each lowered unit sits, so a `CodeRef` can find it and so the
    // answers can be written back.
    let mut placement: HashMap<Key, (ModuleId<'db>, usize)> = HashMap::new();
    for ((module_id, _), (ir_module_id, func_id)) in func_id_map.to_hashmap(db) {
        let Some(lowered) = lowered_functions.get(&module_id) else { continue };
        let Some(idx) = lowered.functions.iter().position(|f| f.id.0 == func_id.0) else {
            continue;
        };
        placement.insert((ir_module_id, func_id), (module_id, idx));
    }

    let mut calls: HashMap<Key, Vec<(Key, Vec<DescriptorShape>)>> = HashMap::new();
    let mut shapes: HashMap<Key, Vec<DescriptorShape>> = HashMap::new();
    for (key, (module_id, idx)) in &placement {
        let unit = &lowered_functions[module_id].functions[*idx];
        shapes.insert(
            *key,
            unit.function_context().map(|c| c.descriptor_shapes.clone()).unwrap_or_default(),
        );
        let mut sites = Vec::new();
        for block in &unit.blocks {
            for instr in &block.instructions {
                let Instruction::Call { func, type_args, .. } = instr else { continue };
                if type_args.is_empty() {
                    continue;
                }
                let callee = match func {
                    CodeRef::Module { module, id } => (*module, FuncId(id.0)),
                    // A local reference inside a module names that module's own.
                    CodeRef::Local(id) => (key.0, FuncId(id.0)),
                    CodeRef::External { .. } => continue,
                };
                sites.push((callee, type_args.clone()));
            }
        }
        calls.insert(*key, sites);
    }

    if let Err(unbounded) = datalove_datafun_ir::close_shapes(&calls, &mut shapes) {
        let module_id = placement.values().next().map(|(m, _)| *m);
        if let Some(module_id) = module_id {
            errors.entry(module_id).or_default().push(format!(
                "a descriptor would be needed for `{}`, a collection that grows \
                 without end: a generic builds a collection of its type parameter and \
                 calls a generic at a strictly larger type, so every call needs a \
                 description one level deeper than the last",
                unbounded,
            ));
        }
        return;
    }

    for (key, shape_set) in shapes {
        let Some((module_id, idx)) = placement.get(&key).copied() else { continue };
        let unit = &mut lowered_functions.get_mut(&module_id).unwrap().functions[idx];
        if let datalove_datafun_ir::CodeUnitContext::Function(ctx) = &mut unit.context {
            ctx.descriptor_shapes = shape_set;
        }
    }
}

/// Lower all module functions./// Lower all module functions.
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
    module_consts: &HashMap<ModuleId<'db>, HashMap<String, (IrType, ConstValue)>>,
    deferred: &mut HashMap<ModuleId<'db>, Vec<String>>,
    restrict: Option<&HashMap<ModuleId<'db>, Vec<String>>>,
) -> HashMap<ModuleId<'db>, ModuleLoweredFunctions> {
    let empty_consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();
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

                // On the second pass only the deferred functions are lowered.
                // Their FuncIds still come from statement position, so the
                // numbering matches whichever pass a function lands in.
                if let Some(restrict) = restrict {
                    let wanted = restrict.get(module_id)
                        .map(|names| names.iter().any(|n| n == &func_name))
                        .unwrap_or(false);
                    if !wanted {
                        continue;
                    }
                }

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
                    module_consts.get(module_id).unwrap_or(&empty_consts),
                ) {
                    Ok(ir_func) => {
                        functions.push(ir_func);
                    }
                    Err(lower::LowerError::BindingNotAvailable(_)) => {
                        // Names a module const that has not been evaluated yet.
                        // Recorded so the caller can lower it once it has.
                        deferred.entry(*module_id).or_default().push(func_name);
                        continue;
                    }
                    Err(_) => {
                        // Errors will be reported during the main lowering phase.
                        continue;
                    }
                }
            }
        }

        if !functions.is_empty() || !func_name_to_id.is_empty() {
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
    lowered_functions: &HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
    func_id_map: FuncIdMap<'db>,
    module_consts: &HashMap<ModuleId<'db>, HashMap<String, (IrType, ConstValue)>>,
) -> HashMap<ModuleId<'db>, ModulePreResolvedConsts<'db>> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let expr_types = single_typecheck.expr_types(db);
        let call_targets = single_typecheck.call_targets(db);

        // Get lowered functions for this module (if any).
        let (funcs, func_map): (&[IrCodeUnit], HashMap<String, FuncId>) = match lowered_functions.get(module_id) {
            Some(lf) => (lf.functions.as_slice(), lf.func_name_to_id.iter().cloned().collect()),
            None => (&[], HashMap::new()),
        };

        let mut consts = Vec::new();
        let mut errors = Vec::new();

        // Module-level consts were evaluated between the lowering strata. They
        // are recorded under a bare name and seeded into every function, so a
        // function-level const can name one.
        let module_level = module_consts.get(module_id).cloned().unwrap_or_default();
        for (name, (ir_type, value)) in &module_level {
            consts.push((name.clone(), ir_type.clone(), value.clone()));
        }

        // Evaluate function-level consts.
        for statement in &parsed.statements {
            if let Statement::Fun(func_stmt) = statement {
                let func_name = func_stmt.name(db).text(db);
                // Get function's return type for try operators in const expressions.
                let func_return_type = func_stmt.return_type(db)
                    .map(|ty| IrType::from_type_hint(db, &ty));
                // Track local consts for this function so later consts can reference earlier ones.
                let mut func_local_consts: HashMap<String, (IrType, ConstValue)> = module_level.clone();

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
/// The first function a const expression calls that is not lowered yet.
///
/// `Module` references reach the module's own functions, which is where a cycle
/// between a const and a function shows up. Local and external references are
/// resolved against units the caller already holds, so they cannot be missing.
fn first_uncallable_target(
    unit: &IrCodeUnit,
    lowered_functions: &[IrCodeUnit],
) -> Option<String> {
    use datalove_datafun_ir::{CodeRef, Instruction};

    for block in &unit.blocks {
        for instr in &block.instructions {
            let func = match instr {
                Instruction::Call { func, .. } | Instruction::ComptimeCall { func, .. } => func,
                _ => continue,
            };
            if let CodeRef::Module { id, .. } = func {
                if !lowered_functions.iter().any(|f| f.id.0 == id.0) {
                    return Some(format!("module function #{}", id.0));
                }
            }
        }
    }
    None
}

/// True if any module in the graph declares a const at module level.
fn module_graph_has_module_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
) -> bool {
    parsed_graph.statements_only(db)
        .iter()
        .any(|(_, parsed)| parsed.statements.iter().any(|s| matches!(s, Statement::Const(_))))
}

/// Evaluate the module-level consts of every module.
///
/// Runs between the two lowering strata, so it can call any function that does
/// not itself name a module const. Consts are evaluated in source order, which
/// is what lets one name another declared above it.
fn evaluate_module_level_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    evaluator: &Rc<RefCell<dyn CtfeEvaluator>>,
    lowered_functions: &HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
    func_id_map: FuncIdMap<'db>,
    errors_out: &mut HashMap<ModuleId<'db>, Vec<String>>,
) -> HashMap<ModuleId<'db>, HashMap<String, (IrType, ConstValue)>> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let expr_types = single_typecheck.expr_types(db);
        let call_targets = single_typecheck.call_targets(db);

        let (funcs, func_map): (&[IrCodeUnit], HashMap<String, FuncId>) = match lowered_functions.get(module_id) {
            Some(lf) => (lf.functions.as_slice(), lf.func_name_to_id.iter().cloned().collect()),
            None => (&[], HashMap::new()),
        };

        let mut consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();
        for statement in &parsed.statements {
            if let Statement::Const(const_stmt) = statement {
                // A module const is outside any function, so there is no return
                // type for an early-return operator to check against.
                match evaluate_single_const(
                    db, const_stmt, expr_types, call_targets, &consts, evaluator,
                    funcs, &func_map, None, func_id_map,
                ) {
                    Ok((name, ir_type, value)) => {
                        consts.insert(name, (ir_type, value));
                    }
                    Err(e) => errors_out.entry(*module_id).or_default().push(e),
                }
            }
        }

        if !consts.is_empty() {
            result.insert(*module_id, consts);
        }
    }

    result
}

fn evaluate_single_const<'db>(
    db: &'db dyn salsa::Database,
    const_stmt: &datalove_datafun_ast::ast::StmtConst<'db>,
    expr_types: &'db datalove_datafun_sema::ExprTypes<'db>,
    call_targets: &'db datalove_datafun_sema::CallTargets<'db>,
    resolved_so_far: &HashMap<String, (IrType, ConstValue)>,
    evaluator: &Rc<RefCell<dyn CtfeEvaluator>>,
    lowered_functions: &[IrCodeUnit],
    func_name_to_id: &HashMap<String, FuncId>,
    func_return_type: Option<IrType>,
    func_id_map: FuncIdMap<'db>,
) -> Result<(String, IrType, ConstValue), String> {
    let name = const_stmt.name.text(db).S();
    let init_expr = const_stmt.value;

    // Get the type from the typechecker.
    let ir_type = match expr_types.get(&datalove_datafun_ast::ast::ExprKey::of(db, init_expr)) {
        Some(ty) => IrType::from_tycheck(db, ty),
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
            // Every function this reaches has to be lowered already, or the
            // interpreter has nothing to call and panics looking for it. A
            // module const whose evaluation needs a function that is itself
            // waiting on a module const is a cycle, and this is where it shows.
            if let Some(missing) = first_uncallable_target(&unit, lowered_functions) {
                return Err(format!(
                    "const '{}': depends on a function that is not available yet, \
                     which means it and that function depend on each other: {}",
                    name, missing
                ));
            }
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
/// - `skip_specialization`: Skip const parameter specialization (for differential testing)
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

    // Phase 5a: lower module functions, in strata.
    //
    // A function that names a module-level const needs that const's value to
    // lower, and evaluating the const may call functions in the same module. So
    // the functions that name no module const go first, the module consts are
    // evaluated against those, and the rest follow. A const whose evaluation
    // needs a function from the second stratum is a cycle, and shows up as the
    // const failing to evaluate rather than as anything lowering wrongly.
    let no_consts_yet: HashMap<ModuleId<'db>, HashMap<String, (IrType, ConstValue)>> = HashMap::new();
    let mut deferred: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
    let mut lowered_functions = lower_all_module_functions(
        db_salsa, parsed_graph, typecheck_result, ownership_analysis, func_id_map,
        &no_consts_yet, &mut deferred, None,
    );

    // Phase 5a/b boundary is where every module function is lowered, so this is
    // where a function's descriptor shapes stop being only what its own body
    // builds. See `shape_closure`: a function handing its type parameter to one
    // that builds a collection of it has to be handed a descriptor too.
    //
    // Done before anything reads a signature, because the shapes are part of
    // one: they say what trailing arguments a call has to pass.
    let mut shape_errors: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
    close_shapes_over_calls(db_salsa, func_id_map, &mut lowered_functions, &mut shape_errors);

    // Phase 5a/b boundary: evaluate module-level consts against what is lowered    // Phase 5a/b boundary: evaluate module-level consts against what is lowered
    // so far. Done even when const inlining is skipped, since a module const is
    // resolved when its reference is lowered rather than by a later pass, so
    // there is nothing for that flag to skip.
    let mut module_const_errors: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
    let module_consts = if deferred.is_empty() && !module_graph_has_module_consts(db_salsa, parsed_graph) {
        HashMap::new()
    } else {
        let func_id_hashmap = func_id_map.to_hashmap(db_salsa);
        let module_registry = build_module_registry_from_lowered(&lowered_functions, &func_id_hashmap);
        evaluator.borrow_mut().set_module_registry(module_registry);
        evaluate_module_level_consts(
            db_salsa, parsed_graph, typecheck_result, &evaluator, &lowered_functions, func_id_map,
            &mut module_const_errors,
        )
    };

    // Second stratum: the functions that were waiting on those values.
    if !deferred.is_empty() {
        let mut still_deferred: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
        let second = lower_all_module_functions(
            db_salsa, parsed_graph, typecheck_result, ownership_analysis, func_id_map,
            &module_consts, &mut still_deferred, Some(&deferred),
        );
        for (module_id, names) in &still_deferred {
            for name in names {
                module_const_errors.entry(*module_id).or_default().push(format!(
                    "function '{}' names a module const that could not be evaluated",
                    name
                ));
            }
        }
        for (module_id, mut module_funcs) in second {
            match lowered_functions.get_mut(&module_id) {
                Some(existing) => {
                    existing.functions.append(&mut module_funcs.functions);
                    // Keep the IR in the order the ids were assigned, so the
                    // stratum a function landed in does not show in the output.
                    existing.functions.sort_by_key(|f| f.id.0);
                }
                None => {
                    lowered_functions.insert(module_id, module_funcs);
                }
            }
        }
    }

    // Phase 5b: Evaluate consts (skip if skip_const_inlining is enabled).
    let resolved_consts = if skip_const_inlining {
        HashMap::new()
    } else {
        // Build a module registry from lowered functions for cross-module CTFE calls.
        let func_id_hashmap = func_id_map.to_hashmap(db_salsa);
        let module_registry = build_module_registry_from_lowered(&lowered_functions, &func_id_hashmap);
        evaluator.borrow_mut().set_module_registry(module_registry);

        evaluate_all_module_consts(db_salsa, parsed_graph, typecheck_result, evaluator, &lowered_functions, func_id_map, &module_consts)
    };

    // Module const failures have to reach the lowering result, or a module
    // whose const could not be evaluated compiles as though the functions that
    // name it were never written.
    let mut resolved_consts = resolved_consts;
    for (module_id, errors) in module_const_errors.into_iter().chain(shape_errors) {
        resolved_consts
            .entry(module_id)
            .or_insert_with(|| ModulePreResolvedConsts::new(module_id, Vec::new()))
            .errors
            .extend(errors);
    }
    let resolved_consts = resolved_consts;

    // Phase 5c: Specialize const parameter functions (union-branch transformation).
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

/// Specialize functions with const parameters using union-branch transformation.
///
/// This performs two transformations:
/// 1. Callee transformation: Functions with const params get transformed to
///    dispatch on a discriminant (union-branch form).
/// 2. Call site transformation: ComptimeCall instructions get transformed to
///    Const(discriminant) + Call with modified args.
fn specialize_comptime_functions<'db>(
    db: &'db dyn salsa::Database,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    resolved_consts: &HashMap<ModuleId<'db>, ModulePreResolvedConsts<'db>>,
    mut lowered_functions: HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
) -> HashMap<ModuleId<'db>, ModuleLoweredFunctions> {
    // Get the combined comptime registry.
    let registry = typecheck_result.comptime_registry(db);
    if registry.is_empty() {
        return lowered_functions;
    }

    // Build a flattened map of const names to values for lookup.
    // Include both qualified names (func_name::const_name) and unqualified names.
    //
    // Gathered in qualified-name order. The unqualified alias below keeps
    // whichever const reaches it first, so when two of them share a short name
    // the order decides which one wins; taken straight from a hash map that
    // would be decided by the seed the process happened to start with.
    let mut all_consts: Vec<_> = resolved_consts.values()
        .flat_map(|pre_resolved| pre_resolved.consts.iter())
        .collect();
    all_consts.sort_by(|a, b| a.0.cmp(&b.0));

    let mut const_values_map: HashMap<String, (IrType, ConstValue)> = HashMap::new();
    for (qualified_name, ir_type, value) in all_consts {
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
    let module_id_to_ir: HashMap<ModuleId<'db>, IrModuleId> = graph.iter_modules(db)
        .enumerate()
        .map(|(idx, module)| (module.id(db), IrModuleId(idx as u32)))
        .collect();

    // Build a global (IrModuleId, FuncId) -> name map for resolving CodeRef during call rewriting.
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
    resolved_consts: &HashMap<ModuleId<'db>, ModulePreResolvedConsts<'db>>,
    lowered_functions: &HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
    skip_const_inlining: bool,
) -> ModuleGraphLoweringResult<'db> {
    let graph = parsed_graph.graph(db);

    // Compute func_id_map (tracked, cached based on parsed_graph).
    let func_id_map = compute_func_id_map(db, parsed_graph);

    // Build module lookup.
    let module_map: HashMap<ModuleId<'db>, Module> = graph.iter_modules(db)
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
            reachable_func_ids(db, graph, func_id_map, *module_id),
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
    resolved_consts: &HashMap<ModuleId<'db>, ModulePreResolvedConsts<'db>>,
    lowered_functions: &HashMap<ModuleId<'db>, ModuleLoweredFunctions>,
    skip_const_inlining: bool,
) -> ModuleGraphLoweringResult<'db> {
    use rmx::rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);

    // Compute func_id_map once (will be cached).
    let func_id_map = compute_func_id_map(db_salsa, parsed_graph);

    // Build module lookup and data for parallel phase.
    let module_map: HashMap<ModuleId<'db>, Module> = graph.iter_modules(db_salsa)
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
                reachable_func_ids(db_salsa, graph, func_id_map, *module_id),
                module_consts,
                skip_const_inlining,
                module_funcs,
            ))
        })
        .collect();

    // Assemble modules in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module, ir_module_id, parsed, single_typecheck, single_ownership_analysis, func_ids, module_consts, skip_const_inlining, module_funcs)| {
        let db_s = db_clone.as_salsa_db();

        // This populates the cache.
        let _ = lower_module(
            db_s,
            module,
            ir_module_id,
            parsed,
            single_typecheck,
            single_ownership_analysis,
            func_ids,
            module_consts,
            skip_const_inlining,
            module_funcs,
        );
    });

    // Delegate to sequential function which aggregates results.
    // All lower_module calls will be cache hits from the parallel phase.
    assemble_module_graph(db_salsa, parsed_graph, typecheck_result, ownership_analysis, resolved_consts, lowered_functions, skip_const_inlining)
}

#[cfg(test)]
mod reachable_func_ids_tests {
    use super::*;
    use bct::module_graph::{ModuleGraph, ModuleGraphBuilder};
    use crate::Database;

    /// One function per module, named after the last segment of its path,
    /// plus one rider function whose module is not in the graph.
    ///
    /// `FuncIdMap` is a tracked struct, so it has to be built inside a tracked
    /// function, whose first argument has to be a salsa struct.
    #[salsa::tracked(returns(copy))]
    fn test_func_id_map<'db>(
        db: &'db dyn salsa::Database,
        graph: ModuleGraph<'db>,
    ) -> FuncIdMap<'db> {
        let mut entries = Vec::new();
        for (index, module) in graph.iter_modules(db).enumerate() {
            let id = module.id(db);
            let name = id.path(db).rsplit('/').next().X().to_string();
            entries.push(((id, name), (IrModuleId(index as u32), FuncId(0))));
        }
        let rider = ModuleId::new(db, "@rider/std".to_string());
        entries.push(((rider, "rider_fn".to_string()), (IrModuleId(99), FuncId(0))));
        FuncIdMap::new(db, entries)
    }

    fn reachable_names<'db>(db: &'db Database, graph: ModuleGraph<'db>, module: ModuleId<'db>) -> Vec<String> {
        let map = test_func_id_map(db, graph);
        let mut names: Vec<String> = reachable_func_ids(db, graph, map, module)
            .into_iter()
            .map(|((_, name), _)| name)
            .collect();
        names.sort();
        names
    }

    /// A module sees itself, what it requires transitively, and the riders,
    /// and does not see modules it has no path to.
    #[test]
    fn reaches_dependencies_but_not_strangers() {
        let db = Database::default();
        let source = |text: &str| bct::input::Source::new(&db, text.to_string());

        // base <- mid <- top, plus a module nobody requires.
        let mut builder = ModuleGraphBuilder::new(&db);
        let base = builder.add_module("local/test/base", source("// base"));
        let mid = builder.add_module("local/test/mid", source("// mid"));
        let top = builder.add_module("local/test/top", source("// top"));
        let stranger = builder.add_module("local/test/stranger", source("// stranger"));
        builder.add_dependency(mid, base);
        builder.add_dependency(top, mid);
        let graph = builder.build();

        // top requires mid requires base, so it reaches all three.
        assert_eq!(reachable_names(&db, graph, top), vec!["base", "mid", "rider_fn", "top"]);
        // mid does not reach top.
        assert_eq!(reachable_names(&db, graph, mid), vec!["base", "mid", "rider_fn"]);
        // base reaches only itself.
        assert_eq!(reachable_names(&db, graph, base), vec!["base", "rider_fn"]);
        // The stranger requires nothing and is required by nothing.
        assert_eq!(reachable_names(&db, graph, stranger), vec!["rider_fn", "stranger"]);
    }

    /// Adding an unrelated module leaves an existing module's entries alone.
    ///
    /// This is the property that keeps lowering cached: the argument compares
    /// equal, so salsa does not run it again.
    #[test]
    fn unrelated_module_does_not_change_the_entries() {
        let db = Database::default();
        let source = |text: &str| bct::input::Source::new(&db, text.to_string());

        let mut builder = ModuleGraphBuilder::new(&db);
        let a = builder.add_module("local/test/a", source("// a"));
        let graph_before = builder.build();
        let before = reachable_func_ids(
            &db, graph_before, test_func_id_map(&db, graph_before), a,
        );

        // The same graph plus an unrelated module, which lands after `a` and
        // so does not disturb its IR module index either.
        let mut builder = ModuleGraphBuilder::new(&db);
        let a2 = builder.add_module("local/test/a", source("// a"));
        let _b = builder.add_module("local/test/b", source("// b"));
        let graph_after = builder.build();
        let after = reachable_func_ids(
            &db, graph_after, test_func_id_map(&db, graph_after), a2,
        );

        assert_eq!(a, a2, "the path is the identity, so these are one module");
        assert_eq!(
            before, after,
            "adding an unrelated module must not change what `a` can reach",
        );
    }
}
