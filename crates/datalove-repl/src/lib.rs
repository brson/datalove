//! The Datalove REPL evaluation engine.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

mod engine;
pub use engine::Engine;

pub mod app;

// Executor implementations.
mod executor_blocking;
mod executor_threaded;

pub use executor_blocking::BlockingExecutor;
pub use executor_threaded::ThreadedExecutor;

#[cfg(target_arch = "wasm32")]
mod executor_webworker;

#[cfg(target_arch = "wasm32")]
pub use executor_webworker::WebWorkerExecutor;

#[cfg(target_arch = "wasm32")]
pub mod worker;

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
    CrashReset(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    ReplCommand(ReplCommand),
    ScriptStatement(String),
    Expression(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplCommand {
    Unknown,
    Exit,
    Help,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalLet {
    pub name: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExpr {
    // Is it a variable binding, function call,
    // math expression, etc.
    pub expr_kind: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalFun {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Eval {
    Nothing,
    SuccessLet(EvalLet),
    SuccessExpr(EvalExpr),
    SuccessFun(EvalFun),
    Error(String),
    CallerInterpret(ReplCommand),
    CrashReset(String),
}


impl Command {
    pub(crate) fn repl_command(command: &str) -> InputParse {
        let command = &command.trim()[1..];
        let c = match command {
            "exit" => ReplCommand::Exit,
            "help" => ReplCommand::Help,
            _ => ReplCommand::Unknown,
        };
        InputParse::Command(Command::ReplCommand(c))
    }

    pub(crate) fn script_statement(input: &str) -> InputParse {
        InputParse::Command(Command::ScriptStatement(S(input)))
    }

    pub(crate) fn expression(input: &str) -> InputParse {
        InputParse::Command(Command::Expression(S(input)))
    }
}


pub(crate) enum InputKind {
    Whitespace,
    ReplCommand,
    OnelineStatement,
    MultilineStatement,
    OpenBraceTree,
    Expression,
}

pub(crate) fn classify_input(input: &str) -> InputKind {
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
