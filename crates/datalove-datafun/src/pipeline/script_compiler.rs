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
use std::sync::Arc;

use datalove_datafun_ast::ast::{Body, ParsedExpr, Statement, TopLevel};
use datalove_datafun_compiler::lower::{
    lower_script_fragment_raw, lower_script_expr, lower_script_functions,
};
use datalove_datafun_compiler::const_eval::{
    evaluate_body_consts, evaluate_const, evaluate_instantiation_consts, ConstError, ConstEvalEnv,
};
use datalove_datafun_const::{inline_script_consts, ScriptFunctionConstsResult};
use datalove_datafun_compiler::tracked_script_lower::{
    UnitLowerRecord, collect_const_graph, external_consts_over, externals_over, lower_context_over,
    script_consts_over,
};
use datalove_datafun_compiler::tracked_script_ownership::{
    analyze_script_fragment_tracked, analyze_script_expr_tracked, ScriptAnalysisData, ExternalBinding,
};
use datalove_datafun_compiler::lower::ScriptFunctionAnalyses;
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, CtfeError, IrCodeUnit, IrType, ResolvedConsts, ConstEvalError};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_compiler::instances::GenericsPolicy;
use datalove_datafun_tycheck::{
    typecheck_script_unit, unit_ast, unit_parses,
    Script, ScriptEnv, ScriptUnit, ScriptUnitKind,
    UnitTypecheckResultTracked,
};

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

/// What a splice re-derived, and where it started.
///
/// The caller drops the runtime state from `from` on and runs `rederived` as
/// appends. `from` is the splice point unless the splice had to start at an
/// earlier binding's provider, to rerun it and restore the binding.
pub struct Splice {
    pub from: usize,
    pub rederived: Vec<(usize, ScriptCompilationResult)>,
}

/// The graph the units' records held before a re-derivation, which the reach
/// is taken over beside the graph as it is now.
struct HeldGraph {
    provides: Vec<BTreeSet<String>>,
    uses: Vec<BTreeSet<String>>,
    /// What each unit took or wrote of earlier units' bindings.
    changed: Vec<BTreeSet<String>>,
    /// What each unit took of earlier units' bindings.
    takes: Vec<BTreeSet<String>>,
}

/// Output from ownership phase.
struct OwnershipOutput<'db> {
    func_analyses: ScriptFunctionAnalyses<'db>,
    script_analysis: Option<ScriptAnalysisData>,
    /// Names this unit exports that hold no value.
    dead_exports: Vec<String>,
    /// Names an earlier unit gave away that this unit assigned to.
    revives: Vec<String>,
    /// The earlier units' bindings this unit gave away.
    takes: Vec<String>,
    /// The earlier units' bindings this unit wrote to.
    writes: Vec<String>,
    /// Those of them it overwrote rather than updated.
    overwrites: Vec<String>,
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
    func_consts: HashMap<String, (IrType, Arc<ConstValue>)>,
    /// Every script-level const in scope, this unit's and earlier units'.
    ///
    /// Held apart from `resolved_consts` because those are what the inlining
    /// pass substitutes, which `skip_const_inlining` turns off, while these are
    /// what a function body naming a script const is lowered against, which it
    /// does not: a body resolves the const where the reference is lowered, so
    /// there is no later pass for the flag to skip.
    script_consts: HashMap<String, (IrType, Arc<ConstValue>)>,
    /// Just this unit's own script-level consts, for its record.
    declared_consts: Vec<(String, IrType, Arc<ConstValue>)>,
}

/// Parsed script unit ready for compilation.
enum ParsedUnit<'db> {
    /// A fragment (statements).
    ///
    /// The parse itself is not carried here: `unit_ast` holds it, keyed on the
    /// unit, and these are the statements the later phases walk.
    Fragment {
        top: TopLevel<'db>,
        body: &'db Body<'db>,
    },
    /// A single expression.
    Expr(ParsedExpr<'db>),
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
        ctfe_evaluator: Box<dyn CtfeEvaluator>,
    ) -> Option<ScriptCompiler<'db>> {
        if self.has_errors() {
            return None;
        }

        // The module *handles*, in the graph's dependency order. Nothing about a
        // module's content is read here: a unit asks `script_module_names` for
        // the modules its imports name, so building this compiler parses no
        // module and resolves no module's names.
        let modules: Vec<bct::module_graph::Module<'db>> =
            self.shared.module_graph.iter_modules(db).collect();

        Some(ScriptCompiler {
            db,
            scripts: Vec::new(),
            env: ScriptEnv::new(
                db, modules, self.shared.module_graph.data(db).C()),
            last_script: None,
            unit_records: Vec::new(),
            last_source: None,
            ctfe_evaluator: RefCell::new(ctfe_evaluator),
            skip_const_inlining: false,
            skip_specialization: false,
            generics: GenericsPolicy::Off,
            shared_context: self.shared.clone(),
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
        self.script_compiler(db, Box::new(evaluator))
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

/// What a unit provides, and how it uses earlier units' bindings.
///
/// Apart from `provides`, the names are of bindings earlier units made. Each
/// name in `uses` is in exactly one of the five kinds of use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitAccesses {
    pub provides: Vec<String>,
    /// Every name the unit asked its environment for.
    pub uses: Vec<String>,
    /// Only read.
    pub reads: Vec<String>,
    /// Given away.
    pub takes: Vec<String>,
    /// Written on every way through the unit before anything read it.
    pub overwrites: Vec<String>,
    /// Written, and the value it held read or kept on some way through.
    pub updates: Vec<String>,
    /// Written after an earlier unit gave it away.
    pub revives: Vec<String>,
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
    generics: GenericsPolicy,
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

    /// The type of `name`, which unit `unit` provides, as datalove writes it.
    pub fn type_of(&self, unit: usize, name: &str) -> &str {
        self.records[unit].type_of(name)
    }

    /// The type of expression unit `unit`'s value, as datalove writes it.
    pub fn value_type(&self, unit: usize) -> &str {
        self.records[unit].value_type.as_deref().expect("the unit is an expression that compiled")
    }

    /// What unit `index` provides, and what it does to earlier units'
    /// bindings, as of its last compile. A unit that did not compile does
    /// nothing.
    pub fn unit_accesses(&self, index: usize) -> UnitAccesses {
        let record = &self.records[index];
        let updates: Vec<String> = record.writes.iter()
            .filter(|name| !record.overwrites.contains(name) && !record.revives.contains(name))
            .cloned()
            .collect();
        let reads = record.uses.iter()
            .filter(|name| ![&record.takes, &record.writes].iter().any(|kind| kind.contains(name)))
            .cloned()
            .collect();
        UnitAccesses {
            provides: record.provides.clone(),
            uses: record.uses.clone(),
            reads,
            takes: record.takes.clone(),
            overwrites: record.overwrites.clone(),
            updates,
            revives: record.revives.clone(),
        }
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
    /// The modules the script is checked against.
    env: ScriptEnv<'db>,
    /// The script as it stood for the last unit compiled.
    ///
    /// Held apart from `scripts` because re-deriving compiles units in the
    /// middle of it, and the diagnostics the caller is about to render are the
    /// last compiled unit's.
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
    ///
    /// In a `RefCell` because the phases that evaluate consts take `&self`.
    ctfe_evaluator: RefCell<Box<dyn CtfeEvaluator>>,
    /// When true, const bindings in functions are lowered as let bindings.
    skip_const_inlining: bool,
    /// When true, comptime calls are left naming the original function.
    skip_specialization: bool,
    /// Which calls to generics are pointed at instances.
    generics: GenericsPolicy,
    /// Shared module context for cross-module CTFE function calls.
    shared_context: Arc<SharedModuleContext<'db>>,
}

impl<'db> ScriptCompiler<'db> {
    /// Compile a script fragment (statements), as the next unit.
    ///
    /// The unit joins the script whether or not it compiles; see
    /// [`Self::compile_unit_at`] for what a unit that failed amounts to.
    pub fn compile_fragment(&mut self, source: &str) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.append_unit(src, false)
    }

    /// Compile a script expression, as the next unit.
    pub fn compile_expr(&mut self, source: &str) -> ScriptCompilationResult {
        let src = bct::input::Source::new(self.db, source.S());
        self.append_unit(src, true)
    }

    /// Compile a unit from a source the caller made, as the next unit.
    ///
    /// For a caller that reads the unit before it is compiled -- for the
    /// modules it requires, say -- and wants that read and the compile to be
    /// of one input.
    pub fn compile_source(&mut self, source: bct::input::Source, is_expr: bool) -> ScriptCompilationResult {
        self.append_unit(source, is_expr)
    }

    /// Put a new unit on the end of the script and compile it.
    ///
    /// **A unit that fails stays.** It provides nothing to what follows, so the
    /// units after it are built as if it were not there, and an edit that
    /// makes it compile is an edit like any other: the reach of an edit
    /// includes a unit that asked for a name in vain and now finds it. Its
    /// executor gives it an index with no frame, so the numbering every
    /// `(unit, value)` reference shares has no hole in it.
    fn append_unit(
        &mut self,
        src: bct::input::Source,
        is_expr: bool,
    ) -> ScriptCompilationResult {
        let script_unit = ScriptUnit::new(self.db, src, is_expr);
        let script = Script::new(self.db, self.scripts.last().copied(), script_unit);
        self.scripts.push(script);
        self.unit_records.push(UnitLowerRecord::default());
        self.rederive_one(self.scripts.len() - 1)
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
    /// bindings drop out of the environment, and stays where it is: the
    /// numbering cannot have a hole in it. Its executor drops its frame.
    pub fn relower_reach(&mut self, edited: usize) -> Vec<(usize, ScriptCompilationResult)> {
        assert!(
            edited < self.scripts.len(),
            "unit {edited} was edited but the script has {} units",
            self.scripts.len(),
        );
        self.rederive_reach(BTreeSet::from([edited]))
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
        self.rederive_reach(seeds)
    }

    /// Drop the units from `n` on.
    ///
    /// The cheap one of the three splice verbs and **the only one that
    /// renumbers nothing**: there is nothing after the units that go, so no
    /// surviving unit's `(unit_index, ValueId)` references move. Usually
    /// nothing is re-derived and the splice starts at `n`. But a unit that goes
    /// may have taken or written an earlier unit's binding, and then the
    /// rerun starts at that binding's provider, and the units from there to
    /// `n` are re-derived to build it again (see [`Self::rerun_start`]).
    /// The caller drops the runtime state from the splice's start on --
    /// `ScriptExecutor::truncate_units` -- and runs what comes back as appends.
    ///
    /// The units' `Source`s are inputs and are simply let go of; appending
    /// afterwards mints a new one, so the index they vacate is reused by a unit
    /// with a key of its own.
    pub fn truncate_units(&mut self, n: usize) -> Splice {
        assert!(
            n <= self.scripts.len(),
            "cannot truncate to {n} units; the script has {}",
            self.scripts.len(),
        );
        let mut units = self.unit_list();
        units.truncate(n);
        self.splice(units, n)
    }

    /// Splice the unit at `i` out, and re-derive everything after it.
    ///
    /// See [`Self::splice`] for why the whole suffix is re-derived. A unit that
    /// read what the removed unit provided stops compiling, and stays.
    pub fn delete_unit(&mut self, i: usize) -> Splice {
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
    /// See [`Self::splice`] for why the whole suffix is re-derived. The new
    /// unit joins the script whether or not it compiles, as an appended one
    /// does.
    pub fn insert_unit(
        &mut self,
        i: usize,
        source: &str,
        is_expr: bool,
    ) -> Splice {
        let src = bct::input::Source::new(self.db, source.S());
        self.insert_source(i, src, is_expr)
    }

    /// The same, from a source the caller made; see [`Self::compile_source`].
    pub fn insert_source(
        &mut self,
        i: usize,
        source: bct::input::Source,
        is_expr: bool,
    ) -> Splice {
        let mut units = self.unit_list();
        assert!(
            i <= units.len(),
            "unit {i} was inserted but the script has {} units",
            units.len(),
        );

        units.insert(i, (source, is_expr));
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
    /// The suffix can start before `at`: a unit from `at` on, which is
    /// dropped or run again, may have taken or written an earlier unit's
    /// binding, which has to be built again first (see [`Self::rerun_start`]).
    ///
    /// A unit of the suffix that stops compiling stays, as an appended unit
    /// that fails does, and so does one that already did not compile.
    fn splice(&mut self, units: Vec<(bct::input::Source, bool)>, at: usize) -> Splice {
        let from = self.rerun_start(at);
        self.set_units(units, from);
        let rederived = self.rederive((from..self.scripts.len()).collect());
        Splice { from, rederived }
    }

    /// Where a splice at `at` has to start for the bindings before it to be as
    /// they were made.
    ///
    /// The units from `at` on are about to be dropped or run again, and one
    /// that took or wrote an earlier unit's binding left it changed: moved out
    /// of, or holding what a `set` put there. Running it again would read the
    /// change, and dropping it would leave the change behind. So the splice
    /// starts at that binding's provider instead, which builds the binding
    /// again. The units between are run again as well, so what they changed
    /// counts too, and the point moves back until nothing earlier is changed.
    fn rerun_start(&self, at: usize) -> usize {
        let held = self.held_graph();
        let provider = |unit: usize, name: &String| {
            (0..unit).rev().find(|earlier| held.provides[*earlier].contains(name))
        };
        let mut from = at;
        loop {
            let earliest = (from..held.changed.len())
                .flat_map(|unit| held.changed[unit].iter().filter_map(move |name| provider(unit, name)))
                .min();
            match earliest {
                Some(source) if source < from => from = source,
                _ => return from,
            }
        }
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
            .map(|index| (index, self.rederive_one(index)))
            .collect()
    }

    /// Compile unit `index`, with the diagnostics the caller renders next
    /// pointed at it.
    fn rederive_one(&mut self, index: usize) -> ScriptCompilationResult {
        let unit = self.scripts[index].unit(self.db);
        self.last_script = Some(self.scripts[index]);
        self.last_source = Some(unit.source(self.db));
        self.compile_unit_at(index)
    }

    /// Compile again the units `seeds` reach, in index order, and say what
    /// each came to.
    ///
    /// What a unit took or wrote of an earlier unit's is only known once the
    /// unit is compiled, and a unit that now takes or writes something reaches
    /// the units after it that use it. So the reach is taken, its units are
    /// compiled, and the reach is taken again with what they now do, until it
    /// settles. What the units did before the edit is held from the
    /// start, being what the running session went through.
    fn rederive_reach(&mut self, seeds: BTreeSet<usize>) -> Vec<(usize, ScriptCompilationResult)> {
        let held = self.held_graph();
        let mut changed = held.changed.clone();
        for _ in 0..=self.unit_records.len() {
            let units = self.reach_from(&seeds, &held, &changed);
            let results = self.rederive(units.clone());
            for (unit, record) in self.unit_records.iter().enumerate() {
                for name in record.takes.iter().chain(&record.writes) {
                    changed[unit].insert(name.C());
                }
            }
            // What the units do now can shrink the reach as well as grow it:
            // an edit that makes a unit overwrite what it updated needs no
            // rerun, which only its new record says. The results are for the
            // reach as it settles; a unit compiled on the way and left out of
            // it compiled to what it was.
            if self.reach_from(&seeds, &held, &changed) == units {
                return results;
            }
        }
        panic!("the reach of an edit did not settle");
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
    /// resolves to a unit already reached. A unit that took or wrote an earlier
    /// unit's binding counts as providing it too, since the units after it see
    /// the binding as it left it. One forward pass gives the whole forward
    /// reach, because a unit's providers all sit before it.
    ///
    /// **And the reach goes back.** A unit runs again against the bindings it
    /// uses as they are now, and they are not as it first found them if it,
    /// or a unit after it, took or wrote them since: it would read a moved-out
    /// value, or one a later `set` left. A change made before it, by a unit
    /// between the provider and it, is just what it read, and stays. So the
    /// provider is reached as well, which builds the binding again. That is a
    /// pass over what the units did before this edit, `held.changed`, which is
    /// what the running session went through. It repeats until no provider is
    /// added.
    ///
    /// **A provider reached that way is only rerun.** Its code is the same
    /// and so are its inputs, so it builds the values it built before, and a
    /// unit that only read them would compute what it already holds. What has
    /// to run again after it is the units that took or wrote what it builds,
    /// which left the binding changed and have to change it again; those are
    /// rerun in turn, and so on down. A unit is re-derived for its own sake
    /// -- a seed, or a user of what such a unit provides -- or to restore a
    /// binding, and the first wins.
    ///
    /// The graph is the union of what it was before the edit and what it is
    /// after, since either alone would miss a case: a binding the edit
    /// *removes* is absent from the new graph although the unit that read it
    /// has to be told, and a name the edit *introduces* is absent from the old
    /// one although a later unit that asked for it in vain now finds it.
    fn reach_from(
        &self,
        seeds: &BTreeSet<usize>,
        held: &HeldGraph,
        changed: &[BTreeSet<String>],
    ) -> Vec<usize> {
        let count = held.provides.len();
        let (now_provides, now_uses) = self.graph_now();
        assert_eq!(
            count, now_provides.len(),
            "a record per unit and a typecheck output per unit, or the two graphs \
             do not line up",
        );

        let union = |held: &BTreeSet<String>, now: &[String]| -> BTreeSet<String> {
            held.iter().chain(now.iter()).cloned().collect()
        };
        let provides: Vec<_> = (0..count)
            .map(|unit| union(&held.provides[unit], &now_provides[unit]))
            .collect();
        let uses: Vec<_> = (0..count)
            .map(|unit| union(&held.uses[unit], &now_uses[unit]))
            .collect();
        let provider = |unit: usize, name: &String| {
            (0..unit).rev().find(|earlier| provides[*earlier].contains(name))
        };

        // Whether `unit` writes the binding `name` without reading what it
        // held, as it does now: an overwrite, or a revival, which cannot read
        // a binding that was given away.
        let writes_without_reading = |unit: usize, name: &String| {
            let record = &self.unit_records[unit];
            record.overwrites.contains(name) || record.revives.contains(name)
        };

        // The nearest unit before `unit` that set the binding `name` without
        // reading it, as the units do now.
        let setter = |unit: usize, name: &String| {
            (0..unit).rev().find(|earlier| {
                provides[*earlier].contains(name) || writes_without_reading(*earlier, name)
            })
        };

        // The unit the binding `name` is as a unit at `unit` finds it: the
        // nearest earlier one that provided, took or wrote it.
        let source = |unit: usize, name: &String| {
            (0..unit).rev().find(|earlier| {
                provides[*earlier].contains(name) || changed[*earlier].contains(name)
            })
        };

        let mut rerun_starts = BTreeSet::new();
        loop {
            let mut rederived = vec![false; count];
            let mut restoring = vec![false; count];
            for unit in 0..count {
                // A unit that writes a binding without reading it does not
                // care how an earlier unit left it, only what the binding is,
                // which its provider says. If its write was retracted it is
                // rerun.
                //
                // Nor does a unit after a take that is still there: the
                // binding holds nothing after it, as it did before.
                rederived[unit] = seeds.contains(&unit)
                    || uses[unit].iter().any(|name| {
                        source(unit, name).is_some_and(|source| {
                            let still_taken = !provides[source].contains(name)
                                && held.takes[source].contains(name)
                                && self.unit_records[source].takes.contains(name);
                            rederived[source]
                                && !still_taken
                                && (provides[source].contains(name) || !writes_without_reading(unit, name))
                        })
                    });
                restoring[unit] = !rederived[unit]
                    && (rerun_starts.contains(&unit)
                        || changed[unit].iter().any(|name| {
                            source(unit, name)
                                .is_some_and(|source| rederived[source] || restoring[source])
                        }));
            }
            let reached = |unit: usize| rederived[unit] || restoring[unit];

            let mut added = false;
            for unit in (0..count).filter(|unit| reached(*unit)) {
                for name in &uses[unit] {
                    let Some(source) = provider(unit, name) else {
                        continue;
                    };
                    // What builds the binding as this unit finds it is the
                    // nearest unit before it that sets it without reading it:
                    // its provider, or an overwrite after that. Running that
                    // one again is enough, and if it runs anyway, nothing is
                    // needed.
                    let setter = setter(unit, name).expect("a binding with a provider has a setter");
                    if reached(setter) {
                        continue;
                    }
                    // A unit that overwrites the binding needs nothing of what
                    // it held, so it does not matter what changed it. The
                    // units after it that changed it are rerun from it.
                    if writes_without_reading(unit, name) {
                        continue;
                    }
                    let since = (unit..count)
                        .take_while(|later| provider(*later, name) == Some(source))
                        .any(|later| held.changed[later].contains(name));
                    if since {
                        added |= rerun_starts.insert(setter);
                    }
                }
            }
            if !added {
                return (0..count).filter(|unit| reached(*unit)).collect();
            }
        }
    }

    /// What each unit provided, used, and took or wrote of earlier units'
    /// bindings, as its record held it before anything is compiled again.
    fn held_graph(&self) -> HeldGraph {
        let set = |names: &[String]| names.iter().cloned().collect::<BTreeSet<String>>();
        HeldGraph {
            provides: self.unit_records.iter().map(|record| set(&record.provides)).collect(),
            uses: self.unit_records.iter().map(|record| set(&record.uses)).collect(),
            changed: self.unit_records.iter()
                .map(|record| record.takes.iter().chain(&record.writes).cloned().collect())
                .collect(),
            takes: self.unit_records.iter().map(|record| set(&record.takes)).collect(),
        }
    }

    /// What each unit provides and what it asks for, as things stand now.
    ///
    /// Both are the typechecker's own record -- `new_vars`, `new_fns` and
    /// `uses` from stage A -- plus the module aliases, which the
    /// typechecker resolves through a different query and so does not put in
    /// `uses`: a `require` in one unit provides an alias that an
    /// `import` in a later unit names.
    fn graph_now(&self) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
        let outputs = self.unit_typecheck_outputs();
        let mut provides = Vec::with_capacity(outputs.len());
        let mut uses = Vec::with_capacity(outputs.len());
        for (index, output) in outputs.iter().enumerate() {
            match output {
                Some(output) => {
                    provides.push(self.unit_provides_names(*output));
                    uses.push(self.unit_used_names(index, *output));
                }
                None => {
                    provides.push(Vec::new());
                    uses.push(Vec::new());
                }
            }
        }
        (provides, uses)
    }

    /// The modules each unit imports from, as things stand now.
    fn imports_now(&self) -> Vec<Vec<String>> {
        self.unit_typecheck_outputs().iter()
            .map(|output| output.map(|output| output.imported_modules(self.db).C()).unwrap_or_default())
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
            .chain(output.new_type_aliases(db).iter().map(|(name, _)| name.as_str(db).S()))
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
            output.uses(db).iter().map(|name| name.as_str(db).S()).collect();
        let qualified_calls = match &unit_ast(db, self.scripts[index].unit(db)).kind {
            ScriptUnitKind::Fragment(parsed) => {
                for statement in parsed.statements(db).iter() {
                    if let Statement::Import(import) = statement {
                        used.push(import.module_name.as_str(db).S());
                    }
                }
                &parsed.qualified_calls
            }
            ScriptUnitKind::Expr(parsed) => &parsed.qualified_calls,
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
        self.generics = session.generics;
    }

    /// Hand back what an edit needs to survive letting this compiler go.
    pub fn into_session(self) -> ScriptSession {
        let units = self.unit_list();
        ScriptSession {
            units,
            records: self.unit_records,
            skip_const_inlining: self.skip_const_inlining,
            skip_specialization: self.skip_specialization,
            generics: self.generics,
        }
    }

    /// The earlier units' bindings each unit took or wrote, in name order.
    ///
    /// What the reach of an edit goes back along; tests derive their
    /// expectations from it.
    pub fn unit_changes(&self) -> Vec<Vec<String>> {
        self.unit_records.iter()
            .map(|record| {
                let mut names: Vec<String> = record.takes.iter().chain(&record.writes).cloned().collect();
                names.sort();
                names.dedup();
                names
            })
            .collect()
    }

    /// Each unit's typecheck output, in order, for the units compiled so far.
    ///
    /// What a unit provided and what it asked the environment for -- the
    /// dependency graph over the session. Empty before anything has compiled.
    /// See `botdocs/plan-script-reactivity.md`.
    ///
    /// `None` for a unit that did not parse, which is not typechecked: it
    /// provides nothing and asks for nothing.
    pub fn unit_typecheck_outputs(
        &self,
    ) -> Vec<Option<datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>>> {
        self.scripts.iter()
            .map(|script| {
                unit_parses(self.db, script.unit(self.db))
                    .then(|| typecheck_script_unit(self.db, *script, self.env))
            })
            .collect()
    }

    /// The source of the unit compiled last.
    pub fn last_source(&self) -> Option<bct::input::Source> {
        self.last_source
    }

    /// Get parse diagnostics from the last compilation.
    pub fn get_parse_diagnostics(&self) -> Vec<&datalove_diagnostic::ParseDiagnostic> {
        if let Some(src) = self.last_source {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, src)
        } else {
            Vec::new()
        }
    }

    /// Unit `index`'s parse, type and ownership diagnostics as they stand,
    /// rendered as text in `style`, quoting the unit as the file `name`.
    ///
    /// A unit's typecheck asks of the units before it, so its diagnostics
    /// include theirs; only those pointing into this unit, or nowhere, are
    /// its. Lowering errors are not diagnostics, and are not rendered.
    pub fn render_unit_diagnostics(&self, index: usize, name: &str, style: bct::render::TextStyle) -> String {
        let script = self.scripts[index];
        let unit = script.unit(self.db);
        let source = unit.source(self.db);
        let parse = if unit.is_expr(self.db) {
            datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, source)
        } else {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, source)
        };
        let mut diagnostics: Vec<bct::diagnostic::Diagnostic<'db>> = parse.iter()
            .map(|diagnostic| diagnostic.to_diagnostic(self.db))
            .collect();
        if unit_parses(self.db, unit) {
            let types = typecheck_script_unit::accumulated::<datalove_diagnostic::TypeDiagnostic>(self.db, script, self.env);
            diagnostics.extend(types.iter()
                .map(|diagnostic| diagnostic.to_diagnostic(self.db))
                .filter(|diagnostic| {
                    diagnostic.labels.is_empty() || diagnostic.labels.iter().any(|label| label.source == source)
                }));
            diagnostics.extend(self.unit_ownership_diagnostics(index).iter()
                .map(|diagnostic| diagnostic.to_diagnostic(self.db)));
        }
        bct::render::render_diagnostics_to_string(
            self.db, diagnostics, std::path::Path::new(name), std::path::Path::new(""), style)
    }

    /// Get type diagnostics from the last compilation.
    ///
    /// Those of the last unit and whatever its typechecking asked of the units
    /// before it, which never includes typechecking one that did not parse.
    pub fn get_type_diagnostics(&self) -> Vec<&datalove_diagnostic::TypeDiagnostic> {
        match self.last_script {
            Some(script) if unit_parses(self.db, script.unit(self.db)) => {
                typecheck_script_unit::accumulated::<datalove_diagnostic::TypeDiagnostic>(
                    self.db, script, self.env)
            }
            _ => Vec::new(),
        }
    }

    /// Ownership diagnostics from the last compilation: those of the last
    /// unit compiled, if it got as far as ownership analysis.
    pub fn get_ownership_diagnostics(&self) -> Vec<&datalove_diagnostic::OwnershipDiagnostic> {
        let Some(last) = self.last_script else {
            return Vec::new();
        };
        let index = self.scripts.iter().position(|script| *script == last)
            .expect("the last unit compiled is in the script");
        self.unit_ownership_diagnostics(index)
    }

    /// Unit `index`'s ownership diagnostics as they stand; none if it does not
    /// get as far as ownership analysis.
    ///
    /// **They are read back from the analysis, as a unit's type diagnostics
    /// are from its typecheck**, but the analysis is keyed on what the units
    /// before it left behind, which the unit records hold, and not on the
    /// unit's position alone. Asked again with what the records say now, it
    /// is the query the unit's compile ran, since an edit that changes what
    /// the unit sees compiles it again.
    pub fn unit_ownership_diagnostics(&self, index: usize) -> Vec<&datalove_diagnostic::OwnershipDiagnostic> {
        let unit = self.scripts[index].unit(self.db);
        if !unit_parses(self.db, unit) {
            return Vec::new();
        }
        let output = typecheck_script_unit(self.db, self.scripts[index], self.env);
        let result = output.result(self.db);
        if !result.errors(self.db).is_empty() {
            return Vec::new();
        }
        let source = unit.source(self.db);
        let (externals, external_consts) = self.ownership_inputs(index, output);
        match self.parsed_unit_at(index) {
            ParsedUnit::Fragment { top, .. } => analyze_script_fragment_tracked::accumulated::<datalove_diagnostic::OwnershipDiagnostic>(
                self.db, source, result, top, externals, external_consts),
            ParsedUnit::Expr(parsed) => analyze_script_expr_tracked::accumulated::<datalove_diagnostic::OwnershipDiagnostic>(
                self.db, source, result, parsed, externals, external_consts),
        }
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

    /// Which calls to generics are pointed at instances; see
    /// `datalove_datafun_compiler::instances`.
    pub fn set_generics_policy(&mut self, policy: GenericsPolicy) {
        self.generics = policy;
    }

    /// Skip compile-time const evaluation and inlining.
    ///
    /// When enabled, const bindings are evaluated at runtime instead of being
    /// replaced with literal values at compile time. This is useful for testing
    /// and debugging const expressions.
    pub fn set_skip_const_inlining(&mut self, enabled: bool) {
        self.skip_const_inlining = enabled;
    }

    /// The result for unit `index`, whose lowering failed with `error`.
    ///
    /// **A name the typechecker found and lowering cannot is one an earlier
    /// unit binds and did not compile.** The typechecker sees what a unit that
    /// typechecked provides, and lowering only what one that compiled does, so
    /// a unit that failed in ownership analysis or later leaves its names to
    /// the one and not the other. That is said as such, rather than as a
    /// binding that is not available yet.
    fn lowering_failure(
        &self,
        index: usize,
        context: &str,
        error: datalove_datafun_compiler::lower::LowerError,
    ) -> ScriptCompilationResult {
        let datalove_datafun_compiler::lower::LowerError::BindingNotAvailable(name) = &error;
        let outputs = self.unit_typecheck_outputs();
        let failed_provider = (0..index).rev()
            .find(|unit| {
                outputs[*unit].is_some_and(|output| self.unit_provides_names(output).contains(name))
            })
            .filter(|unit| self.unit_records[*unit].provides.is_empty())
            .map(|unit| (name, unit));
        let message = match failed_provider {
            Some((name, unit)) => format!("`{name}` is bound by unit {unit}, which did not compile"),
            None => format!("{context}{error}"),
        };
        ScriptCompilationResult {
            typecheck: TypecheckResult::Success,
            ownership: OwnershipResult::Success,
            lowering: LoweringResult::Error { message },
            ir_unit: None,
        }
    }

    /// The result for unit `index`, which did not parse.
    fn parse_failure_at(&self, index: usize) -> ScriptCompilationResult {
        let unit = self.scripts[index].unit(self.db);
        let source = unit.source(self.db);
        let parse_diags = if unit.is_expr(self.db) {
            datalove_datafun_parser::parse_expr::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, source)
        } else {
            datalove_datafun_parser::parse::accumulated::<datalove_diagnostic::ParseDiagnostic>(self.db, source)
        };
        let parse_errors: Vec<String> = parse_diags.iter()
            .map(|d| {
                let diag = d.to_diagnostic(self.db);
                diag.message.as_str(self.db).S()
            })
            .collect();
        ScriptCompilationResult {
            typecheck: TypecheckResult::ParseError { errors: parse_errors },
            ownership: OwnershipResult::Skipped,
            lowering: LoweringResult::Skipped,
            ir_unit: None,
        }
    }

    /// The statements or the expression unit `index` parsed to.
    ///
    /// Read out of `unit_ast`, which holds the parse keyed on the unit alone,
    /// so re-lowering a unit costs no second parse of it.
    fn parsed_unit_at(&self, index: usize) -> ParsedUnit<'db> {
        let spec = unit_ast(self.db, self.scripts[index].unit(self.db));
        match &spec.kind {
            ScriptUnitKind::Fragment(parsed) => {
                ParsedUnit::Fragment { top: parsed.top, body: parsed.body(self.db) }
            }
            ScriptUnitKind::Expr(parsed) => ParsedUnit::Expr(parsed.clone()),
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
    /// provides -- nothing at all, if it failed. A unit that did not parse goes
    /// no further than that: its tree is not typechecked.
    fn compile_unit_at(&mut self, index: usize) -> ScriptCompilationResult {
        if !unit_parses(self.db, self.scripts[index].unit(self.db)) {
            self.unit_records[index] = UnitLowerRecord::default();
            return self.parse_failure_at(index);
        }
        let unit = self.parsed_unit_at(index);
        // Marked for a recorder: ownership analysis is keyed on what the
        // earlier units left behind, so its key alone does not say which
        // position in the script it is.
        let mut result = datalove_ct::query_events::compiling_unit(index, || self.run_phases(index, &unit));
        match &mut result.ir_unit {
            None => self.unit_records[index] = UnitLowerRecord::default(),
            Some(ir_unit) => {
                ir_unit.script_context_mut()
                    .expect("a script unit compiles to a script code unit")
                    .unit = Some(index as u32);
            }
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
        //
        // This unit's data consts are in the first stratum's scope already;
        // see `data_consts`.
        let mut first_consts = script_consts_over(&self.unit_records[..index]);
        if let ParsedUnit::Fragment { body, .. } = unit {
            first_consts.extend(datalove_datafun_compiler::const_eval::data_consts(
                self.db, body, typecheck.expr_types, self.env.data(self.db)));
        }
        let lowered_funcs = match self.phase_lower_functions(
            index, unit, &typecheck, &ownership, &first_consts, true,
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

        // Phase 5: Const parameter specialization.
        let ir_unit = match self.phase_specialize(ir_unit, unit, &typecheck, &lowered_funcs, &consts) {
            Ok(ir) => ir,
            Err(result) => return result,
        };

        // Phase 5': calls to generics pointed at instances, after const
        // specialization so that a const copy, still generic, can be
        // instantiated too.
        let ir_unit = self.phase_instantiate(ir_unit);

        // Phase 6: Every function finished, as a module's are: calls to
        // `inline` functions replaced by their bodies, after specialization so
        // that a specialized copy is inlined too, and consts promoted, after
        // specialization, which reads const arguments out of the `Const`
        // instructions promotion replaces. See `optimize::finish`.
        let ir_unit = self.phase_finish(ir_unit);

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
            .map(|e| e.error(self.db).to_string())
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

    /// What unit `index`'s ownership analysis is asked with besides the unit:
    /// the bindings it asked for of the units before it, and their consts.
    fn ownership_inputs(
        &self,
        index: usize,
        output: datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>,
    ) -> (Vec<ExternalBinding>, Vec<String>) {
        let asked: Vec<String> = output.uses(self.db).iter()
            .map(|name| name.as_str(self.db).S())
            .collect();
        (externals_over(&self.unit_records[..index], &asked), external_consts_over(&self.unit_records[..index]))
    }

    /// Run ownership analysis on the unit.
    ///
    /// Its errors are filed as diagnostics; see
    /// [`ScriptCompiler::unit_ownership_diagnostics`].
    fn phase_ownership(
        &mut self,
        index: usize,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
    ) -> Result<OwnershipOutput<'db>, ScriptCompilationResult> {
        let source = self.scripts[index].unit(self.db).source(self.db);
        let (externals, external_consts) = self.ownership_inputs(index, typecheck.output);
        match unit {
            ParsedUnit::Fragment { top, body } => {
                let ownership_result = analyze_script_fragment_tracked(
                    self.db,
                    source,
                    typecheck.result,
                    *top,
                    externals,
                    external_consts,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                let func_analyses = ownership_result.to_function_analyses_map(self.db, &body.statements);
                let script_analysis = ownership_result.script_analysis(self.db).clone();
                Ok(OwnershipOutput {
                    func_analyses,
                    script_analysis,
                    dead_exports: ownership_result.dead_exports(self.db).clone(),
                    revives: ownership_result.revived_exports(self.db).clone(),
                    takes: ownership_result.taken_externals(self.db).clone(),
                    writes: ownership_result.written_externals(self.db).clone(),
                    overwrites: ownership_result.overwritten_externals(self.db).clone(),
                })
            }
            ParsedUnit::Expr(parsed) => {
                let ownership_result = analyze_script_expr_tracked(
                    self.db,
                    source,
                    typecheck.result,
                    parsed.clone(),
                    externals,
                    external_consts,
                );
                if !ownership_result.errors(self.db).is_empty() {
                    let error_msg = ownership_result.errors(self.db).join("\n");
                    return Err(ScriptCompilationResult {
                        typecheck: TypecheckResult::Success,
                        ownership: OwnershipResult::Error { message: error_msg },
                        lowering: LoweringResult::Skipped,
                        ir_unit: None,
                    });
                }
                Ok(OwnershipOutput {
                    func_analyses: bct::lookup::LookupMap::new(),
                    script_analysis: None,
                    dead_exports: Vec::new(),
                    revives: Vec::new(),
                    takes: ownership_result.taken_externals(self.db).clone(),
                    writes: ownership_result.written_externals(self.db).clone(),
                    overwrites: ownership_result.overwritten_externals(self.db).clone(),
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
        script_consts: &HashMap<String, (IrType, Arc<ConstValue>)>,
        defer_missing_consts: bool,
    ) -> Result<LoweredFunctions, ScriptCompilationResult> {
        let ParsedUnit::Fragment { body, .. } = unit else {
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
        let mut func_param_types: HashMap<String, Vec<datalove_datafun_tycheck::Type<'db>>> = HashMap::new();
        let mut func_return_types: HashMap<String, datalove_datafun_tycheck::Type<'db>> = HashMap::new();
        for (name, func_type) in typecheck.result.function_types(self.db) {
            let param_types = func_type.param_types(self.db).clone();
            func_param_types.insert(name.text(self.db).S(), param_types);

            let return_type = func_type.return_type(self.db);
            func_return_types.insert(name.text(self.db).S(), return_type);
        }

        // Lower just the functions.
        match lower_script_functions(
            self.db,
            typecheck.expr_types,
            typecheck.call_targets,
            &body.statements,
            &ownership.func_analyses,
            Some(&func_param_types),
            Some(&func_return_types),
            self.shared_context.func_id_map,
            self.env.data(self.db),
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
            Err(e) => Err(self.lowering_failure(index, "function lowering error: ", e)),
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
        let ParsedUnit::Fragment { top, body } = unit else {
            // Expressions don't have const bindings.
            return Ok(ConstEvalOutput {
                resolved_consts: ResolvedConsts::new(),
                func_consts: HashMap::new(),
                script_consts: earlier_consts,
                declared_consts: Vec::new(),
            });
        };

        // Collect const graph (memoized).
        let const_graph = collect_const_graph(self.db, *top, typecheck.result);

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
                body,
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
                &body.statements,
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
        earlier_consts: &HashMap<String, (IrType, Arc<ConstValue>)>,
        const_graph: &datalove_datafun_ir::ConstBindingGraph,
        body: &'db Body<'db>,
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
        let mut resolved_consts_map: HashMap<String, (IrType, Arc<ConstValue>)> =
            earlier_consts.clone();

        // A binding's id is its position among the const statements, so the
        // expressions only have to be gathered once.
        let const_exprs: Vec<_> = body.statements.iter()
            .filter_map(|s| match s {
                Statement::Const(c) => Some(body.arena.get(c.value)),
                _ => None,
            })
            .collect();

        let mut evaluator = self.ctfe_evaluator.borrow_mut();
        let mut env = self.const_env(&mut **evaluator, expr_types, call_targets, lowered_funcs);
        for binding in &const_graph.bindings {
            let expr = const_exprs[binding.stmt_id.0 as usize];

            // A script unit returns `!()`, so a script-level const may use `!`;
            // if it does return early, evaluation reports it.
            let value = evaluate_const(
                &mut env, expr, &binding.ir_type, &resolved_consts_map,
                Some(IrType::Result(Box::new(IrType::Unit))), &Default::default(),
            ).map_err(|e| match e {
                ConstError::Lowering(e) => ConstEvalError::LoweringFailed {
                    binding_name: binding.name.clone(),
                    message: e.to_string(),
                },
                ConstError::Ctfe(e) => self.ctfe_error_to_const_eval_error(e, &binding.name),
                ConstError::MissingType | ConstError::Cycle(_) => unreachable!(
                    "a script const's type comes from the graph, and no script function waits on one"),
            })?.expect("nothing is deferred at a script's top level");

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
        script_level_consts: &HashMap<String, (IrType, Arc<ConstValue>)>,
        lowered_funcs: &LoweredFunctions,
    ) -> ScriptFunctionConstsResult {
        let mut evaluator = self.ctfe_evaluator.borrow_mut();
        let mut env = self.const_env(&mut **evaluator, expr_types, call_targets, lowered_funcs);
        let mut consts = HashMap::new();
        let mut errors = Vec::new();

        for statement in statements {
            let Statement::Fun(func_stmt) = statement else { continue };
            let func_name = func_stmt.name().text(self.db);
            // A function's const parameters have a value per instantiation
            // rather than one, so the consts naming them wait for the copies.
            let const_params = func_stmt.params().iter()
                .filter(|p| p.is_comptime)
                .map(|p| p.name.text(self.db).to_string())
                .collect();
            let (body_consts, body_errors) =
                evaluate_body_consts(&mut env, func_stmt, script_level_consts.clone(), const_params);
            for (name, ir_type, value) in body_consts {
                // Stored under a qualified name: func_name::const_name.
                consts.insert(format!("{}::{}", func_name, name), (ir_type, value));
            }
            errors.extend(body_errors.iter().map(|e| format!("{}::{}", func_name, e)));
        }

        ScriptFunctionConstsResult { consts, errors }
    }

    /// Convert a CtfeError to a ConstEvalError.
    fn ctfe_error_to_const_eval_error(&self, e: CtfeError, binding_name: &str) -> ConstEvalError {
        match e {
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
            ParsedUnit::Fragment { body, .. } => {
                // Build func_param_types and func_return_types for type alias support.
                let mut func_param_types: HashMap<String, Vec<datalove_datafun_tycheck::Type<'db>>> = HashMap::new();
                let mut func_return_types: HashMap<String, datalove_datafun_tycheck::Type<'db>> = HashMap::new();
                for (name, func_type) in typecheck.result.function_types(self.db) {
                    let param_types = func_type.param_types(self.db).clone();
                    func_param_types.insert(name.text(self.db).S(), param_types);

                    let return_type = func_type.return_type(self.db);
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
                    self.env.data(self.db),
                    script_ctx,
                    body,
                    ownership.func_analyses.clone(),
                    script_analysis,
                    Some(&func_param_types),
                    Some(&func_return_types),
                    lowered_funcs_arg,
                ).map_err(|e| self.lowering_failure(index, "", e))
            }
            ParsedUnit::Expr(parsed) => {
                lower_script_expr(
                    self.db,
                    typecheck.expr_types,
                    typecheck.call_targets,
                    self.shared_context.func_id_map,
                    self.env.data(self.db),
                    script_ctx,
                    parsed.expr(self.db),
                ).map_err(|e| self.lowering_failure(index, "", e))
            }
        }
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
        scope: HashMap<String, (IrType, Arc<ConstValue>)>,
        comptime_param_indices: &[usize],
        values: &[ConstValue],
        lowered: &[std::sync::Arc<IrCodeUnit>],
        func_name_to_id: &HashMap<String, datalove_datafun_ir::FuncId>,
    ) -> (HashMap<String, Arc<ConstValue>>, Vec<String>) {
        let Some(func_stmt) = statements.iter().find_map(|stmt| match stmt {
            Statement::Fun(f) if f.name().text(self.db) == func_name => Some(f),
            _ => None,
        }) else {
            return (HashMap::new(), Vec::new());
        };
        let mut evaluator = self.ctfe_evaluator.borrow_mut();
        let mut env = ConstEvalEnv {
            db: self.db,
            expr_types,
            call_targets,
            evaluator: &mut **evaluator,
            lowered,
            func_name_to_id,
            func_id_map: self.shared_context.func_id_map,
            callable: None,
            data_files: self.env.data(self.db),
        };
        evaluate_instantiation_consts(&mut env, func_stmt, scope, comptime_param_indices, values)
    }

    /// What this unit's consts are evaluated against.
    fn const_env<'a>(
        &'a self,
        evaluator: &'a mut dyn CtfeEvaluator,
        expr_types: &'db datalove_datafun_tycheck::ExprTypes<'db>,
        call_targets: &'db datalove_datafun_tycheck::CallTargets<'db>,
        lowered_funcs: &'a LoweredFunctions,
    ) -> ConstEvalEnv<'a, 'db> {
        ConstEvalEnv {
            db: self.db,
            expr_types,
            call_targets,
            evaluator,
            lowered: &lowered_funcs.functions,
            func_name_to_id: &lowered_funcs.func_name_to_id,
            func_id_map: self.shared_context.func_id_map,
            callable: None,
            data_files: self.env.data(self.db),
        }
    }

    fn phase_instantiate(&self, mut ir_unit: IrCodeUnit) -> IrCodeUnit {
        if self.generics == GenericsPolicy::Off {
            return ir_unit;
        }
        let registry = &self.shared_context.module_registry;
        let module_unit = |module, id| registry.get_module_function_as_unit(module, id).cloned();
        datalove_datafun_compiler::instances::instantiate_script_unit(&mut ir_unit, &module_unit, self.generics);
        ir_unit
    }

    fn phase_specialize(
        &self,
        ir_unit: IrCodeUnit,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        lowered_funcs: &LoweredFunctions,
        consts: &ConstEvalOutput,
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
            ParsedUnit::Fragment { body, .. } => &body.statements,
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
                    // A script function's consts may name the script's.
                    self.instantiation_consts_from(
                        script_stmts, &name, typecheck.expr_types, typecheck.call_targets,
                        consts.script_consts.clone(), indices, values,
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
                    let parsed_graph = self.shared_context.parsed_graph;
                    let Some(module_id) = datalove_datafun_compiler::tracked_lower::ir_module_ids(self.db, parsed_graph)
                        .iter_unordered()
                        .find(|(_, ir_id)| **ir_id == module)
                        .map(|(module_id, _)| *module_id)
                    else {
                        return (HashMap::new(), Vec::new());
                    };
                    let Some(source_module) = parsed_graph.get_module(self.db, module_id) else {
                        return (HashMap::new(), Vec::new());
                    };
                    let parsed = datalove_datafun_parser::parse_module_ast(self.db, source_module);
                    let module_results = self.shared_context.graph_typecheck.module_results(self.db);
                    let Some(single) = module_results.get(&module_id) else {
                        return (HashMap::new(), Vec::new());
                    };
                    let funcs: Vec<std::sync::Arc<IrCodeUnit>> = registry.iter_module_code_units_with_ids()
                        .filter(|((m, _), _)| *m == module)
                        .map(|(_, u)| std::sync::Arc::new(u.clone()))
                        .collect();
                    let names: HashMap<String, datalove_datafun_ir::FuncId> = funcs.iter()
                        .map(|u| (u.name.clone(), datalove_datafun_ir::FuncId(u.id.0)))
                        .collect();
                    // A module function's consts may name its module's.
                    self.instantiation_consts_from(
                        parsed.statements(self.db), &name,
                        single.expr_types(self.db), single.call_targets(self.db),
                        self.shared_context.module_consts.get(&module).cloned().unwrap_or_default(),
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

    /// Finish the unit's functions; see `optimize::finish`.
    ///
    /// An `inline` callee is found among the unit's functions, an earlier
    /// unit's, or a module's, as each was before being finished.
    ///
    /// The unit's own code is left calling: it names earlier units' bindings,
    /// which a function body cannot, and a splice cannot record the moves of
    /// those it passes.
    fn phase_finish(&self, mut ir_unit: IrCodeUnit) -> IrCodeUnit {
        use datalove_datafun_compiler::optimize;
        use datalove_datafun_ir::inline::{is_inline, with_calls_into_unit, Inliner};
        use datalove_datafun_ir::CodeRef;

        let registry = &self.shared_context.module_registry;
        let local: HashMap<datalove_datafun_ir::CodeUnitId, Arc<IrCodeUnit>> = ir_unit.nested_units.iter()
            .filter(|unit| is_inline(unit))
            .map(|unit| (unit.id, Arc::new(unit.clone())))
            .collect();
        let resolve = |func: &CodeRef| -> Option<Arc<IrCodeUnit>> {
            match func {
                CodeRef::Local(id) => local.get(id).cloned(),
                CodeRef::External { unit, id } => self.unit_records.get(*unit as usize)?
                    .functions.iter()
                    .find(|f| f.id == *id && is_inline(f))
                    .map(|f| Arc::new(with_calls_into_unit(f, *unit))),
                CodeRef::Module { module, id } => registry.get_module_function(*module, *id)
                    .filter(|f| is_inline(f))
                    .cloned(),
            }
        };

        let mut inliner = Inliner::new(&resolve);
        for func in &mut ir_unit.nested_units {
            optimize::finish(func, &mut inliner, !self.skip_const_inlining);
        }
        ir_unit
    }

    // ========================================================================
    // Phase 4: Const Inlining
    // ========================================================================

    /// Inline evaluated const values into the lowered IR.
    ///
    /// This replaces const initializer expressions with their pre-computed
    /// literal values. In skip_const_inlining mode, this is skipped so const
    /// expressions are evaluated at runtime instead of compile time.
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
        let mut const_values: HashMap<String, Arc<ConstValue>> = HashMap::new();

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
            revives: ownership.revives.clone(),
            takes: ownership.takes.clone(),
            writes: ownership.writes.clone(),
            overwrites: ownership.overwrites.clone(),
            provides: self.unit_provides_names(typecheck.output),
            uses: self.unit_used_names(index, typecheck.output),
            imports: typecheck.output.imported_modules(self.db).C(),
            functions: ir_unit.nested_units.iter().cloned().map(Arc::new).collect(),
            types: self.unit_types(typecheck.output),
            value_type: match self.parsed_unit_at(index) {
                ParsedUnit::Expr(parsed) => {
                    let ty = typecheck.expr_types.get(parsed.expr(self.db))
                        .expect("an expression unit that typechecked has a type");
                    Some(datalove_datafun_common::type_to_string(self.db, ty))
                }
                ParsedUnit::Fragment { .. } => None,
            },
        };
    }

    /// The type of each binding and function a unit provides, as datalove
    /// writes it.
    fn unit_types(&self, output: datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>) -> Vec<(String, String)> {
        let vars = output.new_vars(self.db).iter()
            .map(|(name, ty, _)| (name.as_str(self.db).S(), datalove_datafun_common::type_to_string(self.db, ty)));
        let functions = output.new_fns(self.db).iter()
            .map(|(name, ty)| {
                let ty = datalove_datafun_common::Type::Function(*ty);
                (name.as_str(self.db).S(), datalove_datafun_common::type_to_string(self.db, &ty))
            });
        vars.chain(functions).collect()
    }
}
