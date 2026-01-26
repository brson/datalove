//! Public API for datafun typechecking.
//!
//! Provides salsa-tracked entry points and public functions for typechecking
//! scripts, expressions, and module graphs.

use rmx::prelude::*;
use std::collections::{HashMap, BTreeMap};
use bct::text::InternedText;
use datalove_ct::query_log::{log_query, QueryPhase};

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::TypeContext;
use crate::statement::check_statement;
use crate::types::{convert_type_hint_with_aliases, unit_type};

pub use datalove_datafun_ast::spans::DatafunSpans;
pub use bct::module_graph::ModuleId;

pub use crate::{
    PendingDiagnostic,
    Type,
    TypeFunction,
    TypeError,
    TypeErrorEntry,
    ResolvedCallTarget,
    ScriptUnitKind,
    ScriptUnitSpec,
    ScriptBatchSpec,
    ModuleSpec,
    UnitTypecheckResultTracked,
    ScriptUnitsTypecheckResultTracked,
    ParsedModuleGraph,
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
    SingleModuleTypecheckResult,
};

/// Accumulated bindings passed to subsequent script units.
///
/// Plain data struct (not tracked) - compared by Salsa via Eq/Hash.
/// Contains bindings from all prior units in the batch.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
#[derive(salsa::Update)]
pub struct AccumulatedBindings<'db> {
    /// Variables: (name, type, is_mutable).
    pub vars: Vec<(InternedText<'db>, Type<'db>, bool)>,
    pub fns: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    pub fn_asts: Vec<(InternedText<'db>, StmtFun<'db>, Option<ModuleId>)>,
}

/// Output from typecheck_script_unit.
///
/// Includes the typecheck result AND new bindings defined in this unit
/// for accumulation by the caller.
#[salsa::tracked]
pub struct ScriptUnitTypecheckOutput<'db> {
    pub result: UnitTypecheckResultTracked<'db>,
    /// Variables: (name, type, is_mutable).
    #[returns(ref)]
    pub new_vars: Vec<(InternedText<'db>, Type<'db>, bool)>,
    #[returns(ref)]
    pub new_fns: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    #[returns(ref)]
    pub new_fn_asts: Vec<(InternedText<'db>, StmtFun<'db>, Option<ModuleId>)>,
}

/// Create a ScriptBatchSpec inside a tracked function.
///
/// ScriptBatchSpec is a tracked type, so it must be created inside a tracked function.
/// The Source parameter serves as the memoization key.
#[salsa::tracked]
pub fn create_batch_spec<'db>(
    db: &'db dyn crate::Db,
    key: bct::input::Source,
    unit_specs: Vec<ScriptUnitSpec<'db>>,
    module_specs: Vec<ModuleSpec<'db>>,
) -> ScriptBatchSpec<'db> {
    let _ = key; // Used as memoization key.
    ScriptBatchSpec::new(db, unit_specs, module_specs)
}

/// Typecheck a single script unit with accumulated context from prior units.
///
/// Memoized: if unit_spec, module_specs, and accumulated all match a previous call,
/// returns the cached result. This enables per-unit caching when adding new units
/// to a batch - prior units are cache hits.
#[salsa::tracked]
pub fn typecheck_script_unit<'db>(
    db: &'db dyn crate::Db,
    unit_spec: ScriptUnitSpec<'db>,
    module_specs: Vec<ModuleSpec<'db>>,
    accumulated: AccumulatedBindings<'db>,
) -> ScriptUnitTypecheckOutput<'db> {
    // Build module function info for import resolution.
    let (module_functions, path_to_module_id) = build_script_module_functions(db, &module_specs);

    let spans = unit_spec.spans.clone();
    let mut ctx = TypeContext::new(db, spans);

    // Script units have Result<()> return type for try operators.
    let unit_tuple_ty = datalit::tycheck::Type::AnonTuple(datalit::tycheck::TypeAnonTuple { fields: Vec::new() });
    let result_unit_ty = datalit::tycheck::Type::Result(
        datalit::tycheck::TypeResult { inner_type: Box::new(unit_tuple_ty) }
    );
    ctx.expected_return_type = Some(Type::Datalit(result_unit_ty));

    // Seed with accumulated bindings from prior units.
    for (name, ty, is_mutable) in &accumulated.vars {
        ctx.add_variable(*name, ty.clone(), *is_mutable);
    }
    for (name, func_ty) in &accumulated.fns {
        ctx.add_function(*name, *func_ty);
    }
    for (name, func_ast, module_id) in &accumulated.fn_asts {
        ctx.function_asts.insert(*name, (*func_ast, *module_id));
    }

    // Track new bindings defined in this unit.
    let mut new_vars = Vec::new();
    let mut new_fns = Vec::new();
    let mut new_fn_asts = Vec::new();

    // Typecheck this unit based on kind.
    match &unit_spec.kind {
        ScriptUnitKind::Fragment(script) => {
            // Name resolution: collect type aliases and function signatures.
            let collected = resolve_names_impl(db, &script.statements);
            ctx.seed_from_collected_names(&collected, None);

            // Resolve imports using shared helper.
            let (resolved_imports, import_errors) = resolve_script_imports(
                db, script, &module_functions, &path_to_module_id
            );

            // Add resolved imports to context and track as new bindings.
            for (item_name, func_ty, func_ast, source_module_id) in resolved_imports {
                ctx.add_function(item_name, func_ty);
                ctx.function_asts.insert(item_name, (func_ast, source_module_id));
                new_fns.push((item_name, func_ty));
                new_fn_asts.push((item_name, func_ast, source_module_id));
            }

            // Add import errors to context.
            for error in import_errors {
                ctx.add_error(error);
            }

            // Second pass: typecheck all statements.
            for statement in &script.statements {
                check_statement(&mut ctx, &statement);
            }

            // Extract new bindings for subsequent units.
            for stmt in &script.statements {
                match stmt {
                    Statement::Let(let_stmt) => {
                        let name = let_stmt.name;
                        if let Some((ty, is_mutable)) = ctx.variables.get(&name) {
                            new_vars.push((name, ty.clone(), *is_mutable));
                        }
                    }
                    Statement::Var(var_stmt) => {
                        let name = var_stmt.name;
                        if let Some((ty, is_mutable)) = ctx.variables.get(&name) {
                            new_vars.push((name, ty.clone(), *is_mutable));
                        }
                    }
                    Statement::Fun(fun_stmt) => {
                        let name = fun_stmt.name(db);
                        if let Some(func_ty) = ctx.functions.get(&name) {
                            new_fns.push((name, *func_ty));
                            new_fn_asts.push((name, *fun_stmt, None));
                        }
                    }
                    _ => {}
                }
            }
        }
        ScriptUnitKind::Expr(expr) => {
            // Expression unit - just typecheck the expression.
            if let Err(e) = ctx.synthesize_expr(*expr) {
                ctx.add_error(e);
            }
        }
    }

    // Emit pending diagnostics with local spans.
    ctx.emit_pending_diagnostics();

    // Build result for this unit.
    let errors = ctx.errors.into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();
    let function_types: Vec<_> = ctx.functions.into_iter().collect();
    let result = UnitTypecheckResultTracked::new(db, errors, ctx.expr_types, ctx.call_targets, function_types);

    ScriptUnitTypecheckOutput::new(db, result, new_vars, new_fns, new_fn_asts)
}

/// Typecheck multiple script units together, with bindings shared across units.
///
/// Units are processed in order. Bindings from earlier units (let/var/fn)
/// are visible in subsequent units. Delegates to `typecheck_script_unit` for
/// per-unit memoization - prior units are cached when new units are added.
#[salsa::tracked]
pub fn type_check_script_units<'db>(
    db: &'db dyn crate::Db,
    spec: ScriptBatchSpec<'db>,
) -> ScriptUnitsTypecheckResultTracked<'db> {
    let module_specs = spec.modules(db).clone();

    let mut accumulated = AccumulatedBindings::default();
    let mut results = Vec::new();

    for unit_spec in spec.units(db) {
        // Call per-unit tracked function - memoized on (unit_spec, module_specs, accumulated).
        let output = typecheck_script_unit(
            db,
            unit_spec.clone(),
            module_specs.clone(),
            accumulated.clone(),
        );

        results.push(output.result(db));

        // Only accumulate bindings from successful units.
        // Failed units shouldn't export bindings to subsequent units.
        if output.result(db).errors(db).is_empty() {
            accumulated.vars.extend(output.new_vars(db).iter().cloned());
            accumulated.fns.extend(output.new_fns(db).iter().cloned());
            accumulated.fn_asts.extend(output.new_fn_asts(db).iter().cloned());
        }
    }

    ScriptUnitsTypecheckResultTracked::new(db, results)
}

/// Typecheck a single script using the production path.
///
/// Wraps `type_check_script_units()` for tests that typecheck a single script.
pub fn type_check_single_script<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    spans: DatafunSpans,
    parsed: ParsedStatements<'db>,
) -> UnitTypecheckResultTracked<'db> {
    let unit_spec = ScriptUnitSpec::new(source, spans, ScriptUnitKind::Fragment(parsed));
    let batch_spec = create_batch_spec(db, source, vec![unit_spec], vec![]);
    let results = type_check_script_units(db, batch_spec);
    results.results(db)[0]
}

/// Typecheck a module graph (package-agnostic).
///
/// This is the core typechecking function that works with the package-agnostic
/// ModuleGraph abstraction. Modules are processed in dependency order.
use bct::module_graph::Module;

/// Resolved import data as plain tuple (not tracked).
///
/// Contains: (local_name, func_type, func_ast, source_module)
pub type ResolvedImportData<'db> = (InternedText<'db>, TypeFunction<'db>, Option<StmtFun<'db>>, ModuleId);

/// Typecheck a single module with logging.
///
/// This is a tracked function so Salsa can observe per-module execution.
/// The logging only fires when the function actually executes.
///
/// Takes pre-resolved name resolution (type aliases, function signatures) and
/// resolved imports as plain data. Each module's typecheck only depends on its
/// own name resolution and imports, enabling proper per-module caching.
#[salsa::tracked]
pub fn typecheck_module<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed: ParsedStatements<'db>,
    spans: DatafunSpans,
    name_resolution: crate::ModuleNameResolution<'db>,
    resolved_imports: Vec<ResolvedImportData<'db>>,
    import_errors: Vec<TypeError>,
) -> SingleModuleTypecheckResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("typecheck", module_path, QueryPhase::Start);

    // Create type context for this module with module_id for pending diagnostics.
    let mut ctx = TypeContext::with_module_id(db, spans, Some(module_id));

    // Add import errors to context.
    for err in import_errors {
        ctx.add_error(err);
    }

    // Add imported functions to context.
    for (local_name, func_type, func_ast, source_module) in &resolved_imports {
        if let Some(ast) = func_ast {
            ctx.add_imported_function(*local_name, *func_type, *ast, *source_module);
        } else {
            ctx.add_function(*local_name, *func_type);
        }
    }

    // Seed context from pre-computed name resolution (replaces Pass 0/1).
    ctx.seed_from_name_resolution(&name_resolution, Some(module_id));

    // Pass 2: type check all statements.
    for statement in &parsed.statements {
        check_statement(&mut ctx, &statement);
    }

    // Use exports from name resolution (already computed).
    let exports = name_resolution.functions(db).clone();
    let type_aliases = name_resolution.type_aliases(db).clone();

    // Build imports list for result.
    let imports: Vec<_> = resolved_imports.iter()
        .map(|(local_name, _, _, source_module)| (*local_name, *source_module, *local_name))
        .collect();

    log_query("typecheck", module_path, QueryPhase::End);

    SingleModuleTypecheckResult::new(
        db,
        module_id,
        ctx.errors.C(),
        ctx.pending_diagnostics.C(),
        exports,
        type_aliases,
        imports,
        ctx.expr_types.C(),
        ctx.call_targets.C(),
    )
}

/// Prepared data for typechecking a module graph.
struct TypecheckPreparation<'db> {
    graph: bct::module_graph::ModuleGraph,
    module_parsed: HashMap<ModuleId, ParsedStatements<'db>>,
}

/// Prepare data structures needed for typechecking.
///
/// Builds module graph and parsed statements map.
/// This is shared between sequential and parallel typecheck paths.
fn prepare_typecheck<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> TypecheckPreparation<'db> {
    let graph = parsed_graph.graph(db);

    // Build parsed statements map.
    let module_parsed: HashMap<ModuleId, ParsedStatements<'db>> = parsed_graph.statements_only(db)
        .iter()
        .map(|(id, parsed)| (*id, parsed.C()))
        .collect();

    TypecheckPreparation {
        graph,
        module_parsed,
    }
}

/// Typecheck a module graph.
///
/// Resolves names (type aliases, function signatures) for all modules,
/// resolves imports for each module, then typechecks each module.
/// Each module's typecheck depends on its own name resolution and imports,
/// enabling per-module caching.
#[salsa::tracked]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    let prep = prepare_typecheck(db, parsed_graph);

    // ========================================================================
    // NAME RESOLUTION PASS (memoized per module)
    // ========================================================================
    let all_names = resolve_all_names(db, parsed_graph);

    // ========================================================================
    // TYPECHECK PASS
    // ========================================================================
    let mut module_errors: BTreeMap<ModuleId, Vec<TypeError>> = BTreeMap::new();
    let mut module_exports_map: BTreeMap<ModuleId, ModuleExports<'db>> = BTreeMap::new();
    let mut module_imports_map: BTreeMap<ModuleId, ModuleImports<'db>> = BTreeMap::new();
    let mut module_results_map: BTreeMap<ModuleId, SingleModuleTypecheckResult<'db>> = BTreeMap::new();
    let mut combined_expr_types: Vec<Option<Type<'db>>> = Vec::new();
    let mut combined_call_targets: Vec<Option<ResolvedCallTarget<'db>>> = Vec::new();

    for module in prep.graph.iter_modules(db) {
        let module_id = module.id(db);

        let parsed = prep.module_parsed.get(&module_id)
            .cloned()
            .expect("module should have been parsed");

        // Get pre-computed name resolution for this module.
        let name_resolution = *all_names.resolutions(db).get(&module_id)
            .expect("module should have name resolution");

        // Resolve imports for this module (tracked, memoized per module).
        let import_resolution = resolve_module_imports(db, module, parsed_graph);
        let resolved_imports = import_resolution.imports(db).C();
        let import_errors = import_resolution.errors(db).C();

        // Use empty spans inside tracked function.
        let spans = DatafunSpans::new(vec![]);

        // Call the tracked typecheck function.
        let result = typecheck_module(db, module, parsed, spans, name_resolution, resolved_imports, import_errors);

        // Collect errors from typecheck result.
        let errors = result.errors(db).C();
        if !errors.is_empty() {
            module_errors.insert(module_id, errors);
        }

        // Build exports.
        let exports = ModuleExports::new(db, module_id, result.exports(db).C(), result.exported_type_aliases(db).C());
        module_exports_map.insert(module_id, exports);

        // Build imports.
        let imports = ModuleImports::new(db, module_id, result.imports(db).C());
        module_imports_map.insert(module_id, imports);

        // Merge expr_types.
        let new_types = result.expr_types(db);
        if new_types.len() > combined_expr_types.len() {
            combined_expr_types.resize(new_types.len(), None);
        }
        for (i, ty) in new_types.iter().enumerate() {
            if ty.is_some() {
                combined_expr_types[i] = ty.clone();
            }
        }

        // Merge call_targets.
        let new_targets = result.call_targets(db);
        if new_targets.len() > combined_call_targets.len() {
            combined_call_targets.resize(new_targets.len(), None);
        }
        for (i, target) in new_targets.iter().enumerate() {
            if target.is_some() {
                combined_call_targets[i] = *target;
            }
        }

        // Process pending diagnostics with span enrichment.
        emit_pending_diagnostics_for_module(db, &parsed_graph, module_id, result.pending_diagnostics(db));

        // Store the per-module result for use by downstream phases.
        module_results_map.insert(module_id, result);
    }

    ModuleGraphTypecheckResult::new(db, prep.graph, module_errors, module_exports_map, module_imports_map, combined_expr_types, combined_call_targets, module_results_map)
}

/// Typecheck a module graph using parallel execution.
///
/// This function runs name resolution and typechecking in parallel using rayon to warm
/// salsa's memoization cache, then delegates to the tracked `typecheck_module_graph`
/// function. The tracked function will hit the warmed cache for all calls.
///
/// Requires `&dyn DbClone` to enable database cloning for parallel execution.
pub fn typecheck_module_graph_parallel<'db>(
    db: &'db dyn crate::DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    use rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let prep = prepare_typecheck(db_salsa, parsed_graph);

    // ========================================================================
    // PARALLEL NAME RESOLUTION (warms salsa cache)
    // ========================================================================
    let all_names = resolve_all_names_parallel(db, parsed_graph);

    // ========================================================================
    // PARALLEL TYPECHECK PASS (warms salsa cache)
    // ========================================================================

    // Clone databases upfront for parallel execution.
    let work: Vec<_> = prep.graph
        .iter_modules(db_salsa)
        .map(|module| {
            let module_id = module.id(db_salsa);
            let parsed = prep.module_parsed
                .get(&module_id)
                .cloned()
                .expect("module should have been parsed");
            let name_resolution = *all_names.resolutions(db_salsa).get(&module_id)
                .expect("module should have name resolution");
            (db.dyn_clone(), module, parsed, name_resolution)
        })
        .collect();

    // Typecheck modules in parallel - populates salsa's memoization cache.
    // Both resolve_module_imports and typecheck_module are tracked and cached.
    work.into_par_iter().for_each(|(db_clone, module, parsed, name_resolution)| {
        let db_salsa = db_clone.as_salsa_db();

        // Resolve imports (tracked, memoized per module).
        let import_resolution = resolve_module_imports(db_salsa, module, parsed_graph);
        let resolved_imports = import_resolution.imports(db_salsa).C();
        let import_errors = import_resolution.errors(db_salsa).C();

        // Typecheck (tracked, memoized per module).
        let spans = DatafunSpans::new(vec![]);
        let _ = typecheck_module(db_salsa, module, parsed, spans, name_resolution, resolved_imports, import_errors);
    });

    // Delegate to tracked function which aggregates results.
    // All resolve_module_names, resolve_module_imports, and typecheck_module calls will be cache hits.
    typecheck_module_graph(db_salsa, parsed_graph)
}

/// Typecheck module graph with configurable parallelism.
///
/// Use `ParallelMode::Sequential` for standard salsa-tracked behavior, or
/// `ParallelMode::Parallel` to enable rayon parallelization.
pub fn typecheck_module_graph_with_mode<'db>(
    db: &'db dyn crate::DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    mode: crate::ParallelMode,
) -> ModuleGraphTypecheckResult<'db> {
    match mode {
        crate::ParallelMode::Sequential => typecheck_module_graph(db.as_salsa_db(), parsed_graph),
        crate::ParallelMode::Parallel => typecheck_module_graph_parallel(db, parsed_graph),
    }
}

/// Emit pending diagnostics for a module using spans from the parsed module graph.
fn emit_pending_diagnostics_for_module<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: &ParsedModuleGraph<'db>,
    module_id: ModuleId,
    pending: &[PendingDiagnostic<'db>],
) {
    let span_lookup = crate::emit::ModuleGraphSpanLookup::new(parsed_graph, module_id);
    crate::emit::emit_pending_diagnostics(db, pending, &span_lookup);
}

// ============================================================================
// Name Resolution Pass
// ============================================================================

/// Collect function signatures exported from a module.
///
/// This is a tracked function so Salsa can cache per-module export collection.
/// Only depends on parsed statements, not on any other module.
#[salsa::tracked]
pub fn resolve_module_exports<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed: ParsedStatements<'db>,
) -> Vec<(InternedText<'db>, TypeFunction<'db>)> {
    let _ = module; // Used as memoization key.
    resolve_names_impl(db, &parsed.statements).functions
}

/// All module exports collected from the graph.
#[salsa::tracked]
pub struct AllModuleExports<'db> {
    #[returns(ref)]
    pub exports: BTreeMap<ModuleId, Vec<(InternedText<'db>, TypeFunction<'db>)>>,
}

/// Collect exports from all modules in the graph.
///
/// Calls the tracked `resolve_module_exports` for each module, enabling
/// per-module caching of export collection.
#[salsa::tracked]
pub fn resolve_all_exports<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleExports<'db> {
    let graph = parsed_graph.graph(db);
    let module_to_module_obj: HashMap<ModuleId, Module> = graph.iter_modules(db)
        .map(|m| (m.id(db), m))
        .collect();

    let mut all_exports = BTreeMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let module = module_to_module_obj.get(module_id)
            .expect("module should exist in graph");
        let exports = resolve_module_exports(db, *module, parsed.C());
        all_exports.insert(*module_id, exports);
    }

    AllModuleExports::new(db, all_exports)
}

/// All function ASTs collected from the graph.
#[salsa::tracked]
pub struct AllModuleFunctionAsts<'db> {
    #[returns(ref)]
    pub asts: BTreeMap<ModuleId, Vec<(InternedText<'db>, StmtFun<'db>)>>,
}

/// Build function AST maps for all modules.
///
/// Used for function inlining - maps module ID to function name to AST.
#[salsa::tracked]
pub fn build_all_function_ast_maps<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleFunctionAsts<'db> {
    let mut module_function_asts = BTreeMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let mut funcs = Vec::new();
        for statement in &parsed.statements {
            if let Statement::Fun(func) = statement {
                funcs.push((func.name(db), *func));
            }
        }
        module_function_asts.insert(*module_id, funcs);
    }

    AllModuleFunctionAsts::new(db, module_function_asts)
}

/// Resolve imports for a single module (memoized per module).
///
/// This is the tracked entry point for import resolution, following the same
/// pattern as `parse_module_full` and `typecheck_module`. The parallel path
/// calls this in parallel to warm the cache, then the sequential aggregation
/// path hits the cache.
#[salsa::tracked]
pub fn resolve_module_imports<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed_graph: ParsedModuleGraph<'db>,
) -> crate::ModuleImportResolution<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("resolve_imports", module_path, QueryPhase::Start);

    // Get prerequisites (these are cached at the graph level).
    let all_exports = resolve_all_exports(db, parsed_graph);
    let all_function_asts = build_all_function_ast_maps(db, parsed_graph);

    // Get parsed statements for this module.
    let parsed = parsed_graph.statements_only(db)
        .iter()
        .find(|(id, _)| *id == module_id)
        .map(|(_, p)| p.clone())
        .expect("module should have been parsed");

    // Call internal implementation.
    let (imports, errors) = resolve_module_imports_internal(
        db,
        module_id,
        &parsed,
        &parsed_graph,
        all_exports.exports(db),
        all_function_asts.asts(db),
    );

    log_query("resolve_imports", module_path, QueryPhase::End);

    crate::ModuleImportResolution::new(db, module_id, imports, errors)
}

/// Internal import resolution returning plain data (no tracked structs).
///
/// Returns tuples of (local_name, func_type, func_ast, source_module) instead of
/// ResolvedImport tracked structs. This allows the function to be called from
/// inside tracked functions without the "cannot create tracked struct outside
/// tracked function" issue.
fn resolve_module_imports_internal<'db>(
    db: &'db dyn crate::Db,
    module_id: ModuleId,
    parsed: &ParsedStatements<'db>,
    parsed_graph: &ParsedModuleGraph<'db>,
    all_exports: &BTreeMap<ModuleId, Vec<(InternedText<'db>, TypeFunction<'db>)>>,
    module_function_asts: &BTreeMap<ModuleId, Vec<(InternedText<'db>, StmtFun<'db>)>>,
) -> (Vec<(InternedText<'db>, TypeFunction<'db>, Option<StmtFun<'db>>, ModuleId)>, Vec<TypeError>) {
    // Build module alias map from pre-resolved requires.
    let resolved_requires = parsed_graph.get_requires(db, module_id);
    let alias_map: HashMap<InternedText<'db>, ModuleId> = resolved_requires.iter()
        .map(|(alias, target_id)| (*alias, *target_id))
        .collect();

    let mut resolved_imports = Vec::new();
    let mut import_errors = Vec::new();

    for statement in &parsed.statements {
        if let Statement::Import(import) = statement {
            let module_name = import.module_name;
            let item_name = import.item_name;

            if let Some(&source_module_id) = alias_map.get(&module_name) {
                // Look up the function in pre-computed exports.
                if let Some(exports) = all_exports.get(&source_module_id) {
                    let func_opt = exports.iter()
                        .find(|(name, _)| *name == item_name)
                        .map(|(_, func_type)| *func_type);

                    if let Some(func_type) = func_opt {
                        // Look up the function AST (linear search, n is small).
                        let func_ast = module_function_asts
                            .get(&source_module_id)
                            .and_then(|funcs| funcs.iter().find(|(n, _)| *n == item_name))
                            .map(|(_, ast)| *ast);

                        resolved_imports.push((item_name, func_type, func_ast, source_module_id));
                    } else {
                        import_errors.push(TypeError::UnresolvedName(
                            format!("{}.{}", module_name.as_str(db), item_name.as_str(db))
                        ));
                    }
                } else {
                    import_errors.push(TypeError::UnresolvedName(
                        format!("module {} (exports not found)", module_name.as_str(db))
                    ));
                }
            } else {
                import_errors.push(TypeError::UnresolvedName(
                    format!("module {} (not required)", module_name.as_str(db))
                ));
            }
        }
    }

    (resolved_imports, import_errors)
}

/// Collected names from statements (type aliases and optionally functions).
///
/// This is the core name resolution result used by both modules and scripts.
pub struct CollectedNames<'db> {
    /// Type aliases: (name, resolved_type).
    pub type_aliases: Vec<(InternedText<'db>, Type<'db>)>,
    /// Function signatures: (name, function_type). Empty if `collect_functions` was false.
    pub functions: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    /// Function ASTs for inlining: (name, ast). Empty if `collect_functions` was false.
    pub function_asts: Vec<(InternedText<'db>, StmtFun<'db>)>,
    /// Errors encountered during name resolution.
    pub errors: Vec<crate::TypeError>,
}

// ============================================================================
// Name Resolution Pass
// ============================================================================

/// Resolve names from parsed statements.
///
/// This is the core name resolution implementation used by both modules and scripts.
/// It collects type aliases and function signatures, enabling forward references
/// to functions defined later in the same compilation unit.
pub fn resolve_names_impl<'db>(
    db: &'db dyn crate::Db,
    statements: &[Statement<'db>],
) -> CollectedNames<'db> {
    use crate::types::is_primitive_name;

    let mut errors = Vec::new();

    // Pass 0: collect type aliases.
    let mut type_aliases_map: HashMap<InternedText<'db>, Type<'db>> = HashMap::new();
    let mut type_aliases_vec = Vec::new();

    for statement in statements {
        if let Statement::TypeAlias(stmt) = statement {
            let name = stmt.name;
            let name_str = name.as_str(db);

            // Check for shadowing primitive types.
            if is_primitive_name(name_str) {
                errors.push(crate::TypeError::CannotShadowPrimitive(name_str.to_string()));
                continue;
            }

            // Check for duplicate type alias.
            if type_aliases_map.contains_key(&name) {
                errors.push(crate::TypeError::DuplicateTypeAlias(name_str.to_string()));
                continue;
            }

            // Resolve the type hint using already-collected aliases.
            match convert_type_hint_with_aliases(db, stmt.type_hint.clone(), &type_aliases_map) {
                Ok(ty) => {
                    type_aliases_map.insert(name, ty.clone());
                    type_aliases_vec.push((name, ty));
                }
                Err(e) => {
                    errors.push(e);
                }
            }
        }
    }

    // Pass 1: collect function signatures.
    let mut functions = Vec::new();
    let mut function_asts = Vec::new();

    for statement in statements {
        if let Statement::Fun(stmt) = statement {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let return_type = stmt.return_type(db);

            // Convert parameter types and collect modes.
            let mut param_types = Vec::new();
            let mut param_modes = Vec::new();
            let mut has_error = false;
            for param in params {
                match convert_type_hint_with_aliases(db, param.type_hint.clone(), &type_aliases_map) {
                    Ok(ty) => {
                        param_types.push(ty);
                        param_modes.push(param.mode);
                    }
                    Err(e) => {
                        errors.push(e);
                        has_error = true;
                        break;
                    }
                }
            }

            if has_error {
                continue;
            }

            // Convert return type (default to unit if not specified).
            let ret_ty = match return_type {
                Some(type_hint) => {
                    match convert_type_hint_with_aliases(db, type_hint, &type_aliases_map) {
                        Ok(ty) => ty,
                        Err(e) => {
                            errors.push(e);
                            continue;
                        }
                    }
                }
                None => unit_type(db),
            };

            // Create function type and collect AST.
            let func_type = TypeFunction::new(db, param_types, param_modes, ret_ty);
            functions.push((name, func_type));
            function_asts.push((name, *stmt));
        }
    }

    CollectedNames {
        type_aliases: type_aliases_vec,
        functions,
        function_asts,
        errors,
    }
}

/// Resolve names (type aliases and function signatures) for a single module.
///
/// This is a tracked function memoized per module. It collects:
/// - Type aliases (pass 0)
/// - Function signatures (pass 1)
/// - Function ASTs (for inlining)
///
/// These are resolved before typechecking and seeded into the TypeContext.
#[salsa::tracked]
pub fn resolve_module_names<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed: ParsedStatements<'db>,
) -> crate::ModuleNameResolution<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("resolve_names", module_path, QueryPhase::Start);

    // Resolve names: collect type aliases and function signatures.
    let collected = resolve_names_impl(db, &parsed.statements);

    log_query("resolve_names", module_path, QueryPhase::End);

    crate::ModuleNameResolution::new(
        db,
        module_id,
        collected.type_aliases,
        collected.functions,
        collected.function_asts,
        Vec::new(), // No errors from this pass (errors are validation, not collection).
    )
}

/// Resolve names for all modules in a graph.
///
/// Calls the tracked `resolve_module_names` for each module, enabling
/// per-module caching of name resolution.
#[salsa::tracked]
pub fn resolve_all_names<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> crate::AllModuleNameResolutions<'db> {
    let graph = parsed_graph.graph(db);
    let module_to_module_obj: HashMap<ModuleId, Module> = graph.iter_modules(db)
        .map(|m| (m.id(db), m))
        .collect();

    let mut all_resolutions = BTreeMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let module = module_to_module_obj.get(module_id)
            .expect("module should exist in graph");
        let resolution = resolve_module_names(db, *module, parsed.C());
        all_resolutions.insert(*module_id, resolution);
    }

    crate::AllModuleNameResolutions::new(db, all_resolutions)
}

/// Resolve names for all modules in parallel.
///
/// Uses rayon to resolve names for each module in parallel, warming the
/// memoization cache. Then delegates to `resolve_all_names` for aggregation.
pub fn resolve_all_names_parallel<'db>(
    db: &'db dyn crate::DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
) -> crate::AllModuleNameResolutions<'db> {
    use rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);

    // Build module lookup and work items.
    let module_to_module_obj: HashMap<ModuleId, Module> = graph.iter_modules(db_salsa)
        .map(|m| (m.id(db_salsa), m))
        .collect();

    let work: Vec<_> = parsed_graph.statements_only(db_salsa)
        .iter()
        .map(|(module_id, parsed)| {
            let module = *module_to_module_obj.get(module_id)
                .expect("module should exist in graph");
            (db.dyn_clone(), module, parsed.C())
        })
        .collect();

    // Resolve names in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module, parsed)| {
        let db_salsa = db_clone.as_salsa_db();
        let _ = resolve_module_names(db_salsa, module, parsed);
    });

    // Delegate to tracked function which aggregates results.
    // All resolve_module_names calls will be cache hits.
    resolve_all_names(db_salsa, parsed_graph)
}

/// Resolve names with configurable parallelism.
pub fn resolve_all_names_with_mode<'db>(
    db: &'db dyn crate::DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    mode: crate::ParallelMode,
) -> crate::AllModuleNameResolutions<'db> {
    match mode {
        crate::ParallelMode::Sequential => resolve_all_names(db.as_salsa_db(), parsed_graph),
        crate::ParallelMode::Parallel => resolve_all_names_parallel(db, parsed_graph),
    }
}

/// Build module function info for script import resolution.
///
/// Collects function signatures and ASTs from modules, keyed by module path.
/// Returns two maps: one for function info (signature + AST), one for path to ModuleId.
fn build_script_module_functions<'db>(
    db: &'db dyn crate::Db,
    modules: &[ModuleSpec<'db>],
) -> (
    HashMap<String, HashMap<InternedText<'db>, (TypeFunction<'db>, StmtFun<'db>)>>,
    HashMap<String, ModuleId>,
) {
    let mut module_functions: HashMap<String, HashMap<InternedText<'db>, (TypeFunction<'db>, StmtFun<'db>)>> = HashMap::new();
    let mut path_to_module_id: HashMap<String, ModuleId> = HashMap::new();

    for module_spec in modules {
        // Use resolve_names_impl to get function signatures and ASTs.
        let collected = resolve_names_impl(db, &module_spec.parsed.statements);

        // Build function info map with ASTs.
        let ast_map: HashMap<_, _> = collected.function_asts.into_iter().collect();
        let mut funcs = HashMap::new();
        for (name, func_ty) in collected.functions {
            if let Some(ast) = ast_map.get(&name) {
                funcs.insert(name, (func_ty, *ast));
            }
        }

        path_to_module_id.insert(module_spec.path.C(), module_spec.module_id);
        module_functions.insert(module_spec.path.C(), funcs);
    }

    (module_functions, path_to_module_id)
}

/// Resolve imports for a script unit using path-based module lookup.
///
/// Parses require statements to build alias-to-path map, then resolves import
/// statements against the provided module functions.
fn resolve_script_imports<'db>(
    db: &'db dyn crate::Db,
    script: &ParsedStatements<'db>,
    module_functions: &HashMap<String, HashMap<InternedText<'db>, (TypeFunction<'db>, StmtFun<'db>)>>,
    path_to_module_id: &HashMap<String, ModuleId>,
) -> (Vec<(InternedText<'db>, TypeFunction<'db>, StmtFun<'db>, Option<ModuleId>)>, Vec<TypeError>) {
    // Build alias map from require statements.
    let mut alias_to_path: HashMap<InternedText<'db>, String> = HashMap::new();
    for statement in &script.statements {
        if let Statement::Require(StmtRequire::Module(req)) = statement {
            let import_space = req.import_space;
            let package_alias = req.package_alias;
            let module_alias = req.module_alias;
            let full_path = format!(
                "{}/{}/{}",
                import_space.as_str(db),
                package_alias.as_str(db),
                module_alias.as_str(db)
            );
            alias_to_path.insert(module_alias, full_path);
        }
    }

    // Resolve import statements.
    let mut resolved = Vec::new();
    let mut errors = Vec::new();

    for statement in &script.statements {
        if let Statement::Import(import) = statement {
            let module_alias = import.module_name;
            let item_name = import.item_name;

            // Look up the full path from the alias.
            let module_path = alias_to_path.get(&module_alias)
                .map(|s| s.as_str())
                .unwrap_or(module_alias.as_str(db));

            if let Some(funcs) = module_functions.get(module_path) {
                if let Some((func_ty, func_ast)) = funcs.get(&item_name) {
                    let source_module_id = path_to_module_id.get(module_path).cloned();
                    resolved.push((item_name, *func_ty, *func_ast, source_module_id));
                } else {
                    errors.push(TypeError::UnresolvedName(
                        format!("{}.{}", module_path, item_name.as_str(db))
                    ));
                }
            } else {
                errors.push(TypeError::UnresolvedName(
                    format!("module {}", module_path)
                ));
            }
        }
    }

    (resolved, errors)
}
