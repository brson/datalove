//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;


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
    Nothing
}

pub struct Engine {
}

impl Command {
    pub fn parse(command: &str) -> CommandParse {
        CommandParse::Empty
    }
}

impl Engine {
    pub fn new() -> AnyResult<Engine> {
        Ok(Engine {})
    }

    pub fn eval(&mut self, command: Command) -> Eval {
        Eval::Nothing
    }
}
