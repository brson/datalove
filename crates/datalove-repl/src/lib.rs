//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;
use serde::{Serialize, Deserialize};
use bct::input::Source;

pub use datalove_datafun as datafun;

const REPL_COMMAND_SIGIL: char = '/';

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Input {
    Input(String),
    Multiline(String),
}
    
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputParse {
    Empty,
    ReadMultiline(String),
    Command(Command),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    ReplCommand(ReplCommand),
    ScriptStatement(ScriptStatement),
    Expression(String),
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
}

impl Command {
    fn repl_command(command: &str) -> InputParse {
        let command = &command.trim()[1..];
        let c = match command {
            "exit" => ReplCommand::Exit,
            "help" => ReplCommand::Help,
            _ => ReplCommand::Unknown,
        };
        InputParse::Command(Command::ReplCommand(c))
    }

    fn script_statement(input: &str) -> InputParse {
        InputParse::Command(Command::ScriptStatement(
            ScriptStatement(S(input))
        ))
    }

    fn expression(input: &str) -> InputParse {
        InputParse::Command(Command::Expression(S(input)))
    }
}

impl Engine {
    pub fn new() -> AnyResult<Engine> {
        Ok(Engine {
            db: datafun::Database::default(),
            script: None,
        })
    }

    pub fn parse_input(&mut self, input: Input) -> InputParse {
        match input {
            Input::Input(s) => self.parse_input_oneline(&s),
            Input::Multiline(s) => self.parse_input_multiline(&s),
        }
    }

    pub fn parse_input_oneline(&mut self, input: &str) -> InputParse {
        match classify_input(input) {
            InputKind::Whitespace => InputParse::Empty,
            InputKind::ReplCommand => Command::repl_command(input),
            InputKind::OnelineStatement => Command::script_statement(input),
            InputKind::MultilineStatement => InputParse::ReadMultiline(S(input)),
            InputKind::OpenBraceTree => InputParse::ReadMultiline(S(input)),
            InputKind::Expression => Command::expression(input),
        }
    }

    pub fn parse_input_multiline(&mut self, input: &str) -> InputParse {
        match classify_input(input) {
            InputKind::Whitespace => InputParse::Empty,
            InputKind::ReplCommand => Command::repl_command(input),
            InputKind::OnelineStatement => Command::script_statement(input),
            InputKind::MultilineStatement => Command::script_statement(input),
            InputKind::OpenBraceTree => todo!(),
            InputKind::Expression => Command::expression(input),
        }
    }

    pub fn eval(&mut self, command: Command) -> Eval {
        match command {
            Command::ReplCommand(command) => {
                self.eval_repl_command(command)
            }
            Command::ScriptStatement(source) => {
                self.eval_script_statement(source)
            }
            Command::Expression(source) => {
                self.eval_expression(source)
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
        let fun_resolution = datafun::resolution::resolve_functions(db, new_script);
        let let_resolution = datafun::resolution::resolve_let_statement(db, new_script, unit_index);
        //todo check resolution

        // todo run typechecker

        // Execute the statement using the interpreter.
        // The context is dropped after execution, which frees all values.
        // For persistent state, we'd need to store the context or its components.
        let result = self.execute_with_interpreter(new_script);
        match result {
            Ok(_ctx) => Eval::Nothing,
            Err(e) => Eval::Error(format!("execution error: {:?}", e)),
        }
    }

    /// Execute or re-execute the full script using the interpreter.
    ///
    /// Returns the InterpContext after execution, which the caller must handle.
    fn execute_with_interpreter(
        &self,
        script: datafun::script::Script,
    ) -> Result<datafun::interp::InterpContext<'_>, datafun::interp::InterpError> {
        let db = &self.db;

        // Parse all units and collect their statements into a single ast::Script.
        let mut all_statements = Vec::new();
        let units = script.units(db);
        for unit_idx in 0..units.len() {
            let parsed_unit = datafun::parser::parse_script_unit(db, script, unit_idx);
            all_statements.extend(parsed_unit.statements(db).iter().cloned());
        }

        let parsed_script = datafun::ast::Script::new(db, all_statements);

        // Type check the script.
        let tycheck_result = datafun::tycheck::type_check(db, parsed_script);
        if !tycheck_result.errors(db).is_empty() {
            return Err(datafun::interp::InterpError::TypeError(
                format!("{} type error(s)", tycheck_result.errors(db).len())
            ));
        }

        // Build type table.
        let type_table = datafun::type_table::TypeTable::build(db, parsed_script, tycheck_result)
            .map_err(|e| datafun::interp::InterpError::RuntimeError(format!("failed to build type table: {}", e)))?;

        // Create interpreter context.
        let mut ctx = datafun::interp::InterpContext::new(db, type_table);

        // Execute the full script.
        ctx.execute(parsed_script)?;

        Ok(ctx)
    }

    fn eval_expression(&mut self, source: String) -> Eval {
        // Wrap the expression in a let statement with a temporary variable.
        let temp_var = "_expr_result";
        let let_statement = format!("let {} = {}", temp_var, source);

        // Evaluate as a script statement.
        let result = self.eval_script_statement(ScriptStatement(let_statement));

        // If execution succeeded, re-execute to get the context and pretty-print.
        match result {
            Eval::Nothing => {
                // Re-execute to get the context with the temp variable.
                if let Some(script) = self.script {
                    match self.execute_with_interpreter(script) {
                        Ok(mut ctx) => {
                            let db = &self.db;
                            let temp_name = bct::text::InternedText::new(db, S(temp_var));

                            match ctx.pretty_print_variable(temp_name) {
                                Ok(s) => Eval::Error(s), // Using Error variant to display the result.
                                Err(e) => Eval::Error(format!("failed to pretty-print: {:?}", e)),
                            }
                        }
                        Err(e) => Eval::Error(format!("execution error: {:?}", e)),
                    }
                } else {
                    Eval::Error("no script to execute".to_string())
                }
            }
            other => other,
        }
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

        // Extract let bindings from the script.
        let units = script.units(db);
        for unit_idx in 0..units.len() {
            let parsed = datafun::parser::parse_script_unit(db, script, unit_idx);
            for stmt in parsed.statements(db) {
                if let datafun::ast::Statement::Let(let_stmt) = stmt {
                    let name = let_stmt.name(db).as_str(db).to_string();
                    bindings.push((name, "let".to_string()));
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

enum InputKind {
    Whitespace,
    ReplCommand,
    OnelineStatement,
    MultilineStatement,
    OpenBraceTree,
    Expression,
}

fn classify_input(input: &str) -> InputKind {
    let is_whitespace = input.chars().all(char::is_whitespace);
    let is_repl_command = input.trim().starts_with(REPL_COMMAND_SIGIL);
    let is_oneline_statement_keyword = parse_ident(input).map(|ident| match ident {
        "let" => true,
        _ => false
    }).unwrap_or(false);
    let is_multiline_statement_keyword = parse_ident(input).map(|ident| match ident {
        "fun" => true,
        _ => false
    }).unwrap_or(false);
    let is_open_brace_tree = false; // todo

    if is_whitespace {
        InputKind::Whitespace
    } else if is_repl_command {
        InputKind::ReplCommand
    } else if is_oneline_statement_keyword {
        InputKind::OnelineStatement
    } else if is_multiline_statement_keyword {
        InputKind::MultilineStatement
    } else if is_open_brace_tree {
        InputKind::OpenBraceTree
    } else {
        InputKind::Expression
    }
}

fn parse_ident(input: &str) -> Option<&str> {
    alphanumeric_prefix(input.trim())
}

fn alphanumeric_prefix(s: &str) -> Option<&str> {
    s.find(|c: char| !c.is_alphanumeric())
        .map(|pos| &s[..pos])
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: The REPL engine tests require being called from within a Salsa tracked function context
    // because they create ScriptUnits dynamically. Tests of the interpreter integration should be
    // done at a higher level (e.g., integration tests or through actual REPL usage).
} 
