//! Public API for datafun typechecking.
//!
//! Provides salsa-tracked entry points and public functions for typechecking
//! scripts, expressions, and module graphs.

use std::collections::{HashMap, BTreeMap};
use bct::text::InternedText;
use datalove_ct::query_log::{log_query, QueryPhase};

use datalove_datafun_ast::ast::*;
use datalove_datalit as datalit;
use crate::context::{TypeContext, ScriptTypeContext, ScriptTypecheckResultRaw, ExprTypecheckResultRaw};
use crate::statement::{check_statement, collect_function_signature};
use crate::types::{convert_type_hint, unit_type};

pub use datalove_datafun_ast::spans::DatafunSpans;
pub use bct::module_graph::ModuleId;

pub use crate::{
    PendingDiagnostic,
    Type,
    TypeAndHeap,
    TypeFunction,
    TypeError,
    TypeErrorEntry,
    ResolvedCallTarget,
    TypecheckResult,
    ScriptUnitKind,
    ScriptUnitSpec,
    ScriptBatchSpec,
    ScriptUnitInput,
    ModuleInfo,
    ModuleSpec,
    ScriptUnitBatch,
    UnitTypecheckResultTracked,
    ScriptUnitsTypecheckResultTracked,
    ParsedModuleGraph,
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
    SingleModuleTypecheckResult,
};

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

/// Typecheck multiple script units together, with bindings shared across units.
///
/// Units are processed in order. Bindings from earlier units (let/var/fn)
/// are visible in subsequent units.
#[salsa::tracked]
pub fn type_check_script_units<'db>(
    db: &'db dyn crate::Db,
    spec: ScriptBatchSpec<'db>,
) -> ScriptUnitsTypecheckResultTracked<'db> {
    // Build tracked types from specs (already pre-parsed).
    let mut units = Vec::new();
    for unit_spec in spec.units(db) {
        let source = unit_spec.source;
        let kind = unit_spec.kind.clone();
        units.push(ScriptUnitInput::new(db, source, kind));
    }

    let mut modules = Vec::new();
    for module_spec in spec.modules(db) {
        modules.push(ModuleInfo::new(
            db,
            module_spec.path.clone(),
            module_spec.parsed.clone(),
            module_spec.source,
            module_spec.module_id,
        ));
    }

    let _batch = ScriptUnitBatch::new(db, units.clone(), modules.clone());

    // Build module function info for import resolution using shared helper.
    let (module_functions, path_to_module_id) = build_script_module_functions(db, spec.modules(db));

    let mut accumulated_vars: HashMap<InternedText<'db>, TypeAndHeap<'db>> = HashMap::new();
    let mut accumulated_fns: HashMap<InternedText<'db>, TypeFunction<'db>> = HashMap::new();
    let mut accumulated_fn_asts: HashMap<InternedText<'db>, (StmtFun<'db>, Option<ModuleId>)> = HashMap::new();
    let mut results = Vec::new();

    for (unit, unit_spec) in units.iter().zip(spec.units(db).iter()) {
        let spans = unit_spec.spans.clone();
        let mut ctx = TypeContext::new(db, spans);

        // Script units have Result<()> return type for try operators.
        // This allows `!` (try-result) but not `?` (try-option).
        let unit_tuple_ty = datalit::tycheck::TypeAndHeap::new(
            db,
            datalit::ast::Heap::Omitted,
            datalit::tycheck::Type::AnonTuple(datalit::tycheck::TypeAnonTuple { fields: Vec::new() }),
        );
        let result_unit_ty = datalit::tycheck::Type::Result(
            datalit::tycheck::TypeResult { inner_type: unit_tuple_ty }
        );
        ctx.expected_return_type = Some(TypeAndHeap::new(
            db,
            datalit::ast::Heap::Omitted,
            Type::Datalit(result_unit_ty),
        ));

        // Seed with accumulated bindings from prior units.
        for (name, ty) in &accumulated_vars {
            ctx.add_variable(*name, *ty);
        }
        for (name, func_ty) in &accumulated_fns {
            ctx.add_function(*name, *func_ty);
        }
        for (name, (func_ast, module_id)) in &accumulated_fn_asts {
            ctx.function_asts.insert(*name, (*func_ast, *module_id));
        }

        // Typecheck this unit based on kind.
        match unit.kind(db) {
            ScriptUnitKind::Fragment(script) => {
                // First pass: collect function signatures from this unit.
                for statement in &script.statements {
                    if let Statement::Fun(stmt) = statement {
                        collect_function_signature(&mut ctx, stmt, None);
                    }
                }

                // Resolve imports using shared helper.
                let (resolved_imports, import_errors) = resolve_script_imports(
                    db, script, &module_functions, &path_to_module_id
                );

                // Add resolved imports to context and accumulated state.
                for (item_name, func_ty, func_ast, source_module_id) in resolved_imports {
                    ctx.add_function(item_name, func_ty);
                    ctx.function_asts.insert(item_name, (func_ast, source_module_id));
                    // Also add to accumulated so subsequent units can use it.
                    accumulated_fns.insert(item_name, func_ty);
                    accumulated_fn_asts.insert(item_name, (func_ast, source_module_id));
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
                            if let Some(ty) = ctx.variables.get(&name) {
                                accumulated_vars.insert(name, *ty);
                            }
                        }
                        Statement::Var(var_stmt) => {
                            let name = var_stmt.name;
                            if let Some(ty) = ctx.variables.get(&name) {
                                accumulated_vars.insert(name, *ty);
                            }
                        }
                        Statement::Fun(fun_stmt) => {
                            let name = fun_stmt.name(db);
                            if let Some(func_ty) = ctx.functions.get(&name) {
                                accumulated_fns.insert(name, *func_ty);
                                accumulated_fn_asts.insert(name, (*fun_stmt, None));
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
        let result = UnitTypecheckResultTracked::new(db, errors, ctx.expr_types, ctx.call_targets);
        results.push(result);
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

/// Typecheck a script with prior bindings from previous units.
///
/// Non-salsa version that accepts a script type context.
/// Returns raw results (not salsa-tracked) to allow use outside tracked functions.
pub fn type_check_script_with_context<'db>(
    db: &'db dyn crate::Db,
    spans: DatafunSpans,
    parsed: ParsedStatements<'db>,
    prior_ctx: &ScriptTypeContext<'db>,
) -> ScriptTypecheckResultRaw<'db> {
    let mut ctx = TypeContext::new(db, spans);

    // Seed with prior bindings.
    for (name, ty) in &prior_ctx.variables {
        ctx.add_variable(*name, *ty);
    }
    for (name, func_ty) in &prior_ctx.functions {
        ctx.add_function(*name, *func_ty);
    }

    // First pass: collect function signatures from this unit.
    for statement in &parsed.statements {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt, None);
        }
    }

    // Second pass: typecheck all statements.
    for statement in &parsed.statements {
        check_statement(&mut ctx, &statement);
    }

    // Emit pending diagnostics with local spans.
    ctx.emit_pending_diagnostics();

    ScriptTypecheckResultRaw {
        root_parsed: parsed,
        errors: ctx.errors,
        expr_types: ctx.expr_types,
        call_targets: ctx.call_targets,
    }
}

/// Typecheck an expression with prior bindings from previous units.
///
/// Non-salsa version that accepts a script type context.
/// Returns raw results (not salsa-tracked) to allow use outside tracked functions.
pub fn type_check_expr_with_context<'db>(
    db: &'db dyn crate::Db,
    spans: DatafunSpans,
    expr: ExprFun<'db>,
    prior_ctx: &ScriptTypeContext<'db>,
) -> ExprTypecheckResultRaw<'db> {
    let mut ctx = TypeContext::new(db, spans);

    // Seed with prior bindings.
    for (name, ty) in &prior_ctx.variables {
        ctx.add_variable(*name, *ty);
    }
    for (name, func_ty) in &prior_ctx.functions {
        ctx.add_function(*name, *func_ty);
    }

    let _ = ctx.synthesize_expr(expr);

    // Emit pending diagnostics with local spans.
    ctx.emit_pending_diagnostics();

    ExprTypecheckResultRaw {
        errors: ctx.errors,
        expr_types: ctx.expr_types,
    }
}

/// Typecheck a script with module graph support.
///
/// This version of type_check allows scripts to import functions from modules
/// in the module graph. Used for script units that define functions requiring
/// access to imported function signatures.
///
/// Takes a `ParsedModuleGraph` which contains pre-parsed statements for each module.
#[salsa::tracked]
pub fn type_check_with_module_graph<'db>(
    db: &'db dyn crate::Db,
    spans: DatafunSpans,
    parsed: ParsedStatements<'db>,
    parsed_graph: ParsedModuleGraph<'db>,
    graph_typecheck: ModuleGraphTypecheckResult<'db>,
) -> TypecheckResult<'db> {
    let graph = parsed_graph.graph(db);
    let mut ctx = TypeContext::new(db, spans);

    // Build path-to-id map from the module graph.
    let mut path_to_id: HashMap<String, ModuleId> = HashMap::new();
    for module in graph.iter_modules(db) {
        let id = module.id(db);
        path_to_id.insert(id.path(db).clone(), id);
    }

    // Build a map of function ASTs per module from pre-parsed statements.
    let mut module_function_asts: HashMap<ModuleId, HashMap<InternedText<'db>, StmtFun<'db>>> = HashMap::new();
    for (module_id, module_parsed) in parsed_graph.statements_only(db) {
        let mut funcs = HashMap::new();
        for statement in &module_parsed.statements {
            if let Statement::Fun(func) = statement {
                funcs.insert(func.name(db), *func);
            }
        }
        module_function_asts.insert(*module_id, funcs);
    }

    // Build module alias map from require statements.
    let module_exports_map = graph_typecheck.module_exports(db);
    let alias_map = build_module_alias_map_for_graph(db, &parsed, &path_to_id);

    // Process import statements to populate function signatures.
    for statement in &parsed.statements {
        if let Statement::Import(import) = statement {
            let module_name = import.module_name;
            let item_name = import.item_name;

            // Look up the module in the alias map.
            if let Some(&source_module_id) = alias_map.get(&module_name) {
                // Look up the module exports.
                if let Some(exports) = module_exports_map.get(&source_module_id) {
                    // Look up the function in the exports.
                    let func_opt = exports.functions(db).iter()
                        .find(|(name, _)| *name == item_name)
                        .map(|(_, func_type)| *func_type);

                    if let Some(func_type) = func_opt {
                        // Look up the function AST from the source module.
                        if let Some(source_funcs) = module_function_asts.get(&source_module_id) {
                            if let Some(&func_ast) = source_funcs.get(&item_name) {
                                ctx.add_imported_function(item_name, func_type, func_ast, source_module_id);
                            } else {
                                ctx.add_function(item_name, func_type);
                            }
                        } else {
                            ctx.add_function(item_name, func_type);
                        }
                    } else {
                        ctx.add_error(TypeError::UnresolvedName(
                            format!("{}.{}", module_name.as_str(db), item_name.as_str(db))
                        ));
                    }
                } else {
                    ctx.add_error(TypeError::UnresolvedName(
                        format!("module {} (not typechecked)", module_name.as_str(db))
                    ));
                }
            } else {
                ctx.add_error(TypeError::UnresolvedName(
                    format!("module {} (not required)", module_name.as_str(db))
                ));
            }
        }
    }

    // First pass: collect all function signatures (script-local, no module).
    for statement in &parsed.statements {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt, None);
        }
    }

    // Second pass: type check all statements (including function bodies).
    for statement in &parsed.statements {
        check_statement(&mut ctx, &statement);
    }

    // Emit pending diagnostics with local spans.
    ctx.emit_pending_diagnostics();

    let errors = ctx
        .errors
        .into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();

    TypecheckResult::new(db, parsed, errors, ctx.expr_types, ctx.call_targets)
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
/// Takes resolved imports as plain data (not tracked structs) so they can be
/// created outside tracked functions for parallel execution. Each module's
/// typecheck only depends on its own imports, enabling proper per-module caching.
#[salsa::tracked]
pub fn typecheck_module<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed: ParsedStatements<'db>,
    spans: DatafunSpans,
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

    // Collect all function signatures from this module.
    for statement in &parsed.statements {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt, Some(module_id));
        }
    }

    // Type check all statements.
    for statement in &parsed.statements {
        check_statement(&mut ctx, &statement);
    }

    // Collect exports for this module.
    let exports = collect_module_exports(db, parsed);

    // Build imports list for result.
    let imports: Vec<_> = resolved_imports.iter()
        .map(|(local_name, _, _, source_module)| (*local_name, *source_module, *local_name))
        .collect();

    log_query("typecheck", module_path, QueryPhase::End);

    SingleModuleTypecheckResult::new(
        db,
        module_id,
        ctx.errors.clone(),
        ctx.pending_diagnostics.clone(),
        exports,
        imports,
        ctx.expr_types.clone(),
        ctx.call_targets.clone(),
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
        .map(|(id, parsed)| (*id, parsed.clone()))
        .collect();

    TypecheckPreparation {
        graph,
        module_parsed,
    }
}

/// Typecheck a module graph.
///
/// Collects exports from all modules, resolves imports for each module,
/// then typechecks each module. Each module's typecheck only depends on
/// its own parsed statements and resolved imports, enabling per-module caching.
#[salsa::tracked]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    let prep = prepare_typecheck(db, parsed_graph);

    // ========================================================================
    // TYPECHECK PASS
    // ========================================================================
    let mut module_errors: BTreeMap<ModuleId, Vec<TypeError>> = BTreeMap::new();
    let mut module_exports_map: BTreeMap<ModuleId, ModuleExports<'db>> = BTreeMap::new();
    let mut module_imports_map: BTreeMap<ModuleId, ModuleImports<'db>> = BTreeMap::new();
    let mut module_results_map: BTreeMap<ModuleId, SingleModuleTypecheckResult<'db>> = BTreeMap::new();
    let mut combined_expr_types: Vec<Option<TypeAndHeap<'db>>> = Vec::new();
    let mut combined_call_targets: Vec<Option<ResolvedCallTarget<'db>>> = Vec::new();

    for module in prep.graph.iter_modules(db) {
        let module_id = module.id(db);

        let parsed = prep.module_parsed.get(&module_id)
            .cloned()
            .expect("module should have been parsed");

        // Resolve imports for this module (tracked, memoized per module).
        let import_resolution = resolve_module_imports(db, module, parsed_graph);
        let resolved_imports = import_resolution.imports(db).clone();
        let import_errors = import_resolution.errors(db).clone();

        // Use empty spans inside tracked function.
        let spans = DatafunSpans::new(vec![]);

        // Call the tracked typecheck function.
        let result = typecheck_module(db, module, parsed, spans, resolved_imports, import_errors);

        // Collect errors from typecheck result.
        let errors = result.errors(db).clone();
        if !errors.is_empty() {
            module_errors.insert(module_id, errors);
        }

        // Build exports.
        let exports = ModuleExports::new(db, module_id, result.exports(db).clone());
        module_exports_map.insert(module_id, exports);

        // Build imports.
        let imports = ModuleImports::new(db, module_id, result.imports(db).clone());
        module_imports_map.insert(module_id, imports);

        // Merge expr_types.
        let new_types = result.expr_types(db);
        if new_types.len() > combined_expr_types.len() {
            combined_expr_types.resize(new_types.len(), None);
        }
        for (i, ty) in new_types.iter().enumerate() {
            if ty.is_some() {
                combined_expr_types[i] = *ty;
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
/// This function typechecks modules in parallel using rayon to warm salsa's memoization
/// cache, then delegates to the tracked `typecheck_module_graph` function. The tracked
/// function will hit the warmed cache for all `typecheck_module` calls.
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
            (db.dyn_clone(), module, parsed)
        })
        .collect();

    // Typecheck modules in parallel - populates salsa's memoization cache.
    // Both resolve_module_imports and typecheck_module are tracked and cached.
    work.into_par_iter().for_each(|(db_clone, module, parsed)| {
        let db_salsa = db_clone.as_salsa_db();

        // Resolve imports (tracked, memoized per module).
        let import_resolution = resolve_module_imports(db_salsa, module, parsed_graph);
        let resolved_imports = import_resolution.imports(db_salsa).clone();
        let import_errors = import_resolution.errors(db_salsa).clone();

        // Typecheck (tracked, memoized per module).
        let spans = DatafunSpans::new(vec![]);
        let _ = typecheck_module(db_salsa, module, parsed, spans, resolved_imports, import_errors);
    });

    // Delegate to tracked function which aggregates results.
    // All resolve_module_imports and typecheck_module calls will be cache hits.
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

/// Build module alias map from require module statements for ModuleGraph.
///
/// Maps module aliases to ModuleIds by parsing require statements and matching
/// against the path_to_id map.
fn build_module_alias_map_for_graph<'db>(
    db: &'db dyn crate::Db,
    parsed: &ParsedStatements<'db>,
    path_to_id: &HashMap<String, ModuleId>,
) -> HashMap<InternedText<'db>, ModuleId> {
    let mut alias_map = HashMap::new();

    for statement in &parsed.statements {
        if let Statement::Require(StmtRequire::Module(req)) = statement {
            let import_space = req.import_space;
            let package_alias = req.package_alias;
            let module_alias = req.module_alias;

            // Build the module path from the require statement.
            let path = format!(
                "{}/{}/{}",
                import_space.as_str(db),
                package_alias.as_str(db),
                module_alias.as_str(db)
            );

            // Look up the ModuleId by path.
            if let Some(&module_id) = path_to_id.get(&path) {
                alias_map.insert(module_alias, module_id);
            }
        }
    }

    alias_map
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
    collect_module_exports_impl(db, parsed)
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
        let exports = resolve_module_exports(db, *module, parsed.clone());
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

/// Implementation of export collection (non-tracked).
fn collect_module_exports_impl<'db>(
    db: &'db dyn crate::Db,
    parsed: ParsedStatements<'db>,
) -> Vec<(InternedText<'db>, TypeFunction<'db>)> {
    let mut functions = Vec::new();

    // Collect all top-level function signatures.
    for statement in &parsed.statements {
        if let Statement::Fun(stmt) = statement {
            let name = stmt.name(db);
            let params = stmt.params(db);
            let return_type = stmt.return_type(db);

            // Convert parameter types and collect modes.
            let mut param_types = Vec::new();
            let mut param_modes = Vec::new();
            let mut has_error = false;
            for param in params {
                match convert_type_hint(db, param.type_hint) {
                    Ok(ty) => {
                        param_types.push(ty);
                        param_modes.push(param.mode);
                    }
                    Err(_) => {
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
                    match convert_type_hint(db, type_hint) {
                        Ok(ty) => ty,
                        Err(_) => continue,
                    }
                }
                None => {
                    // Functions without explicit return type return unit `()`.
                    unit_type(db)
                }
            };

            // Create function type.
            let func_type = TypeFunction::new(db, param_types, param_modes, ret_ty);
            functions.push((name, func_type));
        }
    }

    functions
}

/// Collect function signatures exported from a module (legacy non-tracked version).
///
/// Used by `typecheck_module` for its internal export collection.
fn collect_module_exports<'db>(
    db: &'db dyn crate::Db,
    parsed: ParsedStatements<'db>,
) -> Vec<(InternedText<'db>, TypeFunction<'db>)> {
    collect_module_exports_impl(db, parsed)
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
        // Use collect_module_exports_impl to get function signatures.
        let exports = collect_module_exports_impl(db, module_spec.parsed.clone());

        // Build function info map with ASTs.
        let mut funcs = HashMap::new();
        for (name, func_ty) in exports {
            // Find the corresponding AST.
            for statement in &module_spec.parsed.statements {
                if let Statement::Fun(stmt) = statement {
                    if stmt.name(db) == name {
                        funcs.insert(name, (func_ty, *stmt));
                        break;
                    }
                }
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
