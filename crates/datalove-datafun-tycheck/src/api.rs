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
    AllModuleNameResolutions,
    AllModuleExports,
    AllModuleFunctionAsts,
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
    ScriptBatchSpec::new(db, unit_specs, module_specs, crate::AutoAdaptMode::Disabled)
}

/// Create a ScriptBatchSpec with auto-adapt mode inside a tracked function.
///
/// Like `create_batch_spec`, but allows specifying auto-adapt mode.
#[salsa::tracked]
pub fn create_batch_spec_with_auto_adapt<'db>(
    db: &'db dyn crate::Db,
    key: bct::input::Source,
    unit_specs: Vec<ScriptUnitSpec<'db>>,
    module_specs: Vec<ModuleSpec<'db>>,
    auto_adapt_mode: crate::AutoAdaptMode,
) -> ScriptBatchSpec<'db> {
    let _ = key; // Used as memoization key.
    ScriptBatchSpec::new(db, unit_specs, module_specs, auto_adapt_mode)
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
    auto_adapt_mode: crate::AutoAdaptMode,
) -> ScriptUnitTypecheckOutput<'db> {
    // Build module function info for import resolution.
    let (module_functions, path_to_module_id) = build_script_module_functions(db, &module_specs);

    let spans = unit_spec.spans.clone();
    let mut ctx = TypeContext::with_options(db, spans, None, auto_adapt_mode);

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
        ScriptUnitKind::Fragment(script, collected) => {
            // Use pre-computed name resolution.
            ctx.seed_from_collected_names(collected, None);

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
                    Statement::Const(const_stmt) => {
                        let name = const_stmt.name;
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
    let auto_adapt_mode = spec.auto_adapt_mode(db);

    let mut accumulated = AccumulatedBindings::default();
    let mut results = Vec::new();

    for unit_spec in spec.units(db) {
        // Call per-unit tracked function - memoized on (unit_spec, module_specs, accumulated, auto_adapt_mode).
        let output = typecheck_script_unit(
            db,
            unit_spec.clone(),
            module_specs.clone(),
            accumulated.clone(),
            auto_adapt_mode,
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
/// Accepts pre-computed name resolution from the resolve crate.
pub fn type_check_single_script<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    spans: DatafunSpans,
    parsed: ParsedStatements<'db>,
    name_resolution: crate::CollectedNames<'db>,
) -> UnitTypecheckResultTracked<'db> {
    let unit_spec = ScriptUnitSpec::new(source, spans, ScriptUnitKind::Fragment(parsed, name_resolution));
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
/// Accepts pre-computed name resolution results from the resolve crate.
/// Resolves imports for each module, then typechecks each module.
/// Each module's typecheck depends on its own name resolution and imports,
/// enabling per-module caching.
#[salsa::tracked]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
    all_names: AllModuleNameResolutions<'db>,
    all_exports: AllModuleExports<'db>,
    all_function_asts: AllModuleFunctionAsts<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    let prep = prepare_typecheck(db, parsed_graph);

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
        let import_resolution = resolve_module_imports(db, module, parsed_graph, all_exports, all_function_asts);
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
/// This function runs typechecking in parallel using rayon to warm salsa's
/// memoization cache, then delegates to the tracked `typecheck_module_graph`
/// function. The tracked function will hit the warmed cache for all calls.
///
/// Accepts pre-computed name resolution results from the resolve crate.
/// Requires `&dyn DbClone` to enable database cloning for parallel execution.
pub fn typecheck_module_graph_parallel<'db>(
    db: &'db dyn crate::DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    all_names: AllModuleNameResolutions<'db>,
    all_exports: AllModuleExports<'db>,
    all_function_asts: AllModuleFunctionAsts<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    use rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let prep = prepare_typecheck(db_salsa, parsed_graph);

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
        let import_resolution = resolve_module_imports(db_salsa, module, parsed_graph, all_exports, all_function_asts);
        let resolved_imports = import_resolution.imports(db_salsa).C();
        let import_errors = import_resolution.errors(db_salsa).C();

        // Typecheck (tracked, memoized per module).
        let spans = DatafunSpans::new(vec![]);
        let _ = typecheck_module(db_salsa, module, parsed, spans, name_resolution, resolved_imports, import_errors);
    });

    // Delegate to tracked function which aggregates results.
    // All resolve_module_names, resolve_module_imports, and typecheck_module calls will be cache hits.
    typecheck_module_graph(db_salsa, parsed_graph, all_names, all_exports, all_function_asts)
}

/// Typecheck module graph with configurable parallelism.
///
/// Accepts pre-computed name resolution results from the resolve crate.
/// Use `ParallelMode::Sequential` for standard salsa-tracked behavior, or
/// `ParallelMode::Parallel` to enable rayon parallelization.
pub fn typecheck_module_graph_with_mode<'db>(
    db: &'db dyn crate::DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    all_names: AllModuleNameResolutions<'db>,
    all_exports: AllModuleExports<'db>,
    all_function_asts: AllModuleFunctionAsts<'db>,
    mode: crate::ParallelMode,
) -> ModuleGraphTypecheckResult<'db> {
    match mode {
        crate::ParallelMode::Sequential => typecheck_module_graph(db.as_salsa_db(), parsed_graph, all_names, all_exports, all_function_asts),
        crate::ParallelMode::Parallel => typecheck_module_graph_parallel(db, parsed_graph, all_names, all_exports, all_function_asts),
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
// Import Resolution
// ============================================================================

/// Resolve imports for a single module (memoized per module).
///
/// This is the tracked entry point for import resolution, following the same
/// pattern as `parse_module_full` and `typecheck_module`. The parallel path
/// calls this in parallel to warm the cache, then the sequential aggregation
/// path hits the cache.
///
/// Accepts pre-computed exports and function ASTs from the resolve crate.
#[salsa::tracked]
pub fn resolve_module_imports<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed_graph: ParsedModuleGraph<'db>,
    all_exports: AllModuleExports<'db>,
    all_function_asts: AllModuleFunctionAsts<'db>,
) -> crate::ModuleImportResolution<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("resolve_imports", module_path, QueryPhase::Start);

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

/// Build module function info for script import resolution.
///
/// Collects function signatures and ASTs from modules, keyed by module path.
/// Returns two maps: one for function info (signature + AST), one for path to ModuleId.
fn build_script_module_functions<'db>(
    _db: &'db dyn crate::Db,
    modules: &[ModuleSpec<'db>],
) -> (
    HashMap<String, HashMap<InternedText<'db>, (TypeFunction<'db>, StmtFun<'db>)>>,
    HashMap<String, ModuleId>,
) {
    let mut module_functions: HashMap<String, HashMap<InternedText<'db>, (TypeFunction<'db>, StmtFun<'db>)>> = HashMap::new();
    let mut path_to_module_id: HashMap<String, ModuleId> = HashMap::new();

    for module_spec in modules {
        // Use pre-computed name resolution from ModuleSpec.
        let collected = &module_spec.name_resolution;

        // Build function info map with ASTs.
        let ast_map: HashMap<_, _> = collected.function_asts.iter().cloned().collect();
        let mut funcs = HashMap::new();
        for (name, func_ty) in &collected.functions {
            if let Some(ast) = ast_map.get(name) {
                funcs.insert(*name, (*func_ty, *ast));
            }
        }

        path_to_module_id.insert(module_spec.path.clone(), module_spec.module_id);
        module_functions.insert(module_spec.path.clone(), funcs);
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
