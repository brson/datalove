//! The Datalove REPL evaluation engine.

#![allow(unused)]

use rmx::prelude::*;

#[derive(Debug)]
pub struct Command {
}

#[derive(Debug)]
pub struct Eval {
}

pub struct Engine {
}

impl Command {
    pub fn parse(command: String) -> AnyResult<Command> {
        Ok(Command {})
    }
}

impl Engine {
    pub fn new() -> AnyResult<Engine> {
        Ok(Engine {})
    }

    pub fn eval(&mut self, command: Command) -> AnyResult<Eval> {
        Ok(Eval {})
    }
}
