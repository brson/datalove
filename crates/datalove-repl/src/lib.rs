//! The Datalove REPL evaluation engine.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

mod engine;
pub use engine::{Engine, EnvBinding, InputResult};

pub mod app;

// Executor implementations.
mod executor_threaded;

pub use executor_threaded::ThreadedExecutor;

// The system library a driver supplies to the engine.
pub use datalove_datafun::pipeline::SystemLibrary;

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

/// A binding a script fragment defined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvalBinding {
    /// A `let` or `const` binding.
    Value { name: String, ty: String, value: String },
    /// A `var` binding.
    Slot { name: String, ty: String, value: String },
    /// A function definition.
    Function { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExpr {
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Eval {
    Nothing,
    /// The bindings a script fragment defined, in source order.
    Success(Vec<EvalBinding>),
    SuccessExpr(EvalExpr),
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
    // A comment is not a command. Both begin with the sigil, and the one a
    // reader means by `// note to self` is the comment.
    let code = first_code_line(input);
    let is_whitespace = code.is_empty();
    let is_repl_command = !starts_a_comment(input.trim())
        && input.trim().starts_with(REPL_COMMAND_SIGIL);
    let is_oneline_statement_keyword = parse_ident(code).map(|ident| match ident {
        "let" | "var" | "const" | "set" | "call" | "require" | "import" => true,
        _ => false
    }).unwrap_or(false);
    let is_multiline_statement_keyword = parse_ident(code).map(|ident| match ident {
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

/// True if a line opens a comment rather than naming a command.
fn starts_a_comment(line: &str) -> bool {
    line.starts_with("//") || line.starts_with("/*")
}

/// The first line with something on it other than a comment.
///
/// What an input is, is decided by the first line of code in it, so that a
/// note written above a definition does not change what the definition is
/// read as. Empty when the input is nothing but blank lines and comments,
/// which is an input with nothing to do.
///
/// A block comment is only recognised where it opens and closes on one line.
/// One spanning several would need the nesting the lexer tracks, and the
/// answer for it is the same either way: whatever it is, it is not a command.
fn first_code_line(input: &str) -> &str {
    input.lines()
        .map(|line| line.trim())
        .find(|line| !line.is_empty() && !is_whole_line_comment(line))
        .unwrap_or("")
}

/// True if a line is a comment and nothing else.
fn is_whole_line_comment(line: &str) -> bool {
    line.starts_with("//") || (line.starts_with("/*") && line.ends_with("*/"))
}

fn parse_ident(input: &str) -> Option<&str> {
    alphanumeric_prefix(input.trim())
}

fn alphanumeric_prefix(s: &str) -> Option<&str> {
    s.find(|c: char| !c.is_alphanumeric())
        .map(|pos| &s[..pos])
}
