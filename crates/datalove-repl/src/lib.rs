//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

const REPL_COMMAND_SIGIL: char = '\\';

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

#[derive(Debug, Serialize, Deserialize)]
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
        CommandParse::Empty
    }
}

impl Engine {
    pub fn new() -> AnyResult<Engine> {
        Ok(Engine {})
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
