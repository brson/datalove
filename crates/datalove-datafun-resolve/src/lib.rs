//! Name resolution for datafun.
//!
//! Provides name resolution (type aliases, function signatures) as a separate
//! phase before typechecking. This crate has no dependency on tycheck.

use std::collections::{HashMap, BTreeMap};
use bct::text::InternedText;
use bct::module_graph::{ModuleId, Module};
use datalove_ct::query_log::{log_query, QueryPhase};

use datalove_datafun_ast::ast::*;

use salsa::Database as Db;

// Re-export shared types from common.
pub use datalove_datafun_common::{
    DbClone,
    ParallelMode,
    parallel_mode_from_env,
    ParsedModuleGraph,
    ModuleNameResolution,
    AllModuleNameResolutions,
    CollectedNames,
    AllModuleExports,
    AllModuleFunctionAsts,
    Type,
    TypeFunction,
    TypeError,
    convert_type_hint_with_aliases,
    is_primitive_name,
    unit_type,
};

pub use datalove_datafun_ast::spans::DatafunSpans;

// ============================================================================
// Name Resolution Implementation
// ============================================================================

/// Resolve names from parsed statements.
///
/// This is the core name resolution implementation used by both modules and scripts.
/// It collects type aliases and function signatures, enabling forward references
/// to functions defined later in the same compilation unit.
pub fn resolve_names_impl<'db>(
    db: &'db dyn Db,
    statements: &[Statement<'db>],
) -> CollectedNames<'db> {
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
                errors.push(TypeError::CannotShadowPrimitive(name_str.to_string()));
                continue;
            }

            // Check for duplicate type alias.
            if type_aliases_map.contains_key(&name) {
                errors.push(TypeError::DuplicateTypeAlias(name_str.to_string()));
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
        // Extract function signature from either regular or native fun.
        // A native function's arguments are passed as they stand, each with a
        // descriptor saying what it is, so its implementation can read the
        // element type off a container at runtime. Nothing is converted, so
        // the positions erasure cannot reach do not apply to one.
        let is_native = matches!(statement, Statement::NativeFun(_));

        let (name, type_params, type_bounds, params, return_type, func_ast) = match statement {
            Statement::Fun(stmt) => {
                (stmt.name(db), stmt.type_params(db).clone(), stmt.type_bounds(db).clone(),
                 stmt.params(db).clone(), stmt.return_type(db), Some(*stmt))
            }
            Statement::NativeFun(stmt) => {
                (stmt.name, stmt.type_params.clone(), stmt.type_bounds.clone(),
                 stmt.params.clone(), stmt.return_type.clone(), None)
            }
            _ => continue,
        };

        // Which of this signature's type parameters have promised an order.
        // A set or map written over one that has not is refused below; see
        // `first_unordered_collection_key`.
        let ordered = |name: bct::text::InternedText<'db>| -> bool {
            type_params.iter().position(|p| *p == name)
                .and_then(|i| type_bounds.get(i).copied().flatten())
                .is_some_and(|bound| bound.implies_ord())
        };
        // Where the signature is, for a diagnostic to point at. A native's
        // interface has no span table, and a native is exempt from the check
        // below anyway.
        let fun_local_index = func_ast.map_or(0, |stmt| stmt.local_index(db));

        // A type parameter resolves like a named type, so it goes in the alias
        // map for the length of this signature. Nothing else can see it, which
        // is what keeps one function's `T` from meaning another's.
        let type_aliases_map = if type_params.is_empty() {
            type_aliases_map.clone()
        } else {
            let mut scoped = type_aliases_map.clone();
            for name in &type_params {
                scoped.insert(*name, Type::Datalit(datalove_datalit::tycheck::Type::Var(*name)));
            }
            scoped
        };

        // Convert parameter types and collect modes/comptime flags.
        let mut param_types = Vec::new();
        let mut param_modes = Vec::new();
        let mut param_comptime = Vec::new();
        let mut has_error = false;
        for param in &params {
            match convert_type_hint_with_aliases(db, param.type_hint.clone(), &type_aliases_map) {
                Ok(ty) => {
                    // Nothing is converted at a borrowed parameter: the value
                    // goes across as it stands and the callee never drops it,
                    // so a type parameter under a container is reachable there
                    // even though erasing one is not. What the callee lacks is
                    // a descriptor, since its own type says `data` where the
                    // parameter was written, and the call site supplies that.
                    // A native's parameters are the same case by construction.
                    let borrowed = matches!(param.mode, ParamMode::Ref | ParamMode::Mut);
                    let position = format!("parameter `{}`", param.name.as_str(db));
                    // A native is exempt from erasability, because nothing is
                    // converted at its boundary, but not from this: whatever
                    // it is handed, the runtime still has to order.
                    if let Some(e) =
                        unordered_collection_key_error(db, &ty, &position, fun_local_index, &ordered)
                    {
                        errors.push(e);
                        has_error = true;
                        break;
                    }
                    if let Some(e) = (!is_native && !borrowed)
                        .then(|| unerasable_type_param_error(db, &ty, &position, fun_local_index))
                        .flatten()
                    {
                        errors.push(e);
                        has_error = true;
                        break;
                    }
                    param_types.push(ty);
                    param_modes.push(param.mode);
                    param_comptime.push(param.is_comptime);
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
                    Ok(ty) => match unordered_collection_key_error(
                        db, &ty, "the return type", fun_local_index, &ordered)
                        .or_else(|| (!is_native)
                            .then(|| unerasable_type_param_error(
                                db, &ty, "the return type", fun_local_index))
                            .flatten())
                    {
                        Some(e) => {
                            errors.push(e);
                            continue;
                        }
                        None => ty,
                    },
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            }
            None => unit_type(db),
        };

        // Create function type. Only collect AST for regular functions (not native).
        let func_type = TypeFunction::new(db, param_types, param_modes, param_comptime, ret_ty);
        functions.push((name, func_type));
        if let Some(ast) = func_ast {
            function_asts.push((name, ast));
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
/// Keyed on the module alone. Taking the statements as a second argument made
/// salsa intern the pair to key on, one entry per module, and left it unable to
/// name the module in the event it reports for this query. Fetching the
/// statements instead says the same thing as a dependency, which is what was
/// meant, and drops the interned pair.
#[salsa::tracked(returns(copy))]
pub fn resolve_module_names<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
) -> ModuleNameResolution<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("resolve_names", module_path, QueryPhase::Start);

    let parsed = datalove_datafun_parser::parse_module_ast(db, module);

    // Resolve names: collect type aliases and function signatures.
    let collected = resolve_names_impl(db, &parsed.statements);

    log_query("resolve_names", module_path, QueryPhase::End);

    ModuleNameResolution::new(
        db,
        module_id,
        collected.type_aliases,
        collected.functions,
        collected.function_asts,
        collected.errors,
    )
}

/// Resolve names for all modules in a graph.
///
/// Calls the tracked `resolve_module_names` for each module, enabling
/// per-module caching of name resolution.
#[salsa::tracked(returns(copy))]
pub fn resolve_all_names<'db>(
    db: &'db dyn Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleNameResolutions<'db> {
    let graph = parsed_graph.graph(db);
    let module_to_module_obj: HashMap<ModuleId<'db>, Module> = graph.iter_modules(db)
        .map(|m| (m.id(db), m))
        .collect();

    let mut all_resolutions = BTreeMap::new();

    for (module_id, _) in parsed_graph.statements_only(db) {
        let module = module_to_module_obj.get(module_id)
            .expect("module should exist in graph");
        let resolution = resolve_module_names(db, *module);
        all_resolutions.insert(*module_id, resolution);
    }

    AllModuleNameResolutions::new(db, all_resolutions)
}

/// Resolve names for all modules in parallel.
///
/// Uses rayon to resolve names for each module in parallel, warming the
/// memoization cache. Then delegates to `resolve_all_names` for aggregation.
pub fn resolve_all_names_parallel<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleNameResolutions<'db> {
    use rmx::rayon::prelude::*;

    let db_salsa = db.as_salsa_db();
    let graph = parsed_graph.graph(db_salsa);

    // Build module lookup and work items.
    let module_to_module_obj: HashMap<ModuleId<'db>, Module> = graph.iter_modules(db_salsa)
        .map(|m| (m.id(db_salsa), m))
        .collect();

    let work: Vec<_> = parsed_graph.statements_only(db_salsa)
        .iter()
        .map(|(module_id, _)| {
            let module = *module_to_module_obj.get(module_id)
                .expect("module should exist in graph");
            (db.dyn_clone(), module)
        })
        .collect();

    // Resolve names in parallel - populates salsa's memoization cache.
    work.into_par_iter().for_each(|(db_clone, module)| {
        let db_salsa = db_clone.as_salsa_db();
        let _ = resolve_module_names(db_salsa, module);
    });

    // Delegate to tracked function which aggregates results.
    // All resolve_module_names calls will be cache hits.
    resolve_all_names(db_salsa, parsed_graph)
}

/// Resolve names with configurable parallelism.
pub fn resolve_all_names_with_mode<'db>(
    db: &'db dyn DbClone,
    parsed_graph: ParsedModuleGraph<'db>,
    mode: ParallelMode,
) -> AllModuleNameResolutions<'db> {
    match mode {
        ParallelMode::Sequential => resolve_all_names(db.as_salsa_db(), parsed_graph),
        ParallelMode::Parallel => resolve_all_names_parallel(db, parsed_graph),
    }
}

// ============================================================================
// Script Name Resolution
// ============================================================================

/// Resolve names for a script (tracked function for scripts).
///
/// This is a tracked function that provides memoization for script name resolution.
/// It uses the source as the memoization key, ensuring that the same script
/// produces the same resolution results.
#[salsa::tracked(returns(clone))]
pub fn resolve_script_names<'db>(
    db: &'db dyn Db,
    source: bct::input::Source,
    parsed: ParsedStatements<'db>,
) -> CollectedNames<'db> {
    let _ = source; // Used as memoization key.
    resolve_names_impl(db, &parsed.statements)
}

// ============================================================================
// Export Resolution
// ============================================================================

/// Collect function signatures exported from a module.
///
/// This is a tracked function so Salsa can cache per-module export collection.
/// Only depends on parsed statements, not on any other module.
#[salsa::tracked(returns(clone))]
pub fn resolve_module_exports<'db>(
    db: &'db dyn Db,
    module: Module<'db>,
    parsed: ParsedStatements<'db>,
) -> Vec<(InternedText<'db>, TypeFunction<'db>)> {
    let _ = module; // Used as memoization key.
    resolve_names_impl(db, &parsed.statements).functions
}

/// Collect exports from all modules in the graph.
///
/// Calls the tracked `resolve_module_exports` for each module, enabling
/// per-module caching of export collection.
#[salsa::tracked(returns(copy))]
pub fn resolve_all_exports<'db>(
    db: &'db dyn Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleExports<'db> {
    let graph = parsed_graph.graph(db);
    let module_to_module_obj: HashMap<ModuleId<'db>, Module> = graph.iter_modules(db)
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

/// Build function AST maps for all modules.
///
/// Used for function inlining - maps module ID to function name to AST.
#[salsa::tracked(returns(copy))]
pub fn build_all_function_ast_maps<'db>(
    db: &'db dyn Db,
    parsed_graph: ParsedModuleGraph<'db>,
) -> AllModuleFunctionAsts<'db> {
    let mut module_function_asts = BTreeMap::new();

    for (module_id, parsed) in parsed_graph.statements_only(db) {
        let mut funcs = Vec::new();
        for statement in parsed.statements.iter() {
            if let Statement::Fun(func) = statement {
                funcs.push((func.name(db), *func));
            }
        }
        module_function_asts.insert(*module_id, funcs);
    }

    AllModuleFunctionAsts::new(db, module_function_asts)
}

/// Reject a signature that puts a type parameter where an order is needed and
/// does not ask for one.
///
/// See `first_unordered_collection_key` for which positions those are.
fn unordered_collection_key_error<'db>(
    db: &'db dyn Db,
    ty: &Type<'db>,
    position: &str,
    fun_local_index: u32,
    ordered: &dyn Fn(bct::text::InternedText<'db>) -> bool,
) -> Option<TypeError> {
    let Type::Datalit(dt) = ty else { return None };
    let name = datalove_datafun_common::generics::first_unordered_collection_key(dt, ordered)?;
    Some(TypeError::CollectionKeyNotOrdered {
        param: name.as_str(db).to_string(),
        position: position.to_string(),
        fun_local_index,
    })
}

/// Reject a signature whose type parameter sits where erasure cannot reach it.
///
/// See `first_unerasable_type_param` for which positions those are.
fn unerasable_type_param_error<'db>(
    db: &'db dyn Db,
    ty: &Type<'db>,
    position: &str,
    fun_local_index: u32,
) -> Option<TypeError> {
    let Type::Datalit(dt) = ty else { return None };
    let name = datalove_datafun_common::generics::first_unerasable_type_param(dt)?;
    Some(TypeError::TypeParamNotErasable {
        param: name.as_str(db).to_string(),
        position: position.to_string(),
        fun_local_index,
    })
}
