//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;

const REPL_COMMAND_SIGIL: char = '\\';

#[derive(Debug)]
pub enum Command {
    ReplCommand(ReplCommand),
    ScriptStatement(ScriptStatement),
}

#[derive(Debug)]
pub enum ReplCommand {
    Exit,
    Help,
}

#[derive(Debug)]
pub struct ScriptStatement(String);

#[derive(Debug)]
pub enum CommandParse {
    Empty,
    ReadAnotherLine,
    Command(Command),
}

#[derive(Debug)]
pub enum Eval {
    Nothing,
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
        CommandParse::Command(
            Command::ReplCommand(ReplCommand::Exit),
        )
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

}
