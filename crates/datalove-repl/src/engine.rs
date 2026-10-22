//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;
use salsa::Setter as _;
use serde::Serialize;
use std::sync::Arc;

use crate::{Command, ReplCommand, Eval, EvalBinding, EvalExpr, InputParse, Input};
use datalove_datafun as datafun;
use datafun::pipeline::{ScriptCompiler, ScriptExecutor, ScriptSession, ModuleCompilationPipeline, PackageLibrary, SystemLibrary, UnitStatus, WorkspaceDescriptor};
use datafun::pipeline::rider_load::{RegisteredNatives, RiderNatives, register_natives};
use datalove_datafun_ir::{ExportBinding, IrCodeUnit};
use bct::input::Source;
use datafun::incremental::Roots;
use rmx::std::collections::{BTreeMap, BTreeSet};

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
}

/// The required modules left out of a compile because they do not compile,
/// by path, with their errors.
type BrokenModules = BTreeMap<String, Vec<String>>;

/// Every module each module of a compile reaches by `require`, itself
/// included, by path.
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

/// One input's parse and eval, and the environment it left behind.
#[derive(Debug, Serialize)]
pub struct InputResult {
    pub input: String,
    pub parse: InputParse,
    pub eval: Option<Eval>,
    pub environment: Vec<EnvBinding>,
}

/// What re-deriving one unit after an edit came to.
#[derive(Debug, Serialize)]
pub struct UnitEdit {
    /// The unit's index in the session.
    pub unit: usize,
    pub eval: Eval,
}

/// An environment binding as reported after an input.
#[derive(Debug, Serialize)]
pub struct EnvBinding {
    pub name: String,
    pub ty: String,
    pub value: String,
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
    /// Compile the workspace's modules and register its native riders.
    fn compile(
        db: &datafun::Database,
        pipeline: &mut ModuleCompilationPipeline,
        natives: &Arc<RiderNatives>,
        debug_output: datafun::DebugOutputMode,
    ) -> AnyResult<Started> {
        let compiled = pipeline.compile(db);

        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Module compilation failed: {}", errors.join("; "));
        }

        // Safe to unwrap since we checked for errors above.
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

        let db = datafun::Database::default();
        let mut pipeline = workspace.to_pipeline(&db);
        // The same natives for the life of the pipeline, so that a reset
        // neither rebuilds riders nor reloads them.
        let natives = Arc::new(RiderNatives::for_workspace(&workspace, &sys.natives));
        pipeline.set_natives(natives.clone());
        Engine::with_pipeline(db, workspace, pipeline, &natives, datafun::DebugOutputMode::Disabled, Vec::new())
    }

    /// The same, over a pipeline the caller built from `workspace` in `db`.
    ///
    /// For a caller that compiles the modules itself first, to report what
    /// stops them compiling in its own way: an engine cannot be started over
    /// modules with errors, and says only that they had some. Compiling them
    /// again here is salsa verifying what the caller already has.
    ///
    /// `natives` are the ones `pipeline` was given, and `debug_output` is where
    /// `debuglog` goes.
    ///
    /// **The engine decides the pipeline's roots**: the modules in `also_roots`,
    /// and whatever the session's units require, which starts as nothing. A
    /// caller compiling first to report errors should narrow to `also_roots` to
    /// compile what the engine will.
    pub fn with_pipeline(
        db: datafun::Database,
        workspace: WorkspaceDescriptor,
        mut pipeline: ModuleCompilationPipeline,
        natives: &Arc<RiderNatives>,
        debug_output: datafun::DebugOutputMode,
        also_roots: Vec<String>,
    ) -> AnyResult<Engine> {
        pipeline.narrow_roots_to_sources(&db, &[], &also_roots);
        let started = Started::compile(&db, &mut pipeline, natives, debug_output)?;

        Ok(Engine {
            db,
            workspace,
            pipeline,
            script: Some(started.script),
            executor: started.executor,
            natives: started.natives,
            debug_output,
            also_roots,
        })
    }

    fn reset(&mut self) {
        // Cleanup the current executor.
        self.executor.destroy_live_values();
        // An empty session requires nothing, so the roots go back to what they
        // were at startup, which compiled then and has not changed since.
        self.pipeline.narrow_roots_to_sources(&self.db, &[], &self.also_roots);
        let started = Started::compile(&self.db, &mut self.pipeline, &self.natives.natives, self.debug_output)
            .expect("the roots compiled successfully at startup");
        self.script = Some(started.script);
        self.executor = started.executor;
        self.natives = started.natives;
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
    /// one that goes takes its modules with it. When that changes which modules
    /// there are, the executor is handed their new registry and their natives
    /// before anything runs. A module keeps its `IrModuleId` across the change,
    /// so the units already lowered still call the modules they named.
    ///
    /// Required modules that do not compile are left out, and come back by
    /// path with their errors; see [`Self::settle_roots`].
    fn with_compiler<R>(
        &mut self,
        units: &[Source],
        work: impl FnOnce(&mut ScriptCompiler<'_>) -> R,
    ) -> (R, BrokenModules) {
        let previous = self.pipeline.roots().clone();
        let broken = self.settle_roots(units, &previous);

        let compiled = self.pipeline.compile(&self.db);
        if *self.pipeline.roots() != previous {
            self.executor.set_module_registry(compiled.shared.module_registry.clone());
            // A module that joined may call natives nothing called before.
            self.natives = register_natives(&self.natives.natives, &compiled, &mut self.executor)
                .expect("the natives of modules that compiled are found");
        }
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("the roots were settled on modules that compile");
        let result = work(&mut compiler);
        self.script = Some(compiler.into_session());
        (result, broken)
    }

    /// Narrow the roots to what `units` require, leaving out what does not
    /// compile.
    ///
    /// **A required module that does not compile is left out, along with
    /// every required module that reaches it**, and the rest stay, so one
    /// broken module costs only the units that require it, the way one failed
    /// unit costs only the units that read it. A unit requiring a left-out
    /// module fails on its `require`, and the engine says why with what comes
    /// back here. An error no module path explains goes back to `previous`,
    /// which compiled, and is put down to the modules this step added.
    ///
    /// The engine cannot yet hold modules with errors, which is what keeps
    /// broken modules out rather than in the session failing.
    fn settle_roots(&mut self, units: &[Source], previous: &Roots) -> BrokenModules {
        self.pipeline.narrow_roots_to_sources(&self.db, units, &self.also_roots);
        let mut broken = BrokenModules::new();
        loop {
            let compiled = self.pipeline.compile(&self.db);
            if !compiled.has_errors() {
                return broken;
            }
            let Roots::From(roots) = self.pipeline.roots().clone() else {
                unreachable!("the engine always narrows");
            };

            let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for by_path in [&compiled.parse_errors, &compiled.path_to_errors,
                            &compiled.ownership_errors, &compiled.lowering_errors] {
                for (path, messages) in by_path.iter().filter(|(_, messages)| !messages.is_empty()) {
                    errors.entry(path.C()).or_default().extend(messages.iter().cloned());
                }
            }

            let reach = module_reach(&self.db, &compiled);
            let failing: Vec<&String> = roots.iter()
                .filter(|root| reach.get(*root).is_some_and(|reached| reached.iter().any(|m| errors.contains_key(m))))
                .collect();

            if compiled.resolution_error.is_some() || failing.is_empty() {
                let Roots::From(before) = previous else {
                    unreachable!("the engine always narrows");
                };
                let all = compiled.all_errors();
                for added in roots.difference(before) {
                    broken.insert(added.C(), all.clone());
                }
                self.pipeline.set_roots(previous.clone());
                return broken;
            }

            for root in &failing {
                let messages = reach[*root].iter()
                    .filter_map(|m| errors.get(m))
                    .flatten()
                    .cloned()
                    .collect();
                broken.insert((*root).C(), messages);
            }
            let kept = roots.iter().filter(|root| !failing.contains(root)).cloned().collect();
            self.pipeline.set_roots(Roots::From(kept));
        }
    }

    /// Say why units that failed on a module that does not compile failed.
    ///
    /// A unit requiring a left-out module fails on its `require` as though the
    /// module did not exist, which is true of the compile and misleading to
    /// read, so its error becomes the module's.
    fn explain(&self, unit: Source, eval: Eval, broken: &BrokenModules) -> Eval {
        let Eval::Error(error) = eval else {
            return eval;
        };
        let reasons: Vec<String> = self.pipeline.required_modules(&self.db, unit).into_iter()
            .filter_map(|path| broken.get(&path).map(|errors| {
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
    fn explain_all(&self, reports: Vec<UnitEdit>, broken: &BrokenModules) -> Vec<UnitEdit> {
        let sources = self.unit_sources();
        reports.into_iter()
            .map(|UnitEdit { unit, eval }| UnitEdit { unit, eval: self.explain(sources[unit], eval, broken) })
            .collect()
    }

    /// The session's units' sources, in order.
    fn unit_sources(&self) -> Vec<Source> {
        self.script.as_ref().expect("a session").unit_sources()
    }

    pub fn parse_input(&mut self, input: Input) -> InputParse {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.parse_input_impl(input.C())
        }));

        match result {
            Ok(parse_result) => parse_result,
            Err(panic_info) => {
                self.reset();
                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.S()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.C()
                } else {
                    "Unknown panic".S()
                };
                InputParse::CrashReset(format!("Parse panic: {}", panic_msg))
            }
        }
    }

    fn parse_input_impl(&mut self, input: Input) -> InputParse {
        match input {
            Input::Input(s) => self.parse_input_oneline(&s),
            Input::Multiline(s) => self.parse_input_multiline(&s),
        }
    }

    fn parse_input_oneline(&mut self, input: &str) -> InputParse {
        match crate::classify_input(input) {
            crate::InputKind::Whitespace => InputParse::Empty,
            crate::InputKind::ReplCommand => Command::repl_command(input),
            crate::InputKind::OnelineStatement => Command::script_statement(input),
            crate::InputKind::MultilineStatement => InputParse::ReadMultiline(S(input)),
            crate::InputKind::OpenBraceTree => InputParse::ReadMultiline(S(input)),
            crate::InputKind::Expression => Command::expression(input),
        }
    }

    fn parse_input_multiline(&mut self, input: &str) -> InputParse {
        match crate::classify_input(input) {
            crate::InputKind::Whitespace => InputParse::Empty,
            crate::InputKind::ReplCommand => Command::repl_command(input),
            crate::InputKind::OnelineStatement => Command::script_statement(input),
            crate::InputKind::MultilineStatement => Command::script_statement(input),
            crate::InputKind::OpenBraceTree => todo!(),
            crate::InputKind::Expression => Command::expression(input),
        }
    }

    pub fn eval(&mut self, command: Command) -> Eval {
        self.eval_reporting(command, &mut |_, _| {})
    }

    /// The same, showing `on_error` a unit that does not compile.
    pub fn eval_reporting(&mut self, command: Command, on_error: OnCompileError<'_>) -> Eval {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.eval_impl(command.C(), on_error)
        }));

        match result {
            Ok(eval_result) => eval_result,
            Err(panic_info) => {
                self.reset();
                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.S()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.C()
                } else {
                    "Unknown panic".S()
                };
                Eval::CrashReset(format!("Eval panic: {}", panic_msg))
            }
        }
    }

    fn eval_impl(&mut self, command: Command, on_error: OnCompileError<'_>) -> Eval {
        match command {
            Command::ReplCommand(repl_command) => self.eval_repl_command(repl_command),
            Command::ScriptStatement(source) => self.eval_script_statement(source, on_error),
            Command::Expression(source) => self.eval_expression(source, on_error),
        }
    }

    fn eval_repl_command(&mut self, command: ReplCommand) -> Eval {
        match command {
            ReplCommand::Unknown => {
                Eval::Error("unknown command".S())
            }
            ReplCommand::Help => {
                Eval::CallerInterpret(command)
            }
            ReplCommand::Exit => {
                Eval::CallerInterpret(command)
            }
            ReplCommand::Reset => {
                self.reset();
                Eval::CallerInterpret(command)
            }
            ReplCommand::Save => {
                Eval::CallerInterpret(command)
            }
        }
    }

    /// Compile and run a fragment as the next unit.
    ///
    /// **A unit that fails stays in the session**, whether it did not compile,
    /// returned early, or read from a unit that did either, and offers no
    /// bindings until an edit makes it run to its end. Only a runtime error
    /// takes the session with it.
    fn eval_script_statement(&mut self, source: String, on_error: OnCompileError<'_>) -> Eval {
        let source = Source::new(&self.db, source);
        let mut units = self.unit_sources();
        units.push(source);
        let (compiled, broken) = self.with_compiler(&units, |compiler| {
            let compiled = compiler.compile_source(source, false);
            if compiled.first_error().is_some() {
                on_error(compiler, &compiled);
            }
            compiled
        });

        if let Some(error) = compiled.first_error() {
            self.executor.skip_unit();
            return self.explain(source, Eval::Error(error), &broken);
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("a fragment that compiled without errors has ir");
        let output = self.executor.execute_fragment(ir_unit);

        if output.starts_with("Error:") {
            return self.failed(output);
        }

        self.outcome(ir_unit, None, output)
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
    /// Returns one report per unit re-derived, in index order, the edited unit
    /// first. A unit whose re-lowering failed is reported as an error and
    /// stays, with its frame destroyed and its bindings gone from the
    /// environment, until an edit makes it compile again.
    pub fn edit_unit(&mut self, unit: usize, source: &str) -> Vec<UnitEdit> {
        let sources = self.script.as_ref().expect("a session").unit_sources();
        assert!(
            unit < sources.len(),
            "unit {unit} was edited but the session has {} units",
            sources.len(),
        );
        sources[unit].set_text(&mut self.db).to(source.S());

        let (redone, broken) = self.with_compiler(&sources, |compiler| compiler.relower_reach(unit));
        let reports = self.rerun(redone);
        self.explain_all(reports, &broken)
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
    pub fn truncate_units(&mut self, n: usize) -> Vec<UnitEdit> {
        let mut units = self.unit_sources();
        let count = units.len();
        assert!(n <= count, "cannot truncate to {n} units; the session has {count}");
        units.truncate(n);
        let (splice, broken) = self.with_compiler(&units, |compiler| compiler.truncate_units(n));
        let reports = self.rerun_spliced(splice);
        self.explain_all(reports, &broken)
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
    pub fn remove_unit(&mut self, unit: usize) -> Vec<UnitEdit> {
        let mut units = self.unit_sources();
        assert!(unit < units.len(), "unit {unit} was removed but the session has {} units", units.len());
        units.remove(unit);
        let (splice, broken) = self.with_compiler(&units, |compiler| compiler.remove_unit(unit));
        let reports = self.rerun_spliced(splice);
        self.explain_all(reports, &broken)
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
    ) -> Vec<UnitEdit> {
        let source = Source::new(&self.db, source.S());
        let mut units = self.unit_sources();
        assert!(unit <= units.len(), "unit {unit} was inserted but the session has {} units", units.len());
        units.insert(unit, source);
        let (splice, broken) = self.with_compiler(&units, |compiler| compiler.insert_source(unit, source, is_expr));
        let reports = self.rerun_spliced(splice);
        self.explain_all(reports, &broken)
    }

    /// Change one module's source and re-derive the script units that import
    /// from it.
    ///
    /// The module-shaped half of [`Self::edit_unit`], and it needs one step
    /// that unit editing does not: the executor calls a module function through
    /// the registry it was handed, so the recompiled modules have to be put in
    /// front of it before anything is run again. Nothing else about a script
    /// unit moves when a module is edited.
    ///
    /// A unit is re-derived when one of its imports resolves into this module,
    /// or when it uses what such a unit provides. Returns one report per unit
    /// re-derived, in index order.
    ///
    /// **An edit whose modules do not compile is put back**, and the errors come
    /// back as `Err`. Nothing in the engine can work against a module set with
    /// errors -- every later line would fail to build a compiler at all -- so a
    /// rejected edit is the only way to leave the session usable. Reporting
    /// module errors while keeping the broken text means carrying them through
    /// every path that compiles, which is not this.
    pub fn edit_module(
        &mut self,
        library: &str,
        package: &str,
        module: &str,
        source: &str,
    ) -> Result<Vec<UnitEdit>, String> {
        let previous = self.pipeline.module_text(&self.db, library, package, module)
            .unwrap_or_else(|| panic!("no module {library}/{package}/{module} in this session"));
        self.pipeline.update_source(&mut self.db, library, package, module, source);
        self.set_workspace_module(library, package, module, source);

        let path = format!("{library}/{package}/{module}");
        let relowered = {
            let compiled = self.pipeline.compile(&self.db);
            if compiled.has_errors() {
                Err(compiled.all_errors().join("; "))
            } else {
                // Before anything is re-executed: a re-lowered unit still names
                // its module functions by `CodeRef::Module`, so the new IR only
                // takes effect once the executor is looking at the new registry.
                self.executor.set_module_registry(compiled.shared.module_registry.clone());
                // A `require` the edit added may have brought in modules, and
                // the natives they call.
                self.natives = register_natives(&self.natives.natives, &compiled, &mut self.executor)
                    .expect("the natives of modules that compiled are found");
                let mut compiler = compiled
                    .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
                    .expect("the modules compiled without errors");
                let redone = compiler.relower_module_reach(std::slice::from_ref(&path));
                self.script = Some(compiler.into_session());
                Ok(redone)
            }
        };

        match relowered {
            Ok(redone) => Ok(self.rerun(redone)),
            Err(errors) => {
                self.pipeline.update_source(&mut self.db, library, package, module, &previous);
                self.set_workspace_module(library, package, module, &previous);
                Err(errors)
            }
        }
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
    ) -> Vec<UnitEdit> {
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
                    if output.starts_with("Error:") {
                        // The session is gone, so there is nothing left
                        // for the rest to run against.
                        reports.push(UnitEdit { unit: index, eval: self.failed(output) });
                        break;
                    }
                    // An expression unit reports the value it now comes to;
                    // a fragment reports the bindings it defines.
                    self.outcome(ir_unit, ty, output)
                }
            };
            reports.push(UnitEdit { unit: index, eval });
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
    fn rerun_spliced(&mut self, splice: datafun::pipeline::Splice) -> Vec<UnitEdit> {
        let datafun::pipeline::Splice { from, redone } = splice;
        self.executor.truncate_units(from);
        let is_expr = self.script.as_ref().expect("a session").unit_is_expr();

        let mut reports = Vec::new();
        for (index, compiled) in redone {
            if let Some(error) = compiled.first_error() {
                self.executor.skip_unit();
                reports.push(UnitEdit { unit: index, eval: Eval::Error(error) });
                continue;
            }
            let ir_unit = compiled.ir_unit.as_ref()
                .expect("a unit that compiled without errors has ir");
            let (ty, output) = if is_expr[index] {
                self.executor.execute_expr(ir_unit)
            } else {
                (None, self.executor.execute_fragment(ir_unit))
            };
            if output.starts_with("Error:") {
                // As in `rerun`: nothing is left for the rest.
                reports.push(UnitEdit { unit: index, eval: self.failed(output) });
                break;
            }
            let eval = self.outcome(ir_unit, ty, output);
            reports.push(UnitEdit { unit: index, eval });
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

    /// Compile and run an expression as the next unit, which stays whatever
    /// becomes of it, as a fragment does.
    fn eval_expression(&mut self, source: String, on_error: OnCompileError<'_>) -> Eval {
        let source = Source::new(&self.db, source);
        let mut units = self.unit_sources();
        units.push(source);
        let (compiled, broken) = self.with_compiler(&units, |compiler| {
            let compiled = compiler.compile_source(source, true);
            if compiled.first_error().is_some() {
                on_error(compiler, &compiled);
            }
            compiled
        });

        if let Some(error) = compiled.first_error() {
            self.executor.skip_unit();
            return self.explain(source, Eval::Error(error), &broken);
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("an expression that compiled without errors has ir");
        let (ty, output) = self.executor.execute_expr(ir_unit);

        if output.starts_with("Error:") {
            return self.failed(output);
        }

        self.outcome(ir_unit, ty, output)
    }

    /// Get current environment bindings (functions and let statements).
    ///
    /// Returns a list of (name, type, value) triples, sorted by name.
    pub fn get_environment(&mut self) -> Vec<(String, String, String)> {
        // Delegate to the executor's get_environment method.
        self.executor.get_environment()
            .into_iter()
            .map(|(name, _kind, ty, value)| (name, ty, value))
            .collect()
    }

    /// Evaluate a script of inputs, one result per input.
    ///
    /// Inputs are separated by a line holding only `---`. An input spanning
    /// several lines is submitted the way the UI submits a multiline entry, so
    /// definitions that span lines are evaluated rather than left waiting for
    /// more input.
    pub fn run_source(&mut self, source: &str) -> Vec<InputResult> {
        let mut results = Vec::new();

        for section in source.split("\n---\n") {
            let input = section.trim();
            if input.is_empty() {
                continue;
            }

            let repl_input = if input.contains('\n') {
                Input::Multiline(input.S())
            } else {
                Input::Input(input.S())
            };

            let parse = self.parse_input(repl_input);
            let eval = match &parse {
                InputParse::Command(command) => Some(self.eval(command.C())),
                _ => None,
            };
            let environment = self.get_environment()
                .into_iter()
                .map(|(name, ty, value)| EnvBinding { name, ty, value })
                .collect();

            results.push(InputResult { input: input.S(), parse, eval, environment });
        }

        results
    }

    /// Execute a script file and print one JSON result per input.
    ///
    /// `local` is the workspace's own library, if it has one.
    pub fn run_script(
        sys: SystemLibrary,
        local: Option<PackageLibrary>,
        script_path: &std::path::Path,
    ) -> AnyResult<()> {
        let mut engine = Self::with_local(sys, local)?;
        let contents = std::fs::read_to_string(script_path)
            .context("failed to read script file")?;

        for result in engine.run_source(&contents) {
            println!("{}", rmx::serde_json::to_string(&result)?);
        }

        // Engine's Drop impl will call destroy_all().
        Ok(())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.executor.destroy_live_values();
    }
}
