//! The Datalove REPL: what is typed at it, and the app that shows what it did.
//!
//! The units it evaluates are run by the script engine, which knows nothing
//! of the prompt, REPL commands, or the history.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

pub use datalove_script_engine as script;
use script::{Engine, Unit};

pub mod app;

// Executor implementations.
mod executor_threaded;

pub use executor_threaded::ThreadedExecutor;

mod batch;
pub use batch::{EnvBinding, InputResult, run_script, run_source};

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
            WorldfileSection::ScriptFragment { .. } => Command::Unit(Unit::Fragment(input.C())),
            WorldfileSection::ScriptExpr { .. } => Command::Unit(Unit::Expression(input.C())),
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    ReplCommand(ReplCommand),
    /// A script unit, for the engine.
    Unit(Unit),
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

/// What evaluating a command came to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Eval {
    /// What the engine made of a script unit.
    Unit(script::Eval),
    /// A REPL command, for the app to carry out.
    CallerInterpret(ReplCommand),
    /// A REPL command that could not be carried out.
    CommandError(String),
    /// The history was saved, as this many inputs, to this path.
    Saved { path: String, inputs: usize },
}

/// Evaluate a command against the engine.
///
/// A REPL command is the app's to carry out, except for what it asks of the
/// engine: a `/reset` empties the session here, in the order the commands
/// came in, so a unit submitted after it runs in the new session.
pub fn eval(engine: &mut Engine, command: Command) -> Eval {
    match command {
        Command::Unit(unit) => Eval::Unit(engine.eval(unit)),
        Command::ReplCommand(ReplCommand::Unknown) => Eval::CommandError("unknown command".S()),
        Command::ReplCommand(ReplCommand::Reset) => {
            engine.reset();
            Eval::CallerInterpret(ReplCommand::Reset)
        }
        Command::ReplCommand(command) => Eval::CallerInterpret(command),
    }
}

/// Read what an input is.
pub fn parse_input(input: Input) -> InputParse {
    match input {
        Input::Input(s) => parse_input_oneline(&s),
        Input::Multiline(s) => parse_input_multiline(&s),
    }
}

fn parse_input_oneline(input: &str) -> InputParse {
    match classify_input(input) {
        InputKind::Whitespace => InputParse::Empty,
        InputKind::ReplCommand => Command::repl_command(input),
        InputKind::OnelineStatement => Command::script_statement(input),
        InputKind::MultilineStatement => InputParse::ReadMultiline(S(input)),
        InputKind::OpenBraceTree => InputParse::ReadMultiline(S(input)),
        InputKind::Expression => Command::expression(input),
    }
}

fn parse_input_multiline(input: &str) -> InputParse {
    match classify_input(input) {
        InputKind::Whitespace => InputParse::Empty,
        InputKind::ReplCommand => Command::repl_command(input),
        InputKind::OnelineStatement => Command::script_statement(input),
        InputKind::MultilineStatement => Command::script_statement(input),
        InputKind::OpenBraceTree => todo!(),
        InputKind::Expression => Command::expression(input),
    }
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
    fn repl_command(command: &str) -> InputParse {
        InputParse::Command(Command::ReplCommand(ReplCommand::named(command)))
    }

    fn script_statement(input: &str) -> InputParse {
        InputParse::Command(Command::Unit(Unit::Fragment(S(input))))
    }

    fn expression(input: &str) -> InputParse {
        InputParse::Command(Command::Unit(Unit::Expression(S(input))))
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
    // A comment is not a command. Both begin with the sigil, and the one a
    // reader means by `// note to self` is the comment.
    let code = first_code_line(input);
    let is_whitespace = code.is_empty();
    let is_repl_command = !starts_a_comment(input.trim())
        && input.trim().starts_with(REPL_COMMAND_SIGIL);
    // Every word a statement can start with, as the parser's statement
    // dispatch has them. None of them starts an expression, so an input
    // starting with one is a statement; one that opens a block reads on to
    // its `end`.
    let is_oneline_statement_keyword = parse_ident(code).map(|ident| match ident {
        "let" | "var" | "const" | "set" | "call" | "require" | "import"
        | "ret" | "break" | "continue" | "debuglog" | "type" | "native" => true,
        _ => false
    }).unwrap_or(false);
    let is_multiline_statement_keyword = parse_ident(code).map(|ident| match ident {
        "fun" | "if" | "match" | "loop" | "for" => true,
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

/// The word `s` starts with, which is all of it when it is one word: a bare
/// `loop` opens a block as much as `loop while` does.
fn alphanumeric_prefix(s: &str) -> Option<&str> {
    let end = s.find(|c: char| !c.is_alphanumeric()).unwrap_or(s.len());
    (end > 0).then(|| &s[..end])
}
