//! Public API for datafun typechecking.
//!
//! Provides salsa-tracked entry points and public functions for typechecking
//! scripts, expressions, and module graphs.

use rmx::prelude::*;
use std::collections::{HashMap, BTreeMap};
use bct::text::InternedText;
use datalove_ct::query_log::{log_query, QueryPhase};

use datalove_datafun_ast::ast::*;
use datalove_datafun_ast::script::{Script, ScriptUnit};
use datalove_datafun_resolve::{module_function_asts, resolve_module_exports};
use datalove_datalit as datalit;
use crate::context::{InheritedBindings, TypeContext};
use crate::statement::check_statement;

pub use datalove_datafun_ast::spans::DatafunSpans;
pub use bct::module_graph::ModuleId;

pub use crate::{
    CollectedNames,
    PendingDiagnostic,
    Type,
    TypeFunction,
    TypeError,
    TypeErrorEntry,
    ScriptUnitKind,
    ScriptUnitSpec,
    ScriptEnv,
    UnitTypecheckResultTracked,
    ScriptUnitsTypecheckResultTracked,
    ParsedModuleGraph,
    ModuleExports,
    ModuleImports,
    ModuleGraphTypecheckResult,
    SingleModuleTypecheckResult,
    AllModuleNameResolutions,
};

/// What one script unit leaves behind for the units after it.
///
/// A cheap projection in front of [`typecheck_script_unit`], and the firewall
/// that makes [`binding_at`] worth having. A unit's whole typecheck output moves
/// whenever anything in its body moves -- it holds the expression types -- so
/// walking back over those to answer "who provides `x`" would re-run the walk on
/// every edit anywhere earlier. This moves only when what the unit *provides*
/// moves, so an edit that changes a body and nothing else stops here.
///
/// A unit that failed to typecheck provides nothing, which is what the
/// accumulate-forward version did too: its bindings would carry types the
/// compiler never settled on.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
#[derive(salsa::SalsaValue)]
pub struct UnitProvides<'db> {
    /// Variables: (name, type, is_mutable).
    pub vars: Vec<(InternedText<'db>, Type<'db>, bool)>,
    pub fns: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    pub fn_asts: Vec<(InternedText<'db>, StmtFun<'db>, Option<ModuleId<'db>>)>,
    /// Module aliases from require statements: (alias, full path).
    pub module_aliases: Vec<(InternedText<'db>, String)>,
    /// Which of `vars` are consts rather than `let`/`var` bindings.
    ///
    /// Const-ness used to be lost at the unit boundary: a script `const`
    /// arrived in the next unit's `variables` and not its `const_bindings`, so
    /// it read as an ordinary binding. That only became visible once function
    /// bodies stopped seeing enclosing bindings, which is what they should
    /// never have seen -- a const is the one kind a body may name.
    pub consts: Vec<InternedText<'db>>,
}

/// What an earlier unit left behind under one name.
///
/// The slots are namespaces rather than alternatives: a name can be a variable
/// and a function at once, because the typechecker keeps those in separate maps
/// and resolves a use against whichever one the use is. Each slot is filled from
/// the nearest earlier unit that provides that *kind* of binding.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
#[derive(salsa::SalsaValue)]
pub struct ScriptBinding<'db> {
    /// The variable bound under this name, and whether it is mutable.
    pub var: Option<(Type<'db>, bool)>,
    /// Whether that variable is a `const`, which decides what a function body
    /// may name and what a const expression may name.
    pub is_const: bool,
    pub func: Option<TypeFunction<'db>>,
    pub func_ast: Option<(StmtFun<'db>, Option<ModuleId<'db>>)>,
}

impl<'db> ScriptBinding<'db> {
    /// Whether no unit provided this name at all.
    fn is_empty(&self) -> bool {
        self.var.is_none() && self.func.is_none() && self.func_ast.is_none()
    }
}

/// Output from typecheck_script_unit.
///
/// Includes the typecheck result AND new bindings defined in this unit
/// for accumulation by the caller.
#[salsa::tracked]
pub struct ScriptUnitTypecheckOutput<'db> {
    #[returns(copy)]
    pub result: UnitTypecheckResultTracked<'db>,
    /// Variables: (name, type, is_mutable).
    #[returns(ref)]
    pub new_vars: Vec<(InternedText<'db>, Type<'db>, bool)>,
    #[returns(ref)]
    pub new_fns: Vec<(InternedText<'db>, TypeFunction<'db>)>,
    #[returns(ref)]
    pub new_fn_asts: Vec<(InternedText<'db>, StmtFun<'db>, Option<ModuleId<'db>>)>,
    /// Module aliases from require statements: (alias, full path).
    #[returns(ref)]
    pub new_module_aliases: Vec<(InternedText<'db>, String)>,
    /// Which of `new_vars` are consts.
    #[returns(ref)]
    pub new_consts: Vec<InternedText<'db>>,
    /// Every name this unit asked the environment about.
    ///
    /// The other fields say what a unit *provides*; this says what it *uses*,
    /// and between them they are the dependency graph script reactivity is
    /// built on. Resolve a name here to the nearest earlier unit that provides
    /// it and that is the edge. See `TypeContext::asked_names` for why a name
    /// the unit binds itself is in here too, and
    /// `botdocs/plan-script-reactivity.md` for what it is for.
    #[returns(ref)]
    pub asked_names: Vec<InternedText<'db>>,
    /// The modules this unit's `import` statements resolve into, by path.
    ///
    /// The module-shaped half of the dependency graph, and the edge a module
    /// edit is propagated along: `asked_names` reaches the units that use what
    /// *another unit* provides, and says nothing about the modules, so editing
    /// a module has no edge to travel without this. Recorded here rather than
    /// recomputed where the reach is walked, because resolving an alias to a
    /// path is the import resolution's own answer -- an alias may come from
    /// this unit's `require` or an earlier unit's, and an alias that names
    /// nothing is taken as a path outright.
    ///
    /// Sorted and without duplicates, since it is part of a memoized value's
    /// identity.
    #[returns(ref)]
    pub imported_modules: Vec<String>,
}

/// One unit's parse, name resolution and spans.
///
/// Keyed on the unit alone, because none of it depends on the units around it.
/// A `ScriptUnit` is interned over a `Source`, so this key survives an edit --
/// the text behind the source changes and this re-runs, and any unit whose text
/// did not change is untouched.
#[salsa::tracked(returns(ref))]
pub fn unit_ast<'db>(
    db: &'db dyn crate::Db,
    unit: ScriptUnit<'db>,
) -> ScriptUnitSpec<'db> {
    let source = unit.source(db);
    let spans = datalove_datafun_parser::datafun_spans(db, source);
    let kind = if unit.is_expr(db) {
        ScriptUnitKind::Expr(datalove_datafun_parser::parse_expr(db, source))
    } else {
        let parsed = datalove_datafun_parser::parse(db, source).parsed.clone();
        let names = datalove_datafun_resolve::resolve_script_names(db, source, parsed.clone());
        ScriptUnitKind::Fragment(parsed, names)
    };
    ScriptUnitSpec::new(source, spans, kind)
}

/// What the last unit of `script` provides to the units after it.
///
/// See [`UnitProvides`] for why this is a separate query and not a read of
/// `typecheck_script_unit`'s output.
#[salsa::tracked(returns(ref))]
pub fn unit_provides<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    env: ScriptEnv<'db>,
) -> UnitProvides<'db> {
    let output = typecheck_script_unit(db, script, env);
    if !output.result(db).errors(db).is_empty() {
        return UnitProvides::default();
    }
    UnitProvides {
        vars: output.new_vars(db).C(),
        fns: output.new_fns(db).C(),
        fn_asts: output.new_fn_asts(db).C(),
        module_aliases: output.new_module_aliases(db).C(),
        consts: output.new_consts(db).C(),
    }
}

/// The binding `name` resolves to among `script`'s units, newest first.
///
/// **This is what replaced seeding a unit with every earlier unit's bindings.**
/// Seeding meant the whole prefix's outputs were part of a per-unit memo key, so
/// editing one unit re-keyed every unit after it whether or not it used anything
/// that changed. Here the dependency *is* the read: a unit depends on the names
/// it asked about and nothing else, and there is no key to keep in step.
///
/// Called with the script up to the unit *before* the one being checked, so a
/// unit never resolves a name against itself through this.
///
/// The walk goes all the way back even once something is found, so that a name
/// which is a variable in one unit and a function in another resolves the way it
/// did when the prefix was accumulated forward: per namespace, nearest wins.
#[salsa::tracked(returns(clone))]
pub fn binding_at<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    env: ScriptEnv<'db>,
    name: InternedText<'db>,
) -> Option<ScriptBinding<'db>> {
    let mut found = ScriptBinding::default();
    let mut current = Some(script);
    while let Some(prefix) = current {
        let provides = unit_provides(db, prefix, env);
        if found.var.is_none() {
            if let Some((_, ty, is_mutable)) =
                provides.vars.iter().rev().find(|(n, _, _)| *n == name)
            {
                found.var = Some((ty.clone(), *is_mutable));
                found.is_const = provides.consts.contains(&name);
            }
        }
        if found.func.is_none() {
            if let Some((_, func_ty)) = provides.fns.iter().rev().find(|(n, _)| *n == name) {
                found.func = Some(*func_ty);
            }
        }
        if found.func_ast.is_none() {
            if let Some((_, ast, module_id)) =
                provides.fn_asts.iter().rev().find(|(n, _, _)| *n == name)
            {
                found.func_ast = Some((*ast, *module_id));
            }
        }
        current = prefix.prev(db);
    }
    if found.is_empty() { None } else { Some(found) }
}

/// The module path `alias` names, from the nearest earlier `require`.
///
/// A require in one REPL line is visible to an import in a later one, and this
/// is that, asked for one alias at a time rather than carried along as a map.
#[salsa::tracked(returns(clone))]
fn module_alias_at<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    env: ScriptEnv<'db>,
    alias: InternedText<'db>,
) -> Option<String> {
    let mut current = Some(script);
    while let Some(prefix) = current {
        let aliases = &unit_provides(db, prefix, env).module_aliases;
        if let Some((_, path)) = aliases.iter().rev().find(|(a, _)| *a == alias) {
            return Some(path.C());
        }
        current = prefix.prev(db);
    }
    None
}

/// Typecheck the last unit of `script`, against the units before it.
///
/// **Keyed on a position in the script and nothing derived from it.** A
/// `Script` handle is "this unit, and the units before it", built out of
/// interned nodes over each unit's `Source`, and a source's identity is
/// independent of its text -- so editing any unit moves no key here. What the
/// earlier units left behind is reached through [`binding_at`] on a lookup miss,
/// which is what confines an edit to the units that used what changed. See
/// `botdocs/plan-script-reactivity.md`.
#[salsa::tracked(returns(copy))]
pub fn typecheck_script_unit<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    env: ScriptEnv<'db>,
) -> ScriptUnitTypecheckOutput<'db> {
    let auto_adapt_mode = env.auto_adapt_mode(db);

    // Handles only. A module's parse and name resolution are asked for one
    // module at a time, where an import names it.
    let modules_by_path = script_modules_by_path(db, env);

    let unit_spec = unit_ast(db, script.unit(db));
    let mut ctx = TypeContext::with_options(db, unit_spec.spans.clone(), None, auto_adapt_mode);

    // The environment the earlier units left is read on a lookup miss rather
    // than seeded into the context, which is the whole of stage B.
    let earlier = script.prev(db);
    ctx.inherited = earlier.map(|script| InheritedBindings { script, env });

    // Script units have Result<()> return type for try operators.
    let unit_tuple_ty = datalit::tycheck::Type::AnonTuple(datalit::tycheck::TypeAnonTuple { fields: Vec::new() });
    let result_unit_ty = datalit::tycheck::Type::Result(
        datalit::tycheck::TypeResult { inner_type: Box::new(unit_tuple_ty) }
    );
    ctx.expected_return_type = Some(Type::Datalit(result_unit_ty));

    // Track new bindings defined in this unit.
    let mut new_vars = Vec::new();
    let mut new_consts = Vec::new();
    let mut new_fns = Vec::new();
    let mut new_fn_asts = Vec::new();
    let mut new_module_aliases = Vec::new();
    let mut imported_modules = Vec::new();

    // Typecheck this unit based on kind.
    match &unit_spec.kind {
        ScriptUnitKind::Fragment(parsed, collected) => {
            // Use pre-computed name resolution.
            ctx.seed_from_collected_names(collected, None);

            new_module_aliases = collect_module_aliases(db, parsed);

            // Resolve imports using shared helper.
            let (resolved_imports, import_errors, imports) = resolve_script_imports(
                db,
                parsed,
                &new_module_aliases,
                |alias| earlier.and_then(|prev| module_alias_at(db, prev, env, alias)),
                &modules_by_path,
            );
            imported_modules = imports;

            // Add resolved imports to context and track as new bindings.
            //
            // A name carried in from an earlier unit is bound already, so an
            // import of a different function under it would decide the calls
            // that one is answering. In a session the two arrive on separate
            // lines, which is exactly where the shadowing is hardest to see.
            let mut bound: HashMap<InternedText<'db>, Option<ModuleId<'db>>> = HashMap::new();

            for (item_name, func_ty, func_ast, source_module_id, local_index) in resolved_imports {
                let first = match bound.get(&item_name) {
                    Some(first) => Some(*first),
                    None => earlier
                        .and_then(|prev| binding_at(db, prev, env, item_name))
                        .and_then(|binding| binding.func_ast)
                        .map(|(_, module_id)| module_id),
                };
                if let Some(first) = first {
                    if first != source_module_id {
                        let err = ctx.error_duplicate_import(
                            local_index,
                            item_name,
                            &module_description(db, first),
                            &module_description(db, source_module_id),
                        );
                        ctx.add_error(err);
                    }
                    continue;
                }
                bound.insert(item_name, source_module_id);
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
            for statement in parsed.statements.iter() {
                check_statement(&mut ctx, &statement);
            }

            // Extract new bindings for subsequent units.
            for stmt in parsed.statements.iter() {
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
                            new_consts.push(name);
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

    // Expressions auto-adapt accepted by adjusting their type; lowering has to
    // supply the conversion the source left out.
    let mut adapt_sites = datalove_datafun_sema::AdaptSites::default();
    for adaptation in ctx.auto_adaptations() {
        adapt_sites.insert(adaptation.expr_key);
    }

    // What this unit asked the environment for, taken before `ctx` is consumed.
    let asked_names = ctx.asked_names();

    // Build result for this unit.
    let errors = ctx.errors.into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();
    // This unit's own functions -- the ones it declared and the ones it
    // imported -- and not the earlier units', which are no longer seeded here.
    // Every consumer looks these up by the name of a function among this unit's
    // statements, so the earlier units' were never read.
    //
    // Sorted because a `HashMap`'s iteration order comes from a per-process seed
    // and this is part of a memoized value's identity.
    let mut function_types: Vec<_> = ctx.functions.into_iter().collect();
    function_types.sort_by(|(a, _), (b, _)| a.as_str(db).cmp(b.as_str(db)));
    let result = UnitTypecheckResultTracked::new(
        db, errors, ctx.expr_types, ctx.call_targets, function_types, adapt_sites,
    );

    ScriptUnitTypecheckOutput::new(
        db, result, new_vars, new_fns, new_fn_asts, new_module_aliases, new_consts,
        asked_names, imported_modules)
}

/// Typecheck every unit of `script`, oldest first.
///
/// An aggregation over [`typecheck_script_unit`], which is where the
/// memoization that matters lives: this re-runs whenever a unit is appended or
/// edited and takes all but the affected units from their memos.
#[salsa::tracked(returns(copy))]
pub fn type_check_script_units<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    env: ScriptEnv<'db>,
) -> ScriptUnitsTypecheckResultTracked<'db> {
    let mut chain = script.chain(db);
    chain.reverse();

    let mut results = Vec::new();
    let mut unit_outputs = Vec::new();
    for prefix in chain {
        let output = typecheck_script_unit(db, prefix, env);
        results.push(output.result(db));
        unit_outputs.push(output);
    }

    ScriptUnitsTypecheckResultTracked::new(db, results, unit_outputs)
}

/// A one-unit script and an empty environment, for checking a fragment alone.
///
/// The environment is empty, so the fragment may not import; everything that
/// wants modules builds its own [`ScriptEnv`].
pub fn single_fragment_script<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
    auto_adapt_mode: crate::AutoAdaptMode,
) -> (Script<'db>, ScriptEnv<'db>) {
    let unit = ScriptUnit::new(db, source, false);
    (Script::new(db, None, unit), ScriptEnv::new(db, Vec::new(), auto_adapt_mode))
}

/// Typecheck a single script fragment using the production path.
pub fn type_check_single_script<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> UnitTypecheckResultTracked<'db> {
    let (script, env) = single_fragment_script(db, source, crate::AutoAdaptMode::Disabled);
    type_check_script_units(db, script, env).results(db)[0]
}

/// Typecheck a module graph (package-agnostic).
///
/// This is the core typechecking function that works with the package-agnostic
/// ModuleGraph abstraction. Modules are processed in dependency order.
use bct::module_graph::Module;

/// Resolved import data as plain tuple (not tracked).
///
/// Contains: (local_name, func_type, func_ast, source_module)
pub type ResolvedImportData<'db> =
    (InternedText<'db>, TypeFunction<'db>, Option<StmtFun<'db>>, ModuleId<'db>, u32);

/// Typecheck a single module with logging.
///
/// This is a tracked function so Salsa can observe per-module execution.
/// The logging only fires when the function actually executes.
///
/// Takes pre-resolved name resolution (type aliases, function signatures) and
/// resolved imports as plain data. Each module's typecheck only depends on its
/// own name resolution and imports, enabling proper per-module caching.
#[salsa::tracked(returns(copy))]
pub fn typecheck_module<'db>(
    db: &'db dyn crate::Db,
    module: Module<'db>,
    parsed: ParsedStatements<'db>,
    spans: DatafunSpans<'db>,
    name_resolution: crate::ModuleNameResolution<'db>,
    resolved_imports: Vec<ResolvedImportData<'db>>,
    import_errors: Vec<TypeError>,
    auto_adapt_mode: crate::AutoAdaptMode,
) -> SingleModuleTypecheckResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("typecheck", module_path, QueryPhase::Start);

    // Create type context for this module with module_id for pending diagnostics.
    let mut ctx = TypeContext::with_options(db, spans, Some(module_id), auto_adapt_mode);

    // Add import errors to context.
    for err in import_errors {
        ctx.add_error(err);
    }

    // Add imported functions to context.
    //
    // A name binds one function, so a second import under one already bound
    // is refused rather than allowed to take it over. The first stays, which
    // keeps the rest of the module resolving to what it was written against.
    let mut import_sources: HashMap<InternedText<'db>, String> = HashMap::new();
    for (local_name, func_type, func_ast, source_module, local_index) in &resolved_imports {
        let source = source_module.path(db).to_string();
        if let Some(first) = import_sources.get(local_name) {
            if *first != source {
                let err = ctx.error_duplicate_import(*local_index, *local_name, first, &source);
                ctx.add_error(err);
            }
            continue;
        }
        import_sources.insert(*local_name, source);
        if let Some(ast) = func_ast {
            ctx.add_imported_function(*local_name, *func_type, *ast, *source_module);
        } else {
            ctx.add_function(*local_name, *func_type);
        }
    }

    // Seed context from pre-computed name resolution (replaces Pass 0/1).
    ctx.seed_from_name_resolution(&name_resolution, Some(module_id));

    // Pass 2a: module-level consts, before anything that could name one. A
    // module const is in scope for every function in the module regardless of
    // where it is written, and functions are checked in source order below.
    for statement in parsed.statements.iter() {
        if matches!(statement, Statement::Const(_)) {
            check_statement(&mut ctx, &statement);
        }
    }

    // Pass 2b: everything else.
    for statement in parsed.statements.iter() {
        if !matches!(statement, Statement::Const(_)) {
            check_statement(&mut ctx, &statement);
        }
    }

    // Use exports from name resolution (already computed).
    let exports = name_resolution.functions(db).clone();
    let type_aliases = name_resolution.type_aliases(db).clone();

    // Build imports list for result.
    let imports: Vec<_> = resolved_imports.iter()
        .map(|(local_name, _, _, source_module, _)| (*local_name, *source_module, *local_name))
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
    graph: bct::module_graph::ModuleGraph<'db>,
    module_parsed: HashMap<ModuleId<'db>, ParsedStatements<'db>>,
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
    let module_parsed: HashMap<ModuleId<'db>, ParsedStatements<'db>> = parsed_graph.statements_only(db)
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
#[salsa::tracked(returns(copy))]
pub fn typecheck_module_graph<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
    all_names: AllModuleNameResolutions<'db>,
    auto_adapt_mode: crate::AutoAdaptMode,
) -> ModuleGraphTypecheckResult<'db> {
    let prep = prepare_typecheck(db, parsed_graph);

    // ========================================================================
    // TYPECHECK PASS
    // ========================================================================
    let mut module_errors: BTreeMap<ModuleId<'db>, Vec<TypeError>> = BTreeMap::new();
    let mut module_exports_map: BTreeMap<ModuleId<'db>, ModuleExports<'db>> = BTreeMap::new();
    let mut module_imports_map: BTreeMap<ModuleId<'db>, ModuleImports<'db>> = BTreeMap::new();
    let mut module_results_map: BTreeMap<ModuleId<'db>, SingleModuleTypecheckResult<'db>> = BTreeMap::new();

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
        let result = typecheck_module(db, module, parsed, spans, name_resolution, resolved_imports, import_errors, auto_adapt_mode);

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

        // Process pending diagnostics with span enrichment.
        emit_pending_diagnostics_for_module(db, &parsed_graph, module_id, result.pending_diagnostics(db));


        // Store the per-module result for use by downstream phases.
        module_results_map.insert(module_id, result);
    }

    ModuleGraphTypecheckResult::new(db, prep.graph, module_errors, module_exports_map, module_imports_map, module_results_map)
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
    auto_adapt_mode: crate::AutoAdaptMode,
) -> ModuleGraphTypecheckResult<'db> {
    use rmx::rayon::prelude::*;

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
        let import_resolution = resolve_module_imports(db_salsa, module, parsed_graph);
        let resolved_imports = import_resolution.imports(db_salsa).C();
        let import_errors = import_resolution.errors(db_salsa).C();

        // Typecheck (tracked, memoized per module).
        let spans = DatafunSpans::new(vec![]);
        let _ = typecheck_module(db_salsa, module, parsed, spans, name_resolution, resolved_imports, import_errors, auto_adapt_mode);
    });

    // Delegate to tracked function which aggregates results.
    // All resolve_module_names, resolve_module_imports, and typecheck_module calls will be cache hits.
    typecheck_module_graph(db_salsa, parsed_graph, all_names, auto_adapt_mode)
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
    mode: crate::ParallelMode,
    auto_adapt_mode: crate::AutoAdaptMode,
) -> ModuleGraphTypecheckResult<'db> {
    match mode {
        crate::ParallelMode::Sequential => typecheck_module_graph(db.as_salsa_db(), parsed_graph, all_names, auto_adapt_mode),
        crate::ParallelMode::Parallel => typecheck_module_graph_parallel(db, parsed_graph, all_names, auto_adapt_mode),
    }
}

/// Emit pending diagnostics for a module using spans from the parsed module graph.
fn emit_pending_diagnostics_for_module<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: &ParsedModuleGraph<'db>,
    module_id: ModuleId<'db>,
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
/// **Keyed on the module and the graph, and nothing wider.** It used to take
/// gathered maps of every module's exports and function ASTs, whose identity is
/// a hash of the lot, so a signature edit in any module gave this query a new
/// key for *every* module -- and a new key is a new query instance, which
/// re-minted the synthetic `StmtFun` a rider import stands behind under a fresh
/// id, which re-typechecked every module that imports from a rider. Eleven of
/// twenty-five on the system library, for an edit none of them could see. The
/// dependencies are asked for per module instead, of the modules this one
/// actually requires, so an edit reaches the importers of what changed and
/// stops there. `import_memo_tests` holds it.
#[salsa::tracked(returns(copy))]
pub fn resolve_module_imports<'db>(
    db: &'db dyn crate::Db,
    module: Module<'db>,
    parsed_graph: ParsedModuleGraph<'db>,
) -> crate::ModuleImportResolution<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("resolve_imports", module_path, QueryPhase::Start);

    // This module's own statements, asked for per module. Finding them in the
    // graph's gathered `statements_only` instead is one dependency edge on
    // every module's AST, which is the rest of the world by another route.
    let parsed = datalove_datafun_parser::parse_module_ast(db, module);

    // The exports and function ASTs of the modules this one requires. The graph
    // is interned, so looking a module up in it costs nothing and depends on
    // nothing computed.
    let graph = parsed_graph.graph(db);
    let mut dep_exports: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, TypeFunction<'db>)>> =
        BTreeMap::new();
    let mut dep_function_asts: BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, StmtFun<'db>)>> =
        BTreeMap::new();
    for (_alias, target_id) in parsed_graph.get_requires(db, module_id) {
        let dep = graph.get_module(db, *target_id)
            .expect("a resolved require names a module in the graph");
        dep_exports.insert(*target_id, resolve_module_exports(db, dep).clone());
        dep_function_asts.insert(*target_id, module_function_asts(db, dep).clone());
    }

    // Call internal implementation.
    let (imports, errors) = resolve_module_imports_internal(
        db,
        module_id,
        parsed,
        &parsed_graph,
        &dep_exports,
        &dep_function_asts,
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
    module_id: ModuleId<'db>,
    parsed: &ParsedStatements<'db>,
    parsed_graph: &ParsedModuleGraph<'db>,
    all_exports: &BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, TypeFunction<'db>)>>,
    module_function_asts: &BTreeMap<ModuleId<'db>, Vec<(InternedText<'db>, StmtFun<'db>)>>,
) -> (Vec<ResolvedImportData<'db>>, Vec<TypeError>) {
    // Build module alias map from pre-resolved requires.
    let resolved_requires = parsed_graph.get_requires(db, module_id);
    let alias_map: HashMap<InternedText<'db>, ModuleId<'db>> = resolved_requires.iter()
        .map(|(alias, target_id)| (*alias, *target_id))
        .collect();

    // Build rider alias map from pre-resolved riders.
    use datalove_datafun_common::RiderInterface;
    let resolved_riders = parsed_graph.get_riders(db, module_id);
    let rider_alias_map: HashMap<InternedText<'db>, &RiderInterface<'db>> = resolved_riders.iter()
        .map(|(alias, rider)| (*alias, rider))
        .collect();

    let mut resolved_imports = Vec::new();
    let mut import_errors = Vec::new();

    for statement in parsed.statements.iter() {
        if let Statement::Import(import) = statement {
            let module_name = import.module_name;
            let item_name = import.item_name;

            if let Some(&source_module_id) = alias_map.get(&module_name) {
                // Module import path.
                if let Some(exports) = all_exports.get(&source_module_id) {
                    let func_opt = exports.iter()
                        .find(|(name, _)| *name == item_name)
                        .map(|(_, func_type)| *func_type);

                    if let Some(func_type) = func_opt {
                        let func_ast = module_function_asts
                            .get(&source_module_id)
                            .and_then(|funcs| funcs.iter().find(|(n, _)| *n == item_name))
                            .map(|(_, ast)| *ast);

                        resolved_imports.push(
                            (item_name, func_type, func_ast, source_module_id, import.local_index),
                        );
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
            } else if let Some(rider) = rider_alias_map.get(&module_name) {
                // Rider import path.
                let func_opt = rider.functions.iter()
                    .find(|(name, _)| *name == item_name)
                    .map(|(_, func_type)| *func_type);

                if let Some(func_type) = func_opt {
                    // Use the rider's pre-created synthetic ModuleId.
                    let synthetic_module_id = rider.module_id;
                    let synthetic_fun = rider.function_stubs.iter()
                        .find(|(n, _)| *n == item_name)
                        .map(|(_, stub)| *stub)
                        .expect("a rider function has a stub, both coming from its interface");
                    resolved_imports.push(
                        (item_name, func_type, Some(synthetic_fun), synthetic_module_id, import.local_index),
                    );
                } else {
                    import_errors.push(TypeError::UnresolvedName(
                        format!("{}.{} (not found in rider)", module_name.as_str(db), item_name.as_str(db))
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

/// The modules an import may name, by the path it names them by.
///
/// Handles only, and deliberately: a module's path is its `ModuleId`'s interned
/// content and a `Module` is interned over that and a `Source`, so building this
/// reads no module's text. The content is [`script_module_spec`], asked for the
/// one module an import resolves into.
fn script_modules_by_path<'db>(
    db: &'db dyn crate::Db,
    env: ScriptEnv<'db>,
) -> HashMap<String, Module<'db>> {
    env.modules(db).iter()
        .map(|module| (module.id(db).path(db).C(), *module))
        .collect()
}

/// What a module declares, for a script unit that imports from it.
///
/// Keyed on the `Module`, which is interned over `(ModuleId, Source)` and so
/// survives a `set_text` -- a unit that imports from one module therefore
/// depends on that module and no other. This replaced a whole-world map over
/// every module's name resolution; see `botdocs/salsa-patterns.md` on a
/// whole-world value in a per-unit key, which this was the sixth of.
///
/// **Only the declarations, which is what makes a body edit free.**
/// `parse_module_ast` compares equal across a body edit -- a `StmtFun`'s body
/// rides a tracked field -- so `resolve_script_names` does not re-run and this
/// backdates. A unit's typecheck cannot depend on a body in any case:
/// `synthesize` reads a looked-up function's `type_params` and `type_bounds`,
/// which are signature, and comptime evaluation of one happens in lowering.
/// This used to return a whole `ModuleSpec`, whose `spans` move with a body, so
/// an importing unit re-typechecked for an edit that could not change its
/// answer.
#[salsa::tracked(returns(ref))]
pub fn script_module_names<'db>(
    db: &'db dyn crate::Db,
    module: Module<'db>,
) -> CollectedNames<'db> {
    let parsed = datalove_datafun_parser::parse_module_ast(db, module).clone();
    datalove_datafun_resolve::resolve_script_names(db, module.source(db), parsed)
}

/// The signature and AST of the function a module exports under `name`.
///
/// Both halves or neither, because a signature with no AST is a function
/// nothing across the boundary can inline or call. Two linear scans of one
/// module's declarations, where this used to be a map built over every module's
/// -- the narrow lookup is the cheap half of keying the spec per module.
fn module_function<'db>(
    collected: &CollectedNames<'db>,
    name: InternedText<'db>,
) -> Option<(TypeFunction<'db>, StmtFun<'db>)> {
    let func_ty = collected.functions.iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, func_ty)| *func_ty)?;
    let ast = collected.function_asts.iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, ast)| *ast)?;
    Some((func_ty, ast))
}

/// Collect the module aliases a script unit's require statements introduce.
///
/// Returns (alias, full path) pairs.
fn collect_module_aliases<'db>(
    db: &'db dyn crate::Db,
    script: &ParsedStatements<'db>,
) -> Vec<(InternedText<'db>, String)> {
    let mut aliases = Vec::new();
    for statement in script.statements.iter() {
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
            aliases.push((module_alias, full_path));
        }
    }
    aliases
}

/// Name a module for an error message, or say it has none.
///
/// A function defined in the session itself has no module to name.
fn module_description<'db>(db: &'db dyn crate::Db, module_id: Option<ModuleId<'db>>) -> String {
    match module_id {
        Some(id) => id.path(db).to_string(),
        None => "this session".to_string(),
    }
}

/// Resolve imports for a script unit using path-based module lookup.
///
/// An alias is looked for among this unit's own `require`s first and then, by
/// `inherited_alias`, among the earlier units' -- a `require` on one REPL line
/// is visible to an `import` on a later one. An alias that names nothing is
/// taken as a path, which is how a module is imported without a require.
///
/// The third return is every module path the unit's imports named, sorted and
/// deduplicated -- including one whose function was not found, since a unit
/// that failed to import from a module still depends on that module: edit it to
/// declare the function and the unit's answer changes.
fn resolve_script_imports<'db>(
    db: &'db dyn crate::Db,
    script: &ParsedStatements<'db>,
    own_aliases: &[(InternedText<'db>, String)],
    inherited_alias: impl Fn(InternedText<'db>) -> Option<String>,
    modules_by_path: &HashMap<String, Module<'db>>,
) -> (
    Vec<(InternedText<'db>, TypeFunction<'db>, StmtFun<'db>, Option<ModuleId<'db>>, u32)>,
    Vec<TypeError>,
    Vec<String>,
) {
    // Resolve import statements.
    let mut resolved = Vec::new();
    let mut errors = Vec::new();
    let mut imported = std::collections::BTreeSet::new();

    for statement in script.statements.iter() {
        if let Statement::Import(import) = statement {
            let module_alias = import.module_name;
            let item_name = import.item_name;

            // Look up the full path from the alias.
            let module_path = own_aliases.iter()
                .rev()
                .find(|(alias, _)| *alias == module_alias)
                .map(|(_, path)| path.C())
                .or_else(|| inherited_alias(module_alias))
                .unwrap_or_else(|| module_alias.as_str(db).S());
            let module_path = module_path.as_str();
            imported.insert(module_path.S());

            // The one module this import names, and no other: asking
            // `script_module_names` here is the dependency edge that confines a
            // module edit to the units that import from it.
            if let Some(module) = modules_by_path.get(module_path) {
                if let Some((func_ty, func_ast)) =
                    module_function(script_module_names(db, *module), item_name)
                {
                    resolved.push(
                        (item_name, func_ty, func_ast, Some(module.id(db)), import.local_index),
                    );
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

    (resolved, errors, imported.into_iter().collect())
}
