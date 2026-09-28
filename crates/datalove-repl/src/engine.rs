//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;
use salsa::Setter as _;
use serde::Serialize;

use crate::{Command, ReplCommand, Eval, EvalBinding, EvalExpr, InputParse, Input};
use datalove_datafun as datafun;
use datafun::pipeline::{ScriptCompiler, ScriptExecutor, ScriptSession, TypecheckResult, OwnershipResult, LoweringResult, ModuleCompilationPipeline, SystemLibrary, WorkspaceDescriptor};
use datafun::pipeline::rider_load::register_linked_natives;
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
}

impl Started {
    /// Compile the workspace's modules and register its native riders.
    fn compile(
        db: &datafun::Database,
        pipeline: &mut ModuleCompilationPipeline,
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
        register_linked_natives(
            &compiled.native_symbols(),
            &sys.natives,
            executor.native_table_mut(),
        )?;

        Ok(Started { script, executor })
    }
}

impl Engine {
    pub fn new(sys: SystemLibrary) -> AnyResult<Engine> {
        let db = datafun::Database::default();
        let workspace = WorkspaceDescriptor::from_system_library(&sys);
        let mut pipeline = workspace.to_pipeline(&db);
        let started = Started::compile(&db, &mut pipeline, &sys)?;

        Ok(Engine {
            db,
            sys,
            workspace,
            pipeline,
            script: Some(started.script),
            executor: started.executor,
        })
    }

    fn reset(&mut self) {
        // Cleanup the current executor.
        self.executor.destroy_live_values();
        // The library compiled at startup and has not changed since, so this is
        // salsa verifying what it has rather than compiling it again.
        let started = Started::compile(&self.db, &mut self.pipeline, &self.sys)
            .expect("system library compiled successfully at startup");
        self.script = Some(started.script);
        self.executor = started.executor;
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

        if let Some(error) = compile_error(&compiled) {
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

        let mut reports = Vec::new();
        for (index, compiled) in redone {
            let eval = match (compile_error(&compiled), &compiled.ir_unit) {
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

        if let Some(error) = compile_error(&compiled) {
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

/// The first error a compilation ran into, in phase order, if any.
fn compile_error(compiled: &datafun::pipeline::ScriptCompilationResult) -> Option<String> {
    match (&compiled.typecheck, &compiled.ownership, &compiled.lowering) {
        (TypecheckResult::ParseError { errors }, _, _) => Some(errors.join("; ")),
        (TypecheckResult::Error { errors }, _, _) => Some(errors.join("; ")),
        (_, OwnershipResult::Error { message }, _) => Some(message.C()),
        (_, _, LoweringResult::Error { message }) => Some(message.C()),
        _ => None,
    }
}



#[cfg(test)]
mod tests {
    //! The edit path, at the level a session actually uses it.
    //!
    //! `crates/datalove-datafun/tests/script_exec_reactivity_tests.rs` is where
    //! the reach itself is measured, against a graph it derives. These say the
    //! engine wires it up: an edit reaches the units it should and the
    //! environment afterwards holds the values it should.

    use super::*;

    /// A B C D, where C uses nothing B provides and D uses `b`.
    const SCRIPT: &str = "let a = 1\n---\nlet b = 2\n---\nlet c = 30\n---\nlet d = b + 5";

    fn value_of(engine: &mut Engine, name: &str) -> String {
        engine.get_environment().into_iter()
            .find(|(bound, _, _)| bound == name)
            .map(|(_, _, value)| value)
            .unwrap_or_else(|| panic!("no binding named {name}"))
    }

    /// Editing B re-derives B and D, and `d` holds the new answer.
    ///
    /// A value-only edit, which is the case that needs execution to cascade
    /// where analysis did not: no type moved, so nothing but B was
    /// re-typechecked, and `d` would sit at 7 if D had not run again.
    #[test]
    fn editing_a_unit_rederives_what_it_reaches() {
        let mut engine = Engine::new(datalove_stdlib::system_library())
            .expect("the engine starts");
        engine.run_source(SCRIPT);
        assert_eq!(value_of(&mut engine, "d"), "7");

        let reports = engine.edit_unit(1, "let b = 3");

        assert_eq!(
            reports.iter().map(|report| report.unit).collect::<Vec<_>>(),
            vec![1, 3],
            "B and D; C uses nothing B provides",
        );
        assert!(
            reports.iter().all(|report| !matches!(report.eval, Eval::Error(_))),
            "both units re-derive cleanly: {reports:?}",
        );
        assert_eq!(value_of(&mut engine, "b"), "3");
        assert_eq!(value_of(&mut engine, "d"), "8", "D ran again against the new `b`");
        assert_eq!(value_of(&mut engine, "c"), "30", "C's frame is untouched");
    }

    /// An edit that breaks a later unit reports the error against that unit.
    #[test]
    fn an_edit_that_breaks_a_later_unit_says_which() {
        let mut engine = Engine::new(datalove_stdlib::system_library())
            .expect("the engine starts");
        engine.run_source(SCRIPT);

        let reports = engine.edit_unit(1, "let b = \"two\"");

        let failed: Vec<usize> = reports.iter()
            .filter(|report| matches!(report.eval, Eval::Error(_)))
            .map(|report| report.unit)
            .collect();
        assert_eq!(failed, vec![3], "`\"two\" + 5` is D's error to report");
        assert_eq!(value_of(&mut engine, "c"), "30", "C never heard about it");
    }

    /// A line submitted after an edit is compiled against the edited session.
    #[test]
    fn a_unit_appended_after_an_edit_sees_the_new_values() {
        let mut engine = Engine::new(datalove_stdlib::system_library())
            .expect("the engine starts");
        engine.run_source(SCRIPT);
        engine.edit_unit(1, "let b = 3");

        engine.run_source("let e = c + d");

        assert_eq!(value_of(&mut engine, "e"), "38", "30 from C and 8 from the new D");
    }
}
