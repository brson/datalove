//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use bct::input::Source;

pub use datalove_datafun as datafun;

const REPL_COMMAND_SIGIL: char = '/';

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    ReplCommand(ReplCommand),
    ScriptStatement(ScriptStatement),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplCommand {
    Unknown,
    Exit,
    Help,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptStatement(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandParse {
    Empty,
    ReadAnotherLine,
    Command(Command),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Eval {
    Nothing,
    Error(String),
    CallerInterpret(ReplCommand),
}

pub struct Engine {
    /// The datafun database for salsa-based compilation.
    db: datafun::Database,
    /// The current script (Salsa input) tracking all submitted statements.
    /// None if no statements have been submitted yet.
    script: Option<datafun::script::Script>,
    /// Buffer for accumulating multiline input (e.g., fun...end fun).
    multiline_buffer: Vec<String>,
}

impl Command {
    pub fn parse(command: &str) -> CommandParse {
        let command = command.trim();
        if command.chars().next() == Some(REPL_COMMAND_SIGIL) {
            Self::repl_command(command)
        } else {
            Self::script_statement(command)
        }
    }

    fn repl_command(command: &str) -> CommandParse {
        let command = command[1..].trim();
        let c = match command {
            "exit" => ReplCommand::Exit,
            "help" => ReplCommand::Help,
            _ => ReplCommand::Unknown,
        };
        CommandParse::Command(Command::ReplCommand(c))
    }

    fn script_statement(command: &str) -> CommandParse {
        if command.trim().is_empty() {
            return CommandParse::Empty;
        }

        CommandParse::Command(Command::ScriptStatement(
            ScriptStatement(command.to_string())
        ))
    }
}

impl Engine {
    pub fn new() -> AnyResult<Engine> {
        Ok(Engine {
            db: datafun::Database::default(),
            script: None,
            multiline_buffer: Vec::new(),
        })
    }

    /// Parse a line of input and determine if we have a complete command.
    /// This handles multiline input for fun statements.
    pub fn parse_line(&mut self, line: &str) -> CommandParse {
        let line = line.trim_end();

        // Check if we're accumulating a multiline statement.
        if !self.multiline_buffer.is_empty() {
            // We're in multiline mode. Add this line and try to parse.
            self.multiline_buffer.push(line.to_string());
            let complete = self.multiline_buffer.join("\n");

            // Try to parse the accumulated buffer.
            let source = Source::new(&self.db, complete.clone().S());
            let parsed = datafun::parser::parse(&self.db, source);
            let statements = parsed.statements(&self.db);

            // Check if we have a Fun statement. If so, require "end fun" to be present.
            let has_fun = statements.iter().any(|s| matches!(s, datafun::ast::Statement::Fun(_)));
            let has_end_fun = self.check_has_end_fun(&complete);
            let has_errors = statements.iter().any(|s| matches!(s, datafun::ast::Statement::ParseError(_)));

            if has_fun && has_end_fun {
                self.multiline_buffer.clear();
                return Command::script_statement(&complete);
            } else {
                return CommandParse::ReadAnotherLine;
            }
        }

        // Not in multiline mode. Check what this line is.
        let trimmed = line.trim();

        // Check for REPL command.
        if trimmed.starts_with(REPL_COMMAND_SIGIL) {
            return Command::repl_command(trimmed);
        }

        // Check for empty line.
        if trimmed.is_empty() {
            return CommandParse::Empty;
        }

        // Try to parse this line to detect what kind of statement it is.
        let source = Source::new(&self.db, trimmed.to_string().S());
        let parsed = datafun::parser::parse(&self.db, source);
        let statements = parsed.statements(&self.db);

        // Check if this is a Fun statement (which requires multiline input in REPL).
        let has_fun = statements.iter().any(|s| matches!(s, datafun::ast::Statement::Fun(_)));

        if has_fun {
            // Function statement. Start multiline mode.
            self.multiline_buffer.push(line.to_string());
            return CommandParse::ReadAnotherLine;
        } else {
            // Not a function, return it (whether complete or error).
            return Command::script_statement(trimmed);
        }
    }

    /// Check if the source text contains "end fun" using the lexer.
    // fixme just do the full statement parse comeon.
    fn check_has_end_fun(&self, source_text: &str) -> bool {
        let source = Source::new(&self.db, source_text.to_string().S());
        let chunk = bct::source_map::basic_source_map(&self.db, source);
        let chunk_lex = bct::lexer::lex_chunk(&self.db, chunk);

        // Filter out whitespace tokens to get just words and sigils.
        let tokens: Vec<_> = chunk_lex.tokens(&self.db).iter()
            .filter(|t| !matches!(t.kind(&self.db), bct::lexer::TokenKind::Whitespace))
            .collect();

        // Look for consecutive "end" and "fun" tokens.
        for i in 0..tokens.len().saturating_sub(1) {
            let t1 = tokens[i];
            let t2 = tokens[i + 1];
            if let (Some("end"), Some("fun")) = (t1.word_str(&self.db), t2.word_str(&self.db)) {
                return true;
            }
        }
        false
    }

    pub fn eval(&mut self, command: Command) -> Eval {
        match command {
            Command::ReplCommand(command) => {
                self.eval_repl_command(command)
            }
            Command::ScriptStatement(source) => {
                self.eval_script_statement(source)
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

    fn eval_script_statement(&mut self, source: ScriptStatement) -> Eval {
        let source_text = source.0;
        let db = &self.db;

        // Create Source input.
        let source_input = Source::new(db, source_text.S());

        // Create new ScriptUnit input.
        let new_unit = datafun::script::ScriptUnit::new(db, source_input);

        // Get current units or start with empty vec.
        let current_units = match self.script {
            Some(script) => script.units(db).clone(),
            None => vec![],
        };

        // Create new Script with the new unit appended.
        let mut updated_units = current_units;
        updated_units.push(new_unit);
        let new_script = datafun::script::Script::new(db, updated_units);

        // Store the new script.
        self.script = Some(new_script);

        // Get the index of the new unit.
        let unit_index = new_script.units(db).len() - 1;

        // Parse the new unit.
        let parsed = datafun::parser::parse_script_unit(db, new_script, unit_index);
        let statements = parsed.statements(db);

        if statements.is_empty() {
            return Eval::Error("no statements parsed".to_string());
        }

        // Check for parse errors.
        for stmt in statements {
            if let datafun::ast::Statement::ParseError(err) = stmt {
                let msg = err.message(db).as_str(db).to_string();
                return Eval::Error(format!("parse error: {}", msg));
            }
        }

        // Run resolution to check if the unit is valid.
        // Determine if it's a function or let statement.
        let has_fun = statements.iter().any(|s| matches!(s, datafun::ast::Statement::Fun(_)));
        let has_let = statements.iter().any(|s| matches!(s, datafun::ast::Statement::Let(_)));

        let fun_resolution = datafun::resolution::resolve_functions(db, new_script);
        let let_resolution = datafun::resolution::resolve_let_statement(db, new_script, unit_index);

        // Successfully parsed and compiled.
        Eval::Nothing
    }

    /// Get current environment bindings (functions and let statements).
    /// Returns a list of (name, description) pairs.
    pub fn get_environment(&self) -> Vec<(String, String)> {
        let mut bindings = Vec::new();

        let Some(script) = self.script else {
            return bindings;
        };

        let db = &self.db;

        // Get resolved function units.
        let fun_resolution = datafun::resolution::resolve_functions(db, script);
        let green_units = fun_resolution.green_units(db);

        // Extract function names from green units.
        for &fun_idx in green_units {
            let parsed = datafun::parser::parse_script_unit(db, script, fun_idx);
            for stmt in parsed.statements(db) {
                if let datafun::ast::Statement::Fun(fun) = stmt {
                    let name = fun.name(db).as_str(db).to_string();
                    bindings.push((name, "function".to_string()));
                }
            }
        }

        // Extract let bindings from all units.
        let units = script.units(db);
        for unit_idx in 0..units.len() {
            let parsed = datafun::parser::parse_script_unit(db, script, unit_idx);
            for stmt in parsed.statements(db) {
                if let datafun::ast::Statement::Let(let_stmt) = stmt {
                    let name = let_stmt.name(db).as_str(db).to_string();
                    bindings.push((name, "let (not evaluated)".to_string()));
                }
            }
        }

        bindings
    }

    /// Execute a script file line by line and output JSON results.
    pub fn run_script(script_path: &std::path::Path) -> AnyResult<()> {
        let mut engine = Self::new()?;
        let contents = std::fs::read_to_string(script_path)
            .context("failed to read script file")?;

        for line in contents.lines() {
            // Skip empty lines.
            if line.trim().is_empty() {
                continue;
            }

            let input = line.to_string();
            let parse_result = Command::parse(&input);
            let eval_result = match &parse_result {
                CommandParse::Command(cmd) => Some(engine.eval(cmd.clone())),
                _ => None,
            };

            let output = serde_json::json!({
                "input": input,
                "parse": parse_result,
                "eval": eval_result,
            });

            println!("{}", serde_json::to_string(&output)?);
        }

        Ok(())
    }

}
