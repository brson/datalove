//! REPL engine for evaluating Datalove expressions and statements.

use rmx::prelude::*;
use std::collections::BTreeMap;

use crate::{Command, ReplCommand, Eval, InputParse, Input};
use crate::datafun;

pub struct Engine<'db> {
    _db: &'db dyn datafun::Db,
    history: ReplHistory,
    /// Module graph for the REPL (empty for now).
    _module_graph: datafun::module_graph::ModuleGraph,
    /// Typecheck result for the module graph.
    _typecheck_result: datafun::module_graph::ModuleGraphTypecheckResult<'db>,
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
    pub fn new(db: &'db dyn datafun::Db) -> AnyResult<Engine<'db>> {
        // Create empty package world.
        let empty_package_world = datafun::package_load::PackageWorld {
            pkglib_system: BTreeMap::new(),
            pkglib_local: BTreeMap::new(),
        };
        let package_world = datafun::package::import_from_loader(db, empty_package_world);

        // Resolve and convert to ModuleGraph.
        let resolution = datafun::package_resolve::resolve_package_world_with_imports(db, package_world);
        let pkg_graph = resolution.result(db)
            .map_err(|e| rmx::anyhow::anyhow!("Package resolution failed: {:?}", e))?;

        // Convert to package-agnostic ModuleGraph and typecheck.
        let module_graph = datafun::to_module_graph(db, package_world, pkg_graph);
        let typecheck_result = datafun::tycheck::typecheck_module_graph(db, module_graph);

        Ok(Engine {
            _db: db,
            history: ReplHistory::new(),
            _module_graph: module_graph,
            _typecheck_result: typecheck_result,
        })
    }

    fn reset(&mut self) {
        self.history = ReplHistory::new();
    }

    pub fn parse_input(&mut self, input: Input) -> InputParse {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.parse_input_impl(input.clone())
        }));

        match result {
            Ok(parse_result) => parse_result,
            Err(panic_info) => {
                self.reset();
                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
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
            self.eval_impl(command.clone())
        }));

        match result {
            Ok(eval_result) => eval_result,
            Err(panic_info) => {
                self.reset();
                let panic_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
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
                Eval::Error("unknown command".to_string())
            }
            ReplCommand::Help => {
                Eval::CallerInterpret(command)
            }
            ReplCommand::Exit => {
                Eval::CallerInterpret(command)
            }
        }
    }

    fn eval_script_statement(&mut self, _source: String) -> Eval {
        todo!("script interpreter gutted - pending frame-based rewrite")
    }

    fn eval_expression(&mut self, _source: String) -> Eval {
        todo!("script interpreter gutted - pending frame-based rewrite")
    }

    /// Get current environment bindings (functions and let statements).
    /// Returns a list of (name, type, value) triples.
    pub fn get_environment(&mut self) -> Vec<(String, String, String)> {
        // TODO: Implement with frame-based interpreter.
        Vec::new()
    }

    /// Execute a script file line by line and output JSON results.
    pub fn run_script(db: &'db dyn datafun::Db, script_path: &std::path::Path) -> AnyResult<()> {
        let mut engine = Self::new(db)?;
        let contents = std::fs::read_to_string(script_path)
            .context("failed to read script file")?;

        for line in contents.lines() {
            let parse_result = engine.parse_input(Input::Input(line.to_string()));
            let eval_result = match &parse_result {
                InputParse::Command(cmd) => Some(engine.eval(cmd.clone())),
                _ => None,
            };

            let output = serde_json::json!({
                "input": line,
                "parse": parse_result,
                "eval": eval_result,
            });

            println!("{}", serde_json::to_string(&output)?);
        }

        Ok(())
    }
} 
