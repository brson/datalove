//! Public API for datafun typechecking.
//!
//! Provides salsa-tracked entry points and public functions for typechecking
//! scripts, expressions, and module graphs.

use rmx::prelude::*;
use std::collections::HashMap;
use bct::lookup::LookupMap;
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
    pub fn_asts: Vec<(InternedText<'db>, FunHead<'db>, Option<ModuleId<'db>>)>,
    /// Module aliases from require statements: (alias, full path).
    pub module_aliases: Vec<(InternedText<'db>, String)>,
    /// Type aliases from `type` statements.
    pub type_aliases: Vec<(InternedText<'db>, Type<'db>)>,
    /// Which of `vars` are consts rather than `let`/`var` bindings.
    ///
    /// Const-ness used to be lost at the unit boundary: a script `const`
    /// arrived in the next unit's `variables` as an ordinary binding. That only became visible once function
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
    pub func_ast: Option<(FunHead<'db>, Option<ModuleId<'db>>)>,
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
    pub new_fn_asts: Vec<(InternedText<'db>, FunHead<'db>, Option<ModuleId<'db>>)>,
    /// Module aliases from require statements: (alias, full path).
    #[returns(ref)]
    pub new_module_aliases: Vec<(InternedText<'db>, String)>,
    /// Type aliases from `type` statements.
    #[returns(ref)]
    pub new_type_aliases: Vec<(InternedText<'db>, Type<'db>)>,
    /// Which of `new_vars` are consts.
    #[returns(ref)]
    pub new_consts: Vec<InternedText<'db>>,
    /// Every name this unit asked the environment about.
    ///
    /// The other fields say what a unit *provides*; this says what it *uses*,
    /// and between them they are the dependency graph script reactivity is
    /// built on. Resolve a name here to the nearest earlier unit that provides
    /// it and that is the edge. See `TypeContext::uses` for why a name
    /// the unit binds itself is in here too, and
    /// `botdocs/plan-script-reactivity.md` for what it is for.
    #[returns(ref)]
    pub uses: Vec<InternedText<'db>>,
    /// The modules this unit's `import` statements resolve into, by path.
    ///
    /// The module-shaped half of the dependency graph, and the edge a module
    /// edit is propagated along: `uses` reaches the units that use what
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

/// Whether a unit's text parsed, as a fragment or as an expression.
///
/// **A unit that did not parse provides nothing** to the units after it:
/// [`unit_provides`] answers for it without typechecking the partial tree the
/// parser left, and a script session does not typecheck it at all. It is
/// invisible to the units around it until an edit makes it parse.
/// [`typecheck_script_unit`] itself still checks whatever tree it is given,
/// which is what the typechecker's own tests of recovery from a broken parse
/// rely on.
#[salsa::tracked(returns(copy))]
pub fn unit_parses<'db>(db: &'db dyn crate::Db, unit: ScriptUnit<'db>) -> bool {
    let source = unit.source(db);
    if unit.is_expr(db) {
        datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(
            db, source).is_empty()
    } else {
        datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(
            db, source).is_empty()
    }
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
    if unit.is_expr(db) {
        let parsed = datalove_datafun_parser::parse_expr(db, source).parsed.clone();
        let spans = datalove_datafun_parser::datafun_expr_spans(db, source);
        return ScriptUnitSpec::new(source, spans, ScriptUnitKind::Expr(parsed));
    }
    let parsed = datalove_datafun_parser::parse(db, source).parsed.clone();
    let spans = datalove_datafun_parser::datafun_spans(db, source);
    ScriptUnitSpec::new(source, spans, ScriptUnitKind::Fragment(parsed))
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
    if !unit_parses(db, script.unit(db)) {
        return UnitProvides::default();
    }
    let output = typecheck_script_unit(db, script, env);
    if !output.result(db).errors(db).is_empty() {
        return UnitProvides::default();
    }
    UnitProvides {
        vars: output.new_vars(db).C(),
        fns: output.new_fns(db).C(),
        fn_asts: output.new_fn_asts(db).C(),
        module_aliases: output.new_module_aliases(db).C(),
        type_aliases: output.new_type_aliases(db).C(),
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
                found.func_ast = Some((ast.clone(), *module_id));
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

/// The type `name` names, from the nearest earlier `type`.
///
/// A type declared in one REPL line is known to a later one, asked for one
/// name at a time as [`binding_at`] asks for a binding.
#[salsa::tracked(returns(clone))]
pub fn type_alias_at<'db>(
    db: &'db dyn crate::Db,
    script: Script<'db>,
    env: ScriptEnv<'db>,
    name: InternedText<'db>,
) -> Option<Type<'db>> {
    let mut current = Some(script);
    while let Some(prefix) = current {
        let aliases = &unit_provides(db, prefix, env).type_aliases;
        if let Some((_, ty)) = aliases.iter().rev().find(|(n, _)| *n == name) {
            return Some(ty.clone());
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
    // By the unit's own source, since a unit is typechecked outside its own
    // compile too.
    datalove_ct::unit_analysis::record(datalove_ct::unit_analysis::Pass::Typecheck, script.unit(db).source(db));
    // Handles only. A module's parse and name resolution are asked for one
    // module at a time, where an import names it.
    let modules_by_path = script_modules_by_path(db, env);

    let unit_spec = unit_ast(db, script.unit(db));
    let mut ctx = TypeContext::new(db, unit_spec.spans.clone());
    ctx.data_files = env.data(db).C();
    ctx.record_uses();

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
    let mut new_type_aliases = Vec::new();
    let mut imported_modules: Vec<String>;
    let mut const_needs = Vec::new();

    // Typecheck this unit based on kind.
    match &unit_spec.kind {
        ScriptUnitKind::Fragment(parsed) => {
            let collected = datalove_datafun_resolve::resolve_names_inheriting(
                db,
                &datalove_datafun_ast::ast::Items::of(db, parsed).items,
                &|name| ctx.inherited_type_alias(name),
            );
            ctx.seed_from_collected_names(&collected, None);
            new_type_aliases = collected.type_aliases;

            new_module_aliases = collect_module_aliases(db, parsed);

            let require_errors = check_script_requires(db, parsed, &modules_by_path);
            for error in &require_errors {
                ctx.report_require_error(error);
                // A second module under an alias leaves the first one there.
                if let crate::RequireErrorKind::AliasTaken { alias, path, .. } = &error.kind {
                    if let Some(at) = new_module_aliases.iter()
                        .rposition(|(a, p)| a.as_str(db) == alias && p == path)
                    {
                        new_module_aliases.remove(at);
                    }
                }
            }

            // Resolve imports using shared helper.
            let (resolved_imports, import_errors, imports) = resolve_script_imports(
                db,
                parsed,
                &new_module_aliases,
                |alias| earlier.and_then(|prev| module_alias_at(db, prev, env, alias)),
                &modules_by_path,
            );
            imported_modules = imports;

            let (qualified, qualified_modules) = resolve_script_qualified_calls(
                db,
                &parsed.qualified_calls,
                &new_module_aliases,
                |alias| earlier.and_then(|prev| module_alias_at(db, prev, env, alias)),
                &modules_by_path,
            );
            ctx.qualified = qualified;
            imported_modules.extend(qualified_modules);
            imported_modules.sort();
            imported_modules.dedup();

            // Add resolved imports to context and track as new bindings.
            //
            // A name carried in from an earlier unit is bound already, so an
            // import of a different function under it would decide the calls
            // that one is answering. In a session the two arrive on separate
            // lines, which is exactly where the shadowing is hardest to see.
            let mut bound: bct::lookup::LookupMap<InternedText<'db>, Option<ModuleId<'db>>> = bct::lookup::LookupMap::new();
            let defined = defined_function_names(parsed.statements(db));

            for (item_name, func_ty, func_ast, source_module_id, local_index) in resolved_imports {
                if defined.contains(&item_name) {
                    let err = ctx.error_import_shadows_function(local_index, item_name);
                    ctx.add_error(err);
                    continue;
                }
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
                ctx.import_below(local_index, item_name, func_ty, Some(func_ast.clone()), source_module_id);
                new_fns.push((item_name, func_ty));
                new_fn_asts.push((item_name, func_ast, source_module_id));
            }

            // An import through a refused require was reported at the require.
            let through_failed: std::collections::HashSet<u32> = parsed.statements(db).iter()
                .filter_map(|statement| match statement {
                    Statement::Import(import)
                        if ctx.failed_aliases.contains(import.module_name.as_str(db)) =>
                    {
                        Some(import.local_index)
                    }
                    _ => None,
                })
                .collect();
            for unresolved in &import_errors {
                if unresolved.item.is_none() && through_failed.contains(&unresolved.local_index) {
                    continue;
                }
                ctx.report_unresolved_import(unresolved);
            }

            // Second pass: typecheck all statements, in source order, each
            // declaration coming into scope where it is written.
            ctx.declare_scope(parsed.statements(db));
            ctx.arena = Some(&parsed.body(db).arena);
            for statement in parsed.statements(db).iter() {
                ctx.reach(statement);
                check_statement(&mut ctx, statement);
            }
            const_needs = ctx.check_const_deps(parsed.statements(db));
            ctx.report_inline_cycles();

            // Extract new bindings for subsequent units.
            for stmt in parsed.statements(db).iter() {
                match stmt {
                    Statement::Let(let_stmt) => {
                        for name in let_stmt.binding.names() {
                            if let Some(binding) = ctx.variables.get(&name) {
                                new_vars.push((name, binding.ty.clone(), binding.is_mutable));
                            }
                        }
                    }
                    Statement::Var(var_stmt) => {
                        for name in var_stmt.binding.names() {
                            if let Some(binding) = ctx.variables.get(&name) {
                                new_vars.push((name, binding.ty.clone(), binding.is_mutable));
                            }
                        }
                    }
                    Statement::Const(const_stmt) => {
                        let name = const_stmt.name;
                        if let Some(binding) = ctx.variables.get(&name) {
                            new_vars.push((name, binding.ty.clone(), binding.is_mutable));
                            new_consts.push(name);
                        }
                    }
                    Statement::Fun(fun_stmt) => {
                        let name = fun_stmt.name();
                        if let Some(func_ty) = ctx.functions.get(&name) {
                            new_fns.push((name, *func_ty));
                            new_fn_asts.push((name, fun_stmt.head(), None));
                        }
                    }
                    _ => {}
                }
            }
        }
        ScriptUnitKind::Expr(parsed) => {
            let (qualified, qualified_modules) = resolve_script_qualified_calls(
                db,
                &parsed.qualified_calls,
                &[],
                |alias| earlier.and_then(|prev| module_alias_at(db, prev, env, alias)),
                &modules_by_path,
            );
            ctx.qualified = qualified;
            imported_modules = qualified_modules;
            imported_modules.sort();
            imported_modules.dedup();

            let expr = parsed.expr(db);
            ctx.arena = Some(expr.arena());
            if let Err(e) = ctx.synthesize_expr(expr) {
                ctx.add_error(e);
            }
        }
    }

    // Emit pending diagnostics with local spans.
    ctx.emit_pending_diagnostics();

    // What this unit asked the environment for, taken before `ctx` is consumed.
    let uses = ctx.uses();

    // Build result for this unit.
    let errors = ctx.errors.into_iter()
        .map(|e| TypeErrorEntry::new(db, e))
        .collect();
    // This unit's own functions -- the ones it declared and the ones it
    // imported -- and not the earlier units', which are no longer seeded here.
    // Every consumer looks these up by the name of a function among this unit's
    // statements, so the earlier units' were never read.
    //
    // Sorted because a lookup map has no order and this is part of a memoized
    // value's identity.
    let mut function_types: Vec<_> = ctx.functions.into_iter_unordered().collect();
    function_types.sort_by(|(a, _), (b, _)| a.as_str(db).cmp(b.as_str(db)));
    let result = UnitTypecheckResultTracked::new(
        db, errors, ctx.expr_types, ctx.call_targets, function_types, ctx.resolved_data, const_needs,
    );

    ScriptUnitTypecheckOutput::new(
        db, result, new_vars, new_fns, new_fn_asts, new_module_aliases, new_type_aliases,
        new_consts, uses, imported_modules)
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
) -> (Script<'db>, ScriptEnv<'db>) {
    let unit = ScriptUnit::new(db, source, false);
    (Script::new(db, None, unit), ScriptEnv::new(db, Vec::new(), std::collections::BTreeMap::new()))
}

/// Typecheck a single script fragment using the production path.
pub fn type_check_single_script<'db>(
    db: &'db dyn crate::Db,
    source: bct::input::Source,
) -> UnitTypecheckResultTracked<'db> {
    let (script, env) = single_fragment_script(db, source);
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
    (InternedText<'db>, TypeFunction<'db>, Option<FunHead<'db>>, ModuleId<'db>, u32);

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
    name_resolution: crate::ModuleNameResolution<'db>,
    resolved_imports: Vec<ResolvedImportData<'db>>,
    import_errors: Vec<crate::UnresolvedImport>,
    require_errors: Vec<crate::RequireError>,
    qualified: crate::QualifiedScope<'db>,
    data_files: crate::DataFiles,
) -> SingleModuleTypecheckResult<'db> {
    let module_id = module.id(db);
    let module_path = module_id.path(db);

    log_query("typecheck", module_path, QueryPhase::Start);

    let parsed = datalove_datafun_parser::parse_module_ast(db, module);

    // Create type context for this module with module_id for pending
    // diagnostics, whose spans are found when they are emitted.
    let mut ctx = TypeContext::with_module_id(
        db, DatafunSpans::new(vec![]), Some(module_id));
    ctx.data_files = data_files;

    for error in &require_errors {
        ctx.report_require_error(error);
    }
    // An import through a refused require was reported at the require. In a
    // module an import names its module by alias.
    for unresolved in &import_errors {
        if unresolved.item.is_none() && ctx.failed_aliases.contains(&unresolved.module) {
            continue;
        }
        ctx.report_unresolved_import(unresolved);
    }
    ctx.qualified = qualified;

    // Add imported functions to context.
    //
    // A name binds one function, so a second import under one already bound
    // is refused rather than allowed to take it over. The first stays, which
    // keeps the rest of the module resolving to what it was written against.
    let mut import_sources: bct::lookup::LookupMap<InternedText<'db>, String> = bct::lookup::LookupMap::new();
    let defined = defined_function_names(parsed.statements(db));
    for (local_name, func_type, func_ast, source_module, local_index) in &resolved_imports {
        if defined.contains(local_name) {
            let err = ctx.error_import_shadows_function(*local_index, *local_name);
            ctx.add_error(err);
            continue;
        }
        let source = source_module.path(db).to_string();
        if let Some(first) = import_sources.get(local_name) {
            if *first != source {
                let err = ctx.error_duplicate_import(*local_index, *local_name, first, &source);
                ctx.add_error(err);
            }
            continue;
        }
        import_sources.insert(*local_name, source);
        ctx.import_below(*local_index, *local_name, *func_type, func_ast.clone(), Some(*source_module));
    }

    // Seed context from pre-computed name resolution (replaces Pass 0/1).
    ctx.seed_from_name_resolution(&name_resolution, Some(module_id));

    // One walk in source order. A declaration is in scope from where it is
    // written, a function throughout; see `botdocs/plan-lexical-scope.md`.
    ctx.declare_scope(parsed.statements(db));
    ctx.arena = Some(&parsed.body(db).arena);
    for statement in parsed.statements(db).iter() {
        ctx.reach(statement);
        check_statement(&mut ctx, statement);
    }
    let const_needs = ctx.check_const_deps(parsed.statements(db));
    ctx.report_inline_cycles();

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
        ctx.errors,
        ctx.pending_diagnostics,
        exports,
        type_aliases,
        imports,
        ctx.expr_types,
        ctx.call_targets,
        ctx.resolved_data,
        const_needs,
    )
}

/// One module's typecheck, as `typecheck_module_graph` asks for it: its
/// imports resolved, then `typecheck_module` keyed with them and its names.
///
/// The parallel compile warms exactly this (`datalove_datafun_common::warm`),
/// so that what it computes ahead is what the graph's typecheck then asks for.
pub fn typecheck_graph_module<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: ParsedModuleGraph<'db>,
    all_names: AllModuleNameResolutions<'db>,
    module: Module<'db>,
) -> SingleModuleTypecheckResult<'db> {
    let name_resolution = *all_names.resolutions(db).get(&module.id(db))
        .expect("module should have name resolution");
    let import_resolution = resolve_module_imports(db, module, parsed_graph);
    typecheck_module(
        db, module, name_resolution,
        import_resolution.imports(db).C(), import_resolution.errors(db).C(),
        import_resolution.require_errors(db).C(), import_resolution.qualified(db).C(),
        parsed_graph.graph(db).data(db).C(),
    )
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
) -> ModuleGraphTypecheckResult<'db> {
    let graph = parsed_graph.graph(db);

    // ========================================================================
    // TYPECHECK PASS
    // ========================================================================
    let mut module_errors: LookupMap<ModuleId<'db>, Vec<TypeError>> = LookupMap::new();
    let mut module_exports_map: LookupMap<ModuleId<'db>, ModuleExports<'db>> = LookupMap::new();
    let mut module_imports_map: LookupMap<ModuleId<'db>, ModuleImports<'db>> = LookupMap::new();
    let mut module_results_map: LookupMap<ModuleId<'db>, SingleModuleTypecheckResult<'db>> = LookupMap::new();

    for module in graph.iter_modules(db) {
        let module_id = module.id(db);
        let result = typecheck_graph_module(db, parsed_graph, all_names, module);

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

    ModuleGraphTypecheckResult::new(db, graph, module_errors, module_exports_map, module_imports_map, module_results_map)
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

    // This module's own requires and imports, asked for per module. Finding
    // them in the graph's gathered `statements_only` instead is one dependency
    // edge on every module's AST, which is the rest of the world by another
    // route.
    let items = datalove_datafun_parser::module_uses(db, module);

    // The exports and function ASTs of the modules this one requires. The graph
    // is interned, so looking a module up in it costs nothing and depends on
    // nothing computed.
    let graph = parsed_graph.graph(db);
    let mut dep_exports: LookupMap<ModuleId<'db>, Vec<(InternedText<'db>, TypeFunction<'db>)>> =
        LookupMap::new();
    let mut dep_function_asts: LookupMap<ModuleId<'db>, Vec<(InternedText<'db>, FunHead<'db>)>> =
        LookupMap::new();
    for (_alias, target_id) in parsed_graph.get_requires(db, module_id) {
        let dep = graph.get_module(db, *target_id)
            .expect("a resolved require names a module in the graph");
        dep_exports.insert(*target_id, resolve_module_exports(db, dep).clone());
        dep_function_asts.insert(*target_id, module_function_asts(db, dep).clone());
    }

    // Call internal implementation.
    let (imports, errors, qualified) = resolve_module_imports_internal(
        db,
        module_id,
        items,
        &parsed_graph,
        &dep_exports,
        &dep_function_asts,
    );

    let require_errors = check_module_requires(db, module_id, items, &parsed_graph);

    log_query("resolve_imports", module_path, QueryPhase::End);

    crate::ModuleImportResolution::new(db, module_id, imports, errors, require_errors, qualified)
}

/// Check each `require` a module writes against what resolution made of it.
///
/// A require that resolution dropped names either a module the world does not
/// have or one that leads back here: a cycle is broken by dropping the require
/// that closes it, so the module it names is still in the graph, reached the
/// other way round.
fn check_module_requires<'db>(
    db: &'db dyn crate::Db,
    module_id: ModuleId<'db>,
    items: &Items<'db>,
    parsed_graph: &ParsedModuleGraph<'db>,
) -> Vec<crate::RequireError> {
    use crate::{RequireError, RequireErrorKind as Kind};

    let graph = parsed_graph.graph(db);
    let resolved = parsed_graph.get_requires(db, module_id);
    let riders = parsed_graph.get_riders(db, module_id);

    let mut errors = Vec::new();
    let mut seen = RequireScope::default();
    for item in &items.items {
        let Item::Require(require) = item else {
            continue;
        };
        match require {
            StmtRequire::Module(req) => {
                let path = require_path(db, req);
                let alias = req.module_alias.as_str(db).S();
                if let Some(kind) = seen.add(&alias, &path) {
                    errors.push(RequireError { local_index: req.local_index, alias, kind });
                    continue;
                }
                let target = ModuleId::new(db, path.C());
                if resolved.iter().any(|(_, id)| *id == target) {
                    continue;
                }
                let kind = match graph.get_module(db, target) {
                    Some(_) => {
                        let back = require_chain(db, parsed_graph, target, module_id)
                            .expect("a require dropped for a cycle has a way back");
                        let mut cycle = vec![module_id.path(db).C()];
                        cycle.extend(back.iter().map(|id| id.path(db).C()));
                        Kind::Cycle { cycle }
                    }
                    None => Kind::ModuleNotFound { path },
                };
                errors.push(RequireError { local_index: req.local_index, alias, kind });
            }
            StmtRequire::Rider(req) => {
                let name = req.name.as_str(db).S();
                if let Some(kind) = seen.add(&name, &fmt!("rider {name}")) {
                    errors.push(RequireError { local_index: req.local_index, alias: name, kind });
                    continue;
                }
                if !riders.iter().any(|(alias, _)| *alias == req.name) {
                    errors.push(RequireError {
                        local_index: req.local_index,
                        alias: name.C(),
                        kind: Kind::RiderNotFound { name },
                    });
                }
            }
        }
    }
    errors
}

/// The path a `require module` names.
fn require_path<'db>(db: &'db dyn crate::Db, req: &StmtRequireModule<'db>) -> String {
    format!(
        "{}/{}/{}",
        req.import_space.as_str(db),
        req.package_alias.as_str(db),
        req.module_alias.as_str(db),
    )
}

/// The modules from `from` to `to` along resolved requires, both ends included.
fn require_chain<'db>(
    db: &'db dyn crate::Db,
    parsed_graph: &ParsedModuleGraph<'db>,
    from: ModuleId<'db>,
    to: ModuleId<'db>,
) -> Option<Vec<ModuleId<'db>>> {
    let mut came_from: LookupMap<ModuleId<'db>, ModuleId<'db>> = LookupMap::new();
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(id) = queue.pop_front() {
        if id == to {
            let mut chain = vec![to];
            let mut at = to;
            while at != from {
                at = came_from[&at];
                chain.push(at);
            }
            chain.reverse();
            return Some(chain);
        }
        for (_, next) in parsed_graph.get_requires(db, id) {
            if *next != from && !came_from.contains_key(next) {
                came_from.insert(*next, id);
                queue.push_back(*next);
            }
        }
    }
    None
}

/// The requires one module or script unit has written so far, for refusing the
/// same one twice and two under one alias.
#[derive(Default)]
struct RequireScope {
    /// What each alias names, by the path or rider it was required as.
    aliases: HashMap<String, String>,
}

impl RequireScope {
    /// Note a require of `what` under `alias`, or say why it is refused.
    fn add(&mut self, alias: &str, what: &str) -> Option<crate::RequireErrorKind> {
        use crate::RequireErrorKind as Kind;
        match self.aliases.get(alias) {
            Some(first) if first == what => Some(Kind::Duplicate { what: what.S() }),
            Some(first) => Some(Kind::AliasTaken { alias: alias.S(), first: first.C(), path: what.S() }),
            None => {
                self.aliases.insert(alias.S(), what.S());
                None
            }
        }
    }
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
    items: &Items<'db>,
    parsed_graph: &ParsedModuleGraph<'db>,
    all_exports: &LookupMap<ModuleId<'db>, Vec<(InternedText<'db>, TypeFunction<'db>)>>,
    module_function_asts: &LookupMap<ModuleId<'db>, Vec<(InternedText<'db>, FunHead<'db>)>>,
) -> (Vec<ResolvedImportData<'db>>, Vec<crate::UnresolvedImport>, crate::QualifiedScope<'db>) {
    // Build module alias map from pre-resolved requires.
    let resolved_requires = parsed_graph.get_requires(db, module_id);
    let alias_map: bct::lookup::LookupMap<InternedText<'db>, ModuleId<'db>> = resolved_requires.iter()
        .map(|(alias, target_id)| (*alias, *target_id))
        .collect();

    // Build rider alias map from pre-resolved riders.
    use datalove_datafun_common::RiderInterface;
    let resolved_riders = parsed_graph.get_riders(db, module_id);
    let rider_alias_map: bct::lookup::LookupMap<InternedText<'db>, &RiderInterface<'db>> = resolved_riders.iter()
        .map(|(alias, rider)| (*alias, rider))
        .collect();

    let mut resolved_imports = Vec::new();
    let mut import_errors = Vec::new();

    for item in &items.items {
        if let Item::Import(import) = item {
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
                            .map(|(_, ast)| ast.clone());

                        resolved_imports.push(
                            (item_name, func_type, func_ast, source_module_id, import.local_index),
                        );
                    } else {
                        import_errors.push(crate::UnresolvedImport {
                            local_index: import.local_index,
                            module: module_name.as_str(db).S(),
                            item: Some(item_name.as_str(db).S()),
                        });
                    }
                } else {
                    // A required module that exports nothing.
                    import_errors.push(crate::UnresolvedImport {
                        local_index: import.local_index,
                        module: module_name.as_str(db).S(),
                        item: Some(item_name.as_str(db).S()),
                    });
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
                        .map(|(_, stub)| stub.clone())
                        .expect("a rider function has a stub, both coming from its interface");
                    resolved_imports.push(
                        (item_name, func_type, Some(synthetic_fun), synthetic_module_id, import.local_index),
                    );
                } else {
                    import_errors.push(crate::UnresolvedImport {
                        local_index: import.local_index,
                        module: module_name.as_str(db).S(),
                        item: Some(item_name.as_str(db).S()),
                    });
                }
            } else {
                import_errors.push(crate::UnresolvedImport {
                    local_index: import.local_index,
                    module: module_name.as_str(db).S(),
                    item: None,
                });
            }
        }
    }

    let qualified = resolve_qualified_calls(&items.qualified_calls, |alias, name| {
        if let Some(&module_id) = alias_map.get(&alias) {
            let func = all_exports.get(&module_id)
                .and_then(|exports| exports.iter().find(|(n, _)| *n == name))
                .map(|(_, func_type)| *func_type);
            return Some(func.map(|func| {
                let ast = module_function_asts.get(&module_id)
                    .and_then(|funcs| funcs.iter().find(|(n, _)| *n == name))
                    .map(|(_, ast)| ast.clone())
                    .expect("an exported function has an AST, both coming from its module");
                (func, ast, module_id)
            }));
        }
        let rider = rider_alias_map.get(&alias)?;
        let func = rider.functions.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, func_type)| *func_type);
        Some(func.map(|func| {
            let stub = rider.function_stubs.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, stub)| stub.clone())
                .expect("a rider function has a stub, both coming from its interface");
            (func, stub, rider.module_id)
        }))
    });

    (resolved_imports, import_errors, qualified)
}

/// Resolve each qualified call a unit writes.
///
/// `target` answers for one: `None` when nothing is required under the alias,
/// otherwise the function the module or rider has under the name, if it has
/// one.
fn resolve_qualified_calls<'db>(
    calls: &[(InternedText<'db>, InternedText<'db>)],
    mut target: impl FnMut(
        InternedText<'db>,
        InternedText<'db>,
    ) -> Option<Option<(TypeFunction<'db>, FunHead<'db>, ModuleId<'db>)>>,
) -> crate::QualifiedScope<'db> {
    let mut scope = crate::QualifiedScope::default();
    for &(alias, name) in calls {
        let Some(found) = target(alias, name) else { continue };
        if !scope.aliases.contains(&alias) {
            scope.aliases.push(alias);
        }
        if let Some((func, ast, module_id)) = found {
            scope.functions.push(crate::QualifiedFunction { alias, name, func, ast, module_id });
        }
    }
    scope
}

/// Resolve each qualified call a script unit writes, through the aliases its
/// own `require`s give and those of the units before it.
///
/// The second return is the path of every module an alias resolved to, found
/// function or not, for the same reason [`resolve_script_imports`] gives.
fn resolve_script_qualified_calls<'db>(
    db: &'db dyn crate::Db,
    calls: &[(InternedText<'db>, InternedText<'db>)],
    own_aliases: &[(InternedText<'db>, String)],
    inherited_alias: impl Fn(InternedText<'db>) -> Option<String>,
    modules_by_path: &HashMap<String, Module<'db>>,
) -> (crate::QualifiedScope<'db>, Vec<String>) {
    let mut paths = Vec::new();
    let scope = resolve_qualified_calls(calls, |alias, name| {
        let path = own_aliases.iter()
            .rev()
            .find(|(own, _)| *own == alias)
            .map(|(_, path)| path.C())
            .or_else(|| inherited_alias(alias))?;
        let module = *modules_by_path.get(&path)?;
        paths.push(path);
        Some(module_function(script_module_names(db, module), name)
            .map(|(func, ast)| (func, ast, module.id(db))))
    });
    (scope, paths)
}

/// The modules an import may name, by the path it names them by.
///
/// Handles only, and deliberately: a module's path is its `ModuleId`'s interned
/// content and a `Module` is interned over that and a `Source`, so building this
/// reads no module's text. The content is [`script_module_names`], asked for the
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
/// `module_items` compares equal across a body edit, so this does not re-run.
/// A unit's typecheck cannot depend on a body in any case:
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
    datalove_datafun_resolve::resolve_names_impl(db, &datalove_datafun_parser::module_items(db, module).items)
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
) -> Option<(TypeFunction<'db>, FunHead<'db>)> {
    let func_ty = collected.functions.iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, func_ty)| *func_ty)?;
    let ast = collected.function_asts.iter()
        .find(|(declared, _)| *declared == name)
        .map(|(_, ast)| ast.clone())?;
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
    for statement in script.statements(db).iter() {
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

/// Check each `require` a script unit writes.
///
/// Within the unit, a module may be required once and an alias name one module.
/// Across units neither is checked: a later line of a session requiring what
/// an earlier one did is redundant rather than wrong, as an import is.
fn check_script_requires<'db>(
    db: &'db dyn crate::Db,
    parsed: &ParsedStatements<'db>,
    modules_by_path: &HashMap<String, Module<'db>>,
) -> Vec<crate::RequireError> {
    use crate::{RequireError, RequireErrorKind as Kind};

    let mut errors = Vec::new();
    let mut seen = RequireScope::default();
    for statement in parsed.statements(db).iter() {
        let Statement::Require(require) = statement else {
            continue;
        };
        match require {
            StmtRequire::Module(req) => {
                let path = require_path(db, req);
                let alias = req.module_alias.as_str(db).S();
                let kind = match seen.add(&alias, &path) {
                    Some(kind) => kind,
                    None if !modules_by_path.contains_key(&path) => Kind::ModuleNotFound { path },
                    None => continue,
                };
                errors.push(RequireError { local_index: req.local_index, alias, kind });
            }
            StmtRequire::Rider(req) => {
                let name = req.name.as_str(db).S();
                errors.push(RequireError {
                    local_index: req.local_index,
                    alias: name.C(),
                    kind: Kind::RiderInScript { name },
                });
            }
        }
    }
    errors
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
    Vec<(InternedText<'db>, TypeFunction<'db>, FunHead<'db>, Option<ModuleId<'db>>, u32)>,
    Vec<crate::UnresolvedImport>,
    Vec<String>,
) {
    // Resolve import statements.
    let mut resolved = Vec::new();
    let mut errors = Vec::new();
    let mut imported = std::collections::BTreeSet::new();

    for statement in script.statements(db).iter() {
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
                    errors.push(crate::UnresolvedImport {
                        local_index: import.local_index,
                        module: module_path.S(),
                        item: Some(item_name.as_str(db).S()),
                    });
                }
            } else {
                errors.push(crate::UnresolvedImport {
                    local_index: import.local_index,
                    module: module_path.S(),
                    item: None,
                });
            }
        }
    }

    (resolved, errors, imported.into_iter().collect())
}

/// The names the functions written in these statements are defined by.
///
/// A name one of them takes is not free for an import, however its
/// signature turned out.
fn defined_function_names<'db>(
    statements: &[Statement<'db>],
) -> bct::lookup::LookupSet<InternedText<'db>> {
    statements.iter()
        .filter_map(|statement| match statement {
            Statement::Fun(stmt) => Some(stmt.name()),
            Statement::NativeFun(stmt) => Some(stmt.name),
            _ => None,
        })
        .collect()
}

/// Report what name resolution found wrong in a rider interface.
///
/// An interface is not typechecked, there being no bodies in it, but its
/// signatures are resolved as a module's are, and what does not resolve is
/// reported the same way: an unknown type (F064), a native declared twice
/// (F086), a set over an unordered parameter (F061).
pub fn report_interface_resolution_errors<'db>(
    db: &'db dyn crate::Db,
    collected: &datalove_datafun_common::CollectedNames<'db>,
    spans: datalove_datafun_ast::spans::DatafunSpans<'db>,
) {
    let mut ctx = TypeContext::new(db, spans);
    ctx.seed_from_collected_names(collected, None);
    ctx.emit_pending_diagnostics();
}
