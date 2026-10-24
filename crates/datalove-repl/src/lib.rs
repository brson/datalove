//! The Datalove REPL: what is typed at it, and the app that shows what it did.
//!
//! The units it evaluates are run by the script engine, which knows nothing
//! of the prompt, REPL commands, or the history.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

pub use datalove_script_engine as script;
use script::Unit;

pub mod app;

// Executor implementations.
mod executor_threaded;

pub use executor_threaded::ThreadedExecutor;

mod batch;
pub use batch::{InputResult, run_script, run_source};

mod session;
pub use session::{NumberedUnit, Session};

// The system library a driver supplies to the engine.
pub use script::{PackageLibrary, SystemLibrary};

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
/// the part of it read so far. So are its `/load`s, which would load the file
/// again, and again, and its `/help`s, which change nothing but what is shown,
/// and would show the help to someone who did not ask.
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
                if !matches!(classify_input(&input), InputKind::ReplCommand) {
                    bail!("{} holds a REPL command that is not one: '{input}'", path.display());
                }
                match ReplCommand::named(&input) {
                    ReplCommand::Save | ReplCommand::Load | ReplCommand::Help => continue,
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
    /// A command named with arguments it does not take, and how it is used.
    Usage(String),
    Exit,
    Help,
    /// Start the session again, forgetting everything it defined.
    Reset,
    /// Write the history to [`SESSION_FILE_NAME`] in the workspace root.
    Save,
    /// Start the session again as [`SESSION_FILE_NAME`] has it, as
    /// `repl --load` does.
    Load,
    /// Replace the source of the unit with this number, keeping its kind:
    /// `/edit N <source>`.
    Edit { unit: u32, source: Unit },
    /// Remove the unit with this number: `/delete N`.
    Delete { unit: u32 },
    /// Add a unit before or after the one with this number:
    /// `/insert before N <source>` or `/insert after N <source>`.
    Insert { place: Place, unit: u32, source: Unit },
    /// Take back the last change to the session: `/undo`.
    Undo,
    /// Make again the last change taken back: `/redo`.
    Redo,
    /// Put a unit back with the number it had, after the unit with the
    /// number `after`, or first.
    ///
    /// What undoing a delete, or redoing an append or an insert, asks of the
    /// session. Nothing reads it from input.
    Restore { unit: u32, after: Option<u32>, source: Unit },
    /// Say what the session has to say about the unit with this number.
    ///
    /// What the app asks to show the selected unit's detail. Nothing reads it
    /// from input.
    Describe { unit: u32 },
    /// Number the next unit made `next`.
    ///
    /// What dropping changes taken back asks of the session, so the numbers
    /// they took are given again, as a replay of the history would. Nothing
    /// reads it from input.
    Renumber { next: u32 },
}

/// Which side of a unit `/insert` puts the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Place {
    Before,
    After,
}

impl Place {
    /// Where a unit inserted on this side of the unit at `index` goes.
    pub fn index(self, index: usize) -> usize {
        match self {
            Place::Before => index,
            Place::After => index + 1,
        }
    }
}

/// What evaluating a command came to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Outcome {
    /// What the engine made of a script unit.
    Unit(script::Eval),
    /// The session was reset by `/reset`.
    Reset,
    /// The help was shown.
    Help,
    /// A saved session was read, and these many of its inputs are to be
    /// replayed.
    Loaded { inputs: usize },
    /// What the session has to say about a unit.
    Detail(UnitDetail),
    /// A REPL command that could not be carried out.
    CommandError(String),
    /// The history was saved, as this many inputs, to this path.
    Saved { path: String, inputs: usize },
    /// What an `/edit`, `/delete` or `/insert` did to each unit it compiled
    /// or re-derived, in the session's order.
    Edited(Vec<UnitStep>),
}

impl Outcome {
    /// Whether a runtime error or a crash emptied the session.
    pub fn reset_the_session(&self) -> bool {
        match self {
            Outcome::Unit(eval) => eval.resets(),
            Outcome::Edited(step) => step.iter().any(|unit| unit.eval.as_ref().is_some_and(script::Eval::resets)),
            _ => false,
        }
    }

    /// Whether the session is empty after this: a `/reset`, or a runtime
    /// error or a crash.
    pub fn empties_session(&self) -> bool {
        self.reset_the_session() || matches!(self, Outcome::Reset)
    }
}

/// What the session has to say about one unit, beyond its source and how it
/// fared, which the app keeps itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitDetail {
    /// The unit's number.
    pub unit: u32,
    /// Its parse, type and ownership diagnostics as they stand, rendered as
    /// text; empty for a unit with none.
    pub diagnostics: String,
    /// What it binds, and what each binding holds now; `None` for one given
    /// away since.
    pub bindings: Vec<(String, Option<String>)>,
    /// The names it provides the units after it.
    pub provides: Vec<String>,
    /// The earlier units' bindings it only reads.
    pub reads: Vec<String>,
    /// The earlier units' bindings it gives away.
    pub takes: Vec<String>,
    /// The earlier units' bindings it writes over whatever they held.
    pub overwrites: Vec<String>,
    /// The earlier units' bindings it changes, needing what they held.
    pub updates: Vec<String>,
    /// The bindings an earlier unit gave away that it gives a value again.
    pub revives: Vec<String>,
}

/// What a step did to one unit, which the REPL names by number.
///
/// The engine's [`script::UnitStep`], with the unit's number for its
/// position, and the positions in a `Blocked` turned into numbers too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStep {
    /// The unit's number.
    pub unit: u32,
    /// Whether the step compiled the unit again.
    pub compiled: bool,
    /// What re-deriving the unit came to, if the step re-derived it.
    pub eval: Option<script::Eval>,
}

impl UnitStep {
    /// Whether the step ran the unit.
    pub fn ran(&self) -> bool {
        self.eval.as_ref().is_some_and(script::Eval::was_run)
    }
}

/// Read what an input is.
///
/// A single line that opens a block reads on to its `end`; the same line
/// submitted as multiline input is the whole unit.
pub fn parse_input(input: Input) -> InputParse {
    let (text, multiline) = match input {
        Input::Input(text) => (text, false),
        Input::Multiline(text) => (text, true),
    };
    match classify_input(&text) {
        InputKind::Whitespace => InputParse::Empty,
        InputKind::ReplCommand => InputParse::Command(Command::ReplCommand(ReplCommand::named(&text))),
        InputKind::MultilineStatement if !multiline => InputParse::ReadMultiline(text),
        _ => InputParse::Command(Command::Unit(read_unit(&text).expect("the text is code"))),
    }
}

/// The unit a source is, a fragment or an expression, as typed input is
/// read; `None` for one with no code in it, or a REPL command.
fn read_unit(source: &str) -> Option<Unit> {
    match classify_input(source) {
        InputKind::Whitespace | InputKind::ReplCommand => None,
        InputKind::Expression => Some(Unit::Expression(source.S())),
        InputKind::OnelineStatement | InputKind::MultilineStatement => Some(Unit::Fragment(source.S())),
    }
}

impl ReplCommand {
    /// The command an input starting with the sigil names.
    ///
    /// A unit's source is whatever follows the unit's number, on that line
    /// and the lines after it.
    fn named(input: &str) -> ReplCommand {
        let (name, args) = split_word(&from_first_code_line(input)[1..]);
        let usage = |usage: &str| ReplCommand::Usage(fmt!("usage: {usage}"));
        let bare = |command: ReplCommand| match args.is_empty() {
            true => command,
            false => usage(&fmt!("/{name}")),
        };
        match name {
            "exit" => bare(ReplCommand::Exit),
            "help" => bare(ReplCommand::Help),
            "reset" => bare(ReplCommand::Reset),
            "save" => bare(ReplCommand::Save),
            "load" => bare(ReplCommand::Load),
            "undo" => bare(ReplCommand::Undo),
            "redo" => bare(ReplCommand::Redo),
            "edit" => match numbered_source(args) {
                Some((unit, source)) => ReplCommand::Edit { unit, source },
                None => usage("/edit N <source>"),
            },
            "delete" => match args.parse() {
                Ok(unit) => ReplCommand::Delete { unit },
                Err(_) => usage("/delete N"),
            },
            "insert" => {
                let (place, rest) = split_word(args);
                let place = match place {
                    "before" => Place::Before,
                    "after" => Place::After,
                    _ => return usage("/insert before|after N <source>"),
                };
                match numbered_source(rest) {
                    Some((unit, source)) => ReplCommand::Insert { place, unit, source },
                    None => usage("/insert before|after N <source>"),
                }
            }
            _ => ReplCommand::Unknown,
        }
    }
}

/// The first word of `s`, and the rest after the whitespace following it.
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    (&s[..end], s[end..].trim_start())
}

/// A unit number and the source that follows it, read as a unit is.
fn numbered_source(args: &str) -> Option<(u32, Unit)> {
    let (number, source) = split_word(args);
    Some((number.parse().ok()?, read_unit(source.trim_end())?))
}

enum InputKind {
    Whitespace,
    ReplCommand,
    OnelineStatement,
    MultilineStatement,
    Expression,
}

fn classify_input(input: &str) -> InputKind {
    // A comment is not a command. Both begin with the sigil, and the one a
    // reader means by `// note to self` is the comment.
    let code = first_code_line(input);
    let is_whitespace = code.is_empty();
    let is_repl_command = !starts_a_comment(code) && code.starts_with(REPL_COMMAND_SIGIL);
    // Every word a statement can start with, as the parser's statement
    // dispatch has them. None of them starts an expression, so an input
    // starting with one is a statement; one that opens a block reads on to
    // its `end`.
    let word = first_word(code);
    let is_oneline_statement_keyword = matches!(word,
        Some("let" | "var" | "const" | "set" | "call" | "require" | "import"
            | "ret" | "break" | "continue" | "debuglog" | "type" | "native"));
    let is_multiline_statement_keyword = matches!(word, Some("fun" | "if" | "match" | "loop" | "for"));

    if is_whitespace {
        InputKind::Whitespace
    } else if is_repl_command {
        InputKind::ReplCommand
    } else if is_oneline_statement_keyword {
        InputKind::OnelineStatement
    } else if is_multiline_statement_keyword {
        InputKind::MultilineStatement
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
    from_first_code_line(input).lines().next().unwrap_or("").trim()
}

/// The input from its first line of code on, without the blank lines and
/// comments before it; a command's arguments are read from here.
fn from_first_code_line(input: &str) -> &str {
    let mut rest = input;
    loop {
        let (line, after) = rest.split_once('\n').unwrap_or((rest, ""));
        let line = line.trim();
        if !line.is_empty() && !is_whole_line_comment(line) {
            return rest.trim();
        }
        if after.is_empty() {
            return "";
        }
        rest = after;
    }
}

/// True if a line is a comment and nothing else.
fn is_whole_line_comment(line: &str) -> bool {
    line.starts_with("//") || (line.starts_with("/*") && line.ends_with("*/"))
}

/// The name a line starts with, which is all of it when it is one word: a
/// bare `loop` opens a block as much as `loop while` does.
///
/// A name goes on through underscores, as the lexer's do, so `for_each(xs)`
/// is a call and not a `for`.
fn first_word(line: &str) -> Option<&str> {
    let line = line.trim();
    let end = line.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(line.len());
    (end > 0).then(|| &line[..end])
}
