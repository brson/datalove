//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;

use crate::{Command, ReplCommand, Eval, EvalBinding, EvalExpr, InputParse, Input};
use datalove_datafun as datafun;
use datafun::pipeline::{ScriptCompiler, ScriptExecutor, TypecheckResult, OwnershipResult, LoweringResult, WorkspaceDescriptor};
use datafun::pipeline::rider_load::{build_and_load_riders, LoadedRider};
use datalove_datafun_ir::ExportBinding;

pub struct Engine<'db> {
    db: &'db datafun::Database,
    /// The system library the session compiles against, kept so the engine can
    /// rebuild its compiler and executor after a crash reset.
    workspace: WorkspaceDescriptor,
    history: ReplHistory,
    /// Script compiler for incremental compilation.
    compiler: ScriptCompiler<'db>,
    /// Script executor for running compiled units.
    executor: ScriptExecutor,
    /// Loaded native rider libraries, which must outlive the executor.
    riders: Vec<LoadedRider>,
}

struct ReplHistory {
    entries: Vec<HistoryEntry>,
}

struct HistoryEntry {
    _command: Command,
    _last_eval: Eval,
}

/// A compiled session: everything the engine rebuilds when it resets.
struct Session<'db> {
    compiler: ScriptCompiler<'db>,
    executor: ScriptExecutor,
    riders: Vec<LoadedRider>,
}

impl<'db> Session<'db> {
    /// Compile a workspace's modules and load its native riders.
    fn compile(db: &'db datafun::Database, workspace: &WorkspaceDescriptor) -> AnyResult<Session<'db>> {
        let mut pipeline = workspace.to_pipeline(db);
        let compiled = pipeline.compile_fresh(db);

        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Module compilation failed: {}", errors.join("; "));
        }

        // Safe to unwrap since we checked for errors above.
        let compiler = compiled.script_compiler_default(db)
            .expect("script_compiler should succeed after error check");
        let mut executor = compiled.script_executor(datafun::DebugOutputMode::Disabled, None)
            .expect("script_executor should succeed after error check");
        let riders = build_and_load_riders(workspace, &compiled, &mut executor)?;

        Ok(Session { compiler, executor, riders })
    }
}

impl ReplHistory {
    fn new() -> Self {
        ReplHistory {
            entries: Vec::new(),
        }
    }

    fn add_non_script_entry(&mut self, command: Command, eval: Eval) {
        self.entries.push(HistoryEntry {
            _command: command,
            _last_eval: eval,
        });
    }
}

impl<'db> Engine<'db> {
    pub fn new(db: &'db datafun::Database) -> AnyResult<Engine<'db>> {
        let workspace = rmx::futures::executor::block_on(WorkspaceDescriptor::load_default_sys())
            .context("failed to load the system library")?;
        let session = Session::compile(db, &workspace)?;

        Ok(Engine {
            db,
            workspace,
            history: ReplHistory::new(),
            compiler: session.compiler,
            executor: session.executor,
            riders: session.riders,
        })
    }

    fn reset(&mut self) {
        self.history = ReplHistory::new();
        // Cleanup the current executor.
        self.executor.destroy_live_values();
        // The system library compiled at startup, so it compiles again here.
        let session = Session::compile(self.db, &self.workspace)
            .expect("system library compiled successfully at startup");
        self.compiler = session.compiler;
        // The old executor must go before the riders its native table points into.
        self.executor = session.executor;
        self.riders = session.riders;
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
            Command::ReplCommand(ref repl_command) => {
                let eval = self.eval_repl_command(repl_command.C());
                self.history.add_non_script_entry(command, eval.C());
                eval
            }
            Command::ScriptStatement(source) => {
                self.eval_script_statement(source)
            }
            Command::Expression(ref source) => {
                let eval = self.eval_expression(source.C());
                self.history.add_non_script_entry(command, eval.C());
                eval
            }
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
        // Compile the fragment.
        let compiled = self.compiler.compile_fragment(&source);

        // Check for parse errors.
        if let TypecheckResult::ParseError { errors } = &compiled.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for typecheck errors.
        if let TypecheckResult::Error { errors } = &compiled.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for ownership errors.
        if let OwnershipResult::Error { message } = &compiled.ownership {
            return Eval::Error(message.C());
        }

        // Check for lowering errors.
        if let LoweringResult::Error { message } = &compiled.lowering {
            return Eval::Error(message.C());
        }

        let ir_unit = compiled.ir_unit.as_ref()
            .expect("a fragment that compiled without errors has ir");
        let output = self.executor.execute_fragment(ir_unit);

        // Check for runtime errors.
        if output.starts_with("Error:") {
            return Eval::Error(output);
        }

        // Report the bindings the fragment defined, as the compiler recorded
        // them. A fragment that defines nothing, like a require or a set,
        // exports nothing.
        let exports = &ir_unit.script_context()
            .expect("a compiled fragment is a script unit")
            .exports;
        let bindings: Vec<EvalBinding> = exports.iter()
            .map(|(name, binding)| self.eval_binding(name, binding))
            .collect();

        if bindings.is_empty() {
            Eval::Nothing
        } else {
            Eval::Success(bindings)
        }
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
        // Compile the expression.
        let compiled = self.compiler.compile_expr(&source);

        // Check for parse errors.
        if let TypecheckResult::ParseError { errors } = &compiled.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for typecheck errors.
        if let TypecheckResult::Error { errors } = &compiled.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for ownership errors.
        if let OwnershipResult::Error { message } = &compiled.ownership {
            return Eval::Error(message.C());
        }

        // Check for lowering errors.
        if let LoweringResult::Error { message } = &compiled.lowering {
            return Eval::Error(message.C());
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

    /// Execute a script file line by line and output JSON results.
    pub fn run_script(db: &'db datafun::Database, script_path: &std::path::Path) -> AnyResult<()> {
        let mut engine = Self::new(db)?;
        let contents = std::fs::read_to_string(script_path)
            .context("failed to read script file")?;

        for line in contents.lines() {
            let parse_result = engine.parse_input(Input::Input(line.S()));
            let eval_result = match &parse_result {
                InputParse::Command(cmd) => Some(engine.eval(cmd.C())),
                _ => None,
            };

            let output = rmx::serde_json::json!({
                "input": line,
                "parse": parse_result,
                "eval": eval_result,
            });

            println!("{}", rmx::serde_json::to_string(&output)?);
        }

        // Engine's Drop impl will call destroy_all().
        Ok(())
    }
}

impl<'db> Drop for Engine<'db> {
    fn drop(&mut self) {
        self.executor.destroy_live_values();
    }
}
