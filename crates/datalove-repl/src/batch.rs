//! Running a script of REPL inputs without the app.

use rmx::prelude::*;
use serde::Serialize;

use crate::script::{Engine, EnvBinding};
use crate::{Outcome, Input, InputParse, PackageLibrary, Session, SystemLibrary};

/// One input's parse and eval, and the environment it left behind.
#[derive(Debug, Serialize)]
pub struct InputResult {
    pub input: String,
    pub parse: InputParse,
    pub eval: Option<Outcome>,
    pub environment: Vec<EnvBinding>,
}

/// Evaluate a script of inputs, one result per input.
///
/// Inputs are separated by lines holding only `---`. An input spanning
/// several lines is submitted the way the UI submits a multiline entry, so
/// definitions that span lines are evaluated rather than left waiting for
/// more input.
pub fn run_source(session: &mut Session, source: &str) -> Vec<InputResult> {
    let mut results = Vec::new();

    for section in sections(source) {
        let input = section.trim();
        if input.is_empty() {
            continue;
        }

        let repl_input = if input.contains('\n') {
            Input::Multiline(input.S())
        } else {
            Input::Input(input.S())
        };

        let parse = crate::parse_input(repl_input);
        let eval = match &parse {
            InputParse::Command(command) => Some(session.eval(command.C())),
            _ => None,
        };
        let environment = session.engine().get_environment();

        results.push(InputResult { input: input.S(), parse, eval, environment });
    }

    results
}

/// Execute a script file and print one JSON result per input.
///
/// `local` is the workspace's own library, if it has one.
pub fn run_script(
    sys: SystemLibrary,
    local: Option<PackageLibrary>,
    script_path: &std::path::Path,
) -> AnyResult<()> {
    let mut session = Session::new(Engine::with_local(sys, local)?);
    let contents = std::fs::read_to_string(script_path)
        .context("failed to read script file")?;

    for result in run_source(&mut session, &contents) {
        println!("{}", rmx::serde_json::to_string(&result)?);
    }
    Ok(())
}

/// The inputs of a script, the text between lines holding only `---`.
fn sections(source: &str) -> Vec<String> {
    let mut sections = vec![String::new()];
    for line in source.lines() {
        if line.trim() == "---" {
            sections.push(String::new());
        } else {
            let section = sections.last_mut().expect("there is always a section");
            section.push_str(line);
            section.push('\n');
        }
    }
    sections
}
