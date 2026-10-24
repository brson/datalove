//! The engine, and how it settles the modules a session compiles against.

use rmx::prelude::*;
use salsa::Setter as _;
use serde::Serialize;
use std::sync::Arc;

use crate::{Unit, Eval, EvalBinding, EvalExpr, EnvBinding};
use datalove_datafun as datafun;
use datafun::pipeline::{ScriptCompiler, ScriptExecutor, ScriptSession, ModuleCompilationPipeline, PackageLibrary, SystemLibrary, UnitStatus, WorkspaceDescriptor};
use datafun::pipeline::rider_load::{RegisteredNatives, RiderNatives, register_natives};
use datalove_datafun_ir::{ExportBinding, IrCodeUnit};
use bct::input::Source;
use datafun::incremental::Roots;
use bct::diagnostic::StoredDiagnostic;
use rmx::std::collections::{BTreeMap, BTreeSet};
use datalove_ct::unit_analysis::{self, Pass};

/// The engine, which owns the database the session is compiled in.
///
/// **It owns it rather than borrowing it**, which is what lets a unit be
/// edited: an edit is `set_text` on the unit's `Source` and needs `&mut db`,
/// and a `ScriptCompiler` holding `&'db dyn Database` cannot be alive across
/// that. So no compiler is kept between calls; [`ScriptSession`] is kept
/// instead and a compiler is built over it per operation. What that costs is
/// re-deriving the module compilation each time, which is salsa verifying what
/// it already has -- the same thing a crash reset has always relied on. See
/// `botdocs/plan-script-reactivity.md`.
pub struct Engine {
    db: datafun::Database,
    /// The workspace the session compiles against.
    workspace: WorkspaceDescriptor,
    /// The pipeline that compiled the modules, kept rather than rebuilt.
    ///
    /// A pipeline owns the `Source` inputs salsa keys its queries on, so throwing
    /// one away and making another means recompiling the library from nothing. A
    /// reset happens on every panic recovery, and the library it recompiles cannot
    /// have changed -- it is the copy embedded in the binary -- so keeping the
    /// pipeline turns a reset from a full compile into salsa verifying what it
    /// already has.
    pipeline: ModuleCompilationPipeline,
    /// The units submitted so far and what each one's compilation produced.
    ///
    /// An `Option` only so that it can be handed to a compiler and taken back;
    /// it is `Some` between operations.
    script: Option<ScriptSession>,
    /// Script executor for running compiled units.
    executor: ScriptExecutor,
    /// The natives `executor`'s native table points to, which `pipeline` was
    /// given too.
    ///
    /// Kept so a reset registers the same ones again: riders already built and
    /// loaded stay that way. Holding it here decides nothing about lifetimes:
    /// the table and the jit each took a share when the symbols were
    /// registered, so neither can be reached after its code has gone whatever
    /// order these fields go in.
    natives: RegisteredNatives,
    /// Where `debuglog` goes, kept so a reset sends it to the same place.
    debug_output: datafun::DebugOutputMode,
    /// Modules compiled whether or not a unit requires them.
    ///
    /// The rest of the roots are what the session's units require, worked out
    /// again before every step; see [`Engine::with_compiler`].
    also_roots: Vec<String>,
    /// The required modules that do not compile, as the last settling of the
    /// roots found them; see [`settle_roots`].
    module_errors: ModuleErrors,
    /// What the last operation on the units did to each unit it compiled or
    /// re-derived; see [`Engine::last_step`].
    last_step: Vec<UnitStep>,
}

/// The modules a session requires that do not compile, and what they said.
///
/// **A required module that does not compile is left out of the compile**,
/// with every required module that reaches it, and the rest stay; see
/// [`settle_roots`]. So the engine holds a session whose modules have errors
/// by holding them outside it, and a unit requiring one fails on that require
/// until the module compiles again.
#[derive(Debug, Default)]
pub struct ModuleErrors {
    /// Each required module left out, by path, with the errors of the modules
    /// it reaches that explain why.
    pub broken: BTreeMap<String, Vec<String>>,
    /// The type and ownership diagnostics of the compile that found them, in
    /// the order it reported them, which a caller renders against
    /// [`Engine::db`].
    pub diagnostics: Vec<StoredDiagnostic>,
    /// The lowering errors of that compile, which carry no spans.
    pub lowering_errors: Vec<String>,
}

impl ModuleErrors {
    /// Whether every required module compiles.
    pub fn is_empty(&self) -> bool {
        self.broken.is_empty()
    }
}

/// Put modules that changed in front of the executor, before anything runs.
///
/// A script unit names a module function by `CodeRef::Module`, resolved
/// against the registry the executor holds, so recompiled modules change
/// nothing until it holds theirs. A module that joined may call natives
/// nothing called before. A module keeps its `IrModuleId` across the change,
/// so the units already lowered still call the modules they named.
fn take_modules(
    executor: &mut ScriptExecutor,
    natives: &mut RegisteredNatives,
    compiled: &datafun::pipeline::CompiledModules<'_>,
) {
    executor.set_module_registry(compiled.shared.module_registry.clone());
    *natives = register_natives(&natives.natives, compiled, executor)
        .expect("the natives of modules that compiled are found");
}

/// Narrow the roots to what `units` require, plus `also`, leaving out what does
/// not compile.
///
/// **A required module that does not compile is left out, along with every
/// required module that reaches it**, and the rest stay, so one broken module
/// costs only the units that require it, the way one failed unit costs only
/// the units that read it. A unit requiring a left-out module fails on its
/// `require`, and the engine says why with what this reports.
///
/// The one error no root's leaving fixes is a rider interface's, since every
/// rider is compiled whether or not anything requires it, and that is an error.
fn settle_roots(
    db: &datafun::Database,
    pipeline: &mut ModuleCompilationPipeline,
    units: &[Source],
    also: &[String],
) -> AnyResult<ModuleErrors> {
    pipeline.narrow_roots_to_sources(db, units, also);
    let mut found = ModuleErrors::default();
    loop {
        let compiled = pipeline.compile(db);
        if !compiled.has_errors() {
            return Ok(found);
        }
        let Roots::From(roots) = pipeline.roots().clone() else {
            unreachable!("the engine always narrows");
        };

        // What the first compile said, since it holds every module the
        // later ones hold and the ones they left out.
        if found.is_empty() {
            let types = compiled.get_module_type_diagnostics(db).into_iter()
                .map(|diagnostic| diagnostic.to_diagnostic(db));
            let ownership = compiled.get_module_ownership_diagnostics(db).into_iter()
                .map(|diagnostic| diagnostic.to_diagnostic(db));
            found.diagnostics = types.chain(ownership)
                .map(|diagnostic| diagnostic.to_stored(db))
                .collect();
            found.lowering_errors = compiled.all_lowering_errors();
        }

        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for by_path in [&compiled.parse_errors, &compiled.path_to_errors,
                        &compiled.ownership_errors, &compiled.lowering_errors] {
            for (path, messages) in by_path.iter().filter(|(_, messages)| !messages.is_empty()) {
                errors.entry(path.C()).or_default().extend(messages.iter().cloned());
            }
        }

        let reach = module_reach(db, &compiled);
        let failing: Vec<String> = roots.iter()
            .filter(|root| reach.get(*root).is_some_and(|reached| reached.iter().any(|m| errors.contains_key(m))))
            .cloned()
            .collect();
        if failing.is_empty() {
            bail!("modules no root requires do not compile: {}", compiled.all_errors().join("; "));
        }

        for root in &failing {
            let messages = reach[root].iter()
                .filter_map(|m| errors.get(m))
                .flatten()
                .cloned()
                .collect();
            found.broken.insert(root.C(), messages);
        }
        let kept = roots.into_iter().filter(|root| !failing.contains(root)).collect();
        pipeline.set_roots(Roots::From(kept));
    }
}

/// Every module and rider each module of a compile reaches by `require`,
/// itself included, by path.
fn module_reach(db: &datafun::Database, compiled: &datafun::pipeline::CompiledModules<'_>) -> BTreeMap<String, BTreeSet<String>> {
    let parsed = compiled.shared.parsed_graph;
    let requires = parsed.resolved_requires(db);
    let path = |module: bct::module_graph::ModuleId<'_>| module.path(db).C();

    let mut reach = BTreeMap::new();
    for module in parsed.graph(db).iter_modules(db) {
        let mut seen = BTreeSet::new();
        let mut stack = vec![module.id(db)];
        while let Some(next) = stack.pop() {
            if seen.insert(path(next)) {
                stack.extend(requires.get(&next).into_iter().flatten().map(|(_, target)| *target));
                // A rider requires nothing, so it is reached and ends there.
                seen.extend(parsed.resolved_riders(db).get(&next).into_iter().flatten()
                    .map(|(_, rider)| path(rider.module_id)));
            }
        }
        reach.insert(path(module.id(db)), seen);
    }
    reach
}

/// What a caller is shown of a unit that did not compile, while the compiler
/// that compiled it is still alive.
///
/// The compiler's diagnostics are those of the last unit it compiled and do
/// not outlive it, and the engine lets each compiler go once its operation is
/// over, so a caller that renders them has to be handed them here.
pub type OnCompileError<'a> =
    &'a mut dyn FnMut(&ScriptCompiler<'_>, &datafun::pipeline::ScriptCompilationResult);

/// What one step did to one unit: which of its analysis passes ran again,
/// and what re-deriving it came to, if it was re-derived.
///
/// A step is any operation on the units, an append included.
#[derive(Debug, Clone, Serialize)]
pub struct UnitStep {
    /// The unit's index in the session.
    pub unit: usize,
    /// The analysis passes salsa ran for the unit in this step.
    pub compiled: Compiled,
    /// What the unit came to, if the step re-derived it: lowered it again
    /// and ran it, or found it failed or blocked. `None` for a unit whose
    /// analysis ran and whose lowering did not change, so it was left as it
    /// was.
    pub eval: Option<Eval>,
}

impl UnitStep {
    /// A unit re-derived, before what ran for it is merged in.
    fn rederived(unit: usize, eval: Eval) -> UnitStep {
        UnitStep { unit, compiled: Compiled::default(), eval: Some(eval) }
    }
}

/// Which analysis passes ran for a unit in a step.
///
/// Analysis is parse and name resolution, typecheck, and ownership; not
/// lowering, which is what re-deriving a unit does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Compiled {
    pub parse: bool,
    pub typecheck: bool,
    pub ownership: bool,
}

impl Compiled {
    /// Whether any pass ran.
    pub fn any(&self) -> bool {
        self.parse || self.typecheck || self.ownership
    }
}

/// A fresh session: everything the engine rebuilds when it resets.
struct Started {
    script: ScriptSession,
    executor: ScriptExecutor,
    /// What the executor's natives were registered from. The table holds its
    /// own share, so this is the caller's copy rather than what keeps them.
    natives: RegisteredNatives,
}

impl Started {
    /// Compile the modules the roots were settled on and register their natives.
    fn compile(
        db: &datafun::Database,
        pipeline: &mut ModuleCompilationPipeline,
        natives: &Arc<RiderNatives>,
        debug_output: datafun::DebugOutputMode,
    ) -> AnyResult<Started> {
        let compiled = pipeline.compile(db);
        assert!(!compiled.has_errors(), "the roots were settled on modules that compile");

        let script = compiled.script_compiler_default(db)
            .expect("script_compiler should succeed after error check")
            .into_session();
        let mut executor = compiled.script_executor(debug_output, None)
            .expect("script_executor should succeed after error check");
        let natives = register_natives(natives, &compiled, &mut executor)?;

        Ok(Started { script, executor, natives })
    }
}

impl Engine {
    pub fn new(sys: SystemLibrary) -> AnyResult<Engine> {
        let workspace = WorkspaceDescriptor::from_system_library(&sys);
        Engine::with_workspace(sys, workspace)
    }

    /// The same, with the workspace's own library, if it has one.
    pub fn with_local(sys: SystemLibrary, local: Option<PackageLibrary>) -> AnyResult<Engine> {
        let workspace = WorkspaceDescriptor::from_system_library(&sys);
        let workspace = match local {
            Some(local) => workspace.with_user_library(local)?,
            None => workspace,
        };
        Engine::with_workspace(sys, workspace)
    }

    /// The same, over a workspace that holds more than the system library.
    ///
    /// The library still comes separately because it carries the addresses of
    /// the native rider functions linked into this binary, which a descriptor
    /// has no way to say.
    pub fn with_workspace(
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
    ) -> AnyResult<Engine> {
        Engine::with_options(sys, workspace, datafun::DebugOutputMode::Disabled, Vec::new())
    }

    /// The same, saying where `debuglog` goes and which modules to compile
    /// whether or not a unit requires them.
    ///
    /// **The engine decides the pipeline's roots**: `also_roots`, and whatever
    /// the session's units require, which starts as nothing. Any of them that
    /// does not compile is left out, and [`Self::module_errors`] says so; the
    /// engine starts regardless.
    pub fn with_options(
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
        debug_output: datafun::DebugOutputMode,
        also_roots: Vec<String>,
    ) -> AnyResult<Engine> {
        Engine::with_options_in(datafun::Database::default(), sys, workspace, debug_output, also_roots)
    }

    /// An engine over the system library alone, in a database the caller made.
    ///
    /// For a database that records what salsa runs
    /// (`datafun::Database::recording`), which is how a test sees which units
    /// were typechecked again.
    pub fn in_database(db: datafun::Database, sys: SystemLibrary) -> AnyResult<Engine> {
        let workspace = WorkspaceDescriptor::from_system_library(&sys);
        Engine::with_options_in(db, sys, workspace, datafun::DebugOutputMode::Disabled, Vec::new())
    }

    fn with_options_in(
        db: datafun::Database,
        sys: SystemLibrary,
        workspace: WorkspaceDescriptor,
        debug_output: datafun::DebugOutputMode,
        also_roots: Vec<String>,
    ) -> AnyResult<Engine> {
        // An engine needs somewhere to build riders, so a caller that did not
        // say gets this installation's work dir rather than an engine that
        // cannot start. It only matters with `DATALOVE_BUILD_SYS_RIDERS` set,
        // where the riders are built rather than taken from this binary --
        // which is exactly the arrangement a caller is least likely to have
        // thought about.
        let workspace = match workspace.work_dir {
            Some(_) => workspace,
            None => workspace.with_work_dir(datalove_paths::work_dir()?),
        };

        let mut pipeline = workspace.to_pipeline(&db);
        // The same natives for the life of the pipeline, so that a reset
        // neither rebuilds riders nor reloads them.
        let natives = Arc::new(RiderNatives::for_workspace(&workspace, &sys.natives));
        pipeline.set_natives(natives.clone());

        let module_errors = settle_roots(&db, &mut pipeline, &[], &also_roots)?;
        let started = Started::compile(&db, &mut pipeline, &natives, debug_output)?;

        Ok(Engine {
            db,
            workspace,
            pipeline,
            script: Some(started.script),
            executor: started.executor,
            natives: started.natives,
            debug_output,
            also_roots,
            module_errors,
            last_step: Vec::new(),
        })
    }

    /// The required modules that do not compile; see [`ModuleErrors`].
    pub fn module_errors(&self) -> &ModuleErrors {
        &self.module_errors
    }

    /// The database the session is compiled in, which is what the stored
    /// diagnostics of [`Self::module_errors`] are read back against.
    pub fn db(&self) -> &datafun::Database {
        &self.db
    }

    /// The workspace the session compiles against.
    pub fn workspace(&self) -> &WorkspaceDescriptor {
        &self.workspace
    }

    /// The pipeline the modules are compiled by, for finding which file a
    /// diagnostic points into.
    pub fn pipeline(&self) -> &ModuleCompilationPipeline {
        &self.pipeline
    }

    /// Start the session again, forgetting every unit in it.
    ///
    /// The modules stay compiled, and the riders built and loaded.
    pub fn reset(&mut self) {
        self.executor.destroy_live_values();
        // An empty session requires nothing, so the roots go back to what they
        // were at startup, less any a module edit has broken since.
        self.module_errors = settle_roots(&self.db, &mut self.pipeline, &[], &self.also_roots)
            .expect("errors no root reaches are a rider's, and the riders compiled at startup");
        let started = Started::compile(&self.db, &mut self.pipeline, &self.natives.natives, self.debug_output)
            .expect("the roots were settled on modules that compile");
        self.script = Some(started.script);
        self.executor = started.executor;
        self.natives = started.natives;
        // No units are left to have done anything.
        self.last_step.clear();
    }

    /// Build a compiler over the current module compilation, run `work` with
    /// it, and take the session back.
    ///
    /// Everything that needs a compiler goes through here, because a compiler
    /// borrows the database and the engine owns it. The session is out of the
    /// engine while `work` runs, so `work` cannot reach back into it.
    ///
    /// **The modules compiled are the ones `units` require**, `units` being
    /// the session's units as they will stand once `work` is done, so a unit
    /// that requires a module nothing required before compiles against it, and
    /// one that goes takes its modules with it. Required modules that do not
    /// compile are left out; see [`settle_roots`].
    fn with_compiler<R>(
        &mut self,
        units: &[Source],
        work: impl FnOnce(&mut ScriptCompiler<'_>) -> R,
    ) -> R {
        let previous = self.pipeline.roots().clone();
        self.module_errors = settle_roots(&self.db, &mut self.pipeline, units, &self.also_roots)
            .expect("errors no root reaches are a rider's, and the riders compiled at startup");
        let moved = *self.pipeline.roots() != previous;

        let compiled = self.pipeline.compile(&self.db);
        if moved {
            take_modules(&mut self.executor, &mut self.natives, &compiled);
        }
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("the roots were settled on modules that compile");
        let result = work(&mut compiler);
        self.script = Some(compiler.into_session());
        result
    }

    /// Say why units that failed on a module that does not compile failed.
    ///
    /// A unit requiring a left-out module fails on its `require` as though the
    /// module did not exist, which is true of the compile and misleading to
    /// read, so its error becomes the module's.
    fn explain(&self, unit: Source, eval: Eval) -> Eval {
        let Eval::Error(error) = eval else {
            return eval;
        };
        let reasons: Vec<String> = self.pipeline.required_modules(&self.db, unit).into_iter()
            .filter_map(|path| self.module_errors.broken.get(&path).map(|errors| {
                fmt!("module `{path}` does not compile: {}", errors.join("; "))
            }))
            .collect();
        if reasons.is_empty() {
            Eval::Error(error)
        } else {
            Eval::Error(reasons.join("\n"))
        }
    }

    /// The same for the reports of a re-derivation.
    fn explain_all(&self, reports: Vec<UnitStep>) -> Vec<UnitStep> {
        // A unit that failed at run time took the session with it, so the
        // units reported before it have no sources left to explain against.
        if reports.iter().any(|report| report.eval.as_ref().is_some_and(Eval::resets)) {
            return reports;
        }
        let sources = self.unit_sources();
        reports.into_iter()
            .map(|UnitStep { unit, compiled, eval }| UnitStep {
                unit,
                compiled,
                eval: eval.map(|eval| self.explain(sources[unit], eval)),
            })
            .collect()
    }

    /// The session's units' sources, in order.
    fn unit_sources(&self) -> Vec<Source> {
        self.script.as_ref().expect("a session").unit_sources()
    }

    /// Compile and run a unit as the session's next.
    ///
    /// A panic resets the session, and is reported as [`Eval::CrashReset`].
    pub fn eval(&mut self, unit: Unit) -> Eval {
        self.eval_reporting(unit, &mut |_, _| {})
    }

    /// The same, showing `on_error` a unit that does not compile.
    pub fn eval_reporting(&mut self, unit: Unit, on_error: OnCompileError<'_>) -> Eval {
        let (result, ran) = unit_analysis::collect(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.eval_impl(unit, on_error)))
        });

        let eval = match result {
            Ok(eval_result) => eval_result,
            Err(panic) => {
                self.reset();
                Eval::CrashReset(fmt!("Eval panic: {}", crate::panic_message(&*panic)))
            }
        };
        // The unit appended is the last, unless it took the session with it.
        let step = match eval.resets() {
            true => Vec::new(),
            false => vec![UnitStep::rederived(self.unit_count() - 1, eval.C())],
        };
        self.record_step(step, ran);
        eval
    }

    /// What the last operation on the units did to each unit it compiled or
    /// re-derived, in index order.
    ///
    /// The same as what an edit, a delete or an insert returns, and for an
    /// appended unit the unit itself and any earlier unit whose analysis it
    /// made run again. A unit is named by its index after the operation, so a
    /// runtime error, which empties the session, leaves nothing to name.
    pub fn last_step(&self) -> &[UnitStep] {
        &self.last_step
    }

    /// Run an operation on the units, and say which passes it ran for each
    /// unit beside what it re-derived.
    fn step(&mut self, operation: impl FnOnce(&mut Engine) -> Vec<UnitStep>) -> Vec<UnitStep> {
        let (reports, ran) = unit_analysis::collect(|| operation(self));
        self.record_step(reports, ran);
        self.last_step.C()
    }

    /// Merge the passes that ran into a step's reports, adding a report for
    /// each unit analyzed and not re-derived, and keep the result.
    ///
    /// The passes name units by source, and the sources are those after the
    /// step: an edited unit keeps its source, an inserted one is new, and a
    /// removed one names nothing any more.
    fn record_step(&mut self, mut reports: Vec<UnitStep>, ran: Vec<(Pass, Source)>) {
        let sources = self.unit_sources();
        for (pass, source) in ran {
            let Some(unit) = sources.iter().position(|unit| *unit == source) else {
                continue;
            };
            let index = match reports.iter().position(|report| report.unit == unit) {
                Some(index) => index,
                None => {
                    reports.push(UnitStep { unit, compiled: Compiled::default(), eval: None });
                    reports.len() - 1
                }
            };
            let compiled = &mut reports[index].compiled;
            match pass {
                Pass::Parse => compiled.parse = true,
                Pass::Typecheck => compiled.typecheck = true,
                Pass::Ownership => compiled.ownership = true,
            }
        }
        reports.sort_by_key(|report| report.unit);
        self.last_step = reports;
    }

    fn eval_impl(&mut self, unit: Unit, on_error: OnCompileError<'_>) -> Eval {
        let is_expr = unit.is_expression();
        let source = match unit {
            Unit::Fragment(source) | Unit::Expression(source) => source,
        };
        self.eval_new_unit(source, is_expr, on_error)
    }

    /// Compile and run a fragment or an expression as the next unit.
    ///
    /// **A unit that fails stays in the session**, whether it did not compile,
    /// returned early, or read from a unit that did either, and offers no
    /// bindings until an edit makes it run to its end. Only a runtime error
    /// takes the session with it.
    fn eval_new_unit(&mut self, source: String, is_expr: bool, on_error: OnCompileError<'_>) -> Eval {
        let source = Source::new(&self.db, source);
        let mut units = self.unit_sources();
        units.push(source);
        let compiled = self.with_compiler(&units, |compiler| {
            let compiled = compiler.compile_source(source, is_expr);
            if compiled.first_error().is_some() {
                on_error(compiler, &compiled);
            }
            compiled
        });

        if let Some(error) = compiled.first_error() {
            self.executor.skip_unit();
            return self.explain(source, Eval::Error(error));
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("a unit that compiled without errors has ir");
        let (ty, output) = self.execute(ir_unit, is_expr);
        if is_runtime_error(&output) {
            return self.failed(output);
        }
        self.outcome(ir_unit, ty, output)
    }

    /// Run a unit as the session's next: an expression for its value, and a
    /// fragment for what it binds.
    fn execute(&mut self, ir_unit: &IrCodeUnit, is_expr: bool) -> (Option<String>, String) {
        if is_expr {
            self.executor.execute_expr(ir_unit)
        } else {
            (None, self.executor.execute_fragment(ir_unit))
        }
    }

    /// A unit failed at run time: the executor has freed everything the
    /// runtime allocated and emptied its session (see
    /// `ScriptExecutor::failed`), so the compiled session starts again too.
    fn failed(&mut self, output: String) -> Eval {
        self.reset();
        Eval::ErrorReset(output)
    }

    /// What a unit that has just been run, or not run, came to.
    ///
    /// `ty` is the type an expression unit's value came to, which is how one
    /// that ran is told from a fragment.
    fn outcome(&mut self, ir_unit: &IrCodeUnit, ty: Option<String>, output: String) -> Eval {
        let index = ir_unit.script_context()
            .and_then(|ctx| ctx.unit)
            .expect("a compiled unit is numbered") as usize;
        match self.executor.unit_status(index).C() {
            UnitStatus::Ran => match ty {
                Some(ty) => Eval::SuccessExpr(EvalExpr { ty, value: output }),
                None => self.report_exports(ir_unit),
            },
            UnitStatus::ReturnedEarly => Eval::ReturnedEarly(output),
            UnitStatus::Blocked { on } => Eval::Blocked { on },
            UnitStatus::NotCompiled => unreachable!("unit {index} compiled"),
        }
    }

    /// Report the bindings a fragment defined, as the compiler recorded them.
    ///
    /// A fragment that defines nothing, like a require or a set, exports
    /// nothing.
    fn report_exports(&mut self, ir_unit: &IrCodeUnit) -> Eval {
        let exports = ir_unit.script_context()
            .expect("a compiled fragment is a script unit")
            .exports
            .clone();
        let bindings: Vec<EvalBinding> = exports.iter()
            .map(|(name, binding)| self.eval_binding(name, binding))
            .collect();

        if bindings.is_empty() {
            Eval::Nothing
        } else {
            Eval::Success(bindings)
        }
    }

    /// Change one unit's text and re-derive what the edit reaches.
    ///
    /// The units the edit does not reach keep the lowering and the frame they
    /// already have, which is sound because a unit that uses nothing the
    /// edited unit provides holds no reference into it. See
    /// `botdocs/plan-script-reactivity.md`.
    ///
    /// Returns what the step did to each unit it analyzed or re-derived, in
    /// index order; see [`Self::last_step`]. A unit whose re-lowering failed
    /// is reported as an error and stays, with its frame destroyed and its
    /// bindings gone from the environment, until an edit makes it compile
    /// again.
    pub fn edit_unit(&mut self, unit: usize, source: &str) -> Vec<UnitStep> {
        self.step(|engine| engine.edit_unit_in_step(unit, source))
    }

    fn edit_unit_in_step(&mut self, unit: usize, source: &str) -> Vec<UnitStep> {
        let sources = self.unit_sources();
        assert!(
            unit < sources.len(),
            "unit {unit} was edited but the session has {} units",
            sources.len(),
        );
        sources[unit].set_text(&mut self.db).to(source.S());

        let redone = self.with_compiler(&sources, |compiler| compiler.relower_reach(unit));
        let reports = self.rerun(redone);
        self.explain_all(reports)
    }

    /// Drop the units from `n` on.
    ///
    /// The cheap one of the three splice verbs: nothing sits after what goes,
    /// so no surviving unit's values change identity, and usually nothing is
    /// re-derived or re-executed. Their bindings leave the environment and the
    /// values they held are destroyed, and the next line appended takes the
    /// index the first dropped unit had. A unit that goes having taken or
    /// written an earlier unit's binding rewinds the session to where that
    /// binding was made, and the units from there to `n` run again.
    pub fn truncate_units(&mut self, n: usize) -> Vec<UnitStep> {
        self.step(|engine| engine.truncate_units_in_step(n))
    }

    fn truncate_units_in_step(&mut self, n: usize) -> Vec<UnitStep> {
        let mut units = self.unit_sources();
        let count = units.len();
        assert!(n <= count, "cannot truncate to {n} units; the session has {count}");
        units.truncate(n);
        let splice = self.with_compiler(&units, |compiler| compiler.truncate_units(n));
        let reports = self.rerun_spliced(splice);
        self.explain_all(reports)
    }

    /// Splice the unit at `unit` out, and re-derive everything after it.
    ///
    /// **The whole suffix is re-derived and re-executed**, not just what the
    /// removal reaches by name: a script value is identified by
    /// `(unit_index, ValueId)`, so taking a unit out renumbers every unit after
    /// it and their IR has to be built again against the indices they now have.
    /// That cost is inherent to the identity being positional.
    ///
    /// A unit of the suffix that read what the removed unit provided stops
    /// compiling and stays, as any unit that fails does.
    pub fn remove_unit(&mut self, unit: usize) -> Vec<UnitStep> {
        self.step(|engine| engine.remove_unit_in_step(unit))
    }

    fn remove_unit_in_step(&mut self, unit: usize) -> Vec<UnitStep> {
        let mut units = self.unit_sources();
        assert!(unit < units.len(), "unit {unit} was removed but the session has {} units", units.len());
        units.remove(unit);
        let splice = self.with_compiler(&units, |compiler| compiler.remove_unit(unit));
        let reports = self.rerun_spliced(splice);
        self.explain_all(reports)
    }

    /// Splice a new unit in at `unit`, and re-derive everything after it.
    ///
    /// `is_expr` says whether the text is a bare expression or a fragment of
    /// statements; nothing in the text decides it, which is why the appending
    /// path has two entry points rather than one.
    ///
    /// The suffix is re-derived whole, for the reason [`Self::remove_unit`]
    /// gives, and the new unit and the suffix stay whether or not they compile.
    pub fn insert_unit(
        &mut self,
        unit: usize,
        source: &str,
        is_expr: bool,
    ) -> Vec<UnitStep> {
        self.step(|engine| engine.insert_unit_in_step(unit, source, is_expr))
    }

    fn insert_unit_in_step(&mut self, unit: usize, source: &str, is_expr: bool) -> Vec<UnitStep> {
        let source = Source::new(&self.db, source.S());
        let mut units = self.unit_sources();
        assert!(unit <= units.len(), "unit {unit} was inserted but the session has {} units", units.len());
        units.insert(unit, source);
        let splice = self.with_compiler(&units, |compiler| compiler.insert_source(unit, source, is_expr));
        let reports = self.rerun_spliced(splice);
        self.explain_all(reports)
    }

    /// Change one module's source and re-derive the script units it reaches.
    ///
    /// The module-shaped half of [`Self::edit_unit`], and it needs one step
    /// that unit editing does not: the executor calls a module function through
    /// the registry it was handed, so the recompiled modules have to be put in
    /// front of it before anything is run again.
    ///
    /// A unit is re-derived when one of its imports resolves into this module,
    /// when it requires a module that requires this one, when it requires a
    /// module the edit broke or fixed, or when it uses what such a unit
    /// provides. Returns what the step did to each unit it analyzed or
    /// re-derived, in index order.
    ///
    /// **An edit that does not compile is kept.** The module is left out of
    /// the compile, with the modules that require it; the units requiring any
    /// of them fail, saying why; [`Self::module_errors`] has its errors; and an
    /// edit that makes it compile brings it and them back. A module no unit
    /// requires is not compiled, so an edit to one reports nothing.
    pub fn edit_module(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) -> Vec<UnitStep> {
        self.step(|engine| engine.edit_module_in_step(library, package, module, source))
    }

    fn edit_module_in_step(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) -> Vec<UnitStep> {
        assert!(
            self.pipeline.contains_module(library, package, module),
            "no module {library}/{package}/{module} in this session",
        );
        self.pipeline.update_source(&mut self.db, library, package, module, source);
        self.set_workspace_module(library, package, module, source);
        let path = format!("{library}/{package}/{module}");

        let broken_before: BTreeSet<String> = self.module_errors.broken.keys().cloned().collect();
        let sources = self.unit_sources();
        self.module_errors = settle_roots(&self.db, &mut self.pipeline, &sources, &self.also_roots)
            .expect("errors no root reaches are a rider's, and the riders compiled at startup");

        let compiled = self.pipeline.compile(&self.db);
        take_modules(&mut self.executor, &mut self.natives, &compiled);

        // The modules whose units the edit reaches: this one, the ones that
        // reach it, and the ones it broke or fixed.
        let mut reached: BTreeSet<String> = module_reach(&self.db, &compiled).into_iter()
            .filter(|(_, reach)| reach.contains(&path))
            .map(|(module, _)| module)
            .collect();
        reached.insert(path);
        let broken_after: BTreeSet<String> = self.module_errors.broken.keys().cloned().collect();
        reached.extend(broken_before.symmetric_difference(&broken_after).cloned());

        // A unit whose require failed is found here too: its imports are read
        // as they now resolve, and they resolve into a module that compiles.
        let reached: Vec<String> = reached.into_iter().collect();

        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("the roots were settled on modules that compile");
        let redone = compiler.relower_module_reach(&reached);
        self.script = Some(compiler.into_session());

        let reports = self.rerun(redone);
        self.explain_all(reports)
    }

    /// Keep the descriptor in step with the pipeline.
    ///
    /// A reset rebuilds from the pipeline rather than from the descriptor, so
    /// nothing reads this today. It is what a `WorkspaceDelta` would be taken
    /// against, and a descriptor saying one thing while the pipeline compiles
    /// another is the kind of disagreement that is found much later.
    fn set_workspace_module(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) {
        let libraries = self.workspace.system_library.iter_mut()
            .chain(self.workspace.user_libraries.iter_mut());
        let held = libraries
            .filter(|held| held.name == library)
            .filter_map(|held| held.packages.get_mut(package))
            .filter_map(|held| held.modules.get_mut(module))
            .next()
            .unwrap_or_else(|| panic!(
                "the pipeline has {library}/{package}/{module} and the workspace \
                 descriptor it was built from does not",
            ));
        held.source = source.into();
    }

    /// Run each re-lowered unit again in place, reporting what it came to.
    fn rerun(
        &mut self,
        redone: Vec<(usize, datafun::pipeline::ScriptCompilationResult)>,
    ) -> Vec<UnitStep> {
        let mut reports = Vec::new();
        for (index, compiled) in redone {
            let eval = match (compiled.first_error(), &compiled.ir_unit) {
                (Some(error), _) => {
                    self.executor.clear_unit(index);
                    Eval::Error(error)
                }
                (None, None) => unreachable!("a unit that compiled without errors has ir"),
                (None, Some(ir_unit)) => {
                    let (ty, output) = self.executor.reexecute_unit(index, ir_unit);
                    if is_runtime_error(&output) {
                        // The session is gone, so there is nothing left
                        // for the rest to run against.
                        reports.push(UnitStep::rederived(index, self.failed(output)));
                        break;
                    }
                    // An expression unit reports the value it now comes to;
                    // a fragment reports the bindings it defines.
                    self.outcome(ir_unit, ty, output)
                }
            };
            reports.push(UnitStep::rederived(index, eval));
        }
        reports
    }

    /// Run a re-derived suffix, which cannot be run in place.
    ///
    /// **This is the one thing a splice needs that an edit does not.**
    /// [`Self::rerun`] re-executes through `replace_frame`, which puts a frame
    /// at the index the unit already had -- sound for an edit, where no index
    /// moved. A splice moves them: the suffix is a different length and every
    /// unit of it has a new index, so the frames from the splice point on belong
    /// to nobody. They are destroyed, and the re-derived units are executed as
    /// appends, each taking the next free index.
    ///
    /// A unit here that did not compile takes its index with no frame.
    fn rerun_spliced(&mut self, splice: datafun::pipeline::Splice) -> Vec<UnitStep> {
        let datafun::pipeline::Splice { from, redone } = splice;
        self.executor.truncate_units(from);
        let is_expr = self.script.as_ref().expect("a session").unit_is_expr();

        let mut reports = Vec::new();
        for (index, compiled) in redone {
            if let Some(error) = compiled.first_error() {
                self.executor.skip_unit();
                reports.push(UnitStep::rederived(index, Eval::Error(error)));
                continue;
            }
            let ir_unit = compiled.ir_unit.as_ref()
                .expect("a unit that compiled without errors has ir");
            let (ty, output) = self.execute(ir_unit, is_expr[index]);
            if is_runtime_error(&output) {
                // As in `rerun`: nothing is left for the rest.
                reports.push(UnitStep::rederived(index, self.failed(output)));
                break;
            }
            let eval = self.outcome(ir_unit, ty, output);
            reports.push(UnitStep::rederived(index, eval));
        }
        reports
    }

    /// Describe one exported binding, looking up its current value.
    fn eval_binding(&mut self, name: &str, binding: &ExportBinding) -> EvalBinding {
        match binding {
            ExportBinding::Function(_) => EvalBinding::Function { name: name.S() },
            ExportBinding::Value(_) => {
                let (ty, value) = self.binding_type_and_value(name);
                EvalBinding::Value { name: name.S(), ty, value }
            }
            ExportBinding::Slot(_) => {
                let (ty, value) = self.binding_type_and_value(name);
                EvalBinding::Slot { name: name.S(), ty, value }
            }
        }
    }

    fn binding_type_and_value(&mut self, name: &str) -> (String, String) {
        self.executor.get_binding(name)
            .expect("the executor registered the unit's exports before executing it")
    }

    /// The bindings the session offers, the nearest of each name, sorted by
    /// name.
    pub fn get_environment(&mut self) -> Vec<EnvBinding> {
        self.executor.get_environment()
            .into_iter()
            .map(|(name, _kind, ty, value)| EnvBinding { name, ty, value })
            .collect()
    }

    /// Unit `unit`'s parse, type and ownership diagnostics as they stand,
    /// rendered as text in `style` quoting the unit as `name`; empty for a
    /// unit with none.
    pub fn unit_diagnostics(&mut self, unit: usize, name: &str, style: bct::render::TextStyle) -> String {
        let units = self.unit_sources();
        self.with_compiler(&units, |compiler| compiler.render_unit_diagnostics(unit, name, style))
    }

    /// How many units the session has, failed ones included.
    pub fn unit_count(&self) -> usize {
        self.unit_sources().len()
    }

    /// What unit `unit` provides and does to earlier units' bindings.
    pub fn unit_effects(&self, unit: usize) -> datafun::pipeline::UnitEffects {
        self.script.as_ref().expect("a session").unit_effects(unit)
    }

    /// What unit `unit` binds and what each binding holds now, `None` for one
    /// given away.
    ///
    /// One unit's frame, where [`Self::get_environment`] is the nearest
    /// binding of each name.
    pub fn unit_bindings(&mut self, unit: usize) -> Vec<(String, Option<String>)> {
        self.executor.unit_bindings(unit)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.executor.destroy_live_values();
    }
}

/// Whether what running a unit printed is a runtime error, which ends the
/// session.
fn is_runtime_error(output: &str) -> bool {
    output.starts_with("Error:")
}
