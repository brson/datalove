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

#[derive(Debug, Serialize, Deserialize)]
pub enum Eval {
    Nothing,
    Error(String),
    Exit,
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
            Self::parse_repl_command(command)
        } else {
            Self::parse_script_statement(command)
        }
    }

    pub fn parse_repl_command(command: &str) -> CommandParse {
        let command = command[1..].trim();
        let c = match command {
            "exit" => ReplCommand::Exit,
            "help" => ReplCommand::Help,
            _ => ReplCommand::Unknown,
        };
        CommandParse::Command(Command::ReplCommand(c))
    }

    pub fn parse_script_statement(command: &str) -> CommandParse {
        if command.trim().is_empty() {
            return CommandParse::Empty;
        }

        // For now, assume single-line statements.
        // We'll handle multiline later.
        // Just wrap it in a ScriptStatement.
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
            // We're in multiline mode. Check if this line ends it.
            // fixme use the actual parser
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.len() >= 2 && words[0] == "end" && words[1] == "fun" {
                // End of multiline statement. Add this line and return complete statement.
                self.multiline_buffer.push(line.to_string());
                let complete = self.multiline_buffer.join("\n");
                self.multiline_buffer.clear();
                return Command::parse_script_statement(&complete);
            } else {
                // Still accumulating.
                self.multiline_buffer.push(line.to_string());
                return CommandParse::ReadAnotherLine;
            }
        }

        // Not in multiline mode. Check what this line is.
        let trimmed = line.trim();

        // Check for REPL command.
        if trimmed.starts_with(REPL_COMMAND_SIGIL) {
            return Command::parse_repl_command(trimmed);
        }

        // Check for empty line.
        if trimmed.is_empty() {
            return CommandParse::Empty;
        }

        // Check if this starts a multiline statement (fun).
        // fixme use the parser!
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if !words.is_empty() && words[0] == "fun" {
            // Start multiline mode.
            self.multiline_buffer.push(line.to_string());
            return CommandParse::ReadAnotherLine;
        }

        // Single-line statement.
        Command::parse_script_statement(trimmed)
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
                Eval::Exit
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
