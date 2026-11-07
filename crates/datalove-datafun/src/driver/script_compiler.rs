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
use salsa::Setter as _;
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
use datalove_datafun_ir::{ConstValue, CtfeEvaluator, CtfeError, ExportBinding, IrCodeUnit, IrType, ResolvedConsts, ConstEvalError};
use datalove_datafun_interp::InterpCtfeEvaluator;
use datalove_datafun_compiler::instances::GenericsPolicy;
use datalove_datafun_tycheck::{
    typecheck_script_unit, unit_ast, unit_parses,
    ScriptEnv, ScriptUnit, ScriptUnitKind,
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

/// One unit of a session: its input, and the id it is known by.
///
/// The id is the unit's for as long as it is in the session, and is what its
/// IR is numbered by and every later unit's IR names it by; see
/// `ScriptContext::unit`. The source is the input's own, held here too so
/// that a session read between compilers needs no database.
#[derive(Clone, Copy, Debug)]
struct SessionUnit {
    id: u32,
    unit: ScriptUnit,
    source: bct::input::Source,
}

/// A unit taken out of a session, as the re-derivation after it needs it.
///
/// What it provided and did to earlier units' bindings is what says which
/// units its going reaches; see [`ScriptCompiler::relower_deleted`].
pub struct RemovedUnit {
    unit: ScriptUnit,
    record: Arc<UnitLowerRecord>,
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
    /// How each unit provided each of its bindings; see [`binding_shape`].
    shapes: Vec<HashMap<String, BindingShape>>,
}

/// How a unit provides a binding: as a slot or a value, and of which type.
///
/// What a later unit that writes the binding without reading it is compiled
/// against. While it stays the same, rerunning that unit writes what it wrote
/// before.
#[derive(Clone, PartialEq, Eq, Debug)]
struct BindingShape {
    slot: bool,
    ty: String,
}

/// How `record` says its unit provides `name`, if it does.
fn binding_shape(record: &UnitLowerRecord, name: &str) -> Option<BindingShape> {
    let (_, export) = record.exports.iter().find(|(bound, _)| bound == name)?;
    let (_, ty) = record.types.iter().find(|(bound, _)| bound == name)?;
    Some(BindingShape { slot: matches!(export, ExportBinding::Slot(_)), ty: ty.C() })
}

/// Every binding `record` says its unit provides, by name, with its shape.
fn binding_shapes(record: &UnitLowerRecord) -> HashMap<String, BindingShape> {
    record.exports.iter()
        .filter_map(|(name, _)| Some((name.C(), binding_shape(record, name)?)))
        .collect()
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

impl LoweredFunctions {
    /// Add a batch of the functions that were waiting, lowered.
    fn add(&mut self, batch: LoweredFunctions, lowered: &[String]) {
        self.functions.extend(batch.functions);
        // In the order the ids were assigned, as one lowering gives them.
        self.functions.sort_by_key(|function| function.id.0);
        self.deferred.retain(|name| !lowered.contains(name));
        self.deferred.extend(batch.deferred);
    }
}

/// What lowering a unit's waiting functions needs, while its consts are
/// evaluated.
#[derive(Clone, Copy)]
struct Waiting<'a, 'db> {
    index: usize,
    unit: &'a ParsedUnit<'db>,
    typecheck: &'a TypecheckOutput<'db>,
    ownership: &'a OwnershipOutput<'db>,
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
            units: Vec::new(),
            next_id: FIRST_UNIT_ID,
            env: ScriptEnv::new(
                db, modules, self.shared.module_graph.data(db).C()),
            last_unit: None,
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
    /// The session holds the units' `ScriptUnit` inputs, the very handles
    /// every per-unit memo is keyed on, so each of those is still good. See
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

/// The id a session's first unit is given.
///
/// From 1, as a REPL numbers units for a person to type, so that the numbers
/// it shows are the ids and need no table beside them.
pub const FIRST_UNIT_ID: u32 = 1;

/// The part of a script compiler that survives a database mutation.
///
/// A [`ScriptCompiler`] borrows the database for as long as it lives, and
/// editing a unit is `set_text` on a `Source`, which needs `&mut db`, as is
/// splicing a unit in or out, which is `set_prev` on the unit after it. So
/// those mean letting the compiler go and building another afterwards, and
/// this is what crosses in between: the units' inputs, whose identity is
/// independent of their text and of where they sit, their ids, and what each
/// unit's compilation produced. Everything tied to the database -- the module
/// specs, the CTFE evaluator -- is derived again on the far side.
///
/// See `botdocs/plan-script-reactivity.md` for why the compiler cannot simply
/// be handed `&mut db` instead.
///
/// `Clone` so that a step can compile on a copy and keep it only once the
/// compile is done: a compile that does not finish -- one salsa cancels, say
/// -- leaves the session as it was, to compile again.
#[derive(Clone)]
pub struct ScriptSession {
    units: Vec<SessionUnit>,
    /// The id the next unit made is given; ids are not given twice, unless a
    /// unit is put back under the one it had.
    next_id: u32,
    /// What each unit's compilation produced, in the units' order.
    records: Vec<Arc<UnitLowerRecord>>,
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
        self.units.iter().map(|unit| unit.source).collect()
    }

    /// The id of each unit, in order.
    pub fn unit_ids(&self) -> Vec<u32> {
        self.units.iter().map(|unit| unit.id).collect()
    }

    /// The id of the last unit, the one appended last.
    pub fn last_unit_id(&self) -> u32 {
        self.units.last().expect("a unit has been appended").id
    }

    /// Where the unit with id `id` sits, if it is in the session.
    pub fn position_of(&self, id: u32) -> Option<usize> {
        self.units.iter().position(|unit| unit.id == id)
    }

    /// Where the unit with id `id` sits, which must be in the session.
    fn position(&self, id: u32) -> usize {
        self.position_of(id).unwrap_or_else(|| panic!("no unit {id} in the session"))
    }

    /// The id the next unit made is given.
    pub fn next_id(&self) -> u32 {
        self.next_id
    }

    /// Give the next unit made the id `next`.
    ///
    /// For a caller that numbers the units as an earlier session did, as the
    /// REPL does when it drops the changes it took back: the units those made
    /// are gone, and their ids are given again.
    pub fn set_next_id(&mut self, next: u32) {
        assert!(
            self.units.iter().all(|unit| unit.id < next),
            "the next id is {next}, and a unit in the session already has it or a later one",
        );
        self.next_id = next;
    }

    /// Splice a unit in at position `at`, as a fragment or a bare expression,
    /// with the id `id` or else the next.
    ///
    /// **Only the link of the unit after it moves**: it is set to the new
    /// unit, which is all a lookup walking back from any later unit has to
    /// find. No unit's handle moves, so no unit's memo is lost, and a unit's
    /// typecheck runs again only if the new unit binds a name it asks for.
    /// The new unit is compiled by [`ScriptCompiler::relower_inserted`].
    ///
    /// Returns the new unit's id.
    pub fn insert_unit<D: salsa::Database>(
        &mut self,
        db: &mut D,
        at: usize,
        source: bct::input::Source,
        is_expr: bool,
        id: Option<u32>,
    ) -> u32 {
        assert!(at <= self.units.len(), "unit inserted at {at} in a session of {}", self.units.len());
        let id = match id {
            Some(id) => {
                assert!(self.position_of(id).is_none(), "unit {id} is put back while it is in the session");
                self.next_id = self.next_id.max(id + 1);
                id
            }
            None => {
                self.next_id += 1;
                self.next_id - 1
            }
        };
        let prev = at.checked_sub(1).map(|before| self.units[before].unit);
        let unit = ScriptUnit::new(&*db, source, is_expr, prev);
        if let Some(after) = self.units.get(at) {
            after.unit.set_prev(db).to(Some(unit));
        }
        self.units.insert(at, SessionUnit { id, unit, source });
        self.records.insert(at, Arc::default());
        id
    }

    /// Splice the unit at position `at` out, linking the unit after it to
    /// the one before.
    ///
    /// What it provided and did is handed back for
    /// [`ScriptCompiler::relower_deleted`] to work out what its going
    /// reaches.
    pub fn delete_unit<D: salsa::Database>(&mut self, db: &mut D, at: usize) -> RemovedUnit {
        assert!(at < self.units.len(), "unit deleted at {at} in a session of {}", self.units.len());
        let removed = self.units.remove(at);
        let record = self.records.remove(at);
        if let Some(after) = self.units.get(at) {
            let prev = removed.unit.prev(&*db);
            after.unit.set_prev(db).to(prev);
        }
        RemovedUnit { unit: removed.unit, record }
    }

    /// Move the unit at position `from` to position `to`, counted in the
    /// session as it is after the move, with its id and its record.
    ///
    /// **A splice out and a splice in, relinking three units**: the one after
    /// it where it was, the one after it where it goes, and itself. No unit's
    /// handle moves. Its record goes with it, being what the running session
    /// went through, and [`ScriptCompiler::relower_moved`] works out what
    /// the move reaches.
    pub fn move_unit<D: salsa::Database>(&mut self, db: &mut D, from: usize, to: usize) {
        let count = self.units.len();
        assert!(from < count && to < count, "unit moved from {from} to {to} in a session of {count}");
        assert_ne!(from, to, "a unit is moved to where it is");
        let moved = self.units.remove(from);
        let record = self.records.remove(from);
        if let Some(after) = self.units.get(from) {
            let prev = moved.unit.prev(&*db);
            after.unit.set_prev(db).to(prev);
        }
        let prev = to.checked_sub(1).map(|before| self.units[before].unit);
        moved.unit.set_prev(db).to(prev);
        if let Some(after) = self.units.get(to) {
            after.unit.set_prev(db).to(Some(moved.unit));
        }
        self.units.insert(to, moved);
        self.records.insert(to, record);
    }

    /// The type of `name`, which unit `id` provides, as datalove writes it.
    pub fn type_of(&self, id: u32, name: &str) -> &str {
        self.records[self.position(id)].type_of(name)
    }

    /// The type of expression unit `id`'s value, as datalove writes it.
    pub fn value_type(&self, id: u32) -> &str {
        self.records[self.position(id)].value_type.as_deref().expect("the unit is an expression that compiled")
    }

    /// What unit `id` provides, and what it does to earlier units'
    /// bindings, as of its last compile. A unit that did not compile does
    /// nothing.
    pub fn unit_accesses(&self, id: u32) -> UnitAccesses {
        let record = &self.records[self.position(id)];
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
}

/// Script compiler for compiling script units.
///
/// Handles compilation only. No interpreter dependency.
/// Use `compile_fragment()` or `compile_expr()` to compile units.
pub struct ScriptCompiler<'db> {
    db: &'db dyn salsa::Database,
    /// The units, in order: the handle every per-unit query about a unit is
    /// keyed on, and its id.
    ///
    /// A handle is an input, so neither editing a unit's text nor splicing a
    /// unit in or out moves any of them. See
    /// `botdocs/script-reactivity-architecture.md`.
    units: Vec<SessionUnit>,
    /// The id the next unit made is given.
    next_id: u32,
    /// The modules the script is checked against.
    env: ScriptEnv<'db>,
    /// The unit compiled last.
    ///
    /// Held apart from `units` because re-deriving compiles units in the
    /// middle of it, and the diagnostics the caller is about to render are the
    /// last compiled unit's.
    last_unit: Option<ScriptUnit>,
    /// What each unit's compilation produced, in the units' order.
    ///
    /// **Not a running total.** Unit `i` is lowered against the fold of the
    /// records before it, computed on demand, so re-lowering one unit replaces
    /// one record and leaves the rest where they are. A fold could not do that:
    /// after an edit it would describe the pre-edit suffix as well.
    unit_records: Vec<Arc<UnitLowerRecord>>,
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
    /// executor holds its id with no frame.
    fn append_unit(
        &mut self,
        source: bct::input::Source,
        is_expr: bool,
    ) -> ScriptCompilationResult {
        let prev = self.units.last().map(|unit| unit.unit);
        let unit = ScriptUnit::new(self.db, source, is_expr, prev);
        let id = self.next_id;
        self.next_id += 1;
        self.units.push(SessionUnit { id, unit, source });
        self.unit_records.push(Arc::default());
        self.rederive_one(self.units.len() - 1)
    }

    /// The id of the unit at position `index`.
    pub fn unit_id(&self, index: usize) -> u32 {
        self.units[index].id
    }

    /// The id of each unit, in order.
    pub fn unit_ids(&self) -> Vec<u32> {
        self.units.iter().map(|unit| unit.id).collect()
    }

    /// The id of the last unit, the one appended last.
    pub fn last_unit_id(&self) -> u32 {
        self.units.last().expect("a unit has been appended").id
    }

    /// Re-lower the units an edit to unit `edited` reaches, in index order.
    ///
    /// The units the edit does not reach keep the IR they already have, which
    /// is what makes this worth doing and is sound because **a script value is
    /// identified by `(unit id, ValueId)`**: a unit's IR names the units it
    /// reads from, so a unit that uses nothing the edited unit provides holds
    /// no reference to it and cannot see it change.
    ///
    /// Each returned index is paired with what compiling that unit came to.
    /// The caller re-executes them in the order given -- which is index order,
    /// because a unit reads only from units before it.
    ///
    /// A unit that fails to re-lower is left providing nothing, so its
    /// bindings drop out of the environment, and stays where it is. Its
    /// executor drops its frame.
    pub fn relower_reach(&mut self, edited: usize) -> Vec<(usize, ScriptCompilationResult)> {
        assert!(
            edited < self.units.len(),
            "unit {edited} was edited but the script has {} units",
            self.units.len(),
        );
        self.rederive_reach(BTreeSet::from([edited]), BTreeSet::new())
    }

    /// Re-lower the units an edit to the modules at `edited` reaches.
    ///
    /// The module-shaped sibling of [`Self::relower_reach`], and the reason
    /// there has to be one: stage B made typechecking lazy, so an earlier unit
    /// re-runs when a later one asks `binding_before` about a name it provides, and
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
    /// **A unit that requires a module that broke or was fixed is reached
    /// too**, being `broken_or_fixed`, whether or not it imports from it: a
    /// `require` of a module that does not compile fails, and one of a module
    /// that compiles again runs. A body edit leaves a unit that only requires
    /// the module as it was.
    ///
    /// `edited` and `broken_or_fixed` are module paths in
    /// `library/package/module` form, which is what an import resolves an
    /// alias to and what a `require` names.
    ///
    /// The caller must put the new module IR in front of the executor --
    /// `ScriptExecutor::set_module_registry` -- before re-executing what comes
    /// back: a script unit's IR names a module function by
    /// `CodeRef::Module`, so re-lowering it does not by itself change the code
    /// that call lands on.
    pub fn relower_module_reach(
        &mut self,
        edited: &[String],
        broken_or_fixed: &[String],
    ) -> Vec<(usize, ScriptCompilationResult)> {
        let mut seeds = self.module_importers(edited);
        seeds.extend(self.module_requirers(broken_or_fixed));
        self.rederive_reach(seeds, BTreeSet::new())
    }

    /// Re-lower the units an edit to the data file at `data` reaches.
    ///
    /// The data-shaped sibling of [`Self::relower_module_reach`], needed for
    /// the same reason: nothing asks about a unit whose data changed. A unit
    /// is reached when one of its `require data` consts names `data`, or when
    /// one of its imports resolves into one of `modules`, the modules whose
    /// own consts reach the file, and then the units that use what it provides
    /// are reached as an edit's are.
    ///
    /// **A data file is compiled into the unit that requires it**, as the
    /// value of a const, so the unit is re-derived rather than only rerun,
    /// though its code is the same.
    pub fn relower_data_reach(
        &mut self,
        data: &str,
        modules: &[String],
    ) -> Vec<(usize, ScriptCompilationResult)> {
        let db = self.db;
        let mut seeds = self.module_importers(modules);
        seeds.extend(self.unit_typecheck_outputs().iter().enumerate()
            .filter(|(_, output)| output.is_some_and(|output| output.result(db).data_files(db).contains_key(data)))
            .map(|(unit, _)| unit));
        self.rederive_reach(seeds, BTreeSet::new())
    }

    /// Re-derive what the unit [`ScriptSession::insert_unit`] put at `at`
    /// reaches, in index order, the new unit first among them.
    ///
    /// **An insertion is an edit from nothing**, and its reach is an edit's:
    /// the units after it that use what it provides, which is how a unit
    /// now finds a binding the new unit shadows, and back to the provider of
    /// a binding the new unit reads that a later unit took or wrote since.
    /// Every other unit keeps its IR and its frame, since a unit's id is all a
    /// later unit's IR names it by, and no id moved.
    pub fn relower_inserted(&mut self, at: usize) -> Vec<(usize, ScriptCompilationResult)> {
        assert!(at < self.units.len(), "unit {at} was inserted but the script has {} units", self.units.len());
        self.rederive_reach(BTreeSet::from([at]), BTreeSet::new())
    }

    /// Re-derive what taking `removed` out of the script from position `at`
    /// reaches, in index order.
    ///
    /// **A deletion is an edit to nothing.** The units after it that found a
    /// binding it provided, took or wrote now find an older one or none, so
    /// they are re-derived, and the units that use what they provide after
    /// them. And a binding of an earlier unit it took or wrote is left as it
    /// left it, so the unit that sets that binding is rerun to build it as it
    /// was, as an edit's reach goes back; see [`Self::reach_from`]. A unit that
    /// read what `removed` provided stops compiling, and stays.
    pub fn relower_deleted(&mut self, at: usize, removed: RemovedUnit) -> Vec<(usize, ScriptCompilationResult)> {
        assert!(at <= self.units.len(), "unit {at} was removed but the script has {} units", self.units.len());
        let RemovedUnit { unit, record } = removed;
        // What it provided as its typecheck says as well as its record, since a
        // unit that typechecked and failed later provides its names to the
        // typechecker and not to lowering: a later unit fails on one of them,
        // and may compile once it is gone.
        let mut provided: BTreeSet<String> = record.provides.iter().cloned().collect();
        if unit_parses(self.db, unit) {
            provided.extend(self.unit_provides_names(typecheck_script_unit(self.db, unit, self.env)));
        }
        let others: Vec<usize> = (0..self.units.len()).collect();
        let (seeds, restore) = self.departure(&record, &provided, &others[..at], &others[at..]);
        self.rederive_reach(seeds, restore)
    }

    /// Re-derive what moving a unit from position `from` to `to` reaches,
    /// in index order; `to` is where it sits now, and `from` where it sat.
    ///
    /// **A move is a deletion and an insertion, taken as one step**, so the
    /// session as it would stand between them is never compiled or run. The
    /// other units keep their order, so what leaving `from` reaches is
    /// worked out as [`Self::relower_deleted`] works it out, over the units
    /// as they are now, and what arriving at `to` reaches is an insertion's:
    /// the moved unit, and what uses what it provides.
    pub fn relower_moved(&mut self, from: usize, to: usize) -> Vec<(usize, ScriptCompilationResult)> {
        let count = self.units.len();
        assert!(from < count && to < count, "unit moved from {from} to {to} in a script of {count} units");
        let record = self.unit_records[to].clone();
        let (now_provides, _) = self.graph_now();
        let provided: BTreeSet<String> = record.provides.iter().chain(&now_provides[to]).cloned().collect();
        let others: Vec<usize> = (0..count).filter(|unit| *unit != to).collect();
        let (mut seeds, restore) = self.departure(&record, &provided, &others[..from], &others[from..]);
        seeds.insert(to);
        self.rederive_reach(seeds, restore)
    }

    /// What a unit leaving its place reaches: the seeds and the units to
    /// restore from, as [`Self::rederive_reach`] takes them.
    ///
    /// `record` is what it did there and `provided` the names it provided.
    /// `earlier` and `later` are the units that were before it and after it,
    /// in order, by where they sit now.
    fn departure(
        &self,
        record: &UnitLowerRecord,
        provided: &BTreeSet<String>,
        earlier: &[usize],
        later: &[usize],
    ) -> (BTreeSet<usize>, BTreeSet<usize>) {
        let (now_provides, now_uses) = self.graph_now();
        let changed: BTreeSet<String> = record.takes.iter().chain(&record.writes).cloned().collect();

        // A later unit found a binding of the departed unit's when no unit
        // between them provided, took or wrote the name.
        let sets = |unit: usize, name: &String| {
            now_provides[unit].contains(name)
                || [&self.unit_records[unit].provides, &self.unit_records[unit].takes, &self.unit_records[unit].writes]
                    .iter().any(|names| names.contains(name))
        };
        let seeds: BTreeSet<usize> = later.iter().enumerate()
            .filter(|(nth, unit)| {
                now_uses[**unit].iter().chain(&self.unit_records[**unit].uses).any(|name| {
                    (provided.contains(name) || changed.contains(name))
                        && !later[..*nth].iter().any(|between| sets(*between, name))
                })
            })
            .map(|(_, unit)| *unit)
            .collect();

        // The unit that builds a binding the departed unit took or wrote as
        // it was before: its provider, or an overwrite or revival after that.
        let restore: BTreeSet<usize> = changed.iter()
            .filter_map(|name| {
                earlier.iter().rev().copied().find(|unit| {
                    let record = &self.unit_records[*unit];
                    now_provides[*unit].contains(name)
                        || [&record.provides, &record.overwrites, &record.revives].iter().any(|names| names.contains(name))
                })
            })
            .collect();
        (seeds, restore)
    }

    /// Drop the units from `n` on, and re-derive what their going reaches.
    ///
    /// Nothing sits after what goes, so usually nothing is. But a unit that
    /// goes may have taken or written an earlier unit's binding, and then the
    /// unit that sets that binding is rerun to build it as it was, as
    /// [`Self::relower_deleted`] does for one unit. Returns those, in index
    /// order; the caller drops the runtime state of the units that went.
    pub fn truncate_units(&mut self, n: usize) -> Vec<(usize, ScriptCompilationResult)> {
        assert!(
            n <= self.units.len(),
            "cannot truncate to {n} units; the script has {}",
            self.units.len(),
        );
        // The graph of the units that stay only: the ones going are not
        // typechecked again, which they would be if the modules moved.
        let gone = self.unit_records.split_off(n);
        self.units.truncate(n);
        let (now_provides, _) = self.graph_now();
        let sets = |record: &UnitLowerRecord, name: &String| {
            [&record.provides, &record.overwrites, &record.revives].iter().any(|names| names.contains(name))
        };
        let mut restore = BTreeSet::new();
        for (at, record) in gone.iter().enumerate() {
            for name in record.takes.iter().chain(&record.writes) {
                // A binding a unit that goes set is gone with it.
                if gone[..at].iter().any(|earlier| sets(earlier, name)) {
                    continue;
                }
                let setter = (0..n).rev().find(|earlier| {
                    now_provides[*earlier].contains(name) || sets(&self.unit_records[*earlier], name)
                });
                restore.extend(setter);
            }
        }
        self.rederive_reach(BTreeSet::new(), restore)
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
        let unit = self.units[index];
        self.last_unit = Some(unit.unit);
        self.last_source = Some(unit.source);
        self.compile_unit_at(index)
    }

    /// Compile again the units `seeds` reach, in index order, and say what
    /// each came to; `restore` are units to rerun from the start, to build a
    /// binding again as it was made.
    ///
    /// What a unit took or wrote of an earlier unit's is only known once the
    /// unit is compiled, and a unit that now takes or writes something reaches
    /// the units after it that use it. So the reach is taken, its units are
    /// compiled, and the reach is taken again with what they now do, until it
    /// settles. What the units did before the edit is held from the
    /// start, being what the running session went through.
    fn rederive_reach(
        &mut self,
        seeds: BTreeSet<usize>,
        restore: BTreeSet<usize>,
    ) -> Vec<(usize, ScriptCompilationResult)> {
        let held = self.held_graph();
        let mut changed = held.changed.clone();
        for _ in 0..=self.unit_records.len() {
            let units = self.reach_from(&seeds, &restore, &held, &changed);
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
            if self.reach_from(&seeds, &restore, &held, &changed) == units {
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

    /// The units that require one of `modules`, as their code says now.
    fn module_requirers(&self, modules: &[String]) -> BTreeSet<usize> {
        let db = self.db;
        self.unit_typecheck_outputs().iter().enumerate()
            .filter(|(_, output)| output.is_some_and(|output| {
                output.new_module_aliases(db).iter().any(|(_, path)| modules.contains(path))
            }))
            .map(|(unit, _)| unit)
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
    ///
    /// `restore` are providers reached that way from the start, by a splice
    /// that took out a unit that took or wrote what they build.
    fn reach_from(
        &self,
        seeds: &BTreeSet<usize>,
        restore: &BTreeSet<usize>,
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

        // Whether unit `provider` provides `name` other than it did before
        // the edit.
        let reshaped = |provider: usize, name: &String| {
            held.shapes[provider].get(name) != binding_shape(&self.unit_records[provider], name).as_ref()
        };

        let mut rerun_starts = restore.clone();
        loop {
            let mut rederived = vec![false; count];
            let mut restoring = vec![false; count];
            for unit in 0..count {
                // A unit that writes a binding without reading it does not
                // care how an earlier unit left it, only what the binding is,
                // which its provider says. While the provider provides it as
                // it did, the unit writes what it wrote before, so the units
                // after it see no new value. It is rerun, since its provider
                // undid its write.
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
                                && (!writes_without_reading(unit, name)
                                    || (provides[source].contains(name) && reshaped(source, name)))
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
            shapes: self.unit_records.iter().map(|record| binding_shapes(record)).collect(),
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

    /// The names a unit asks its environment for, each once.
    fn unit_used_names(
        &self,
        index: usize,
        output: datalove_datafun_tycheck::ScriptUnitTypecheckOutput<'db>,
    ) -> Vec<String> {
        let db = self.db;
        let mut used: Vec<String> =
            output.uses(db).iter().map(|name| name.as_str(db).S()).collect();
        let qualified_calls = match &unit_ast(db, self.units[index].unit).kind {
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
        // A unit calling two functions of a module asks for its alias twice.
        let mut seen = std::collections::HashSet::new();
        used.retain(|name| seen.insert(name.C()));
        used
    }

    /// Take up the units and the per-unit records a session left off with.
    fn resume(&mut self, session: ScriptSession) {
        self.units = session.units;
        self.next_id = session.next_id;
        self.last_unit = self.units.last().map(|unit| unit.unit);
        self.unit_records = session.records;
        self.skip_const_inlining = session.skip_const_inlining;
        self.skip_specialization = session.skip_specialization;
        self.generics = session.generics;
    }

    /// Hand back what an edit needs to survive letting this compiler go.
    pub fn into_session(self) -> ScriptSession {
        ScriptSession {
            units: self.units,
            next_id: self.next_id,
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
        self.units.iter()
            .map(|unit| {
                unit_parses(self.db, unit.unit)
                    .then(|| typecheck_script_unit(self.db, unit.unit, self.env))
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
        let unit = self.units[index].unit;
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
            let types = typecheck_script_unit::accumulated::<datalove_diagnostic::TypeDiagnostic>(self.db, unit, self.env);
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
        match self.last_unit {
            Some(unit) if unit_parses(self.db, unit) => {
                typecheck_script_unit::accumulated::<datalove_diagnostic::TypeDiagnostic>(
                    self.db, unit, self.env)
            }
            _ => Vec::new(),
        }
    }

    /// Ownership diagnostics from the last compilation: those of the last
    /// unit compiled, if it got as far as ownership analysis.
    pub fn get_ownership_diagnostics(&self) -> Vec<&datalove_diagnostic::OwnershipDiagnostic> {
        let Some(last) = self.last_unit else {
            return Vec::new();
        };
        let index = self.units.iter().position(|unit| unit.unit == last)
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
        let unit = self.units[index].unit;
        if !unit_parses(self.db, unit) {
            return Vec::new();
        }
        let output = typecheck_script_unit(self.db, unit, self.env);
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
            .map(|unit| (name, self.units[unit].id));
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
        let unit = self.units[index].unit;
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
        let spec = unit_ast(self.db, self.units[index].unit);
        match &spec.kind {
            ScriptUnitKind::Fragment(parsed) => {
                ParsedUnit::Fragment { top: parsed.top, body: parsed.body(self.db) }
            }
            ScriptUnitKind::Expr(parsed) => ParsedUnit::Expr(parsed.clone()),
        }
    }

    /// Compile unit `index` through the phases, against the units before it.
    ///
    /// Phases:
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
        if !unit_parses(self.db, self.units[index].unit) {
            self.unit_records[index] = Arc::default();
            return self.parse_failure_at(index);
        }
        let unit = self.parsed_unit_at(index);
        // Marked for a recorder: ownership analysis is keyed on what the
        // earlier units left behind, so its key alone does not say which
        // position in the script it is.
        let mut result = datalove_ct::query_events::compiling_unit(index, || self.run_phases(index, &unit));
        match &mut result.ir_unit {
            None => self.unit_records[index] = Arc::default(),
            Some(ir_unit) => {
                ir_unit.script_context_mut()
                    .expect("a script unit compiles to a script code unit")
                    .unit = Some(self.units[index].id);
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
        // A function body may name a script-level const, and evaluating a
        // script-level const may call a function in the same unit. So the
        // bodies that need no value from this unit's consts go first, and the
        // rest wait for const evaluation to lower them, each once the last
        // const it names has its value. This unit's data consts are in scope
        // here already; see `data_consts`.
        let mut first_consts = script_consts_over(&self.unit_records[..index]);
        if let ParsedUnit::Fragment { body, .. } = unit {
            first_consts.extend(datalove_datafun_compiler::const_eval::data_consts(
                self.db, body, typecheck.expr_types, self.env.data(self.db)));
        }
        let mut lowered_funcs = match self.phase_lower_functions(
            index, unit, &typecheck, &ownership, &first_consts, true, None,
        ) {
            Ok(lf) => lf,
            Err(result) => return result,
        };

        // Phase 3: Const Evaluation
        // Evaluates const expressions using the lowered functions + interpretation,
        // lowering the functions that were waiting as it goes.
        let consts = match self.phase_const_eval(index, unit, &typecheck, &ownership, &mut lowered_funcs) {
            Ok(c) => c,
            Err(result) => return result,
        };

        // The bodies still waiting are those no const reaches, which waited for
        // every const, and any naming a binding of an earlier unit that did
        // not compile, which lowering without deferring reports.
        if !lowered_funcs.deferred.is_empty() {
            let waiting = std::mem::take(&mut lowered_funcs.deferred);
            match self.phase_lower_functions(
                index, unit, &typecheck, &ownership, &consts.script_consts, false, Some(&waiting),
            ) {
                Ok(rest) => lowered_funcs.add(rest, &waiting),
                Err(result) => return result,
            }
        }

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
        // Keyed on the unit and nothing about the units around it, which is
        // what lets a change stop at the units that used what changed. See
        // `botdocs/script-reactivity-architecture.md`.
        //
        // The mode reaches here through `env`, not just ownership analysis: a
        // mismatch `@` would fix is a type error first.
        let output = typecheck_script_unit(self.db, self.units[index].unit, self.env);
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
        let ids = self.unit_ids();
        (
            externals_over(&ids[..index], &self.unit_records[..index], &asked),
            external_consts_over(&self.unit_records[..index]),
        )
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
        let source = self.units[index].source;
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
    /// yet, for const evaluation to lower once it has one. `only` lowers just
    /// the functions it names.
    #[allow(clippy::too_many_arguments)]
    fn phase_lower_functions(
        &self,
        index: usize,
        unit: &ParsedUnit<'db>,
        typecheck: &TypecheckOutput<'db>,
        ownership: &OwnershipOutput<'db>,
        script_consts: &HashMap<String, (IrType, Arc<ConstValue>)>,
        defer_missing_consts: bool,
        only: Option<&[String]>,
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
        let script_ctx = lower_context_over(&self.unit_ids()[..index], &self.unit_records[..index]);

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
            only,
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
        ownership: &OwnershipOutput<'db>,
        lowered_funcs: &mut LoweredFunctions,
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
                Waiting { index, unit, typecheck, ownership },
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
        waiting: Waiting<'_, 'db>,
        lowered_funcs: &mut LoweredFunctions,
    ) -> Result<ResolvedConsts, ConstEvalError> {
        let mut resolved = ResolvedConsts::new();
        let expr_types = waiting.typecheck.expr_types;
        let call_targets = waiting.typecheck.call_targets;
        // The const each waiting function a const reaches waits on, by its
        // place among the unit's consts, which is a binding's id. The others
        // wait for every const.
        let needs: HashMap<String, u32> = waiting.typecheck.result.const_needs(self.db).iter()
            .map(|(name, index)| (name.text(self.db).S(), *index))
            .collect();

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

        // In source order, so that each waiting function is lowered before any
        // const that can call it. A const names only consts above it, so this
        // is an order the graph's dependencies allow.
        let mut bindings: Vec<_> = const_graph.bindings.iter().collect();
        bindings.sort_by_key(|binding| binding.stmt_id.0);
        let mut evaluator = self.ctfe_evaluator.borrow_mut();
        for binding in bindings {
            let expr = const_exprs[binding.stmt_id.0 as usize];
            let mut env = self.const_env(&mut **evaluator, expr_types, call_targets, lowered_funcs);

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
                ConstError::MissingType => unreachable!("a script const's type comes from the graph"),
            })?.expect("nothing is deferred at a script's top level");

            resolved.insert(binding.stmt_id, binding.name.clone(), value.clone());
            resolved_consts_map.insert(binding.name.clone(), (binding.ir_type.clone(), value));

            let ready: Vec<String> = lowered_funcs.deferred.iter()
                .filter(|name| needs.get(*name) == Some(&binding.stmt_id.0))
                .cloned()
                .collect();
            if !ready.is_empty() {
                let Ok(batch) = self.phase_lower_functions(
                    waiting.index, waiting.unit, waiting.typecheck, waiting.ownership,
                    &resolved_consts_map, true, Some(&ready),
                ) else {
                    unreachable!("lowering that defers what it cannot lower refuses nothing");
                };
                lowered_funcs.add(batch, &ready);
            }
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
        let script_ctx = lower_context_over(&self.unit_ids()[..index], &self.unit_records[..index]);

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
    /// The mirror of what the module compiler does for its own copies: seed the
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
                CodeRef::External { unit, id } => self.units.iter()
                    .position(|held| held.id == *unit)
                    .map(|index| &self.unit_records[index])?
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
        self.unit_records[index] = Arc::new(UnitLowerRecord {
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
        });
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
