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

pub use crate::{
    DatafunSpans,
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
    ModuleId,
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
            module_spec.parsed,
            module_spec.source,
            module_spec.module_id,
        ));
    }

    let _batch = ScriptUnitBatch::new(db, units.clone(), modules.clone());

    // Build module function info for import resolution.
    // Map: module_path -> (function_name -> (signature, ast))
    let mut module_functions: HashMap<String, HashMap<InternedText<'db>, (TypeFunction<'db>, StmtFun<'db>)>> = HashMap::new();
    // Map: module_path -> ModuleId (for call target resolution).
    let mut path_to_module_id: HashMap<String, ModuleId> = HashMap::new();
    for (module_info, module_spec) in modules.iter().zip(spec.modules(db).iter()) {
        let mut funcs = HashMap::new();
        // First pass: collect function signatures.
        let module_spans = module_spec.spans.clone();
        let mut temp_ctx = TypeContext::new(db, module_spans);
        for statement in module_info.parsed(db).statements(db) {
            if let Statement::Fun(stmt) = statement {
                collect_function_signature(&mut temp_ctx, stmt, None);
            }
        }
        // Extract function info.
        for statement in module_info.parsed(db).statements(db) {
            if let Statement::Fun(stmt) = statement {
                let name = stmt.name(db);
                if let Some(func_ty) = temp_ctx.functions.get(&name) {
                    funcs.insert(name, (*func_ty, *stmt));
                }
            }
        }
        let path = module_info.path(db).clone();
        path_to_module_id.insert(path.clone(), module_info.module_id(db));
        module_functions.insert(path, funcs);
    }

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
                for statement in script.statements(db) {
                    if let Statement::Fun(stmt) = statement {
                        collect_function_signature(&mut ctx, stmt, None);
                    }
                }

                // Build alias map from require statements.
                // Maps module alias (e.g., "utils") to full path (e.g., "local/test/utils").
                let mut alias_to_path: HashMap<InternedText<'db>, String> = HashMap::new();
                for statement in script.statements(db) {
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

                // Process import statements.
                for statement in script.statements(db) {
                    if let Statement::Import(import) = statement {
                        let module_alias = import.module_name;
                        let item_name = import.item_name;

                        // Look up the full path from the alias.
                        let module_path = alias_to_path.get(&module_alias)
                            .map(|s| s.as_str())
                            .unwrap_or(module_alias.as_str(db));

                        if let Some(funcs) = module_functions.get(module_path) {
                            if let Some((func_ty, func_ast)) = funcs.get(&item_name) {
                                // Get the source module's ModuleId for call target resolution.
                                let source_module_id = path_to_module_id.get(module_path).copied();
                                ctx.add_function(item_name, *func_ty);
                                ctx.function_asts.insert(item_name, (*func_ast, source_module_id));
                                // Also add to accumulated so subsequent units can use it.
                                accumulated_fns.insert(item_name, *func_ty);
                                accumulated_fn_asts.insert(item_name, (*func_ast, source_module_id));
                            } else {
                                ctx.add_error(TypeError::UnresolvedName(
                                    format!("{}.{}", module_path, item_name.as_str(db))
                                ));
                            }
                        } else {
                            ctx.add_error(TypeError::UnresolvedName(
                                format!("module {}", module_path)
                            ));
                        }
                    }
                }

                // Second pass: typecheck all statements.
                for statement in script.statements(db) {
                    check_statement(&mut ctx, statement);
                }

                // Extract new bindings for subsequent units.
                for stmt in script.statements(db) {
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
    for statement in parsed.statements(db) {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt, None);
        }
    }

    // Second pass: typecheck all statements.
    for statement in parsed.statements(db) {
        check_statement(&mut ctx, statement);
    }

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
    for (module_id, module_parsed, _spans) in parsed_graph.parsed_statements(db) {
        let mut funcs = HashMap::new();
        for statement in module_parsed.statements(db) {
            if let Statement::Fun(func) = statement {
                funcs.insert(func.name(db), *func);
            }
        }
        module_function_asts.insert(*module_id, funcs);
    }

    // Build module alias map from require statements.
    let module_exports_map = graph_typecheck.module_exports(db);
    let alias_map = build_module_alias_map_for_graph(db, parsed, &path_to_id);

    // Process import statements to populate function signatures.
    for statement in parsed.statements(db) {
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
    for statement in parsed.statements(db) {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, stmt, None);
        }
    }

    // Second pass: type check all statements (including function bodies).
    for statement in parsed.statements(db) {
        check_statement(&mut ctx, statement);
    }

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

use crate::ResolvedImport;

/// Typecheck a single module with logging.
///
/// This is a tracked function so Salsa can observe per-module execution.
/// The logging only fires when the function actually executes.
///
/// Takes resolved imports from the caller (resolved using exports from pass 1).
#[salsa::tracked]
pub fn typecheck_module<'db>(
    db: &'db dyn crate::Db,
    module: Module,
    parsed: ParsedStatements<'db>,
    spans: DatafunSpans,
    resolved_imports: Vec<ResolvedImport<'db>>,
) -> SingleModuleTypecheckResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("typecheck", module_path, QueryPhase::Start);

    // Create type context for this module.
    let mut ctx = TypeContext::new(db, spans);

    // Add imported functions to context.
    for import in &resolved_imports {
        let local_name = import.local_name(db);
        let func_type = import.func_type(db);
        let source_module = import.source_module(db);

        if let Some(func_ast) = import.func_ast(db) {
            ctx.add_imported_function(local_name, func_type, func_ast, source_module);
        } else {
            ctx.add_function(local_name, func_type);
        }
    }

    // Collect all function signatures from this module.
    for statement in parsed.statements(db) {
        if let Statement::Fun(stmt) = statement {
            collect_function_signature(&mut ctx, &stmt, Some(module_id));
        }
    }

    // Type check all statements.
    for statement in parsed.statements(db) {
        check_statement(&mut ctx, statement);
    }

    // Collect exports for this module.
    let exports = collect_module_exports(db, parsed);

    // Build imports list for result.
    let imports: Vec<_> = resolved_imports.iter()
        .map(|imp| (imp.local_name(db), imp.source_module(db), imp.source_name(db)))
        .collect();

    log_query("typecheck", module_path, QueryPhase::End);

    SingleModuleTypecheckResult::new(
        db,
        module_id,
        ctx.errors.clone(),
        exports,
        imports,
        ctx.expr_types.clone(),
        ctx.call_targets.clone(),
    )
}

/// Typecheck a module graph using a two-pass approach.
///
/// Pass 1: Collect exports (function signatures) from all modules.
/// Pass 2: Typecheck each module with full import context available.
///
/// This structure allows per-module typechecking to be cached by Salsa,
/// since all imports can be resolved before typechecking begins.
#[salsa::tracked]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> ModuleGraphTypecheckResult<'db> {
    let graph = parsed_graph.graph(db);

    // Build maps for parsed statements, spans, and function ASTs from pre-parsed modules.
    let mut module_parsed: HashMap<ModuleId, ParsedStatements<'db>> = HashMap::new();
    let mut module_spans: HashMap<ModuleId, DatafunSpans> = HashMap::new();
    let mut module_function_asts: HashMap<ModuleId, HashMap<InternedText<'db>, StmtFun<'db>>> = HashMap::new();
    let mut module_to_module_obj: HashMap<ModuleId, Module> = HashMap::new();

    for (module_id, parsed, spans) in parsed_graph.parsed_statements(db) {
        let mut funcs = HashMap::new();
        for statement in parsed.statements(db) {
            if let Statement::Fun(func) = statement {
                funcs.insert(func.name(db), *func);
            }
        }
        module_parsed.insert(*module_id, *parsed);
        module_spans.insert(*module_id, spans.clone());
        module_function_asts.insert(*module_id, funcs);
    }

    for module in graph.iter_modules(db) {
        module_to_module_obj.insert(module.id(db), module);
    }

    // ========================================================================
    // PASS 1: Collect exports from all modules (just function signatures).
    // ========================================================================
    let mut all_exports: HashMap<ModuleId, Vec<(InternedText<'db>, TypeFunction<'db>)>> = HashMap::new();

    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let parsed = module_parsed.get(&module_id)
            .copied()
            .expect("module should have been parsed");
        let exports = collect_module_exports(db, parsed);
        all_exports.insert(module_id, exports);
    }

    // ========================================================================
    // PASS 2: Typecheck each module with full import context.
    // ========================================================================
    let mut module_errors: BTreeMap<ModuleId, Vec<TypeError>> = BTreeMap::new();
    let mut module_exports_map: BTreeMap<ModuleId, ModuleExports<'db>> = BTreeMap::new();
    let mut module_imports_map: BTreeMap<ModuleId, ModuleImports<'db>> = BTreeMap::new();
    let mut combined_expr_types: Vec<Option<TypeAndHeap<'db>>> = Vec::new();
    let mut combined_call_targets: Vec<Option<ResolvedCallTarget<'db>>> = Vec::new();

    for module in graph.iter_modules(db) {
        let module_id = module.id(db);

        let parsed = module_parsed.get(&module_id)
            .copied()
            .expect("module should have been parsed");
        let spans = module_spans.get(&module_id)
            .cloned()
            .expect("module should have spans");

        // Build module alias map from pre-resolved requires.
        let resolved_requires = parsed_graph.get_requires(db, module_id);
        let alias_map: HashMap<InternedText<'db>, ModuleId> = resolved_requires.iter()
            .map(|(alias, target_id)| (*alias, *target_id))
            .collect();

        // Resolve imports for this module using pass 1 exports.
        let mut resolved_imports: Vec<ResolvedImport<'db>> = Vec::new();
        let mut import_errors: Vec<TypeError> = Vec::new();

        for statement in parsed.statements(db) {
            if let Statement::Import(import) = statement {
                let module_name = import.module_name;
                let item_name = import.item_name;

                if let Some(&source_module_id) = alias_map.get(&module_name) {
                    // Look up the function in pass 1 exports.
                    if let Some(exports) = all_exports.get(&source_module_id) {
                        let func_opt = exports.iter()
                            .find(|(name, _)| *name == item_name)
                            .map(|(_, func_type)| *func_type);

                        if let Some(func_type) = func_opt {
                            // Look up the function AST.
                            let func_ast = module_function_asts
                                .get(&source_module_id)
                                .and_then(|funcs| funcs.get(&item_name))
                                .copied();

                            resolved_imports.push(ResolvedImport::new(
                                db,
                                item_name,
                                func_type,
                                func_ast,
                                source_module_id,
                                item_name,
                            ));
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

        // Call the tracked typecheck function.
        let result = typecheck_module(db, module, parsed, spans, resolved_imports);

        // Collect errors (import errors + typecheck errors).
        let mut errors = import_errors;
        errors.extend(result.errors(db).iter().cloned());
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
    }

    ModuleGraphTypecheckResult::new(db, graph, module_errors, module_exports_map, module_imports_map, combined_expr_types, combined_call_targets)
}

/// Build module alias map from require module statements for ModuleGraph.
///
/// Maps module aliases to ModuleIds by parsing require statements and matching
/// against the path_to_id map.
fn build_module_alias_map_for_graph<'db>(
    db: &'db dyn crate::Db,
    parsed: ParsedStatements<'db>,
    path_to_id: &HashMap<String, ModuleId>,
) -> HashMap<InternedText<'db>, ModuleId> {
    let mut alias_map = HashMap::new();

    for statement in parsed.statements(db) {
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

/// Collect function signatures exported from a module.
fn collect_module_exports<'db>(
    db: &'db dyn crate::Db,
    parsed: ParsedStatements<'db>,
) -> Vec<(InternedText<'db>, TypeFunction<'db>)> {
    let mut functions = Vec::new();

    // Collect all top-level function signatures.
    for statement in parsed.statements(db) {
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
