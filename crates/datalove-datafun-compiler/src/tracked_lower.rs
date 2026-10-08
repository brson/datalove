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
use std::sync::Arc;
use datalove_datafun_ir::{CodeRef, CodeUnitId, ConstValue, CtfeEvaluator, SharedConst, IrCodeUnit, IrType, FuncId, IrModuleId, ModuleFunctionRegistry};
use datalove_datafun_tycheck::{
    DbClone, ParallelMode,
    SingleModuleTypecheckResult,
    ModuleGraphTypecheckResult,
    ParsedModuleGraph,
};

use datalove_datafun_const::{
    inline_function_consts, inline_module_functions, promote_function_consts,
};
use crate::IrTypeExt;
use crate::const_cache::ConstCache;
use crate::const_eval::{
    const_type, evaluate_body_consts, evaluate_const, evaluate_instantiation_consts, ConstEvalEnv,
    NamedConstError,
};
use crate::lower;
use crate::specialize::{
    CalleeKey, MAX_ROUNDS, MonomorphizationPlan, collect_instantiations_into,
    module_callee_key, monomorphize_function, over_limit, rewrite_comptime_calls,
};
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

/// `FuncIdMap`'s entries in a form that can be looked up in.
pub type FuncIdLookup<'db> = HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>;

/// The lookup form of a `FuncIdMap`.
///
/// Building it means `to_hashmap`, which clones every function name in the
/// world. It used to be built once per const binding evaluated, and on the
/// system library that was most of what an unchanged recompile cost.
///
/// Tracked, because what it is keyed on is a tracked struct whose identity is
/// its entries: it only has to be built again when the set of functions in the
/// world changes, which an edit to a body does not do. So a recompile gets it
/// for a memo hit rather than for a clone of every name.
///
/// Asked for past phase 5's gates rather than up front, so that a program with
/// no const and no const parameter never builds it at all.
#[salsa::tracked(returns(ref))]
pub fn func_id_lookup<'db>(
    db: &'db dyn salsa::Database,
    func_id_map: FuncIdMap<'db>,
) -> FuncIdLookup<'db> {
    func_id_map.to_hashmap(db)
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
///
/// Interned rather than returned as a plain `Vec`, because both lowering
/// queries take it and each would otherwise intern the whole list into its own
/// memo key, once per module per compile. As an id it is one word to either.
#[salsa::interned]
pub struct ReachableFuncIds<'db> {
    #[returns(ref)]
    pub entries: Vec<((ModuleId<'db>, String), (IrModuleId, FuncId))>,
}

#[salsa::tracked(returns(copy))]
fn reachable_func_ids<'db>(
    db: &'db dyn salsa::Database,
    graph: bct::module_graph::ModuleGraph<'db>,
    func_id_map: FuncIdMap<'db>,
    module_id: ModuleId<'db>,
) -> ReachableFuncIds<'db> {
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

    let entries: Vec<((ModuleId<'db>, String), (IrModuleId, FuncId))> = func_id_map.entries(db)
        .iter()
        .filter(|((mid, _), _)| reachable.contains(mid) || !in_graph.contains(mid))
        .cloned()
        .collect();
    ReachableFuncIds::new(db, entries)
}

/// The `IrModuleId` of every module the program has, riders included.
///
/// **The riders come first, and that is the whole point.** They used to be
/// numbered after the regular modules -- `rider_module_idx =
/// regular_module_count` -- so adding or removing any module anywhere shifted
/// every rider's id. Every module's `reachable_func_ids` keeps every rider
/// entry, riders being the entries the graph does not account for, so every
/// module's `ReachableFuncIds` moved; that re-keyed `lower_module_functions` for
/// all of them and re-minted the `ModuleLowered` handles the rest of phase 5 is
/// keyed on. A module appearing anywhere re-lowered and re-assembled the whole
/// world. Numbered first, a rider's id depends on the rider sources alone.
///
/// **This is the one place that decides, and it is worth keeping that way.**
/// Four other places used to derive the same numbering from a position in the
/// graph, and all four had to agree with this one for a call to land on the
/// function it named.
///
/// Riders are ordered by their synthetic path, which comes from the rider's
/// name and so from its source. Ordering them by `ModuleId` would compare
/// salsa ids, which is interning order.
#[salsa::tracked(returns(ref))]
pub fn ir_module_ids<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
) -> BTreeMap<ModuleId<'db>, IrModuleId> {
    let mut riders_by_path: BTreeMap<String, ModuleId<'db>> = BTreeMap::new();
    for riders in parsed_graph.resolved_riders(db).values() {
        for (_alias, rider) in riders {
            riders_by_path.insert(rider.module_id.path(db).clone(), rider.module_id);
        }
    }

    let mut ids = BTreeMap::new();
    let mut next = 0u32;
    for rider_module_id in riders_by_path.into_values() {
        ids.insert(rider_module_id, IrModuleId(next));
        next += 1;
    }
    for (module_id, _) in parsed_graph.statements_only(db) {
        ids.insert(*module_id, IrModuleId(next));
        next += 1;
    }
    ids
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
    let module_ids = ir_module_ids(db, parsed_graph);

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let ir_module_id = *module_ids.get(module_id).expect("every parsed module is numbered");
        let mut next_func_id: u32 = 0;

        for statement in parsed.statements.iter() {
            if let Statement::Fun(func) = statement {
                let func_name = func.name(db).text(db).S();
                let func_id = FuncId(next_func_id);
                next_func_id += 1;
                entries.push(((*module_id, func_name), (ir_module_id, func_id)));
            }
        }
    }

    // Add rider functions, under the ids assigned above.
    for (_module_id, riders) in parsed_graph.resolved_riders(db).iter() {
        for (_alias, rider) in riders {
            let synthetic_module_id = rider.module_id;
            let rider_ir_module_id = *module_ids.get(&synthetic_module_id)
                .expect("every required rider is numbered");

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

    /// Evaluated const bindings: name -> (type, value). Hashed by shape; see
    /// `SharedConst`.
    pub consts: Vec<(String, IrType, SharedConst)>,

    /// Errors encountered during const evaluation.
    pub errors: Vec<String>,
}

impl<'db> ModulePreResolvedConsts<'db> {
    /// Create a new pre-resolved consts container.
    pub fn new(module_id: ModuleId<'db>, consts: Vec<(String, IrType, SharedConst)>) -> Self {
        Self { module_id, consts, errors: Vec::new() }
    }

    /// Create with errors.
    pub fn with_errors(module_id: ModuleId<'db>, consts: Vec<(String, IrType, SharedConst)>, errors: Vec<String>) -> Self {
        Self { module_id, consts, errors }
    }

    /// Convert to a HashMap for efficient lookup.
    pub fn to_hashmap(&self) -> HashMap<String, (IrType, Arc<ConstValue>)> {
        self.consts
            .iter()
            .map(|(name, ty, val)| (name.clone(), (ty.clone(), Arc::clone(val))))
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
    ///
    /// Behind `Arc`s so that building the interpreter's registry out of them
    /// does not copy every instruction in the program.
    #[returns(ref)]
    pub functions: Vec<Arc<IrCodeUnit>>,

    /// Lowering errors (IR generation only, not ownership analysis).
    #[returns(ref)]
    pub errors: Vec<String>,

    /// Function name to FuncId mapping for this module.
    #[returns(ref)]
    pub func_ids: Vec<(String, FuncId)>,

    /// The module's module-level consts, in name order.
    ///
    /// A script that specializes one of the module's comptime functions
    /// evaluates the copy's consts, which may name these. Hashed by shape; see
    /// `SharedConst`.
    #[returns(ref)]
    pub consts: Vec<(String, IrType, SharedConst)>,
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
                r.functions(db).iter().map(move |f| (ir_mod, FuncId(f.id.0), (**f).clone()))
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
/// If `lowered` is provided, those functions are reused instead of being
/// re-lowered from the AST. It arrives as a handle rather than as the IR, so
/// this query's memo key holds a word rather than a module's instructions.
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
    func_ids: ReachableFuncIds<'db>,
    pre_resolved_consts: Option<ModulePreResolvedConsts<'db>>,
    skip_const_inlining: bool,
    lowered: Option<ModuleLowered<'db>>,
) -> SingleModuleLoweringResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("lower", module_path, QueryPhase::Start);

    let expr_types = typecheck_result.expr_types(db);
    let call_targets = typecheck_result.call_targets(db);

    let func_id_hashmap: HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)> =
        func_ids.entries(db).iter().cloned().collect();

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
    for statement in parsed.statements.iter() {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();
            let func_id = FuncId(next_func_id);
            next_func_id += 1;
            func_ids.push((func_name, func_id));
        }
    }

    // Module-level consts, for the from-scratch path below. They are stored
    // under a bare name; a function-level one is qualified with its function.
    let module_level_consts: HashMap<String, (IrType, Arc<ConstValue>)> = pre_resolved_consts
        .as_ref()
        .map(|c| c.consts.iter()
            .filter(|(name, _, _)| !name.contains("::"))
            .map(|(name, ty, value)| (name.clone(), (ty.clone(), Arc::clone(value))))
            .collect())
        .unwrap_or_default();

    // If lowered functions are provided, use them directly.
    if let Some(lowered) = lowered {
        functions = lowered.functions(db).functions.clone();
        // func_ids was already computed above, and should match lf.func_name_to_id.
    } else {
        // Lower functions from scratch.
        functions = Vec::new();
        let mut func_idx = 0;
        for statement in parsed.statements.iter() {
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
                    typecheck_result.data_files(db),
                ) {
                    Ok(ir_func) => {
                        functions.push(Arc::new(ir_func));
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
            let const_values: HashMap<String, Arc<ConstValue>> = pre_resolved.consts
                .iter()
                .map(|(name, _ir_type, value)| (name.clone(), Arc::clone(value)))
                .collect();
            // Inline const values directly into the functions.
            inline_module_functions(&mut functions, &const_values);
        }
        // A function's consts, and a specialized copy's const parameters, are
        // built once and borrowed rather than built at every call.
        for func in &mut functions {
            promote_function_consts(Arc::make_mut(func));
        }
    }

    log_query("lower", module_path, QueryPhase::End);

    let mut consts: Vec<(String, IrType, SharedConst)> = module_level_consts.into_iter()
        .map(|(name, (ty, value))| (name, ty, SharedConst(value)))
        .collect();
    consts.sort_by(|a, b| a.0.cmp(&b.0));

    SingleModuleLoweringResult::new(
        db,
        module_id,
        ir_module_id,
        functions,
        errors,
        func_ids,
        consts,
    )
}

/// Lowered functions for a single module.
///
/// Functions are lowered once, then reused for const evaluation
/// and final module assembly.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ModuleLoweredFunctions {
    /// The lowered IR code units (functions) for this module.
    ///
    /// Behind `Arc`s because the registries built from these copied every
    /// instruction to do it, twice per compile.
    pub functions: Vec<Arc<IrCodeUnit>>,
    /// Map from function name to FuncId.
    /// Sorted vector for deterministic hashing.
    pub func_name_to_id: Vec<(String, FuncId)>,
}

/// A handle to one module's lowered IR.
///
/// The point of it is the `#[tracked]` field. A tracked struct's identity is a
/// hash of its *untracked* fields, so this one's identity is a module's, and
/// passing it to a query costs a word rather than a hash of every instruction
/// the module holds. The IR itself is read through its own dependency edge, so
/// a consumer still re-runs when the IR changes and still backdates when it
/// does not.
///
/// That is what phase 5a's passes are keyed on. Before this they took
/// `HashMap<ModuleId, Arc<ModuleLoweredFunctions>>` by value, which meant
/// hashing the whole program's IR to answer a memo, and hashing it was around
/// a tenth of what an unchanged recompile cost.
#[salsa::tracked]
pub struct ModuleLowered<'db> {
    /// The module this is the IR of. Untracked, so it is the identity.
    #[returns(copy)]
    pub module_id: ModuleId<'db>,

    /// The IR. Tracked, so it may change without the handle changing.
    #[tracked]
    #[returns(ref)]
    pub functions: Arc<ModuleLoweredFunctions>,
}

impl<'db> ModuleLowered<'db> {
    /// The IR as an `Arc`, for the plain passes that still take a map of them.
    fn ir(self, db: &'db dyn salsa::Database) -> Arc<ModuleLoweredFunctions> {
        Arc::clone(self.functions(db))
    }
}

/// The lowered IR of every module, in graph order.
///
/// A `Vec` of handles rather than a map, because it is a memo key and has to
/// hash the same way twice; graph order is the order `IrModuleId`s are
/// assigned in, so it is the order everything else here walks modules in.
type LoweredModules<'db> = Vec<(ModuleId<'db>, ModuleLowered<'db>)>;

/// The lowered IR keyed for lookup, which is what the plain passes want.
fn ir_map<'db>(
    db: &'db dyn salsa::Database,
    modules: &LoweredModules<'db>,
) -> HashMap<ModuleId<'db>, Arc<ModuleLoweredFunctions>> {
    modules.iter().map(|(id, lowered)| (*id, lowered.ir(db))).collect()
}

/// One module's code units for CTFE, keyed by the id a `CodeRef` names.
///
/// Tracked on the handle, so the registry below is a refcount per module rather
/// than an insert per function when one module's IR moves.
#[salsa::tracked(returns(ref))]
fn ctfe_module_units<'db>(
    db: &'db dyn salsa::Database,
    lowered: ModuleLowered<'db>,
) -> Arc<datalove_datafun_ir::ModuleCodeUnits> {
    Arc::new(
        lowered.functions(db).functions.iter()
            .map(|unit| (unit.id, Arc::clone(unit)))
            .collect(),
    )
}

/// Create native IrCodeUnits for rider functions and add them to the registry.
///
/// Here rather than at the backend boundary, because consts call natives while
/// the modules are still lowering. What a native's symbol is called is left to
/// [`NativeContext::new`](datalove_datafun_ir::NativeContext::new).
pub fn add_native_rider_units<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    func_id_map: &HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)>,
    registry: &mut ModuleFunctionRegistry,
) {
    use datalove_datafun_ir::NativeContext;
    use std::collections::BTreeSet;

    let mut seen_riders: BTreeSet<String> = BTreeSet::new();

    for (_module_id, riders) in parsed_graph.resolved_riders(db).iter() {
        for (alias, rider) in riders {
            let alias_str = alias.text(db);
            if !seen_riders.insert(alias_str.S()) {
                continue; // Already processed this rider.
            }

            let synthetic_module_id = rider.module_id;

            for (func_name, func_type) in &rider.functions {
                let name = func_name.text(db).S();

                // Look up the assigned IrModuleId and FuncId.
                let Some(&(ir_module_id, func_id)) = func_id_map.get(&(synthetic_module_id, name.clone())) else {
                    continue;
                };

                // Convert types from tycheck to IR.
                let param_types: Vec<IrType> = func_type.param_types(db)
                    .iter()
                    .map(|ty| IrType::from_tycheck(db, ty))
                    .collect();
                // Convert AST ParamMode to IR ParamMode.
                let param_modes: Vec<datalove_datafun_ir::ParamMode> = func_type.param_modes(db)
                    .iter()
                    .map(|m| match m {
                        datalove_datafun_ast::ast::ParamMode::In => datalove_datafun_ir::ParamMode::In,
                        datalove_datafun_ast::ast::ParamMode::Out => datalove_datafun_ir::ParamMode::Out,
                        datalove_datafun_ast::ast::ParamMode::Ref => datalove_datafun_ir::ParamMode::Ref,
                        datalove_datafun_ast::ast::ParamMode::Mut => datalove_datafun_ir::ParamMode::Mut,
                    })
                    .collect();
                let return_type = IrType::from_tycheck(db, &func_type.return_type(db));

                // What the shape closure was told this native needs, said
                // the same way here so that the two cannot disagree about
                // the trailing arguments. A native makes no calls, so its
                // set is exactly what its signature says.
                let descriptor_shapes = rider.generic_functions.iter()
                    .find(|(n, _)| *n == *func_name)
                    .map(|(_, generics)| generics.undetermined.iter()
                        .map(|i| datalove_datafun_ir::DescriptorShape::Param(*i))
                        .collect())
                    .unwrap_or_default();

                let native_ctx = NativeContext::new(
                    alias_str,
                    &name,
                    param_modes,
                    param_types,
                    return_type,
                    descriptor_shapes,
                );
                let code_unit = datalove_datafun_ir::IrCodeUnit::native(
                    CodeUnitId(func_id.0),
                    name,
                    native_ctx,
                );
                registry.add_module_code_unit(ir_module_id, code_unit.id, std::sync::Arc::new(code_unit));
            }
        }
    }
}

/// Build a module function registry from lowered functions.
///
/// This creates a registry for CTFE to use when evaluating const expressions
/// that call functions from other modules, or natives.
///
/// Tracked, because it is built from what phase 5a produced and both const
/// phases want one: keyed on the handles it is a memo, and an unchanged
/// recompile stops rebuilding an entry per function in the world and then
/// dropping it.
#[salsa::tracked(returns(ref))]
fn ctfe_module_registry<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    modules: LoweredModules<'db>,
    func_id_map: FuncIdMap<'db>,
) -> Arc<ModuleFunctionRegistry> {
    // Which `IrModuleId` each module was given. The entries are iterated rather
    // than collected, because `to_hashmap` clones every function name in the
    // world to build a map this only walks.
    let mut ir_module_ids: HashMap<ModuleId<'db>, IrModuleId> = HashMap::new();
    for ((module_id, _), (ir_module_id, _)) in func_id_map.entries(db) {
        ir_module_ids.entry(*module_id).or_insert(*ir_module_id);
    }

    let mut registry = ModuleFunctionRegistry::new();
    for (module_id, lowered) in &modules {
        // A module the id map does not name declares no functions.
        let Some(ir_module_id) = ir_module_ids.get(module_id).copied() else {
            continue;
        };
        registry.set_module_code_units(ir_module_id, Arc::clone(ctfe_module_units(db, *lowered)));
    }
    add_native_rider_units(db, parsed_graph, func_id_lookup(db, func_id_map), &mut registry);
    Arc::new(registry)
}

/// Give each module function the descriptor shapes its callees need of it.
///
/// Lowering knows what a function's own body builds; this adds what it carries
/// Every native that wants a descriptor, keyed the way a `CodeRef` names it.
///
/// See `RiderInterface::undetermined_type_params` for which those are. The keys
/// have to match `compute_func_id_map`'s numbering, so the position of a
/// function in `rider.functions` is its `FuncId` here as it is there.
fn native_shape_keys<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    func_id_map: FuncIdMap<'db>,
) -> Vec<((IrModuleId, FuncId), Vec<datalove_datafun_ir::DescriptorShape>)> {
    use datalove_datafun_ir::DescriptorShape;

    // Only the riders' entries. `to_hashmap` would clone every function name in
    // the world to answer a handful of lookups, and this runs whenever the
    // shape closure does.
    let rider_modules: std::collections::BTreeSet<ModuleId<'db>> = parsed_graph.resolved_riders(db)
        .values()
        .flat_map(|riders| riders.iter().map(|(_, rider)| rider.module_id))
        .collect();
    let ids: HashMap<(ModuleId<'db>, &'db str), (IrModuleId, FuncId)> = func_id_map.entries(db)
        .iter()
        .filter(|((module_id, _), _)| rider_modules.contains(module_id))
        .map(|((module_id, name), location)| ((*module_id, name.as_str()), *location))
        .collect();

    let mut found = Vec::new();
    for (_module_id, riders) in parsed_graph.resolved_riders(db).iter() {
        for (_alias, rider) in riders {
            for (func_name, generics) in &rider.generic_functions {
                if generics.undetermined.is_empty() {
                    continue;
                }
                let key = (rider.module_id, func_name.text(db).as_str());
                let Some(&(ir_module_id, func_id)) = ids.get(&key) else { continue };
                let shapes = generics.undetermined.iter()
                    .map(|i| DescriptorShape::Param(*i)).collect();
                found.push(((ir_module_id, func_id), shapes));
            }
        }
    }
    found
}

/// The module IR after the shape closure, and what the closure refused.
#[derive(Clone, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
struct ShapeClosure<'db> {
    /// Every module's IR, in the order it came in.
    modules: LoweredModules<'db>,
    /// Cycles the closure could not settle, per module.
    errors: Vec<(ModuleId<'db>, Vec<String>)>,
}

/// on a callee's behalf. See `close_shapes` for why the iteration settles and
/// what it refuses. Done before anything reads a signature, because the shapes
/// are part of one: they say what trailing arguments a call has to pass.
///
/// **This never reads the IR.** It is a fixpoint over the call graph, so it
/// cannot be split per module and it is keyed on every module at once. That
/// makes what it *depends on* the thing that decides whether an edit re-runs
/// it, and it used to depend on every module's instructions -- so a body edit
/// anywhere re-ran the whole closure even though no shape had moved. It reads
/// `module_shape_inputs` instead, which is per module and backdates when a
/// function's shapes and call targets come out the same, which is what an edit
/// to a body usually does. Writing the answer back is `shape_closed_module`,
/// also per module.
#[salsa::tracked(returns(ref))]
fn close_shapes_over_calls<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    func_id_map: FuncIdMap<'db>,
    modules: LoweredModules<'db>,
) -> ShapeClosure<'db> {
    use datalove_datafun_ir::DescriptorShape;
    use rustc_hash::FxHashMap;

    type Key = (IrModuleId, FuncId);

    // Which `IrModuleId` each module was given, which `compute_func_id_map`
    // assigns by position in the graph. Read off the entries rather than
    // recomputed, and the entries are iterated rather than collected because
    // `to_hashmap` clones every function name in the world to build a map this
    // only walks.
    let mut ir_module_ids: HashMap<ModuleId<'db>, IrModuleId> = HashMap::new();
    for ((module_id, _), (ir_module_id, _)) in func_id_map.entries(db) {
        ir_module_ids.entry(*module_id).or_insert(*ir_module_id);
    }

    // What each module puts in, out of its own memo. A module the id map does
    // not name declares no functions, so it has nothing to put in.
    let per_module: HashMap<ModuleId<'db>, (IrModuleId, &Vec<FunctionShapeInputs>)> = modules.iter()
        .filter_map(|(module_id, lowered)| {
            let ir_module_id = ir_module_ids.get(module_id).copied()?;
            Some((*module_id, (ir_module_id, module_shape_inputs(db, *lowered, ir_module_id))))
        })
        .collect();

    // Keyed on eight bytes and rebuilt whenever this runs at all, which is why
    // the hasher was worth changing here and nowhere else.
    let mut calls: FxHashMap<Key, Vec<(Key, Vec<DescriptorShape>)>> = FxHashMap::default();
    let mut shapes: FxHashMap<Key, Vec<DescriptorShape>> = FxHashMap::default();

    // The natives go in first. They are not lowered units, so nothing writes an
    // answer back to them; they are here so that a caller of one learns it has
    // to be handed a descriptor. A native makes no calls of its own, so the
    // closure never adds to its set and what goes in is what comes out --
    // which is why `add_native_rider_units` can say the same thing from the
    // signature alone and still agree.
    for key in native_shape_keys(db, parsed_graph, func_id_map) {
        shapes.insert(key.0, key.1);
    }

    for (ir_module_id, functions) in per_module.values() {
        for function in functions.iter() {
            let key = (*ir_module_id, function.func_id);
            shapes.insert(key, function.own.clone());
            calls.insert(key, function.calls.clone());
        }
    }

    if let Err(growing) = datalove_datafun_ir::close_shapes(&calls, &mut shapes) {
        let name = |key: Key| per_module.values()
            .filter(|(ir_module_id, _)| *ir_module_id == key.0)
            .flat_map(|(_, functions)| functions.iter())
            .find(|function| function.func_id == key.1)
            .map(|function| function.name.clone())
            .unwrap_or_else(|| "?".to_string());
        // Reported against the module the cycle runs through, or against the
        // first there is if the growing function is a native with no module.
        let module_id = modules.iter()
            .map(|(module_id, _)| *module_id)
            .find(|module_id| {
                per_module.get(module_id)
                    .is_some_and(|(ir_module_id, _)| *ir_module_id == growing.function.0)
            })
            .or_else(|| modules.first().map(|(module_id, _)| *module_id));
        let mut errors = Vec::new();
        if let Some(module_id) = module_id {
            errors.push((module_id, vec![format!(
                "`{}` calls `{}` binding its type parameter {} to `{}`, which holds \
                 one of `{}`'s own, so every call round that cycle needs a descriptor \
                 one level deeper than the last. `{}` would be handed `{}` and there \
                 is no end to it",
                name(growing.caller), name(growing.callee), growing.param, growing.bound,
                name(growing.caller), name(growing.function), growing.shape,
            )]));
        }
        return ShapeClosure { modules, errors };
    }

    // With the sets settled, hand each module the part of the answer that is
    // about it: what its own functions carry, and what the functions it calls
    // want of it. Restricted rather than handed the whole map, because the
    // whole map moves whenever any module's shapes do, and then every module's
    // apply would run again for a change it has nothing to do with.
    //
    // Empty sets are left out. The closure only ever adds to what a body
    // declared, so an empty settled set means the function declared none and
    // there is nothing to write; and a callee wanting nothing is a call whose
    // descriptor list lowering already left empty.
    let mut out = Vec::with_capacity(modules.len());
    let mut errors: Vec<(ModuleId<'db>, Vec<String>)> = Vec::new();
    for (module_id, incoming) in &modules {
        let Some((ir_module_id, functions)) = per_module.get(module_id).copied() else {
            out.push((*module_id, *incoming));
            continue;
        };

        let own: Vec<(FuncId, Vec<DescriptorShape>)> = functions.iter()
            .filter_map(|function| {
                let settled = shapes.get(&(ir_module_id, function.func_id))?;
                (!settled.is_empty()).then(|| (function.func_id, settled.clone()))
            })
            .collect();
        let wanted: BTreeMap<Key, Vec<DescriptorShape>> = functions.iter()
            .flat_map(|function| function.callees.iter())
            .filter_map(|callee| {
                let settled = shapes.get(callee)?;
                (!settled.is_empty()).then(|| (*callee, settled.clone()))
            })
            .collect();

        if own.is_empty() && wanted.is_empty() {
            out.push((*module_id, *incoming));
            continue;
        }

        let closed = shape_closed_module(
            db, *incoming, ir_module_id, own, wanted.into_iter().collect());
        out.push((*module_id, closed.lowered));
        if !closed.errors.is_empty() {
            errors.push((*module_id, closed.errors.clone()));
        }
    }

    ShapeClosure { modules: out, errors }
}

/// One module's IR with the settled shapes written into it.
///
/// Per module and keyed on what that module's part of the answer is, so a
/// module the closure did not move takes this out of a memo rather than
/// walking its instructions. It is also the only thing here that reads the IR.
///
/// A module it does not change comes back under the handle it went in under,
/// rather than under a fresh one saying the same thing: a tracked struct's
/// identity map belongs to the query instance that created it, so minting one
/// here would give phase 5d a new key for a module whose IR is the same.
#[salsa::tracked(returns(ref))]
fn shape_closed_module<'db>(
    db: &'db dyn salsa::Database,
    lowered: ModuleLowered<'db>,
    ir_module_id: IrModuleId,
    own_shapes: Vec<(FuncId, Vec<datalove_datafun_ir::DescriptorShape>)>,
    callee_shapes: Vec<((IrModuleId, FuncId), Vec<datalove_datafun_ir::DescriptorShape>)>,
) -> ClosedModule<'db> {
    use datalove_datafun_ir::{CodeRef, CodeUnitContext, DescriptorShape};
    use rustc_hash::FxHashMap;

    let own_by_id: FxHashMap<FuncId, &Vec<DescriptorShape>> =
        own_shapes.iter().map(|(func_id, shapes)| (*func_id, shapes)).collect();
    let callees: FxHashMap<(IrModuleId, FuncId), &Vec<DescriptorShape>> =
        callee_shapes.iter().map(|(key, shapes)| (*key, shapes)).collect();
    let wanted = |code_ref: &CodeRef| -> Vec<DescriptorShape> {
        let callee = match code_ref {
            CodeRef::Module { module, id } => (*module, FuncId(id.0)),
            CodeRef::Local(id) => (ir_module_id, FuncId(id.0)),
            CodeRef::External { .. } => return Vec::new(),
        };
        callees.get(&callee).map(|shapes| (*shapes).clone()).unwrap_or_default()
    };

    let held = lowered.functions(db);
    let mut functions: Option<ModuleLoweredFunctions> = None;
    let mut errors = Vec::new();

    for (idx, unit) in held.functions.iter().enumerate() {
        let own: &[DescriptorShape] = own_by_id.get(&FuncId(unit.id.0))
            .map(|shapes| shapes.as_slice())
            .unwrap_or(&[]);

        let carries = match &unit.context {
            CodeUnitContext::Function(ctx) => ctx.descriptor_shapes != own,
            _ => false,
        };
        let resolved = match datalove_datafun_ir::resolve_call_descriptors(unit, own, &wanted) {
            Ok(resolved) => resolved,
            Err(missing) => {
                errors.push(format!(
                    "`{}` hands a type parameter to a function that builds a collection \
                     of it and was given no descriptor for `{}` to pass on",
                    unit.name, missing,
                ));
                continue;
            }
        };
        if !carries && resolved.is_none() {
            continue;
        }

        // Copied only now that there is something to write. The `Arc` is shared
        // with phase 5a's memo, so reaching for `make_mut` before knowing costs
        // a copy of the function for nothing.
        let functions = functions.get_or_insert_with(|| held.as_ref().clone());
        let unit = Arc::make_mut(&mut functions.functions[idx]);
        if let CodeUnitContext::Function(ctx) = &mut unit.context {
            ctx.descriptor_shapes = own.to_vec();
        }
        if let Some(resolved) = resolved {
            datalove_datafun_ir::write_call_descriptors(unit, resolved);
        }
    }

    let lowered = match functions {
        None => lowered,
        Some(functions) => ModuleLowered::new(db, lowered.module_id(db), Arc::new(functions)),
    };
    ClosedModule { lowered, errors }
}

/// One module's IR after the closure, and what the closure refused in it.
#[derive(Clone, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
struct ClosedModule<'db> {
    lowered: ModuleLowered<'db>,
    errors: Vec<String>,
}

/// What one of a module's functions puts into the shape closure.
///
/// The `Vec`s are almost always empty: a function that names no type parameter
/// builds no shape and binds none at its calls.
///
/// This carries everything about a function the graph-wide pass needs,
/// including its name, so that the pass never reads the IR. That is the point
/// of it: reading one module's IR would make the fixpoint depend on all of
/// them, and then an edit anywhere re-runs it whether or not any shape moved.
#[derive(Clone, PartialEq, Eq)]
struct FunctionShapeInputs {
    /// Its id, which is also where the closure writes its answer back.
    func_id: FuncId,
    /// Its name, for the message the closure gives when it refuses a cycle.
    name: String,
    /// The shapes its own body builds.
    own: Vec<datalove_datafun_ir::DescriptorShape>,
    /// Its calls that bind a callee's type parameters, and to what.
    calls: Vec<((IrModuleId, FuncId), Vec<datalove_datafun_ir::DescriptorShape>)>,
    /// Everything it calls, whether or not type parameters are bound.
    ///
    /// The apply step asks what each callee wants for every call, not only the
    /// generic ones, so this is what decides which of the settled shapes reach
    /// that module's memo key.
    callees: Vec<(IrModuleId, FuncId)>,
}

/// One module's contribution to the shape closure, in the order its functions
/// sit in, so an index here is an index into the module's IR.
///
/// Tracked on the handle, which is the whole point: the closure is a fixpoint
/// over the call graph and has to re-run whenever any module's IR moves, but
/// reading the graph back out of the IR is per module and does not. Reading it
/// is also the expensive half by a wide margin -- on the system library it was
/// about a sixth of what an edit cost, against a twenty-fifth for the fixpoint
/// over it.
#[salsa::tracked(returns(ref))]
fn module_shape_inputs<'db>(
    db: &'db dyn salsa::Database,
    lowered: ModuleLowered<'db>,
    ir_module_id: IrModuleId,
) -> Vec<FunctionShapeInputs> {
    use datalove_datafun_ir::CodeRef;

    lowered.functions(db).functions.iter()
        .map(|unit| {
            let own = unit.function_context()
                .map(|c| c.descriptor_shapes.clone())
                .unwrap_or_default();
            let mut calls = Vec::new();
            let mut callees = std::collections::BTreeSet::new();
            for block in &unit.blocks {
                for instr in &block.instructions {
                    let Some((func, type_args)) = instr.call_target() else { continue };
                    let callee = match func {
                        CodeRef::Module { module, id } => (*module, FuncId(id.0)),
                        // A local reference inside a module names that module's own.
                        CodeRef::Local(id) => (ir_module_id, FuncId(id.0)),
                        CodeRef::External { .. } => continue,
                    };
                    callees.insert(callee);
                    if type_args.is_empty() {
                        continue;
                    }
                    calls.push((callee, type_args.to_vec()));
                }
            }
            FunctionShapeInputs {
                func_id: FuncId(unit.id.0),
                name: unit.name.clone(),
                own,
                calls,
                callees: callees.into_iter().collect(),
            }
        })
        .collect()
}

/// What lowering one module's functions produced.
#[derive(Clone, PartialEq, Eq)]
#[derive(salsa::SalsaValue)]
pub struct ModuleLowerOutcome<'db> {
    /// Absent when the module declares no functions at all.
    lowered: Option<ModuleLowered<'db>>,
    /// Functions that named a module const which is not evaluated yet.
    deferred: Vec<String>,
}

/// Lower one module's functions.
///
/// Tracked, so an edit re-lowers the module that changed rather than every
/// module in the world. The arguments are what that costs: each has to be
/// something the memo can be keyed on, and each has to be narrow enough that an
/// unrelated edit leaves it equal.
///
/// The statements are pulled rather than passed. They are the same value
/// `parse_module_graph` put in the graph, and keeping them out of the key means
/// an edit does not re-hash every module's AST into a second memo key on top of
/// the one `lower_module` already builds.
///
/// - `func_ids` comes from `reachable_func_ids` rather than the whole map, for
///   the reason given there.
/// - `module_consts` is sorted, since it arrives as a `HashMap` and the key has
///   to hash the same way twice.
/// - `restrict` is `None` on the first stratum and `Some` on the second, where
///   it names the functions that were waiting on a module const. `Some(empty)`
///   means this module has nothing to lower, which is not the same as `None`.
#[salsa::tracked]
pub fn lower_module_functions<'db>(
    db: &'db dyn salsa::Database,
    module: Module<'db>,
    single_typecheck: SingleModuleTypecheckResult<'db>,
    single_ownership: SingleModuleAnalysis<'db>,
    func_ids: ReachableFuncIds<'db>,
    module_consts: Vec<(String, (IrType, SharedConst))>,
    restrict: Option<Vec<String>>,
) -> ModuleLowerOutcome<'db> {
    log_query("lower_functions", module.id(db).path(db), QueryPhase::Start);

    let parsed = &crate::module_graph::parse_module_full(db, module).parsed;
    let func_id_hashmap: HashMap<(ModuleId<'db>, String), (IrModuleId, FuncId)> =
        func_ids.entries(db).iter().cloned().collect();
    let module_consts: HashMap<String, (IrType, Arc<ConstValue>)> =
        module_consts.into_iter().map(|(name, (ty, value))| (name, (ty, value.0))).collect();
    let restricting = restrict.is_some();
    let restrict = restrict.as_ref();
    let func_id_hashmap = &func_id_hashmap;
    let module_consts = &module_consts;

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
    let mut deferred = Vec::new();

    // Assign FuncIds and lower each function.
    let mut next_func_id: u32 = 0;
    for statement in parsed.statements.iter() {
        if let Statement::Fun(func) = statement {
            let func_name = func.name(db).text(db).S();
            let func_id = FuncId(next_func_id);
            next_func_id += 1;

            func_name_to_id.push((func_name.clone(), func_id));

            // On the second pass only the deferred functions are lowered.
            // Their FuncIds still come from statement position, so the
            // numbering matches whichever pass a function lands in.
            if restricting {
                let wanted = restrict
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
                func_id_hashmap,
                *func,
                func_id,
                analysis,
                resolved_params,
                resolved_return,
                module_consts,
                single_typecheck.data_files(db),
            ) {
                Ok(ir_func) => {
                    functions.push(Arc::new(ir_func));
                }
                Err(lower::LowerError::BindingNotAvailable(_)) => {
                    // Names a module const that has not been evaluated yet.
                    // Recorded so the caller can lower it once it has.
                    deferred.push(func_name);
                    continue;
                }
                Err(_) => {
                    // Errors will be reported during the main lowering phase.
                    continue;
                }
            }
        }
    }

    let lowered = if functions.is_empty() && func_name_to_id.is_empty() {
        None
    } else {
        let ir = Arc::new(ModuleLoweredFunctions { functions, func_name_to_id });
        Some(ModuleLowered::new(db, module.id(db), ir))
    };

    log_query("lower_functions", module.id(db).path(db), QueryPhase::End);

    ModuleLowerOutcome { lowered, deferred }
}

/// Lower all module functions.
///
/// This lowers all functions across all modules. The lowered functions are
/// reused for const evaluation and final module assembly.
///
/// Returns a map of module_id -> lowered functions.
///
/// The work itself is `lower_module_functions`, which is tracked, so this walks
/// the graph but only pays for the modules whose inputs moved. Under
/// `ParallelMode::Parallel` the misses are computed on rayon's pool and read
/// back here: the outcome names a tracked struct, which carries the database
/// lifetime and so cannot travel out of a worker's borrow of its own clone.
/// The second pass is every module's memo already warm.
#[allow(clippy::too_many_arguments)]
pub fn lower_all_module_functions<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    ownership_analysis: ModuleGraphAnalysis<'db>,
    func_id_map: FuncIdMap<'db>,
    module_consts: &HashMap<ModuleId<'db>, HashMap<String, (IrType, Arc<ConstValue>)>>,
    deferred: &mut HashMap<ModuleId<'db>, Vec<String>>,
    restrict: Option<&HashMap<ModuleId<'db>, Vec<String>>>,
    mode: ParallelMode,
) -> LoweredModules<'db> {
    use rmx::rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);
    let typecheck_module_results = typecheck_result.module_results(db_salsa);
    let ownership_analysis_results = ownership_analysis.module_results(db_salsa);
    let module_map: HashMap<ModuleId<'db>, Module> = graph.iter_modules(db_salsa)
        .map(|m| (m.id(db_salsa), m))
        .collect();

    // The modules that have everything lowering needs, in graph order, with the
    // arguments their query is keyed on. Built here rather than in the workers
    // so that the two modes key the query identically.
    let ready: Vec<_> = parsed_graph.statements_only(db_salsa)
        .iter()
        .filter_map(|(module_id, _parsed)| {
            let module = *module_map.get(module_id)?;
            let single_typecheck = *typecheck_module_results.get(module_id)?;
            let single_ownership = *ownership_analysis_results.get(module_id)?;

            // Sorted, because it arrives as a `HashMap` and the memo key has to
            // hash the same way for the same consts.
            let mut consts: Vec<(String, (IrType, SharedConst))> = module_consts
                .get(module_id)
                .map(|m| m.iter().map(|(k, (ty, v))| (k.clone(), (ty.clone(), SharedConst(Arc::clone(v))))).collect())
                .unwrap_or_default();
            consts.sort_by(|a, b| a.0.cmp(&b.0));

            Some((
                *module_id,
                module,
                single_typecheck,
                single_ownership,
                reachable_func_ids(db_salsa, graph, func_id_map, *module_id),
                consts,
                restrict.map(|r| r.get(module_id).cloned().unwrap_or_default()),
            ))
        })
        .collect();

    if matches!(mode, ParallelMode::Parallel) {
        // Clone the database up front, one per module: `&dyn DbClone` is not
        // `Sync`, so the clones cannot be made inside the parallel section.
        let work: Vec<_> = ready.iter()
            .map(|item| (db.dyn_clone(), item.clone()))
            .collect();

        work.into_par_iter().for_each(
            |(db_clone, (_module_id, module, tc, own, func_ids, consts, restrict))| {
                let _ = lower_module_functions(
                    db_clone.as_salsa_db(), module, tc, own, func_ids, consts, restrict,
                );
            },
        );
    }

    let mut result = Vec::with_capacity(ready.len());
    for (module_id, module, tc, own, func_ids, consts, restrict) in ready {
        let outcome = lower_module_functions(
            db_salsa, module, tc, own, func_ids, consts, restrict,
        );
        if !outcome.deferred.is_empty() {
            deferred.entry(module_id).or_default().extend(outcome.deferred.iter().cloned());
        }
        if let Some(lowered) = outcome.lowered {
            result.push((module_id, lowered));
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
///
/// With a `cache`, a module whose closure is unchanged since its body consts
/// last evaluated cleanly takes them from there; see `const_cache`. Those of a
/// module in `module_level_errors` are not kept, having been evaluated against
/// module consts that are missing some.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_all_module_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    evaluator: &mut dyn CtfeEvaluator,
    lowered_functions: &HashMap<ModuleId<'db>, Arc<ModuleLoweredFunctions>>,
    callable: &ModuleFunctionRegistry,
    func_id_map: &FuncIdLookup<'db>,
    module_consts: &HashMap<ModuleId<'db>, HashMap<String, (IrType, Arc<ConstValue>)>>,
    module_level_errors: &HashMap<ModuleId<'db>, Vec<String>>,
    mut cache: Option<&mut ConstCache>,
) -> HashMap<ModuleId<'db>, ModulePreResolvedConsts<'db>> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let path = module_id.path(db);

        let mut consts = Vec::new();
        let mut errors = Vec::new();

        // Module-level consts were evaluated between the lowering strata. They
        // are recorded under a bare name and seeded into every function, so a
        // function-level const can name one.
        let module_level = module_consts.get(module_id).cloned().unwrap_or_default();
        for (name, (ir_type, value)) in &module_level {
            consts.push((name.clone(), ir_type.clone(), SharedConst(Arc::clone(value))));
        }

        let cached = cache.as_deref().and_then(|c| c.body(path)).cloned();
        let verifying = cache.as_deref().is_some_and(ConstCache::verifying);
        if let Some(body) = &cached {
            if !verifying {
                cache.as_deref_mut().expect("a hit came from the cache").note_reused(path);
                consts.extend(body.iter().cloned());
                consts.sort_by(|a, b| a.0.cmp(&b.0));
                if !consts.is_empty() {
                    result.insert(*module_id, ModulePreResolvedConsts::new(*module_id, consts));
                }
                continue;
            }
        }

        let expr_types = single_typecheck.expr_types(db);
        let call_targets = single_typecheck.call_targets(db);

        // Get lowered functions for this module (if any).
        let (funcs, func_map): (&[Arc<IrCodeUnit>], HashMap<String, FuncId>) = match lowered_functions.get(module_id) {
            Some(lf) => (lf.functions.as_slice(), lf.func_name_to_id.iter().cloned().collect()),
            None => (&[], HashMap::new()),
        };

        // Evaluate function-level consts. A function's const parameters have a
        // value per instantiation rather than one, so the consts naming them
        // wait for the copies.
        let mut env = ConstEvalEnv {
            db, expr_types, call_targets, evaluator, lowered: funcs,
            func_name_to_id: &func_map, func_id_map, callable: Some(callable),
            data_files: single_typecheck.data_files(db),
        };
        let mut body = Vec::new();
        for statement in parsed.statements.iter() {
            if let Statement::Fun(func_stmt) = statement {
                let func_name = func_stmt.name(db).text(db);
                let const_params = func_stmt.params(db).iter()
                    .filter(|p| p.is_comptime)
                    .map(|p| p.name.text(db).S())
                    .collect();
                let (body_consts, body_errors) =
                    evaluate_body_consts(&mut env, func_stmt, module_level.clone(), const_params);
                for (name, ir_type, value) in body_consts {
                    // Stored under a qualified name: func_name::const_name.
                    body.push((format!("{}::{}", func_name, name), ir_type, SharedConst(value)));
                }
                errors.extend(body_errors.iter().map(|e| format!("{}::{}", func_name, e)));
            }
        }
        body.sort_by(|a, b| a.0.cmp(&b.0));

        if let Some(cache) = cache.as_deref_mut() {
            match cached {
                Some(cached) => {
                    assert_eq!(cached, body, "the const cache kept stale function consts for {}", path);
                    cache.note_reused(path);
                }
                None => {
                    cache.note_evaluated(path);
                    if errors.is_empty() && !module_level_errors.contains_key(module_id) {
                        cache.store_body(path, body.clone());
                    }
                }
            }
        }
        consts.extend(body);

        // Sorted, because the module-level ones above were read out of a
        // `HashMap` and this ends up in `lower_module`'s memo key. Two freshly
        // built `HashMap`s do not iterate the same way even in one process --
        // `RandomState::new` increments a per-thread counter, so every map
        // instance hashes differently -- so an unchanged recompile was handing
        // `lower_module` a key it had never seen and re-assembling the modules
        // that declare consts. Nothing downstream reads this in order.
        consts.sort_by(|a, b| a.0.cmp(&b.0));

        if !consts.is_empty() || !errors.is_empty() {
            result.insert(*module_id, ModulePreResolvedConsts::with_errors(*module_id, consts, errors));
        }
    }

    result
}

/// Whether a module declares a const at module level, and whether it declares
/// one anywhere -- at module level or inside a function body.
///
/// Per module, because the two graph-wide answers below are asked on every
/// compile and a body edit would otherwise make them walk every statement in
/// the program to say "no" again. Keyed on the module, so an edit re-reads one.
#[salsa::tracked(returns(copy))]
fn module_const_kinds<'db>(
    db: &'db dyn salsa::Database,
    module: Module<'db>,
) -> (bool, bool) {
    let parsed = &crate::module_graph::parse_module_full(db, module).parsed;
    let mut module_level = false;
    let mut anywhere = false;
    for statement in parsed.statements.iter() {
        match statement {
            Statement::Const(_) => {
                module_level = true;
                anywhere = true;
            }
            Statement::Fun(func) => {
                anywhere |= func.body(db).iter().any(|s| matches!(s, Statement::Const(_)));
            }
            _ => {}
        }
    }
    (module_level, anywhere)
}

/// True if any module in the graph declares a const at module level.
#[salsa::tracked(returns(copy))]
fn module_graph_has_module_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
) -> bool {
    // `graph` rather than `statements_only`: the graph is the identity field
    // and reading it is not a dependency on every module's statements, which
    // is the whole point of asking this per module.
    parsed_graph.graph(db).iter_modules(db)
        .any(|module| module_const_kinds(db, module).0)
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
    evaluator: &mut dyn CtfeEvaluator,
    lowered_functions: &HashMap<ModuleId<'db>, Arc<ModuleLoweredFunctions>>,
    callable: &ModuleFunctionRegistry,
    func_id_map: &FuncIdLookup<'db>,
    errors_out: &mut HashMap<ModuleId<'db>, Vec<String>>,
    mut cache: Option<&mut ConstCache>,
) -> HashMap<ModuleId<'db>, HashMap<String, (IrType, Arc<ConstValue>)>> {
    let typecheck_module_results = typecheck_result.module_results(db);
    let mut result = HashMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let Some(single_typecheck) = typecheck_module_results.get(module_id) else {
            continue;
        };
        let path = module_id.path(db);
        let cached = cache.as_deref().and_then(|c| c.module_level(path)).cloned();
        let verifying = cache.as_deref().is_some_and(ConstCache::verifying);
        if let Some(consts) = &cached {
            if !verifying {
                cache.as_deref_mut().expect("a hit came from the cache").note_reused(path);
                if !consts.is_empty() {
                    result.insert(*module_id, consts.clone());
                }
                continue;
            }
        }

        let expr_types = single_typecheck.expr_types(db);
        let call_targets = single_typecheck.call_targets(db);

        let (funcs, func_map): (&[Arc<IrCodeUnit>], HashMap<String, FuncId>) = match lowered_functions.get(module_id) {
            Some(lf) => (lf.functions.as_slice(), lf.func_name_to_id.iter().cloned().collect()),
            None => (&[], HashMap::new()),
        };

        let mut env = ConstEvalEnv {
            db, expr_types, call_targets, evaluator, lowered: funcs,
            func_name_to_id: &func_map, func_id_map, callable: Some(callable),
            data_files: single_typecheck.data_files(db),
        };
        let mut consts: HashMap<String, (IrType, Arc<ConstValue>)> = HashMap::new();
        let mut failed = false;
        for statement in parsed.statements.iter() {
            if let Statement::Const(const_stmt) = statement {
                // A module const is outside any function, so there is no return
                // type for an early-return operator to check against, and no
                // const parameters for it to name.
                let name = const_stmt.name.text(db).S();
                let evaluated = const_type(&env, const_stmt.value).and_then(|ty| {
                    evaluate_const(&mut env, const_stmt.value, &ty, &consts, None, &Default::default())
                        .map(|value| (ty, value.expect("nothing is deferred at module level")))
                });
                match evaluated {
                    Ok((ir_type, value)) => {
                        consts.insert(name, (ir_type, value));
                    }
                    Err(error) => {
                        failed = true;
                        errors_out.entry(*module_id).or_default()
                            .push(NamedConstError { name, error }.to_string());
                    }
                }
            }
        }

        if let Some(cache) = cache.as_deref_mut() {
            match cached {
                Some(cached) => {
                    assert_eq!(cached, consts, "the const cache kept stale module consts for {}", path);
                    cache.note_reused(path);
                }
                None => {
                    cache.note_evaluated(path);
                    if !failed {
                        cache.store_module_level(path, consts.clone());
                    }
                }
            }
        }

        if !consts.is_empty() {
            result.insert(*module_id, consts);
        }
    }

    result
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
    evaluator: &mut dyn CtfeEvaluator,
    skip_const_inlining: bool,
    skip_specialization: bool,
    mut const_cache: Option<&mut ConstCache>,
) -> ModuleGraphLoweringResult<'db> {
    let db_salsa = db.as_salsa_db();
    // Keying every module costs a little, and a graph with no consts in it
    // runs neither const phase, so there is nothing to key them for.
    if let Some(cache) = const_cache.as_deref_mut() {
        if graph_declares_consts(db_salsa, parsed_graph) {
            cache.begin(db_salsa, parsed_graph.graph(db_salsa));
        } else {
            cache.begin_without_consts();
        }
    }

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
    //
    // What the first stratum does have is the data consts, which need no
    // evaluating; see `data_consts`.
    let typecheck_module_results = typecheck_result.module_results(db_salsa);
    let data_consts: HashMap<ModuleId<'db>, HashMap<String, (IrType, Arc<ConstValue>)>> =
        parsed_graph.statements_only(db_salsa).iter()
            .filter_map(|(module_id, parsed)| {
                let single = typecheck_module_results.get(module_id)?;
                let consts = crate::const_eval::data_consts(
                    db_salsa, &parsed.statements, single.expr_types(db_salsa), single.data_files(db_salsa));
                (!consts.is_empty()).then_some((*module_id, consts))
            })
            .collect();
    let mut deferred: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
    let first_stratum = lower_all_module_functions(
        db, parsed_graph, typecheck_result, ownership_analysis, func_id_map,
        &data_consts, &mut deferred, None, mode,
    );

    // Phase 5a/b boundary is where every module function is lowered, so this is
    // where a function's descriptor shapes stop being only what its own body
    // builds. See `shape_closure`: a function handing its type parameter to one
    // that builds a collection of it has to be handed a descriptor too.
    //
    // Done before anything reads a signature, because the shapes are part of
    // one: they say what trailing arguments a call has to pass.
    let closed = close_shapes_over_calls(db_salsa, parsed_graph, func_id_map, first_stratum);
    let mut lowered_modules = closed.modules.clone();
    let mut shape_errors = closed.errors.clone();

    // Phase 5a/b boundary: evaluate module-level consts against what is lowered
    // so far. Done even when const inlining is skipped, since a module const is
    // resolved when its reference is lowered rather than by a later pass, so
    // there is nothing for that flag to skip.
    let mut module_const_errors: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
    let module_consts = if deferred.is_empty() && !module_graph_has_module_consts(db_salsa, parsed_graph) {
        HashMap::new()
    } else {
        let lowered_functions = ir_map(db_salsa, &lowered_modules);
        let func_ids = func_id_lookup(db_salsa, func_id_map);
        let module_registry = ctfe_module_registry(db_salsa, parsed_graph, lowered_modules.clone(), func_id_map);
        evaluator.set_module_registry(Arc::clone(module_registry));
        evaluate_module_level_consts(
            db_salsa, parsed_graph, typecheck_result, evaluator, &lowered_functions, module_registry,
            func_ids, &mut module_const_errors, const_cache.as_deref_mut(),
        )
    };

    // Second stratum: the functions that were waiting on those values.
    if !deferred.is_empty() {
        let mut still_deferred: HashMap<ModuleId<'db>, Vec<String>> = HashMap::new();
        let second = lower_all_module_functions(
            db, parsed_graph, typecheck_result, ownership_analysis, func_id_map,
            &module_consts, &mut still_deferred, Some(&deferred), mode,
        );
        for (module_id, names) in &still_deferred {
            for name in names {
                module_const_errors.entry(*module_id).or_default().push(format!(
                    "function '{}' names a module const that could not be evaluated",
                    name
                ));
            }
        }
        lowered_modules = merge_strata(db_salsa, lowered_modules, second);

        // And close the shapes again, over everything now that the second
        // stratum is in.
        //
        // The first pass saw only what was lowered before the consts were
        // evaluated. A function that names a module const is lowered here
        // instead, and one of those calling a generic that builds a collection
        // was left without the descriptor the call has to pass -- the callee
        // then looked for a shape its frame had never been given and brought
        // the interpreter down on "a shape built with is one this function
        // declared".
        //
        // Closing again rather than only over the new ones, because a shape
        // reaches whatever calls into it: a second-stratum function is a caller
        // the first pass did not know about.
        let closed = close_shapes_over_calls(db_salsa, parsed_graph, func_id_map, lowered_modules);
        lowered_modules = closed.modules.clone();
        shape_errors.extend(closed.errors.iter().cloned());
    }

    // Phase 5b: Evaluate consts (skip if skip_const_inlining is enabled, or if
    // no module declares a const at all -- then there is nothing to evaluate,
    // and building the registry to evaluate it with is pure cost).
    let resolved_consts = if skip_const_inlining || !graph_declares_consts(db_salsa, parsed_graph) {
        HashMap::new()
    } else {
        // Build a module registry from lowered functions for cross-module CTFE calls.
        let lowered_functions = ir_map(db_salsa, &lowered_modules);
        let func_ids = func_id_lookup(db_salsa, func_id_map);
        let module_registry = ctfe_module_registry(db_salsa, parsed_graph, lowered_modules.clone(), func_id_map);
        evaluator.set_module_registry(Arc::clone(module_registry));

        evaluate_all_module_consts(
            db_salsa, parsed_graph, typecheck_result, evaluator, &lowered_functions,
            module_registry, func_ids, &module_consts, &module_const_errors, const_cache.as_deref_mut(),
        )
    };

    // Module const failures have to reach the lowering result, or a module
    // whose const could not be evaluated compiles as though the functions that
    // name it were never written.
    let mut resolved_consts = resolved_consts;

    // Phase 5c: Specialize const parameter functions by monomorphization. Each
    // function with const parameters gains a copy per instantiation, and the
    // call sites that named one are pointed at it.
    let (lowered_modules, specialize_errors) = if skip_specialization {
        (lowered_modules, Vec::new())
    } else {
        specialize_module_graph(
            db_salsa, parsed_graph, typecheck_result, evaluator, func_id_map,
            &module_consts, lowered_modules,
        )
    };

    // Module const failures have to reach the lowering result, or a module
    // whose const could not be evaluated compiles as though the functions that
    // name it were never written. Specialization failures travel the same way.
    for (module_id, errors) in module_const_errors.into_iter()
        .chain(shape_errors)
        .chain(specialize_errors)
    {
        resolved_consts
            .entry(module_id)
            .or_insert_with(|| ModulePreResolvedConsts::new(module_id, Vec::new()))
            .errors
            .extend(errors);
    }
    let resolved_consts = resolved_consts;

    // Phase 5d: Assemble modules with lowered functions, then inline consts.
    // CTFE errors from resolved_consts are included in lower_module via pre_resolved_consts.errors.
    //
    // `lower_module` is tracked, so it takes its arguments by value. The IR
    // travels as a `ModuleLowered` handle, so neither the parallel path's work
    // items nor the aggregation that follows them copies an instruction, and
    // the memo key does not hash one either.
    let lowered_modules: HashMap<ModuleId<'db>, ModuleLowered<'db>> =
        lowered_modules.into_iter().collect();
    match mode {
        ParallelMode::Sequential => {
            assemble_module_graph(db_salsa, parsed_graph, typecheck_result, ownership_analysis, &resolved_consts, &lowered_modules, skip_const_inlining)
        }
        ParallelMode::Parallel => {
            assemble_module_graph_parallel(db, parsed_graph, typecheck_result, ownership_analysis, &resolved_consts, &lowered_modules, skip_const_inlining)
        }
    }
}

/// The two lowering strata for one module, as one module's IR.
///
/// Tracked, so an unchanged recompile takes the merge out of a memo rather
/// than concatenating and re-sorting every module's functions. Keyed on the
/// two handles, which is two words.
#[salsa::tracked(returns(copy))]
fn merge_module_strata<'db>(
    db: &'db dyn salsa::Database,
    first: ModuleLowered<'db>,
    second: ModuleLowered<'db>,
) -> ModuleLowered<'db> {
    let mut merged = first.functions(db).as_ref().clone();
    merged.functions.extend(second.functions(db).functions.iter().cloned());
    // Keep the IR in the order the ids were assigned, so the stratum a
    // function landed in does not show in the output.
    merged.functions.sort_by_key(|f| f.id.0);
    ModuleLowered::new(db, first.module_id(db), Arc::new(merged))
}

/// Fold the second lowering stratum into the first, module by module.
///
/// A module the second stratum reached but the first did not keeps the second
/// stratum's handle; one that both reached is merged. The result is in the
/// first stratum's order, with anything only the second stratum found after
/// it, so it is still graph order.
fn merge_strata<'db>(
    db: &'db dyn salsa::Database,
    first: LoweredModules<'db>,
    second: LoweredModules<'db>,
) -> LoweredModules<'db> {
    let mut second: HashMap<ModuleId<'db>, ModuleLowered<'db>> = second.into_iter().collect();
    let mut merged: LoweredModules<'db> = first.into_iter()
        .map(|(module_id, first)| match second.remove(&module_id) {
            Some(second) => (module_id, merge_module_strata(db, first, second)),
            None => (module_id, first),
        })
        .collect();

    let mut only_second: LoweredModules<'db> = second.into_iter().collect();
    only_second.sort_by(|a, b| a.0.path(db).cmp(b.0.path(db)));
    merged.extend(only_second);
    merged
}

/// True if any module declares a const anywhere, at module level or in a body.
///
/// Phase 5b has nothing to do for a program with no consts in it, and the
/// registry it builds to have CTFE on hand is not free.
#[salsa::tracked(returns(copy))]
fn graph_declares_consts<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
) -> bool {
    parsed_graph.graph(db).iter_modules(db)
        .any(|module| module_const_kinds(db, module).1)
}


/// True if any of this module's functions makes a comptime call.
///
/// The gate in front of phase 5c. Working out the plan means reading every
/// instruction of every function, and for a program that names no const
/// parameter anywhere the answer is always that there is nothing to do. Keyed
/// on the handle, so an unchanged recompile answers from a memo.
#[salsa::tracked(returns(copy))]
fn module_has_comptime_calls<'db>(
    db: &'db dyn salsa::Database,
    lowered: ModuleLowered<'db>,
) -> bool {
    use datalove_datafun_ir::Instruction;

    lowered.functions(db).functions.iter().any(|unit| {
        unit.blocks.iter().any(|block| {
            block.instructions.iter()
                .any(|instr| matches!(instr, Instruction::ComptimeCall { .. }))
        })
    })
}

/// One module's IR under a handle, once specialization has rewritten it.
///
/// This is the one place a handle is minted from the IR rather than from
/// another handle, so it is the one place the key holds a module's
/// instructions. Phase 5d used to pay that for every module on every compile;
/// now only a program that actually specializes something does, and it pays it
/// once instead.
#[salsa::tracked(returns(copy))]
fn specialized_module<'db>(
    db: &'db dyn salsa::Database,
    module_id: ModuleId<'db>,
    functions: Arc<ModuleLoweredFunctions>,
) -> ModuleLowered<'db> {
    ModuleLowered::new(db, module_id, functions)
}

/// Phase 5c over the whole graph, in and out under handles.
fn specialize_module_graph<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    evaluator: &mut dyn CtfeEvaluator,
    func_id_map: FuncIdMap<'db>,
    module_consts: &HashMap<ModuleId<'db>, HashMap<String, (IrType, Arc<ConstValue>)>>,
    modules: LoweredModules<'db>,
) -> (LoweredModules<'db>, Vec<(ModuleId<'db>, Vec<String>)>) {
    if !modules.iter().any(|(_, lowered)| module_has_comptime_calls(db, *lowered)) {
        return (modules, Vec::new());
    }

    // What the instantiations' consts can call. The same memo phase 5b asked
    // for, when it ran, and given to the evaluator in case it did not.
    let callable = ctfe_module_registry(db, parsed_graph, modules.clone(), func_id_map);
    evaluator.set_module_registry(Arc::clone(callable));

    let (lowered_functions, errors) = specialize_comptime_functions(
        db, parsed_graph, typecheck_result, evaluator, callable, func_id_lookup(db, func_id_map),
        module_consts, ir_map(db, &modules),
    );

    let specialized = modules.iter()
        .map(|(module_id, _)| {
            let ir = Arc::clone(&lowered_functions[module_id]);
            (*module_id, specialized_module(db, *module_id, ir))
        })
        .collect();

    (specialized, errors)
}

/// Specialize functions with const parameters by monomorphization.
///
/// Every instantiation named by a call site in the module graph gets a copy of
/// the callee with the const parameters substituted away, and the call site is
/// pointed at it. The original is kept, so a call site this pass cannot see --
/// a script unit's, which compiles later -- still calls something that takes
/// the const argument.
///
/// Returns the errors specialization could not resolve, per module.
#[allow(clippy::too_many_arguments)]
fn specialize_comptime_functions<'db>(
    db: &'db dyn salsa::Database,
    parsed_graph: ParsedModuleGraph<'db>,
    typecheck_result: ModuleGraphTypecheckResult<'db>,
    evaluator: &mut dyn CtfeEvaluator,
    callable: &ModuleFunctionRegistry,
    func_id_map: &FuncIdLookup<'db>,
    module_consts: &HashMap<ModuleId<'db>, HashMap<String, (IrType, Arc<ConstValue>)>>,
    mut lowered_functions: HashMap<ModuleId<'db>, Arc<ModuleLoweredFunctions>>,
) -> (HashMap<ModuleId<'db>, Arc<ModuleLoweredFunctions>>, Vec<(ModuleId<'db>, Vec<String>)>) {
    // Modules paired with their `IrModuleId`, which is also the order the copies
    // are numbered in. Taken from `ir_module_ids` rather than from a position in
    // the graph, so that this cannot disagree with what a call site was compiled
    // to expect.
    let graph = typecheck_result.graph(db);
    let module_ids = ir_module_ids(db, parsed_graph);
    let modules: Vec<(ModuleId<'db>, IrModuleId)> = graph.iter_modules(db)
        .map(|module| {
            let id = module.id(db);
            (id, *module_ids.get(&id).expect("every module in the graph is numbered"))
        })
        .collect();

    // The source of each comptime function, for evaluating its consts once the
    // instantiation is known.
    let mut func_asts: HashMap<(ModuleId<'db>, String), datalove_datafun_ast::ast::StmtFun<'db>> =
        HashMap::new();
    for (module_id, parsed) in parsed_graph.statements_only(db) {
        for statement in parsed.statements.iter() {
            if let Statement::Fun(func_stmt) = statement {
                func_asts.insert((*module_id, func_stmt.name(db).text(db).S()), *func_stmt);
            }
        }
    }
    let typecheck_module_results = typecheck_result.module_results(db);

    // Build the copies. Ids follow the source-derived ones, which
    // `compute_func_id_map` assigns from statement order and cannot assign here
    // because these functions are in nobody's source.
    //
    // A round at a time, because a comptime function only says what it passes
    // to another one once its own const parameters have been substituted, so
    // copying can uncover instantiations the scan before it could not see.
    let mut plan = MonomorphizationPlan::default();
    let mut errors: Vec<(ModuleId<'db>, Vec<String>)> = Vec::new();

    for _ in 0..MAX_ROUNDS {
        for (module_id, _) in &modules {
            let Some(module_funcs) = lowered_functions.get(module_id) else {
                continue;
            };
            for func in &module_funcs.functions {
                collect_instantiations_into(&mut plan, func, &module_callee_key);
            }
        }
        if plan.is_empty() {
            return (lowered_functions, Vec::new());
        }

        let mut made_any = false;
        for (module_id, ir_module_id) in &modules {
            let Some(module_funcs) = lowered_functions.get_mut(module_id) else {
                continue;
            };
            let module_funcs = Arc::make_mut(module_funcs);

            let mut next_id = module_funcs.functions.iter()
                .map(|f| f.id.0 + 1)
                .max()
                .unwrap_or(0);
            let mut module_errors = Vec::new();
            let mut copies = Vec::new();

            for (callee, mono) in plan.funcs.iter_mut() {
                let CalleeKey::Module(plan_module, callee_id) = callee else {
                    continue;
                };
                if plan_module != ir_module_id {
                    continue;
                }
                let Some(original) = module_funcs.functions.iter().find(|f| f.id == *callee_id) else {
                    continue;
                };

                if let Some(error) = over_limit(&original.name, mono) {
                    module_errors.push(error);
                    continue;
                }

                for values in mono.instantiations.iter().skip(mono.copies.len()) {
                    let new_id = CodeUnitId(next_id);
                    next_id += 1;
                    let mut copy = monomorphize_function(
                        original,
                        &mono.comptime_param_indices,
                        values,
                        new_id,
                        format!("{}__ct{}", original.name, new_id.0),
                    );

                    // Now that the parameters have values, the body's consts
                    // have one each, so they are evaluated and written in.
                    if let (Some(func_stmt), Some(single_typecheck)) = (
                        func_asts.get(&(*module_id, original.name.clone())),
                        typecheck_module_results.get(module_id),
                    ) {
                        let func_name_to_id = module_funcs.func_name_to_id.iter().cloned().collect();
                        let mut env = ConstEvalEnv {
                            db,
                            expr_types: single_typecheck.expr_types(db),
                            call_targets: single_typecheck.call_targets(db),
                            evaluator,
                            lowered: &module_funcs.functions,
                            func_name_to_id: &func_name_to_id,
                            func_id_map,
                            callable: Some(callable),
                            data_files: single_typecheck.data_files(db),
                        };
                        let (evaluated, const_errors) = evaluate_instantiation_consts(
                            &mut env,
                            func_stmt,
                            module_consts.get(module_id).cloned().unwrap_or_default(),
                            &mono.comptime_param_indices,
                            values,
                        );
                        module_errors.extend(const_errors);
                        inline_function_consts(&mut copy, &evaluated);
                    }

                    copies.push(copy);
                    mono.copies.push(CodeRef::Module { module: *ir_module_id, id: new_id });
                }
            }

            made_any |= !copies.is_empty();
            for copy in copies {
                module_funcs.func_name_to_id.push((copy.name.clone(), FuncId(copy.id.0)));
                module_funcs.functions.push(Arc::new(copy));
            }
            module_funcs.functions.sort_by_key(|f| f.id.0);
            module_funcs.func_name_to_id.sort_by(|a, b| a.0.cmp(&b.0));

            if !module_errors.is_empty() {
                errors.push((*module_id, module_errors));
            }
        }

        if !made_any {
            break;
        }
    }

    // Point the call sites at the copies.
    for (module_id, _) in &modules {
        let Some(module_funcs) = lowered_functions.get_mut(module_id) else {
            continue;
        };
        let module_funcs = Arc::make_mut(module_funcs);
        module_funcs.functions = module_funcs.functions.iter()
            .map(|func| Arc::new(rewrite_comptime_calls(func, &plan, &module_callee_key)))
            .collect();
    }

    (lowered_functions, errors)
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
    lowered_modules: &HashMap<ModuleId<'db>, ModuleLowered<'db>>,
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

    let module_ids = ir_module_ids(db, parsed_graph);
    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let ir_module_id = *module_ids.get(module_id).expect("every parsed module is numbered");

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
        let module_funcs = lowered_modules.get(module_id).copied();

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
    lowered_modules: &HashMap<ModuleId<'db>, ModuleLowered<'db>>,
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
    let module_ids = ir_module_ids(db_salsa, parsed_graph);
    let work: Vec<_> = parsed_graph.statements_only(db_salsa)
        .iter()
        .filter_map(|(module_id, parsed)| {
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
            let module_funcs = lowered_modules.get(module_id).copied();

            Some((
                db.dyn_clone(),
                module,
                *module_ids.get(module_id).expect("every parsed module is numbered"),
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
    assemble_module_graph(db_salsa, parsed_graph, typecheck_result, ownership_analysis, resolved_consts, lowered_modules, skip_const_inlining)
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
            .entries(db)
            .iter()
            .map(|((_, name), _)| name.clone())
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
        ).entries(&db).clone();

        // The same graph plus an unrelated module, which lands after `a` and
        // so does not disturb its IR module index either.
        let mut builder = ModuleGraphBuilder::new(&db);
        let a2 = builder.add_module("local/test/a", source("// a"));
        let _b = builder.add_module("local/test/b", source("// b"));
        let graph_after = builder.build();
        let after = reachable_func_ids(
            &db, graph_after, test_func_id_map(&db, graph_after), a2,
        ).entries(&db).clone();

        assert_eq!(a, a2, "the path is the identity, so these are one module");
        assert_eq!(
            before, after,
            "adding an unrelated module must not change what `a` can reach",
        );
    }
}
