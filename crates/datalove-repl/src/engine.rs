//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;
use salsa::Setter as _;
use serde::Serialize;

use crate::{Command, ReplCommand, Eval, EvalBinding, EvalExpr, InputParse, Input};
use datalove_datafun as datafun;
use datafun::pipeline::{ScriptCompiler, ScriptExecutor, ScriptSession, ModuleCompilationPipeline, SystemLibrary, WorkspaceDescriptor};
use datafun::pipeline::rider_load::{RegisteredNatives, register_natives};
use datalove_datafun_ir::{ExportBinding, IrCodeUnit};

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
    /// The system library the session compiles against, kept so the engine can
    /// rebuild its session and executor after a crash reset.
    sys: SystemLibrary,
    /// The workspace built from `sys`.
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
    /// The rider libraries `executor`'s native table points into.
    ///
    /// Only non-empty when the riders were built rather than taken from this
    /// binary; see `rider_load::build_sys_riders`. Declared after `executor`
    /// so that it is dropped after it: dropping one unmaps the code every
    /// registered pointer leads to, and the executor destroying its live
    /// values can still call into them.
    natives: RegisteredNatives,
}

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
    /// Rider libraries the executor's native table points into, to be kept
    /// for as long as that executor is.
    natives: RegisteredNatives,
}

impl Started {
    /// Compile the workspace's modules and register its native riders.
    fn compile(
        db: &datafun::Database,
        pipeline: &mut ModuleCompilationPipeline,
        workspace: &WorkspaceDescriptor,
        sys: &SystemLibrary,
    ) -> AnyResult<Started> {
        let compiled = pipeline.compile_fresh(db);

        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Module compilation failed: {}", errors.join("; "));
        }

        // Safe to unwrap since we checked for errors above.
        let script = compiled.script_compiler_default(db)
            .expect("script_compiler should succeed after error check")
            .into_session();
        let mut executor = compiled.script_executor(datafun::DebugOutputMode::Disabled, None)
            .expect("script_executor should succeed after error check");
        let natives = register_natives(
            workspace, &compiled, &sys.natives, &mut executor)?;

        Ok(Started { script, executor, natives })
    }
}

impl Engine {
    pub fn new(sys: SystemLibrary) -> AnyResult<Engine> {
        // The work dir is named whether or not it is used: with
        // `DATALOVE_BUILD_SYS_RIDERS` set the riders are built rather than
        // taken from this binary, and that is where they build.
        let workspace = WorkspaceDescriptor::from_system_library(&sys)
            .with_work_dir(datalove_paths::work_dir()?);
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
        let db = datafun::Database::default();
        let mut pipeline = workspace.to_pipeline(&db);
        let started = Started::compile(&db, &mut pipeline, &workspace, &sys)?;

        Ok(Engine {
            db,
            sys,
            workspace,
            pipeline,
            script: Some(started.script),
            executor: started.executor,
            natives: started.natives,
        })
    }

    fn reset(&mut self) {
        // Cleanup the current executor.
        self.executor.destroy_live_values();
        // The library compiled at startup and has not changed since, so this is
        // salsa verifying what it has rather than compiling it again.
        let started = Started::compile(&self.db, &mut self.pipeline, &self.workspace, &self.sys)
            .expect("system library compiled successfully at startup");
        self.script = Some(started.script);
        self.executor = started.executor;
        // Assigned after the executor, so the libraries the old one pointed
        // into stay mapped until it is gone.
        self.natives = started.natives;
    }

    /// Build a compiler over the current module compilation, run `work` with
    /// it, and take the session back.
    ///
    /// Everything that needs a compiler goes through here, because a compiler
    /// borrows the database and the engine owns it. The session is out of the
    /// engine while `work` runs, so `work` cannot reach back into it.
    fn with_compiler<R>(&mut self, work: impl FnOnce(&mut ScriptCompiler<'_>) -> R) -> R {
        let compiled = self.pipeline.compile_fresh(&self.db);
        let mut compiler = compiled
            .script_compiler_resumed(&self.db, self.script.take().expect("a session"))
            .expect("the library compiled at startup and has not changed");
        let result = work(&mut compiler);
        self.script = Some(compiler.into_session());
        result
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
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.eval_impl(command.C())
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

    fn eval_impl(&mut self, command: Command) -> Eval {
        match command {
            Command::ReplCommand(repl_command) => self.eval_repl_command(repl_command),
            Command::ScriptStatement(source) => self.eval_script_statement(source),
            Command::Expression(source) => self.eval_expression(source),
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
        }
    }

    fn eval_script_statement(&mut self, source: String) -> Eval {
        let compiled = self.with_compiler(|compiler| compiler.compile_fragment(&source));

        if let Some(error) = compiled.first_error() {
            return Eval::Error(error);
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("a fragment that compiled without errors has ir");
        let output = self.executor.execute_fragment(ir_unit);

        // Check for runtime errors.
        if output.starts_with("Error:") {
            return Eval::Error(output);
        }

        self.report_exports(ir_unit)
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
    /// first. A unit whose re-lowering failed is reported as an error and keeps
    /// the frame it had, so its bindings are gone from the environment but its
    /// values are still there to be destroyed when the session ends.
    pub fn edit_unit(&mut self, unit: usize, source: &str) -> Vec<UnitEdit> {
        let sources = self.script.as_ref().expect("a session").unit_sources();
        assert!(
            unit < sources.len(),
            "unit {unit} was edited but the session has {} units",
            sources.len(),
        );
        sources[unit].set_text(&mut self.db).to(source.S());

        let redone = self.with_compiler(|compiler| compiler.relower_reach(unit));
        self.rerun(redone)
    }

    /// Drop the units from `n` on.
    ///
    /// The cheap one of the three splice verbs: nothing sits after what goes,
    /// so no surviving unit's values change identity and nothing is re-derived
    /// or re-executed. Their bindings leave the environment and the values they
    /// held are destroyed, and the next line appended takes the index the first
    /// dropped unit had.
    pub fn truncate_units(&mut self, n: usize) {
        let count = self.script.as_ref().expect("a session").unit_sources().len();
        assert!(n <= count, "cannot truncate to {n} units; the session has {count}");
        self.with_compiler(|compiler| compiler.truncate_units(n));
        self.executor.truncate_units(n);
    }

    /// Splice the unit at `unit` out, and re-derive everything after it.
    ///
    /// **The whole suffix is re-derived and re-executed**, not just what the
    /// removal reaches by name: a script value is identified by
    /// `(unit_index, ValueId)`, so taking a unit out renumbers every unit after
    /// it and their IR has to be built again against the indices they now have.
    /// That cost is inherent to the identity being positional.
    ///
    /// **A removal whose suffix does not compile is rejected and put back**, with
    /// the errors returned, which is the policy [`Self::edit_module`] already
    /// follows and for a related reason: a unit that fails to compile has no IR
    /// and so no frame, which would leave a hole in the numbering the frame
    /// store and every `(unit_index, ValueId)` reference share. So **a unit a
    /// later unit depends on cannot be removed without removing its dependents
    /// first.** A rejected removal leaves the session exactly as it was -- the
    /// frame store is not touched until the whole suffix has compiled.
    pub fn remove_unit(&mut self, unit: usize) -> Result<Vec<UnitEdit>, String> {
        match self.with_compiler(|compiler| compiler.remove_unit(unit)) {
            Ok(redone) => Ok(self.rerun_spliced(unit, redone)),
            Err(errors) => Err(errors.join("; ")),
        }
    }

    /// Splice a new unit in at `unit`, and re-derive everything after it.
    ///
    /// `is_expr` says whether the text is a bare expression or a fragment of
    /// statements; nothing in the text decides it, which is why the appending
    /// path has two entry points rather than one.
    ///
    /// The suffix is re-derived whole and an insertion whose suffix does not
    /// compile is rejected, both for the reasons [`Self::remove_unit`] gives.
    pub fn insert_unit(
        &mut self,
        unit: usize,
        source: &str,
        is_expr: bool,
    ) -> Result<Vec<UnitEdit>, String> {
        match self.with_compiler(|compiler| compiler.insert_unit(unit, source, is_expr)) {
            Ok(redone) => Ok(self.rerun_spliced(unit, redone)),
            Err(errors) => Err(errors.join("; ")),
        }
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
            let compiled = self.pipeline.compile_fresh(&self.db);
            if compiled.has_errors() {
                Err(compiled.all_errors().join("; "))
            } else {
                // Before anything is re-executed: a re-lowered unit still names
                // its module functions by `CodeRef::Module`, so the new IR only
                // takes effect once the executor is looking at the new registry.
                self.executor.set_module_registry(compiled.shared.module_registry.clone());
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
                (Some(error), _) => Eval::Error(error),
                (None, None) => unreachable!("a unit that compiled without errors has ir"),
                (None, Some(ir_unit)) => {
                    let (ty, output) = self.executor.reexecute_unit(index, ir_unit);
                    match (output.starts_with("Error:"), ty) {
                        (true, _) => Eval::Error(output),
                        // An expression unit reports the value it now comes to;
                        // a fragment reports the bindings it defines.
                        (false, Some(ty)) => Eval::SuccessExpr(EvalExpr { ty, value: output }),
                        (false, None) => self.report_exports(ir_unit),
                    }
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
    /// Every unit here compiled, because a splice whose suffix did not was
    /// rejected before the frame store was touched.
    fn rerun_spliced(
        &mut self,
        at: usize,
        redone: Vec<(usize, datafun::pipeline::ScriptCompilationResult)>,
    ) -> Vec<UnitEdit> {
        self.executor.truncate_units(at);
        let is_expr = self.script.as_ref().expect("a session").unit_is_expr();

        let mut reports = Vec::new();
        for (index, compiled) in redone {
            let ir_unit = compiled.ir_unit.as_ref()
                .expect("a splice whose suffix failed to compile was rejected");
            let eval = if is_expr[index] {
                let (ty, output) = self.executor.execute_expr(ir_unit);
                match (output.starts_with("Error:"), ty) {
                    (true, _) => Eval::Error(output),
                    (false, Some(ty)) => Eval::SuccessExpr(EvalExpr { ty, value: output }),
                    (false, None) => self.report_exports(ir_unit),
                }
            } else {
                let output = self.executor.execute_fragment(ir_unit);
                match output.starts_with("Error:") {
                    true => Eval::Error(output),
                    false => self.report_exports(ir_unit),
                }
            };
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

    fn eval_expression(&mut self, source: String) -> Eval {
        let compiled = self.with_compiler(|compiler| compiler.compile_expr(&source));

        if let Some(error) = compiled.first_error() {
            return Eval::Error(error);
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("an expression that compiled without errors has ir");
        let (ty, output) = self.executor.execute_expr(ir_unit);

        // Check for runtime errors.
        if output.starts_with("Error:") {
            return Eval::Error(output);
        }

        Eval::SuccessExpr(EvalExpr {
            ty: ty.expect("an executed expression unit has a result type"),
            value: output,
        })
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
    pub fn run_script(
        sys: SystemLibrary,
        script_path: &std::path::Path,
    ) -> AnyResult<()> {
        let mut engine = Self::new(sys)?;
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
