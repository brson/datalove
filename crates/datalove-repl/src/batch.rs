//! Running a script of REPL inputs without the app.

use rmx::prelude::*;
use serde::Serialize;

use crate::script::Engine;
use crate::{Eval, Input, InputParse, PackageLibrary, SystemLibrary};

/// One input's parse and eval, and the environment it left behind.
#[derive(Debug, Serialize)]
pub struct InputResult {
    pub input: String,
    pub parse: InputParse,
    pub eval: Option<Eval>,
    pub environment: Vec<EnvBinding>,
}

/// An environment binding as reported after an input.
#[derive(Debug, Serialize)]
pub struct EnvBinding {
    pub name: String,
    pub ty: String,
    pub value: String,
}

/// Evaluate a script of inputs, one result per input.
///
/// Inputs are separated by a line holding only `---`. An input spanning
/// several lines is submitted the way the UI submits a multiline entry, so
/// definitions that span lines are evaluated rather than left waiting for
/// more input.
pub fn run_source(engine: &mut Engine, source: &str) -> Vec<InputResult> {
    let mut results = Vec::new();

    for section in source.split("\n---\n") {
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
            InputParse::Command(command) => Some(crate::eval(engine, command.C())),
            _ => None,
        };
        let environment = engine.get_environment()
            .into_iter()
            .map(|(name, ty, value)| EnvBinding { name, ty, value })
            .collect();

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
    let mut engine = Engine::with_local(sys, local)?;
    let contents = std::fs::read_to_string(script_path)
        .context("failed to read script file")?;

    for result in run_source(&mut engine, &contents) {
        println!("{}", rmx::serde_json::to_string(&result)?);
    }

    // Engine's Drop impl will call destroy_all().
    Ok(())
}
