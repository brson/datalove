//! Script compilation for script units.
//!
//! The [`ScriptCompiler`] incrementally compiles script fragments and expressions
//! to IR. It maintains accumulated state across compilations to support REPL-style
//! workflows where later units can reference bindings from earlier ones.
//!
//! Create a compiler via [`CompiledModules::script_compiler_default()`]. For execution,
//! use a separate [`ScriptExecutor`](super::ScriptExecutor).
//!
//! # Example
//!
//! ```ignore
//! let mut compiler = compiled.script_compiler_default(&db).unwrap();
//!
//! // Compile a fragment (statements).
//! let result = compiler.compile_fragment("let x = 42");
//! if let Some(ir_unit) = result.ir_unit {
//!     // Pass to executor for execution.
//! }
//!
//! // Later compilation can reference x.
//! let result2 = compiler.compile_fragment("let y = x + 1");
//! ```

use rmx::prelude::*;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use datalove_datafun_ast::ast::{ExprFun, Statement};
use datalove_datafun_compiler::lower::{
    lower_script_fragment_raw, lower_script_expr, lower_script_functions,
    lower_const_binding,
};
use datalove_datafun_const::{inline_script_consts, PreparedConst, ScriptFunctionConstsResult, evaluate_prepared_const};
use datalove_datafun_compiler::tracked_script_lower::{
    UnitLowerRecord, collect_const_graph, dead_externals_over, lower_context_over,
    script_consts_over,
};
use datalove_datafun_compiler::tracked_script_ownership::{
    analyze_script_fragment_tracked, analyze_script_expr_tracked, ScriptAnalysisData,
    emit_ownership_diagnostics, AnalysisError,
};
use datalove_datafun_compiler::lower::ScriptFunctionAnalyses;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, CtfeError, IrCodeUnit, IrType, ResolvedConsts, ConstEvalError};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_tycheck::{
    type_check_script_units, typecheck_script_unit, unit_ast,
    Script, ScriptEnv, ScriptUnit, ScriptUnitKind,
    UnitTypecheckResultTracked,
    AutoAdaptMode,
};
use datalove_datafun_compiler::IrTypeExt;

use super::compiled_modules::{CompiledModules, SharedModuleContext};
use super::result::{TypecheckResult, OwnershipResult, LoweringResult, ScriptCompilationResult};

// ============================================================================
// Phase output types
// ============================================================================

/// Output from typecheck phase.
struct TypecheckOutput<'db> {
    /// The unit's whole output, which carries what it provides and what it
    /// asked for -- the dependency graph the reach of an edit is walked over.
    output: datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>,
    result: UnitTypecheckResultTracked<'db>,
    expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
    call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
}

/// Output from ownership phase.
struct OwnershipOutput<'db> {
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: Option<ScriptAnalysisData<'db>>,
    /// Names this unit exports that hold no value.
    dead_exports: Vec<String>,
    /// Names from earlier units this unit assigned to.
    revived: Vec<String>,
}

/// Lowered functions from the lowering phase.
///
/// Functions are lowered once, then reused for const evaluation
/// and the final IR assembly.
#[derive(Clone)]
struct LoweredFunctions {
    /// Behind `Arc`s to match what module lowering produces, so the const
    /// evaluator takes one slice type from both paths.
    functions: Vec<std::sync::Arc<datalove_datafun_ir::IrCodeUnit>>,
    func_name_to_id: HashMap<String, datalove_datafun_ir::FuncId>,
    /// Functions held back because they named a script const with no value yet.
    deferred: Vec<String>,
}

/// Output from const evaluation phase.
struct ConstEvalOutput {
    resolved_consts: ResolvedConsts,
    func_consts: HashMap<String, (IrType, ConstValue)>,
    /// Every script-level const in scope, this unit's and earlier units'.
    ///
    /// Held apart from `resolved_consts` because those are what the inlining
    /// pass substitutes, which `skip_const_inlining` turns off, while these are
    /// what a function body naming a script const is lowered against, which it
    /// does not: a body resolves the const where the reference is lowered, so
    /// there is no later pass for the flag to skip.
    script_consts: HashMap<String, (IrType, ConstValue)>,
    /// Just this unit's own script-level consts, for its record.
    declared_consts: Vec<(String, IrType, ConstValue)>,
}

/// Parsed script unit ready for compilation.
enum ParsedUnit<'db> {
    /// A fragment (statements).
    ///
    /// The parse itself is not carried here: `unit_ast` holds it, keyed on the
    /// unit, and these are the statements the later phases walk.
    Fragment {
        stmts: Vec<Statement<'db>>,
    },
    /// A single expression.
    Expr(ExprFun<'db>),
}

// Extension impl for CompiledModules to create script compiler.
impl<'db> CompiledModules<'db> {
    /// Create a script compiler for compiling script units.
    ///
    /// The `ctfe_evaluator` is used for compile-time evaluation of const expressions.
    /// Use `InterpCtfeEvaluator` from the interpreter crate.
    ///
    /// Returns `None` if module compilation failed (has errors).
    /// The compiler handles only compilation; use `script_executor()` for execution.
    pub fn script_compiler(
        &self,
        db: &'db dyn salsa::Database,
        ctfe_evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    ) -> Option<ScriptCompiler<'db>> {
        if self.has_errors() {
            return None;
        }

        // The module *handles*, in the graph's dependency order. Nothing about a
        // module's content is read here: a unit asks `script_module_spec` for
        // the modules its imports name, so building this compiler parses no
        // module and resolves no module's names.
        let modules: Vec<bct::module_graph::Module<'db>> =
            self.shared.module_graph.iter_modules(db).collect();

        Some(ScriptCompiler {
            db,
            scripts: Vec::new(),
            env: ScriptEnv::new(db, modules, AutoAdaptMode::Disabled),
            last_script: None,
            unit_records: Vec::new(),
            last_source: None,
            ctfe_evaluator,
            skip_const_inlining: false,
            skip_specialization: false,
            shared_context: self.shared.clone(),
            auto_adapt_mode: AutoAdaptMode::Disabled,
            last_ownership_errors: Vec::new(),
            last_spans: None,
        })
    }

    /// Create a script compiler with the default interpreter-based CTFE evaluator.
    ///
    /// This is the recommended way to create a script compiler. It uses the
    /// interpreter for compile-time evaluation of const expressions.
    ///
    /// Returns `None` if module compilation failed (has errors).
    pub fn script_compiler_default(
        &self,
        db: &'db dyn salsa::Database,
    ) -> Option<ScriptCompiler<'db>> {
        // Use CTFE evaluator with module registry for cross-module const function calls.
        let evaluator = InterpCtfeEvaluator::with_module_registry(self.shared.module_registry.clone());
        let evaluator = match &self.natives {
            Some(natives) => evaluator.with_native_resolver(natives.clone()),
            None => evaluator,
        };
        let evaluator = Rc::new(RefCell::new(evaluator));
        self.script_compiler(db, evaluator)
    }

    /// Create a script compiler that takes up a session an edit interrupted.
    ///
    /// The units' `Source`s are inputs, so interning them again gives back the
    /// very same `ScriptUnit` and `Script` handles the session had before the
    /// edit, and every memo keyed on one of those is still good. See
    /// [`ScriptSession`].
    ///
    /// Returns `None` if module compilation failed (has errors).
    pub fn script_compiler_resumed(
        &self,
        db: &'db dyn salsa::Database,
        session: ScriptSession,
    ) -> Option<ScriptCompiler<'db>> {
        let mut compiler = self.script_compiler_default(db)?;
        compiler.resume(session);
        Some(compiler)
    }
}

/// The part of a script compiler that survives a database mutation.
///
/// A [`ScriptCompiler`] borrows the database for as long as it lives, and
/// editing a unit is `set_text` on a `Source`, which needs `&mut db`. So an
/// edit means letting the compiler go and building another afterwards, and
/// this is what crosses in between: the units' sources, whose identity is
/// independent of their text, and what each unit's compilation produced.
/// Everything tied to the database -- the interned `Script` handles, the
/// module specs, the CTFE evaluator -- is derived again on the far side.
///
/// See `botdocs/plan-script-reactivity.md` for why the compiler cannot simply
/// be handed `&mut db` instead.
pub struct ScriptSession {
    /// Each unit's source and whether it was submitted as a bare expression.
    units: Vec<(bct::input::Source, bool)>,
    /// What each unit's compilation produced, indexed by unit.
    records: Vec<UnitLowerRecord>,
    skip_const_inlining: bool,
    skip_specialization: bool,
    auto_adapt_mode: AutoAdaptMode,
}

impl ScriptSession {
    /// The source of each unit, in order.
    ///
    /// A unit's `Source` is an input, so one of these is the handle an edit is
    /// applied to: `set_text` changes that unit's text and moves no memo key.
    /// The edit needs `&mut db`, which is why it is offered here rather than on
    /// the compiler -- a compiler holding `&'db dyn Database` cannot be alive
    /// across it. See `botdocs/plan-script-reactivity.md`.
    pub fn unit_sources(&self) -> Vec<bct::input::Source> {
        self.units.iter().map(|(source, _)| *source).collect()
    }

    /// Whether each unit was submitted as a bare expression, in order.
    ///
    /// What a re-executed unit has to be run *as*: an expression computes a
    /// value that needs somewhere to land and a fragment computes none, and
    /// after a splice the suffix is run again from the top rather than in
    /// place, so the caller has to know which it is holding.
    pub fn unit_is_expr(&self) -> Vec<bool> {
        self.units.iter().map(|(_, is_expr)| *is_expr).collect()
    }
}

/// Script compiler for compiling script units.
///
/// Handles compilation only. No interpreter dependency.
/// Use `compile_fragment()` or `compile_expr()` to compile units.
pub struct ScriptCompiler<'db> {
    db: &'db dyn salsa::Database,
    /// The script as it stood after each unit, so `scripts[i]` is the handle
    /// every per-unit query about unit `i` is keyed on.
    ///
    /// A handle is a chain node over the unit's `Source`, so appending leaves
    /// the earlier handles alone and editing a unit's text leaves all of them
    /// alone. See `botdocs/plan-script-reactivity.md`.
    scripts: Vec<Script<'db>>,
    /// The modules the script is checked against, and the auto-adapt mode.
    env: ScriptEnv<'db>,
    /// The script as it stood for the last unit *attempted*.
    ///
    /// Held apart from `scripts` because a unit that failed is taken back off
    /// that -- it provides nothing to what follows -- and the diagnostics the
    /// caller is about to render are the failing unit's.
    last_script: Option<Script<'db>>,
    /// What each unit's compilation produced, indexed by unit.
    ///
    /// **Not a running total.** Unit `i` is lowered against the fold of the
    /// records before it, computed on demand, so re-lowering one unit replaces
    /// one record and leaves the rest where they are. A fold could not do that:
    /// after an edit it would describe the pre-edit suffix as well.
    unit_records: Vec<UnitLowerRecord>,
    last_source: Option<bct::input::Source>,
    /// CTFE evaluator for const expression evaluation.
    ctfe_evaluator: Rc<RefCell<dyn CtfeEvaluator>>,
    /// When true, const bindings in functions are lowered as let bindings.
    skip_const_inlining: bool,
    /// When true, comptime calls are left naming the original function.
    skip_specialization: bool,
    /// Shared module context for cross-module CTFE function calls.
    shared_context: Arc<SharedModuleContext<'db>>,
    /// Auto-adapt mode for ownership analysis.
    auto_adapt_mode: AutoAdaptMode,
    /// Last ownership errors from compilation (for diagnostic rendering).
    last_ownership_errors: Vec<AnalysisError<'db>>,
    /// Spans for the last compilation unit (for diagnostic rendering).
    last_spans: Option<datalove_datafun_ast::spans::DatafunSpans<'db>>,
}

impl<'db> ScriptCompiler<'db> {
    /// Compile a script fragment (statements).
    pub fn compile_fragment(&mut self, source: &str) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);

        let parse_diags = datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return result;
        }

        self.append_unit(src, false)
    }

    /// Compile a script expression.
    pub fn compile_expr(&mut self, source: &str) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);

        let parse_diags = datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src);
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return result;
        }

        self.append_unit(src, true)
    }

    /// Put a new unit on the end of the script and compile it.
    ///
    /// A unit that fails comes back off: it provides nothing to what follows,
    /// and leaving it in place would put a hole in the numbering the frame
    /// store and every `(unit, value)` reference share.
    fn append_unit(
        &mut self,
        src: bct::input::Source,
        is_expr: bool,
    ) -> ScriptCompilationResult {
        let script_unit = ScriptUnit::new(self.db, src, is_expr);
        let script = Script::new(self.db, self.scripts.last().copied(), script_unit);
        self.scripts.push(script);
        self.last_script = Some(script);
        self.last_spans = Some(unit_ast(self.db, script_unit).spans.clone());
        self.unit_records.push(UnitLowerRecord::default());

        let index = self.scripts.len() - 1;
        let result = self.compile_unit_at(index);
        if result.ir_unit.is_none() {
            self.scripts.pop();
            self.unit_records.pop();
        }
        result
    }

    /// Re-lower the units an edit to unit `edited` reaches, in index order.
    ///
    /// The units the edit does not reach keep the IR they already have, which
    /// is what makes this worth doing and is sound because **a script value is
    /// identified by `(unit_index, ValueId)`**: a unit's IR names the units it
    /// reads from, so a unit that uses nothing the edited unit provides holds
    /// no reference to it and cannot see it change. Editing rather than
    /// removing a unit leaves the indices alone, so the identification stays
    /// good.
    ///
    /// Each returned index is paired with what compiling that unit came to.
    /// The caller re-executes them in the order given -- which is index order,
    /// because a unit reads only from units before it.
    ///
    /// A unit that fails to re-lower is left providing nothing, so its
    /// bindings drop out of the environment; its frame stays where it is until
    /// the session ends, because the numbering cannot have a hole in it.
    pub fn relower_reach(&mut self, edited: usize) -> Vec<(usize, ScriptCompilationResult)> {
        assert!(
            edited < self.scripts.len(),
            "unit {edited} was edited but the script has {} units",
            self.scripts.len(),
        );
        self.rederive(self.edit_reach(edited))
    }

    /// Re-lower the units an edit to the modules at `edited` reaches.
    ///
    /// The module-shaped sibling of [`Self::relower_reach`], and the reason
    /// there has to be one: stage B made typechecking lazy, so an earlier unit
    /// re-runs when a later one asks `binding_at` about a name it provides, and
    /// **a module edit makes nobody ask**. Without an entry point that says "a
    /// module changed", the units that import from it keep the values they
    /// computed against the module as it was.
    ///
    /// A unit is reached when one of its imports resolves into one of `edited`,
    /// and then the units that use what *it* provides are reached the same way
    /// [`Self::relower_reach`] reaches them. Narrow on purpose: after a module
    /// signature change any unit that is asked will re-typecheck anyway, so a
    /// driver that re-derived the whole script would over-propagate where the
    /// old behaviour under-propagated.
    ///
    /// `edited` are module paths in `library/package/module` form, which is
    /// what an import resolves an alias to.
    ///
    /// The caller must put the new module IR in front of the executor --
    /// `ScriptExecutor::set_module_registry` -- before re-executing what comes
    /// back: a script unit's IR names a module function by
    /// `CodeRef::Module`, so re-lowering it does not by itself change the code
    /// that call lands on.
    pub fn relower_module_reach(
        &mut self,
        edited: &[String],
    ) -> Vec<(usize, ScriptCompilationResult)> {
        let seeds = self.module_importers(edited);
        self.rederive(self.reach_from(&seeds))
    }

    /// Drop the units from `n` on.
    ///
    /// The cheap one of the three splice verbs and **the only one that
    /// renumbers nothing**: there is nothing after the units that go, so no
    /// surviving unit's `(unit_index, ValueId)` references move and nothing has
    /// to be re-derived. The caller drops the runtime state of the same units --
    /// `ScriptExecutor::truncate_units` -- and the session carries on at `n`.
    ///
    /// The units' `Source`s are inputs and are simply let go of; appending
    /// afterwards mints a new one, so the index they vacate is reused by a unit
    /// with a key of its own.
    pub fn truncate_units(&mut self, n: usize) {
        assert!(
            n <= self.scripts.len(),
            "cannot truncate to {n} units; the script has {}",
            self.scripts.len(),
        );
        self.scripts.truncate(n);
        self.unit_records.truncate(n);
        self.last_script = self.scripts.last().copied();
    }

    /// Splice the unit at `i` out, and re-derive everything after it.
    ///
    /// See [`Self::splice`] for why the whole suffix is re-derived and what
    /// happens when a unit of it stops compiling. A unit a later unit depends
    /// on cannot be removed without removing its dependents first: the later
    /// unit would fail to compile, which rejects the removal.
    pub fn remove_unit(
        &mut self,
        i: usize,
    ) -> Result<Vec<(usize, ScriptCompilationResult)>, Vec<String>> {
        let mut units = self.unit_list();
        assert!(
            i < units.len(),
            "unit {i} was removed but the script has {} units",
            units.len(),
        );
        units.remove(i);
        self.splice(units, i)
    }

    /// Splice a new unit in at `i`, and re-derive everything after it.
    ///
    /// `is_expr` says whether the text is a bare expression or a fragment of
    /// statements, the way [`Self::compile_fragment`] and
    /// [`Self::compile_expr`] say it for an append: nothing in the text decides
    /// it.
    ///
    /// See [`Self::splice`] for why the whole suffix is re-derived and what
    /// happens when a unit of it stops compiling.
    pub fn insert_unit(
        &mut self,
        i: usize,
        source: &str,
        is_expr: bool,
    ) -> Result<Vec<(usize, ScriptCompilationResult)>, Vec<String>> {
        let mut units = self.unit_list();
        assert!(
            i <= units.len(),
            "unit {i} was inserted but the script has {} units",
            units.len(),
        );

        let src = bct::input::Source::new(self.db, source.S());
        self.last_source = Some(src);
        let parse_diags = if is_expr {
            datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        };
        if let Some(result) = self.check_parse_errors(&parse_diags) {
            return Err(vec![
                result.first_error().expect("a parse error is an error"),
            ]);
        }

        units.insert(i, (src, is_expr));
        self.splice(units, i)
    }

    /// Put `units` in place of the unit list and re-derive from `at` on.
    ///
    /// **The whole suffix, not the reach.** A script value is identified by
    /// `(unit_index, ValueId)`, so splicing the list renumbers every unit after
    /// the splice point and every one of their IRs has to be built again. That
    /// cost is inherent rather than over-propagation: the ids really did
    /// change. The reach is for an edit, which moves no index.
    ///
    /// **A splice whose suffix does not compile is rejected and put back**, with
    /// the errors returned. A unit that fails to compile has no IR, so it has no
    /// frame, so it would leave a hole in the numbering the frame store and
    /// every `(unit_index, ValueId)` reference share; keeping the numbering
    /// hole-free would need a placeholder frame, and that is not built. This is
    /// the policy a module edit that does not compile already follows. Putting
    /// the old list back is cheap -- the same `Source` handles go back in, so
    /// every memo keyed on them is still good.
    ///
    /// A session can already hold a unit that does not compile, since
    /// [`Self::relower_reach`] leaves one in place after an edit that broke it.
    /// So such a unit rejects every splice before it until it is fixed or
    /// removed, and the restore path's own re-derivation reports its errors
    /// again, which are discarded: the rule is that the suffix must compile,
    /// and the session going back to exactly what it was is the point.
    fn splice(
        &mut self,
        units: Vec<(bct::input::Source, bool)>,
        at: usize,
    ) -> Result<Vec<(usize, ScriptCompilationResult)>, Vec<String>> {
        let held = self.unit_list();
        self.set_units(units, at);
        let redone = self.rederive((at..self.scripts.len()).collect());

        let failed: Vec<String> = redone.iter()
            .filter_map(|(index, result)| {
                result.first_error().map(|error| format!("unit {index}: {error}"))
            })
            .collect();
        if failed.is_empty() {
            return Ok(redone);
        }

        self.set_units(held, at);
        self.rederive((at..self.scripts.len()).collect());
        Err(failed)
    }

    /// Each unit's source and kind, which is the list a splice is taken over.
    fn unit_list(&self) -> Vec<(bct::input::Source, bool)> {
        self.scripts.iter()
            .map(|script| {
                let unit = script.unit(self.db);
                (unit.source(self.db), unit.is_expr(self.db))
            })
            .collect()
    }

    /// Put `units` in place of the unit list, keeping the records before `at`.
    ///
    /// The chain is built again because `scripts[i]` is the handle every
    /// per-unit query about unit `i` is keyed on, and after a splice the unit at
    /// `i` is a different unit. The records from `at` on are cleared rather than
    /// shifted: each belongs to an index, and the re-derivation that follows
    /// writes every one of them, in index order, so each unit is still lowered
    /// against the records of the units before it.
    fn set_units(&mut self, units: Vec<(bct::input::Source, bool)>, at: usize) {
        self.scripts = self.chain_over(&units);
        self.unit_records.truncate(at);
        self.unit_records.resize(units.len(), UnitLowerRecord::default());
        self.last_script = self.scripts.last().copied();
    }

    /// The chain node for each of `units`, in order.
    ///
    /// One interned node per unit, each over the one before it, so
    /// `chain[i]` is "unit `i` and the units before it".
    /// [`Script::from_units`] folds the same chain but hands back only its tail,
    /// where a splice needs every prefix.
    fn chain_over(&self, units: &[(bct::input::Source, bool)]) -> Vec<Script<'db>> {
        units.iter().fold(Vec::new(), |mut chain, (src, is_expr)| {
            let unit = ScriptUnit::new(self.db, *src, *is_expr);
            chain.push(Script::new(self.db, chain.last().copied(), unit));
            chain
        })
    }

    /// Compile each of `units` again, in the order given.
    fn rederive(&mut self, units: Vec<usize>) -> Vec<(usize, ScriptCompilationResult)> {
        units
            .into_iter()
            .map(|index| {
                self.last_script = Some(self.scripts[index]);
                self.last_spans =
                    Some(unit_ast(self.db, self.scripts[index].unit(self.db)).spans.clone());
                let result = self.compile_unit_at(index);
                (index, result)
            })
            .collect()
    }

    /// The units an edit to `edited` reaches, in index order, `edited` included.
    fn edit_reach(&self, edited: usize) -> Vec<usize> {
        self.reach_from(&BTreeSet::from([edited]))
    }

    /// The units whose imports resolve into one of `edited`.
    ///
    /// Over the union of the held and the current graph, for the reason
    /// [`Self::reach_from`] takes the union of both: an import the edit to the
    /// *script* removed is absent from one and an import it added from the
    /// other. A module edit moves neither, but the union costs nothing and
    /// there is no second rule to keep in step.
    fn module_importers(&self, edited: &[String]) -> BTreeSet<usize> {
        let now = self.imports_now();
        (0..self.unit_records.len())
            .filter(|unit| {
                self.unit_records[*unit].imports.iter()
                    .chain(now[*unit].iter())
                    .any(|path| edited.iter().any(|changed| changed == path))
            })
            .collect()
    }

    /// The units reachable from `seeds`, in index order, the seeds included.
    ///
    /// A name a unit uses resolves to the nearest earlier unit that provides
    /// it, so the reach extends to a unit when one of the names it uses
    /// resolves to a unit already reached. One forward pass gives the whole
    /// transitive reach, because a unit's providers all sit before it.
    ///
    /// The graph is the union of what it was before the edit and what it is
    /// after, since either alone would miss a case: a binding the edit
    /// *removes* is absent from the new graph although the unit that read it
    /// has to be told, and a name the edit *introduces* is absent from the old
    /// one although a later unit that asked for it in vain now finds it.
    fn reach_from(&self, seeds: &BTreeSet<usize>) -> Vec<usize> {
        let count = self.unit_records.len();
        let (now_provides, now_uses) = self.graph_now();
        assert_eq!(
            count, now_provides.len(),
            "a record per unit and a typecheck output per unit, or the two graphs \
             do not line up",
        );

        let union = |held: &[String], now: &[String]| -> BTreeSet<String> {
            held.iter().chain(now.iter()).cloned().collect()
        };
        let provides: Vec<_> = (0..count)
            .map(|unit| union(&self.unit_records[unit].provides, &now_provides[unit]))
            .collect();
        let uses: Vec<_> = (0..count)
            .map(|unit| union(&self.unit_records[unit].uses, &now_uses[unit]))
            .collect();

        let mut reached = vec![false; count];
        for unit in 0..count {
            reached[unit] = seeds.contains(&unit)
                || uses[unit].iter().any(|name| {
                    (0..unit)
                        .rev()
                        .find(|earlier| provides[*earlier].contains(name))
                        .is_some_and(|provider| reached[provider])
                });
        }
        (0..count).filter(|unit| reached[*unit]).collect()
    }

    /// What each unit provides and what it asks for, as things stand now.
    ///
    /// Both are the typechecker's own record -- `new_vars`, `new_fns` and
    /// `asked_names` from stage A -- plus the module aliases, which the
    /// typechecker resolves through a different query and so does not put in
    /// `asked_names`: a `require` in one unit provides an alias that an
    /// `import` in a later unit names.
    fn graph_now(&self) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
        let outputs = self.unit_typecheck_outputs();
        let mut provides = Vec::with_capacity(outputs.len());
        let mut uses = Vec::with_capacity(outputs.len());
        for (index, output) in outputs.iter().enumerate() {
            provides.push(self.unit_provides_names(*output));
            uses.push(self.unit_used_names(index, *output));
        }
        (provides, uses)
    }

    /// The modules each unit imports from, as things stand now.
    fn imports_now(&self) -> Vec<Vec<String>> {
        self.unit_typecheck_outputs().iter()
            .map(|output| output.imported_modules(self.db).C())
            .collect()
    }

    /// The names a unit provides to the units after it.
    ///
    /// **A unit that failed to typecheck provides nothing**, the same answer
    /// `unit_provides` gives: its bindings would carry types the compiler never
    /// settled on, and a later unit naming one resolves past it rather than to
    /// it. Reading the raw output instead would claim an edge `binding_at` does
    /// not have.
    fn unit_provides_names(
        &self,
        output: datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>,
    ) -> Vec<String> {
        let db = self.db;
        if !output.result(db).errors(db).is_empty() {
            return Vec::new();
        }
        output.new_vars(db).iter().map(|(name, _, _)| name.as_str(db).S())
            .chain(output.new_fns(db).iter().map(|(name, _)| name.as_str(db).S()))
            .chain(output.new_module_aliases(db).iter().map(|(alias, _)| alias.as_str(db).S()))
            .collect()
    }

    /// The names a unit asks its environment for.
    fn unit_used_names(
        &self,
        index: usize,
        output: datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>,
    ) -> Vec<String> {
        let db = self.db;
        let mut used: Vec<String> =
            output.asked_names(db).iter().map(|name| name.as_str(db).S()).collect();
        let qualified_calls = match &unit_ast(db, self.scripts[index].unit(db)).kind {
            ScriptUnitKind::Fragment(parsed, _) => {
                for statement in parsed.statements.iter() {
                    if let Statement::Import(import) = statement {
                        used.push(import.module_name.as_str(db).S());
                    }
                }
                &parsed.qualified_calls
            }
            ScriptUnitKind::Expr(_, qualified_calls) => qualified_calls,
        };
        // A qualified call uses its alias the way an import does.
        for (alias, _) in qualified_calls.iter() {
            used.push(alias.as_str(db).S());
        }
        used
    }

    /// Take up the units and the per-unit records a session left off with.
    fn resume(&mut self, session: ScriptSession) {
        self.scripts = self.chain_over(&session.units);
        self.last_script = self.scripts.last().copied();
        self.unit_records = session.records;
        self.skip_const_inlining = session.skip_const_inlining;
        self.skip_specialization = session.skip_specialization;
        self.set_auto_adapt_mode(session.auto_adapt_mode);
    }

    /// Hand back what an edit needs to survive letting this compiler go.
    pub fn into_session(self) -> ScriptSession {
        let units = self.unit_list();
        ScriptSession {
            units,
            records: self.unit_records,
            skip_const_inlining: self.skip_const_inlining,
            skip_specialization: self.skip_specialization,
            auto_adapt_mode: self.auto_adapt_mode,
        }
    }

    /// Each unit's typecheck output, in order, for the units compiled so far.
    ///
    /// What a unit provided and what it asked the environment for -- the
    /// dependency graph over the session. Empty before anything has compiled.
    /// See `botdocs/plan-script-reactivity.md`.
    pub fn unit_typecheck_outputs(
        &self,
    ) -> Vec<datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>> {
        match self.scripts.last() {
            None => Vec::new(),
            Some(script) => type_check_script_units(self.db, *script, self.env)
                .unit_outputs(self.db),
        }
    }

    /// Get parse diagnostics from the last compilation.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Get type diagnostics from the last compilation.
    pub fn get_type_diagnostics(&self) -> Vec<&datalove_diagnostic::TypeDiagnostic> {
        match self.last_script {
            Some(script) => type_check_script_units::accumulated::<datalove_diagnostic::TypeDiagnostic>(
                self.db, script, self.env),
            None => Vec::new(),
        }
    }

    /// Emit ownership diagnostics from the last compilation.
    ///
    /// This should be called when ownership errors occurred to emit proper
    /// ariadne diagnostics with span information. Call this before displaying
    /// the error message.
    pub fn emit_ownership_diagnostics(&self) {
        if let Some(spans) = &self.last_spans {
            emit_ownership_diagnostics(self.db, &self.last_ownership_errors, spans);
        }
    }

    /// Get ownership diagnostics from the last compilation (salsa accumulator path).
    ///
    /// Note: This currently returns empty because ownership diagnostics are
    /// emitted outside of tracked functions. Use `get_ownership_errors()` and
    /// `get_last_spans()` for direct rendering instead.
    pub fn get_ownership_diagnostics(&self) -> Vec<&datalove_diagnostic::OwnershipDiagnostic> {
        Vec::new()
    }

    /// Get the structured ownership errors from the last compilation.
    ///
    /// Use with `get_last_spans()` for direct diagnostic rendering.
    pub fn get_ownership_errors(&self) -> &[AnalysisError<'_>] {
        &self.last_ownership_errors
    }

    /// Get the spans from the last compilation for diagnostic rendering.
    pub fn get_last_spans(&self) -> Option<&datalove_datafun_ast::spans::DatafunSpans<'_>> {
        self.last_spans.as_ref()
    }

    /// Get the database reference.
    pub fn db(&self) -> &'db dyn salsa::Database {
        self.db
    }

    /// Skip const parameter specialization.
    ///
    /// When enabled, a comptime call keeps naming the original function, which
    /// still takes the const argument. Used for differential testing.
    pub fn set_skip_specialization(&mut self, enabled: bool) {
        self.skip_specialization = enabled;
    }

    /// Skip compile-time const evaluation and inlining.
    ///
    /// When enabled, const bindings are evaluated at runtime instead of being
    /// replaced with literal values at compile time. This is useful for testing
    /// and debugging const expressions.
    pub fn set_skip_const_inlining(&mut self, enabled: bool) {
        self.skip_const_inlining = enabled;
    }

    /// Set auto-adapt mode for ownership analysis.
    ///
    /// When enabled, recoverable ownership errors (use-after-move, double-move,
    /// move-in-loop) are suppressed, treating them as if `@` was inserted.
    pub fn set_auto_adapt_mode(&mut self, mode: AutoAdaptMode) {
        self.auto_adapt_mode = mode;
        // The mode is part of what a unit is checked against, so the handle the
        // per-unit queries are keyed on has to carry it.
        let modules: Vec<bct::module_graph::Module<'db>> = self.env.modules(self.db).C();
        self.env = ScriptEnv::new(self.db, modules, mode);
    }

    /// Get current auto-adapt mode.
    pub fn auto_adapt_mode(&self) -> AutoAdaptMode {
        self.auto_adapt_mode
    }

    /// Check for parse errors and return early result if any.
    fn check_parse_errors(&self, parse_diags: &[&datalove_diagnostic::ParseDiagnostic]) -> Option<ScriptCompilationResult> {
        if parse_diags.is_empty() {
            return None;
        }
        let parse_errors: Vec<String> = parse_diags.iter()
            .map(|d| {
                let diag = d.to_diagnostic(self.db);
                diag.message.as_str(self.db).S()
            })
            .collect();
        Some(ScriptCompilationResult {
            typecheck: TypecheckResult::ParseError { errors: parse_errors },
            ownership: OwnershipResult::Skipped,
            lowering: LoweringResult::Skipped,
            ir_unit: None,
        })
    }

    /// The statements or the expression unit `index` parsed to.
    ///
    /// Read out of `unit_ast`, which holds the parse keyed on the unit alone,
    /// so re-lowering a unit costs no second parse of it.
    fn parsed_unit_at(&self, index: usize) -> ParsedUnit<'db> {
        let spec = unit_ast(self.db, self.scripts[index].unit(self.db));
        match &spec.kind {
            ScriptUnitKind::Fragment(parsed, _) => {
                ParsedUnit::Fragment { stmts: parsed.statements.to_vec() }
            }
            ScriptUnitKind::Expr(expr, _) => ParsedUnit::Expr(*expr),
        }
    }

    /// Compile unit `index` through the pipeline, against the units before it.
    ///
    /// Pipeline phases:
    /// 1. Analysis
    ///    a. Typecheck - type inference and checking
    ///    b. Ownership - borrow checking and drop scheduling
    /// 2. Lowering - generate IR (const bindings lowered as let bindings)
    /// 3. Const Evaluation - evaluate const expressions at compile time
    /// 4. Const Inlining - replace const bindings with evaluated values
    ///
    /// Every exit leaves `unit_records[index]` describing what this unit now
    /// provides -- nothing at all, if it failed.
    fn compile_unit_at(&mut self, index: usize) -> ScriptCompilationResult {
        let unit = self.parsed_unit_at(index);
        let result = self.run_phases(index, &unit);
        if result.ir_unit.is_none() {
            self.unit_records[index] = UnitLowerRecord::default();
        }
        result
    }

    fn run_phases(&mut self, index: usize, unit: &ParsedUnit<'db>) -> ScriptCompilationResult {
        // Phase 1a: Typecheck
        let typecheck = match self.phase_typecheck(index) {
            Ok(tc) => tc,
            Err(result) => return result,
        };

        // Phase 1b: Ownership Analysis
        let ownership = match self.phase_ownership(index, unit, &typecheck) {
            Ok(own) => own,
            Err(result) => return result,
        };

        // Phase 2: Lowering
        // Functions are lowered first, then reused for const evaluation.
        //
        // In two strata, for the reason the module pipeline has two: a function
        // body may name a script-level const, and evaluating a script-level
        // const may call a function in the same unit. So the bodies that need
        // no value from this unit's consts go first, the consts are evaluated
        // against those, and the rest follow.
        let earlier_consts = script_consts_over(&self.unit_records[..index]);
        let lowered_funcs = match self.phase_lower_functions(
            index, unit, &typecheck, &ownership, &earlier_consts, true,
        ) {
            Ok(lf) => lf,
            Err(result) => return result,
        };

        // Phase 3: Const Evaluation
        // Evaluates const expressions using the lowered functions + interpretation.
        let consts = match self.phase_const_eval(index, unit, &typecheck, &lowered_funcs) {
            Ok(c) => c,
            Err(result) => return result,
        };

        // Second stratum: the bodies that were waiting on a const's value.
        // Everything is lowered again rather than just those, since a FuncId is
        // the function's position among the statements either way and the
        // bodies that did not wait lower to the same IR.
        let lowered_funcs = if lowered_funcs.deferred.is_empty() {
            lowered_funcs
        } else {
            match self.phase_lower_functions(
                index, unit, &typecheck, &ownership, &consts.script_consts, false,
            ) {
                Ok(lf) => lf,
                Err(result) => return result,
            }
        };

        // Phase 2 (continued): Assemble final IR
        // Combines lowered functions with script-level code.
        let ir_unit = match self.phase_assemble_ir(index, unit, &typecheck, &ownership, &consts, &lowered_funcs) {
            Ok(ir) => ir,
            Err(result) => return result,
        };

        // Phase 4: Const Inlining
        // Replaces const initializer expressions with their evaluated values.
        // Skipped in skip_const_inlining mode (consts evaluated at runtime).
        let ir_unit = self.phase_const_inline(ir_unit, &consts);
        let ir_unit = self.phase_resolve_shape_descriptors(ir_unit);

        // Phase 5: Const parameter specialization.
        let ir_unit = match self.phase_specialize(ir_unit, unit, &typecheck, &lowered_funcs) {
            Ok(ir) => ir,
            Err(result) => return result,
        };

        self.record_unit(index, &ir_unit, &typecheck, &ownership, &consts);

        let ir_dump = format!("{}", ir_unit);
        ScriptCompilationResult {
            typecheck: TypecheckResult::Success,
            ownership: OwnershipResult::Success,
            lowering: LoweringResult::Success { ir: ir_dump },
            ir_unit: Some(ir_unit),
        }
    }

    // ========================================================================
    // Phase 1: Typecheck
    // ========================================================================

    /// Run typechecking on unit `index`.
    fn phase_typecheck(
        &mut self,
        index: usize,
    ) -> Result<TypecheckOutput<'db>, ScriptCompilationResult> {
        // Keyed on the unit's position in the script and nothing derived from
        // the units around it, which is what lets an edit stop at the units
        // that used what changed. See `botdocs/plan-script-reactivity.md`.
        //
        // The mode reaches here through `env`, not just ownership analysis: a
        // mismatch `@` would fix is a type error first.
        let output = typecheck_script_unit(self.db, self.scripts[index], self.env);
        let result = output.result(self.db);

        // Check for errors.
        let errors: Vec<_> = result.errors(self.db).into_iter()
            .map(|e| format!("{:?}", e.error(self.db)))
            .collect();
        if !errors.is_empty() {
            return Err(ScriptCompilationResult {
                typecheck: TypecheckResult::Error { errors },
                ownership: OwnershipResult::Skipped,
                lowering: LoweringResult::Skipped,
                ir_unit: None,
            });
        }

        Ok(TypecheckOutput {
            output,
            result,
            expr_types: result.expr_types(self.db),
            call_targets: result.call_targets(self.db),
        })
    }

    // ========================================================================
    // Phase 1b: Ownership Analysis
    // ========================================================================

    /// Run ownership analysis on the unit.
    fn phase_ownership(
        &mut self,
        index: usize,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
    ) -> Result<OwnershipOutput<'db>, ScriptCompilationResult> {
        let dead_externals = dead_externals_over(&self.unit_records[..index]);
        match unit {
            ParsedUnit::Fragment { stmts, .. } => {
                let ownership_result = analyze_script_fragment_tracked(
                    self.db,
                    typecheck.result,
                    stmts.clone(),
                    self.auto_adapt_mode,
                    dead_externals,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    // Store structured errors and spans for CLI rendering.
                    self.last_ownership_errors = ownership_result.structured_errors(self.db).to_vec();
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                let mut func_analyses = ownership_result.to_function_analyses_map(self.db, stmts);
                let mut script_analysis = ownership_result.script_analysis(self.db).clone();

                // Type adaptations are decided during typechecking, and lowering
                // needs them alongside the ones ownership analysis decided.
                let type_adapts = typecheck.result.adapt_sites(self.db);
                if !type_adapts.is_empty() {
                    if let Some(analysis) = script_analysis.as_mut() {
                        analysis.adapt_sites.extend(type_adapts);
                    }
                    for analysis in func_analyses.values_mut() {
                        analysis.adapt_sites.extend(type_adapts);
                    }
                }
                Ok(OwnershipOutput {
                    func_analyses,
                    script_analysis,
                    dead_exports: ownership_result.dead_exports(self.db).clone(),
                    revived: ownership_result.revived_exports(self.db).clone(),
                })
            }
            ParsedUnit::Expr(expr) => {
                let ownership_result = analyze_script_expr_tracked(
                    self.db,
                    typecheck.result,
                    *expr,
                    self.auto_adapt_mode,
                    dead_externals,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    // Store structured errors and spans for CLI rendering.
                    self.last_ownership_errors = ownership_result.structured_errors(self.db).to_vec();
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                Ok(OwnershipOutput {
                    func_analyses: HashMap::new(),
                    script_analysis: None,
                    dead_exports: Vec::new(),
                    revived: Vec::new(),
                })
            }
        }
    }

    // ========================================================================
    // Phase 2a: Lower Functions
    // ========================================================================

    /// Lower functions to IR.
    ///
    /// Functions are lowered once, then reused for const evaluation
    /// and final IR assembly.
    ///
    /// `script_consts` are the script-level const values a body may name.
    /// `defer_missing_consts` holds back a body that names one with no value
    /// yet; the caller lowers again once the consts are evaluated.
    #[allow(clippy::too_many_arguments)]
    fn phase_lower_functions(
        &mut self,
        index: usize,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
        script_consts: &HashMap<String, (IrType, ConstValue)>,
        defer_missing_consts: bool,
    ) -> Result<LoweredFunctions, ScriptCompilationResult> {
        let ParsedUnit::Fragment { stmts, .. } = unit else {
            // Expressions don't have function definitions.
            return Ok(LoweredFunctions {
                functions: Vec::new(),
                func_name_to_id: HashMap::new(),
                deferred: Vec::new(),
            });
        };

        // The context this unit is lowered against: what the units before it
        // provide, and nothing the units after it do.
        let script_ctx = lower_context_over(&self.unit_records[..index]);

        // Get module function ID map for resolving module function calls.

        // Build func_param_types and func_return_types for type alias support.
        let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
        let mut func_return_types: HashMap<String, IrType> = HashMap::new();
        for (name, func_type) in typecheck.result.function_types(self.db) {
            let param_types: Vec<IrType> = func_type.param_types(self.db)
                .iter()
                .map(|ty| IrType::from_tycheck(self.db, ty))
                .collect();
            func_param_types.insert(name.text(self.db).S(), param_types);

            let return_type = IrType::from_tycheck(self.db, &func_type.return_type(self.db));
            func_return_types.insert(name.text(self.db).S(), return_type);
        }

        // Lower just the functions.
        match lower_script_functions(
            self.db,
            typecheck.expr_types,
            typecheck.call_targets,
            stmts,
            &ownership.func_analyses,
            Some(&func_param_types),
            Some(&func_return_types),
            self.shared_context.func_id_map,
            script_ctx,
            script_consts,
            defer_missing_consts,
        ) {
            Ok(lowered) => {
                let functions = lowered.functions.into_iter().map(std::sync::Arc::new).collect();
                Ok(LoweredFunctions {
                    functions,
                    func_name_to_id: lowered.func_name_to_id,
                    deferred: lowered.deferred,
                })
            }
            Err(e) => {
                Err(ScriptCompilationResult {
                    typecheck: TypecheckResult::Success,
                    ownership: OwnershipResult::Success,
                    lowering: LoweringResult::Error {
                        message: format!("function lowering error: {}", e),
                    },
                    ir_unit: None,
                })
            }
        }
    }

    // ========================================================================
    // Phase 3: Const Evaluation
    // ========================================================================

    /// Evaluate compile-time constants (fragment only).
    ///
    /// Uses the "lower then evaluate" pattern:
    /// 1. For each const binding in dependency order, lower it to IR or extract simple value
    /// 2. Evaluate the IR unit via CTFE if needed
    fn phase_const_eval(
        &mut self,
        index: usize,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<ConstEvalOutput, ScriptCompilationResult> {
        let earlier_consts = script_consts_over(&self.unit_records[..index]);
        let ParsedUnit::Fragment { stmts, .. } = unit else {
            // Expressions don't have const bindings.
            return Ok(ConstEvalOutput {
                resolved_consts: ResolvedConsts::new(),
                func_consts: HashMap::new(),
                script_consts: earlier_consts,
                declared_consts: Vec::new(),
            });
        };

        // Collect const graph (memoized).
        let const_graph = collect_const_graph(self.db, stmts.clone(), typecheck.result);

        // Evaluate script-level consts.
        //
        // Done even under `skip_const_inlining`, since a function body naming a
        // script const resolves it where the reference is lowered rather than
        // by the inlining pass, so that flag has nothing to skip here. What it
        // still skips is substituting the const's own definition, which
        // `phase_const_inline` decides.
        let resolved_consts = if const_graph.is_empty() {
            ResolvedConsts::new()
        } else {
            match self.evaluate_script_consts(
                &earlier_consts,
                &const_graph,
                stmts,
                typecheck.expr_types,
                typecheck.call_targets,
                lowered_funcs,
            ) {
                Ok(resolved) => resolved,
                Err(e) => {
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Success,
                        lowering: LoweringResult::Error {
                            message: format!("const evaluation error: {}", e),
                        },
                        ir_unit: None,
                    });
                }
            }
        };

        // Build the script-level consts map, over the earlier units' consts,
        // which this unit's declarations shadow.
        let mut script_consts = earlier_consts;
        let mut declared_consts = Vec::new();
        for (name, value) in resolved_consts.iter() {
            let ir_type = const_graph.bindings.iter()
                .find(|b| &b.name == name)
                .map(|b| b.ir_type.clone())
                .expect("every resolved const is a binding of the graph it came from");
            script_consts.insert(name.to_string(), (ir_type.clone(), value.clone()));
            declared_consts.push((name.to_string(), ir_type, value.clone()));
        }

        // Evaluate function-level consts.
        // Skip if skip_const_inlining is enabled - consts will be lowered as let bindings.
        let func_consts_result = if !self.skip_const_inlining {
            self.evaluate_function_consts(
                stmts,
                typecheck.expr_types,
                typecheck.call_targets,
                &script_consts,
                lowered_funcs,
            )
        } else {
            ScriptFunctionConstsResult::empty()
        };

        if !func_consts_result.errors.is_empty() {
            return Err(ScriptCompilationResult {
                typecheck: TypecheckResult::Success,
                ownership: OwnershipResult::Success,
                lowering: LoweringResult::Error {
                    message: format!("const evaluation errors:\n  {}", func_consts_result.errors.join("\n  ")),
                },
                ir_unit: None,
            });
        }

        Ok(ConstEvalOutput {
            resolved_consts,
            func_consts: func_consts_result.consts,
            script_consts,
            declared_consts,
        })
    }

    /// Evaluate script-level const bindings using "lower then evaluate" pattern.
    #[allow(clippy::too_many_arguments)]
    fn evaluate_script_consts(
        &self,
        earlier_consts: &HashMap<String, (IrType, ConstValue)>,
        const_graph: &datalove_datafun_ir::ConstBindingGraph,
        statements: &[Statement<'db>],
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<ResolvedConsts, ConstEvalError> {
        let mut resolved = ResolvedConsts::new();

        // Seeded with the consts earlier units left, so that a const may name
        // one: starting empty meant `const J = K` could only see a `K` declared
        // beside it, and reading one from an earlier unit failed to lower.
        //
        // This unit's own bindings are inserted below as each is resolved, in
        // statement order, so a redeclaration shadows the earlier value rather
        // than the other way about.
        let mut resolved_consts_map: HashMap<String, (IrType, ConstValue)> =
            earlier_consts.clone();

        // A binding's id is its position among the const statements, so the
        // expressions only have to be gathered once.
        let const_exprs: Vec<_> = statements.iter()
            .filter_map(|s| match s {
                Statement::Const(c) => Some(c.value),
                _ => None,
            })
            .collect();

        for binding in &const_graph.bindings {
            let expr = const_exprs[binding.stmt_id.0 as usize];

            // Lower the const binding to get either a simple value or an IR unit.
            // Pass the module func_id_map for cross-module CTFE function calls.
            let (unit_opt, value_opt) = lower_const_binding(
                self.db,
                expr,
                &binding.ir_type,
                expr_types,
                call_targets,
                &resolved_consts_map,
                // A script unit returns `!()`, so a script-level const may use
                // `!`; if it does return early, evaluation reports it.
                Some(IrType::Result(Box::new(IrType::Unit))),
                &lowered_funcs.functions,
                &lowered_funcs.func_name_to_id,
                Some(&self.shared_context.func_id_map),
            ).map_err(|e| ConstEvalError::LoweringFailed {
                binding_name: binding.name.clone(),
                message: e.to_string(),
            })?;

            // Evaluate to get the const value.
            let value = match (unit_opt, value_opt) {
                (None, Some(v)) => v,
                (Some(unit), None) => {
                    let prepared = PreparedConst::Unit(unit);
                    evaluate_prepared_const(&prepared, &binding.ir_type, &self.ctfe_evaluator)
                        .map_err(|e| self.ctfe_error_to_const_eval_error(e, &binding.name))?
                }
                _ => unreachable!("lower_const_binding returns exactly one of unit or value"),
            };

            resolved.insert(binding.stmt_id, binding.name.clone(), value.clone());
            resolved_consts_map.insert(binding.name.clone(), (binding.ir_type.clone(), value));
        }

        Ok(resolved)
    }

    /// Evaluate function-level const bindings using "lower then evaluate" pattern.
    fn evaluate_function_consts(
        &self,
        statements: &[Statement<'db>],
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        script_level_consts: &HashMap<String, (IrType, ConstValue)>,
        lowered_funcs: &LoweredFunctions,
    ) -> ScriptFunctionConstsResult {
        let mut consts = HashMap::new();
        let mut errors = Vec::new();

        for statement in statements {
            if let Statement::Fun(func_stmt) = statement {
                let func_name = func_stmt.name(self.db).text(self.db);
                // Get function's return type for early-return operators.
                let func_return_type = func_stmt.return_type(self.db)
                    .map(|ty| IrType::from_type_hint(self.db, &ty));
                // Track local consts for this function.
                let mut func_local_consts: HashMap<String, (IrType, ConstValue)> = HashMap::new();

                // Names whose value this function does not have one of: its
                // const parameters, and the consts already deferred for naming
                // one, since those have no value here either.
                let mut deferred: std::collections::BTreeSet<String> = func_stmt.params(self.db)
                    .iter()
                    .filter(|p| p.is_comptime)
                    .map(|p| p.name.text(self.db).to_string())
                    .collect();

                for func_body_stmt in func_stmt.body(self.db).iter() {
                    if let Statement::Const(const_stmt) = func_body_stmt {
                        let name = const_stmt.name.text(self.db).to_string();
                        let init_expr = const_stmt.value;

                        // Get the type from the typechecker.
                        let key = datalove_datafun_ast::ast::ExprKey::of(self.db, init_expr);
                        let ir_type = match expr_types.get(&key) {
                            Some(ty) => IrType::from_tycheck(self.db, ty),
                            None => {
                                errors.push(format!("{}::{}: missing type information", func_name, name));
                                continue;
                            }
                        };

                        // Build lookup map: script-level + function-local consts.
                        let mut lookup_map = script_level_consts.clone();
                        for (local_name, (ty, val)) in &func_local_consts {
                            lookup_map.insert(local_name.clone(), (ty.clone(), val.clone()));
                        }

                        // Lower the const binding.
                        // Pass the module func_id_map for cross-module CTFE function calls.
                        let lower_result = lower_const_binding(
                            self.db,
                            init_expr,
                            &ir_type,
                            expr_types,
                            call_targets,
                            &lookup_map,
                            func_return_type.clone(),
                            &lowered_funcs.functions,
                            &lowered_funcs.func_name_to_id,
                            Some(&self.shared_context.func_id_map),
                        );

                        let value = match lower_result {
                            Ok((None, Some(v))) => v,
                            Ok((Some(unit), None)) => {
                                let prepared = PreparedConst::Unit(unit);
                                match evaluate_prepared_const(&prepared, &ir_type, &self.ctfe_evaluator) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        errors.push(format!("{}::{}: {}", func_name, name, e));
                                        continue;
                                    }
                                }
                            }
                            Ok(_) => unreachable!("lower_const_binding returns exactly one of unit or value"),
                            // A const naming a const parameter has a value per
                            // instantiation rather than one, so there is nothing
                            // to evaluate until the copies are made. It lowers
                            // as an ordinary binding, and specialization
                            // evaluates it once the parameter has a value.
                            Err(datalove_datafun_compiler::lower::LowerError::BindingNotAvailable(ref missing))
                                if deferred.contains(missing) =>
                            {
                                deferred.insert(name.clone());
                                continue;
                            }
                            Err(e) => {
                                errors.push(format!("{}::{}: {}", func_name, name, e));
                                continue;
                            }
                        };

                        // Store locally for other consts in this function.
                        func_local_consts.insert(name.clone(), (ir_type.clone(), value.clone()));

                        // Store with qualified name for the result.
                        let qualified_name = format!("{}::{}", func_name, name);
                        consts.insert(qualified_name, (ir_type, value));
                    }
                }
            }
        }

        ScriptFunctionConstsResult { consts, errors }
    }

    /// Convert a CtfeError to a ConstEvalError.
    fn ctfe_error_to_const_eval_error(&self, e: CtfeError, binding_name: &str) -> ConstEvalError {
        match e {
            CtfeError::InterpError(msg) if msg.contains("gas") => {
                ConstEvalError::GasExpired { binding_name: binding_name.to_string() }
            }
            CtfeError::InterpError(msg) => {
                ConstEvalError::LoweringFailed {
                    binding_name: binding_name.to_string(),
                    message: msg,
                }
            }
            CtfeError::EarlyReturn(msg) => {
                ConstEvalError::EarlyReturn {
                    binding_name: binding_name.to_string(),
                    message: msg,
                }
            }
            CtfeError::UnsupportedType(ty) => {
                ConstEvalError::UnsupportedType {
                    binding_name: binding_name.to_string(),
                    type_name: ty,
                }
            }
        }
    }

    // ========================================================================
    // Phase 2b: Assemble Final IR
    // ========================================================================

    /// Assemble the final IR by combining lowered functions with script-level code.
    ///
    /// Const bindings are lowered as let bindings with their initializer expressions.
    /// Const inlining happens as a separate pass after IR assembly.
    #[allow(clippy::too_many_arguments)]
    fn phase_assemble_ir(
        &mut self,
        index: usize,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
        _consts: &ConstEvalOutput,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<IrCodeUnit, ScriptCompilationResult> {
        let script_ctx = lower_context_over(&self.unit_records[..index]);

        match unit {
            ParsedUnit::Fragment { stmts, .. } => {
                // Build func_param_types and func_return_types for type alias support.
                let mut func_param_types: HashMap<String, Vec<IrType>> = HashMap::new();
                let mut func_return_types: HashMap<String, IrType> = HashMap::new();
                for (name, func_type) in typecheck.result.function_types(self.db) {
                    let param_types: Vec<IrType> = func_type.param_types(self.db)
                        .iter()
                        .map(|ty| IrType::from_tycheck(self.db, ty))
                        .collect();
                    func_param_types.insert(name.text(self.db).S(), param_types);

                    let return_type = IrType::from_tycheck(self.db, &func_type.return_type(self.db));
                    func_return_types.insert(name.text(self.db).S(), return_type);
                }

                let script_analysis = ownership.script_analysis.clone()
                    .expect("script_analysis required for fragment units");

                // Const bindings are lowered as let bindings.
                // Const inlining happens in phase_const_inline after IR assembly.
                // Pass lowered functions to avoid re-lowering them.
                let lowered_funcs_arg = if lowered_funcs.functions.is_empty() {
                    None
                } else {
                    Some((
                        lowered_funcs.functions.iter().map(|f| (**f).clone()).collect(),
                        lowered_funcs.func_name_to_id.clone(),
                    ))
                };

                lower_script_fragment_raw(
                    self.db,
                    typecheck.expr_types,
                    typecheck.call_targets,
                    self.shared_context.func_id_map,
                    script_ctx,
                    stmts.clone(),
                    ownership.func_analyses.clone(),
                    script_analysis,
                    Some(&func_param_types),
                    Some(&func_return_types),
                    lowered_funcs_arg,
                ).map_err(|e| ScriptCompilationResult {
                    typecheck: TypecheckResult::Success,
                    ownership: OwnershipResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    ir_unit: None,
                })
            }
            ParsedUnit::Expr(expr) => {
                lower_script_expr(
                    self.db,
                    typecheck.expr_types,
                    typecheck.call_targets,
                    self.shared_context.func_id_map,
                    script_ctx,
                    *expr,
                ).map_err(|e| ScriptCompilationResult {
                    typecheck: TypecheckResult::Success,
                    ownership: OwnershipResult::Success,
                    lowering: LoweringResult::Error { message: format!("{}", e) },
                    ir_unit: None,
                })
            }
        }
    }

    // ========================================================================
    // Phase 4: Const Inlining
    // ========================================================================

    /// Inline evaluated const values into the lowered IR.
    ///
    /// This replaces const initializer expressions with their pre-computed
    /// literal values. In skip_const_inlining mode, this is skipped so const
    /// expressions are evaluated at runtime instead of compile time.
    /// Say what each call in the script hands a generic that needs a
    /// descriptor, and give the script's own functions the shapes their
    /// callees need of them.
    ///
    /// The functions defined beside the script settled this among themselves
    /// while they were lowered, but only what they said to each other: a call
    /// into a module was not in front of them, because the modules are lowered
    /// elsewhere. Here both are to hand, so the closure is run again with the
    /// module functions and the natives standing as fixed points -- their
    /// signatures are settled and nothing here may add to them.
    ///
    /// A script has no type parameters of its own, so it never forwards one;
    /// everything it hands over is a descriptor for a type it named outright.
    fn phase_resolve_shape_descriptors(&self, mut ir_unit: IrCodeUnit) -> IrCodeUnit {
        use datalove_datafun_ir::{CodeRef, CodeUnitId, DescriptorShape, IrModuleId};

        /// A function this closure has to name, whether beside the script or
        /// in a module.
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        enum Key {
            Local(u32),
            Module(u32, u32),
        }

        let registry = self.shared_context.module_registry.clone();
        let module_shapes = |module: IrModuleId, id: CodeUnitId| -> Vec<DescriptorShape> {
            let Some(unit) = registry.get_module_function_as_unit(module, id) else {
                return Vec::new();
            };
            // A native says this from its own context, having no function
            // context to say it in.
            if let Some(native) = unit.native_context() {
                return native.descriptor_shapes.clone();
            }
            unit.function_context()
                .map(|c| c.descriptor_shapes.clone())
                .unwrap_or_default()
        };
        let key_of = |code_ref: &CodeRef| -> Option<Key> {
            match code_ref {
                CodeRef::Local(id) => Some(Key::Local(id.0)),
                CodeRef::Module { module, id } => Some(Key::Module(module.0, id.0)),
                CodeRef::External { .. } => None,
            }
        };

        let mut shapes: HashMap<Key, Vec<DescriptorShape>> = HashMap::new();
        let mut calls: HashMap<Key, Vec<(Key, Vec<DescriptorShape>)>> = HashMap::new();

        for unit in &ir_unit.nested_units {
            let key = Key::Local(unit.id.0);
            shapes.insert(key, unit.function_context()
                .map(|c| c.descriptor_shapes.clone()).unwrap_or_default());
            let mut sites = Vec::new();
            for block in &unit.blocks {
                for instr in &block.instructions {
                    let Some((func, type_args)) = instr.call_target() else { continue };
                    // Every callee's set is wanted, whether or not this site
                    // binds anything: a module callee is a fixed point that has
                    // to be in `shapes` for the closure to see it.
                    if let CodeRef::Module { module, id } = func {
                        shapes.entry(Key::Module(module.0, id.0)).or_insert_with(
                            || module_shapes(*module, CodeUnitId(id.0)));
                    }
                    if type_args.is_empty() {
                        continue;
                    }
                    if let Some(callee) = key_of(func) {
                        sites.push((callee, type_args.to_vec()));
                    }
                }
            }
            calls.insert(key, sites);
        }

        // Refused only by a shape that grows without end, which the module path
        // reports against the module that wrote it; a script naming the same
        // functions gets the same answer there.
        let _ = datalove_datafun_ir::close_shapes(&calls, &mut shapes);

        for unit in &mut ir_unit.nested_units {
            let Some(shape_set) = shapes.get(&Key::Local(unit.id.0)) else { continue };
            if let datalove_datafun_ir::CodeUnitContext::Function(ctx) = &mut unit.context {
                ctx.descriptor_shapes = shape_set.clone();
            }
        }

        // A module function's set is read from the registry rather than from
        // `shapes`, which holds only the ones some nested unit happened to
        // call. The script's own body may call others.
        let callee_shapes = |code_ref: &CodeRef| -> Vec<DescriptorShape> {
            match code_ref {
                CodeRef::Local(id) => shapes.get(&Key::Local(id.0)).cloned().unwrap_or_default(),
                CodeRef::Module { module, id } => module_shapes(*module, CodeUnitId(id.0)),
                CodeRef::External { .. } => Vec::new(),
            }
        };
        for unit in &mut ir_unit.nested_units {
            let own = shapes.get(&Key::Local(unit.id.0)).cloned().unwrap_or_default();
            let _ = datalove_datafun_ir::set_call_descriptors(unit, &own, &callee_shapes);
        }
        // Failure here means the script would forward a type parameter, which
        // it has none of, so it cannot happen.
        let _ = datalove_datafun_ir::set_call_descriptors(
            &mut ir_unit, &[], &callee_shapes);
        ir_unit
    }

    // ========================================================================
    // Phase 5: Const Parameter Specialization
    // ========================================================================

    /// Give each comptime call in this unit a copy of its callee to run.
    ///
    /// The copies go in this unit's own `nested_units`, so a script line can
    /// name an instantiation no module call site asked for without the module
    /// it came from having to change. A call whose const argument did not
    /// survive as a constant -- under `skip_const_inlining` none of them do --
    /// keeps naming the original, which still takes it.
    ///
    /// This runs after the descriptors are resolved, so that a copy arrives
    /// with the ones its own module worked out and nothing here recomputes
    /// them against a script that has no type parameters of its own.
    /// Evaluate one function's const bindings for one instantiation.
    ///
    /// The mirror of what the module pipeline does for its own copies: seed the
    /// const parameters with what this instantiation passes, and every const in
    /// the body becomes evaluable by the same CTFE that evaluates every other
    /// const. Returns the values under their local names, which is how the
    /// copy's `const_values` records them.
    #[allow(clippy::too_many_arguments)]
    fn instantiation_consts_from(
        &self,
        statements: &[Statement<'db>],
        func_name: &str,
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        comptime_param_indices: &[usize],
        values: &[ConstValue],
        lowered: &[std::sync::Arc<IrCodeUnit>],
        func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    ) -> (HashMap<String, ConstValue>, Vec<String>) {
        let Some(func_stmt) = statements.iter().find_map(|stmt| match stmt {
            Statement::Fun(f) if f.name(self.db).text(self.db) == func_name => Some(f),
            _ => None,
        }) else {
            return (HashMap::new(), Vec::new());
        };

        let params = func_stmt.params(self.db);
        let mut seeded: HashMap<String, (IrType, ConstValue)> = HashMap::new();
        for (&param_idx, value) in comptime_param_indices.iter().zip(values.iter()) {
            let Some(param) = params.get(param_idx) else { continue };
            seeded.insert(
                param.name.text(self.db).to_string(),
                (datalove_datafun_ir::ir_type_of_const_value(value), value.clone()),
            );
        }

        let func_return_type = func_stmt.return_type(self.db)
            .map(|ty| IrType::from_type_hint(self.db, &ty));

        let mut evaluated = HashMap::new();
        let mut errors = Vec::new();
        for body_stmt in func_stmt.body(self.db).iter() {
            let Statement::Const(const_stmt) = body_stmt else { continue };
            let name = const_stmt.name.text(self.db).to_string();
            let key = datalove_datafun_ast::ast::ExprKey::of(self.db, const_stmt.value);
            let Some(ty) = expr_types.get(&key) else {
                errors.push(format!("{}::{}: missing type information", func_name, name));
                continue;
            };
            let ir_type = IrType::from_tycheck(self.db, ty);

            let lowered_const = lower_const_binding(
                self.db, const_stmt.value, &ir_type, expr_types, call_targets, &seeded,
                func_return_type.clone(), lowered, func_name_to_id,
                Some(&self.shared_context.func_id_map),
            );
            let value = match lowered_const {
                Ok((None, Some(v))) => v,
                Ok((Some(unit), None)) => {
                    match evaluate_prepared_const(
                        &PreparedConst::Unit(unit), &ir_type, &self.ctfe_evaluator)
                    {
                        Ok(v) => v,
                        Err(e) => {
                            errors.push(format!("{}::{}: {}", func_name, name, e));
                            continue;
                        }
                    }
                }
                Ok(_) => unreachable!("lower_const_binding returns exactly one of unit or value"),
                Err(e) => {
                    errors.push(format!("{}::{}: {}", func_name, name, e));
                    continue;
                }
            };

            seeded.insert(name.clone(), (ir_type, value.clone()));
            evaluated.insert(name, value);
        }

        (evaluated, errors)
    }

    fn phase_specialize(
        &self,
        ir_unit: IrCodeUnit,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        lowered_funcs: &LoweredFunctions,
    ) -> Result<IrCodeUnit, ScriptCompilationResult> {
        use datalove_datafun_compiler::specialize::CalleeKey;

        if self.skip_specialization {
            return Ok(ir_unit);
        }

        let registry = self.shared_context.module_registry.clone();
        let module_unit = |module, id| registry.get_module_function_as_unit(module, id).cloned();

        // A copy's const bindings are evaluated here, where the parameters
        // finally have values. The source is the script's own statements for a
        // function defined beside it, and the module's for one it called into.
        let script_stmts: &[Statement<'db>] = match unit {
            ParsedUnit::Fragment { stmts, .. } => stmts.as_slice(),
            ParsedUnit::Expr(_) => &[],
        };
        let instantiation_consts = |callee: CalleeKey, indices: &[usize], values: &[ConstValue]| {
            match callee {
                CalleeKey::Local(id) => {
                    let Some(name) = lowered_funcs.func_name_to_id.iter()
                        .find(|(_, func_id)| func_id.0 == id.0)
                        .map(|(name, _)| name.clone())
                    else {
                        return (HashMap::new(), Vec::new());
                    };
                    self.instantiation_consts_from(
                        script_stmts, &name, typecheck.expr_types, typecheck.call_targets,
                        indices, values,
                        &lowered_funcs.functions, &lowered_funcs.func_name_to_id,
                    )
                }
                CalleeKey::Module(module, id) => {
                    let Some(original) = registry.get_module_function_as_unit(module, id) else {
                        return (HashMap::new(), Vec::new());
                    };
                    let name = original.name.clone();
                    // The module's own expressions were typechecked with the
                    // module graph, not with this script.
                    let Some((module_id, parsed)) = self.shared_context.parsed_graph
                        .statements_only(self.db)
                        .get(module.0 as usize)
                    else {
                        return (HashMap::new(), Vec::new());
                    };
                    let module_results = self.shared_context.graph_typecheck.module_results(self.db);
                    let Some(single) = module_results.get(module_id) else {
                        return (HashMap::new(), Vec::new());
                    };
                    let funcs: Vec<std::sync::Arc<IrCodeUnit>> = registry.iter_module_code_units_with_ids()
                        .filter(|((m, _), _)| *m == module)
                        .map(|(_, u)| std::sync::Arc::new(u.clone()))
                        .collect();
                    let names: HashMap<String, datalove_datafun_ir::FuncId> = funcs.iter()
                        .map(|u| (u.name.clone(), datalove_datafun_ir::FuncId(u.id.0)))
                        .collect();
                    self.instantiation_consts_from(
                        &parsed.statements, &name,
                        single.expr_types(self.db), single.call_targets(self.db),
                        indices, values, &funcs, &names,
                    )
                }
            }
        };

        let (ir_unit, errors) = datalove_datafun_compiler::specialize::specialize_script_unit(
            &ir_unit, &module_unit, &instantiation_consts);

        if !errors.is_empty() {
            return Err(ScriptCompilationResult {
                typecheck: TypecheckResult::Success,
                ownership: OwnershipResult::Success,
                lowering: LoweringResult::Error { message: errors.join("; ") },
                ir_unit: None,
            });
        }

        Ok(ir_unit)
    }

    fn phase_const_inline(
        &self,
        ir_unit: IrCodeUnit,
        consts: &ConstEvalOutput,
    ) -> IrCodeUnit {
        // In skip_const_inlining mode, skip inlining - consts are evaluated at runtime.
        if self.skip_const_inlining {
            return ir_unit;
        }

        // Build the const values map for inlining.
        // Combine script-level and function-level consts.
        let mut const_values: HashMap<String, ConstValue> = HashMap::new();

        // Add script-level consts.
        for (name, value) in consts.resolved_consts.iter() {
            const_values.insert(name.to_string(), value.clone());
        }

        // Add function-level consts (already qualified as "func_name::const_name").
        for (qualified_name, (_, value)) in &consts.func_consts {
            // Extract the local const name for function-level consts.
            // The key in func_consts is "func_name::const_name".
            if let Some(pos) = qualified_name.rfind("::") {
                let local_name = &qualified_name[pos + 2..];
                const_values.insert(local_name.to_string(), value.clone());
            }
        }

        // Run the const inlining pass.
        inline_script_consts(ir_unit, &const_values)
    }

    // ========================================================================
    // Per-unit record
    // ========================================================================

    /// Record what unit `index` produced, for the units after it to be built on.
    ///
    /// Replaces whatever was there, which is what re-lowering a unit needs:
    /// what the unit provided before the edit is gone, and only what it
    /// provides now stands between the units around it.
    fn record_unit(
        &mut self,
        index: usize,
        ir_unit: &IrCodeUnit,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
        consts: &ConstEvalOutput,
    ) {
        let script_ctx = ir_unit.script_context();
        self.unit_records[index] = UnitLowerRecord {
            exports: script_ctx.map(|ctx| ctx.exports.clone()).unwrap_or_default(),
            value_types: ir_unit.value_types.clone(),
            slot_types: ir_unit.slot_types.clone(),
            consts: consts.declared_consts.clone(),
            dead_exports: ownership.dead_exports.clone(),
            revived: ownership.revived.clone(),
            provides: self.unit_provides_names(typecheck.output),
            uses: self.unit_used_names(index, typecheck.output),
            imports: typecheck.output.imported_modules(self.db).C(),
        };
    }
}
