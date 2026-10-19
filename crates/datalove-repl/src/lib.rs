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
pub use datalove_datafun::pipeline::{PackageLibrary, SystemLibrary};

const REPL_COMMAND_SIGIL: char = '/';

/// The worldfile `/save` writes in the workspace root.
pub const SESSION_FILE_NAME: &str = "repl-session.world";

/// One input of a saved session: the text as it was typed, and what it was.
#[derive(Debug, Clone)]
pub struct SavedInput {
    pub input: String,
    pub command: Command,
}

/// The inputs of a session a worldfile holds, in the order they were typed.
///
/// A session is script units and REPL commands and nothing else, so a
/// worldfile with modules, data, or riders in it is not one. Its `/save`s are
/// left out: replaying one would overwrite the file being loaded with only
/// the part of it read so far.
pub fn load_session(path: &std::path::Path) -> AnyResult<Vec<SavedInput>> {
    use datalove_datafun::package_load_worldfile::{WorldfileSection, parse_worldfile_sections};

    let file = std::fs::File::open(path)
        .with_context(|| fmt!("failed to open {}", path.display()))?;
    let parsed = parse_worldfile_sections(file)
        .with_context(|| fmt!("failed to read {}", path.display()))?;
    let mut inputs = Vec::new();
    for section in parsed.sections {
        let input = section.source().unwrap_or_default().trim().S();
        let command = match section {
            WorldfileSection::ScriptFragment { .. } => Command::ScriptStatement(input.C()),
            WorldfileSection::ScriptExpr { .. } => Command::Expression(input.C()),
            WorldfileSection::ReplCommand { .. } => {
                if !input.starts_with(REPL_COMMAND_SIGIL) {
                    bail!("{} holds a REPL command that is not one: '{input}'", path.display());
                }
                match ReplCommand::named(&input) {
                    ReplCommand::Save => continue,
                    command => Command::ReplCommand(command),
                }
            }
            other => bail!(
                "{} holds a '{}' section, and a REPL session is script units and \
                 REPL commands only",
                path.display(),
                other.header(),
            ),
        };
        inputs.push(SavedInput { input, command });
    }
    Ok(inputs)
}

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
    /// Start the session again, forgetting everything it defined.
    Reset,
    /// Write the history to [`SESSION_FILE_NAME`] in the workspace root.
    Save,
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
    /// A runtime error, after which the session is empty.
    ///
    /// An error frees everything the runtime allocated rather than unwinding
    /// a value at a time, and every binding the session had was a value the
    /// runtime allocated, so the session starts again.
    ErrorReset(String),
    /// The history was saved, as this many inputs, to this path.
    Saved { path: String, inputs: usize },
}


impl ReplCommand {
    /// The command an input starting with the sigil names.
    fn named(input: &str) -> ReplCommand {
        match &input.trim()[1..] {
            "exit" => ReplCommand::Exit,
            "help" => ReplCommand::Help,
            "reset" => ReplCommand::Reset,
            "save" => ReplCommand::Save,
            _ => ReplCommand::Unknown,
        }
    }
}

impl Command {
    pub(crate) fn repl_command(command: &str) -> InputParse {
        InputParse::Command(Command::ReplCommand(ReplCommand::named(command)))
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
