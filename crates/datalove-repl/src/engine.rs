//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;

use crate::{Command, ReplCommand, Eval, EvalLet, EvalExpr, EvalFun, InputParse, Input};
use datalove_datafun as datafun;
use datafun::pipeline::{ModuleCompilationPipeline, ScriptCompilationContext, TypecheckResult, LoweringResult};

pub struct Engine<'db> {
    db: &'db datafun::Database,
    history: ReplHistory,
    /// Script compilation context for incremental evaluation.
    ctx: ScriptCompilationContext<'db>,
}

struct ReplHistory {
    entries: Vec<HistoryEntry>,
}

struct HistoryEntry {
    _command: Command,
    _last_eval: Eval,
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
        // Create an empty module pipeline and compile.
        let mut pipeline = ModuleCompilationPipeline::new();
        let compiled = pipeline.compile_fresh(db);

        // Check for errors.
        if compiled.has_errors() {
            let errors = compiled.all_errors();
            bail!("Module compilation failed: {}", errors.join("; "));
        }

        // Create script compilation context.
        let ctx = compiled.script_context(db, datalove_datafun::DebugOutputMode::Disabled);

        Ok(Engine {
            db,
            history: ReplHistory::new(),
            ctx,
        })
    }

    fn reset(&mut self) {
        self.history = ReplHistory::new();
        // Cleanup the current context.
        self.ctx.destroy_all();
        // Create a new context.
        let mut pipeline = ModuleCompilationPipeline::new();
        let compiled = pipeline.compile_fresh(self.db);
        self.ctx = compiled.script_context(self.db, datalove_datafun::DebugOutputMode::Disabled);
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
        let result = self.ctx.eval_fragment(&source);

        // Check for parse errors.
        if let TypecheckResult::ParseError { errors } = &result.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for typecheck errors.
        if let TypecheckResult::Error { errors } = &result.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for lowering errors.
        if let LoweringResult::Error { message } = &result.lowering {
            return Eval::Error(message.C());
        }

        // Check for runtime errors.
        if result.output.starts_with("Error:") {
            return Eval::Error(result.output);
        }

        // Detect what kind of statement was evaluated.
        let trimmed = source.trim();
        if trimmed.starts_with("let ") {
            // Extract let binding name (simple parsing).
            let after_let = &trimmed[4..];
            let name = after_let.split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?")
                .S();

            // Look up type and value from the context.
            let (ty, value) = self.ctx.get_binding(&name)
                .unwrap_or(("?".S(), "?".S()));

            Eval::SuccessLet(EvalLet { name, ty, value })
        } else if trimmed.starts_with("fun ") {
            // Extract function name.
            let after_fun = &trimmed[4..];
            let name = after_fun.split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?")
                .S();
            Eval::SuccessFun(EvalFun { name })
        } else if trimmed.starts_with("var ") {
            // Extract var binding name.
            let after_var = &trimmed[4..];
            let name = after_var.split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .unwrap_or("?")
                .S();

            // Look up type and value from the context.
            let (ty, value) = self.ctx.get_binding(&name)
                .unwrap_or(("?".S(), "?".S()));

            Eval::SuccessLet(EvalLet { name, ty, value })
        } else {
            Eval::Nothing
        }
    }

    fn eval_expression(&mut self, source: String) -> Eval {
        let result = self.ctx.eval_expr(&source);

        // Check for parse errors.
        if let TypecheckResult::ParseError { errors } = &result.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for typecheck errors.
        if let TypecheckResult::Error { errors } = &result.typecheck {
            return Eval::Error(errors.join("; "));
        }

        // Check for lowering errors.
        if let LoweringResult::Error { message } = &result.lowering {
            return Eval::Error(message.C());
        }

        // Check for runtime errors.
        if result.output.starts_with("Error:") {
            return Eval::Error(result.output);
        }

        Eval::SuccessExpr(EvalExpr {
            expr_kind: "expr".S(),
            ty: result.ty.unwrap_or_else(|| "?".S()),
            value: result.output,
        })
    }

    /// Get current environment bindings (functions and let statements).
    ///
    /// Returns a list of (name, type, value) triples, sorted by name.
    pub fn get_environment(&mut self) -> Vec<(String, String, String)> {
        // Delegate to the script context's get_environment method.
        self.ctx.get_environment()
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

            let output = serde_json::json!({
                "input": line,
                "parse": parse_result,
                "eval": eval_result,
            });

            println!("{}", serde_json::to_string(&output)?);
        }

        // Engine's Drop impl will call destroy_all().
        Ok(())
    }
}

impl<'db> Drop for Engine<'db> {
    fn drop(&mut self) {
        self.ctx.destroy_all();
    }
}
