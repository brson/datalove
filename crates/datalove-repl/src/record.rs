//! Recording a session as a trace a web page can replay.
//!
//! A trace is everything the REPL would show after each input: what the input
//! came to, the session's units with their statuses and the marks of the last
//! step, the environment, and each unit's detail. A player steps through it
//! without an engine, so it needs no knowledge of the REPL's rules.

use rmx::prelude::*;
use serde::Serialize;
use std::collections::HashMap;

use crate::app::{UnitStatus, status_of};
use crate::script::{self, Engine, EnvBinding};
use crate::{Command, Outcome, ReplCommand, SavedInput, Session, SystemLibrary, UnitDetail};

/// The version of the trace's shape, which a player checks.
pub const TRACE_VERSION: u32 = 1;

/// A recorded session: the state the REPL shows after each input.
#[derive(Debug, Serialize)]
pub struct Trace {
    pub version: u32,
    pub steps: Vec<TraceStep>,
}

/// One input, and the session as it left it.
#[derive(Debug, Serialize)]
pub struct TraceStep {
    /// The input as it was typed.
    pub input: String,
    /// What it came to.
    pub outcome: Outcome,
    /// The session's units, in order.
    pub units: Vec<TraceUnit>,
    pub environment: Vec<EnvBinding>,
}

/// One unit of the session, as the code panel and the Detail tab show it.
#[derive(Debug, Serialize)]
pub struct TraceUnit {
    pub number: u32,
    pub expression: bool,
    pub source: String,
    /// How it fared the last time it was compiled or run.
    pub status: UnitStatus,
    /// What it came to the last time it was re-derived.
    pub outcome: script::Eval,
    /// Whether the last step to change the session compiled it again.
    pub compiled: bool,
    /// Whether the last step to change the session ran it.
    pub ran: bool,
    pub detail: UnitDetail,
}

/// Record a session's inputs.
pub fn record_inputs(session: &mut Session, inputs: Vec<SavedInput>) -> Trace {
    // What each unit came to the last time a step re-derived it, by number.
    let mut outcomes: HashMap<u32, script::Eval> = HashMap::new();
    let mut steps = Vec::new();

    for SavedInput { input, command } in inputs {
        let outcome = session.eval(command);
        if outcome.empties_session() {
            outcomes.clear();
        }
        // The last step stays the last until another replaces it, so taking
        // it again after a command that changed nothing changes nothing.
        for step in session.last_step() {
            if let Some(eval) = &step.eval {
                outcomes.insert(step.unit, eval.C());
            }
        }
        let units = trace_units(session, &outcomes);
        let environment = session.engine().get_environment();
        steps.push(TraceStep { input, outcome, units, environment });
    }

    Trace { version: TRACE_VERSION, steps }
}

/// Record the session a worldfile holds, against the library it holds.
///
/// The worldfile is a session as `/save` writes it, with the modules, data
/// and riders it requires beside it; see [`crate::read_world`]. Its modules
/// are compiled from the start, as `script-world` compiles them, and one
/// that does not compile is an error in the worldfile.
pub fn record_world(sys: SystemLibrary, path: &std::path::Path) -> AnyResult<Trace> {
    let world = crate::read_world(path)?;
    let (workspace, modules) = world.workspace(&sys)?;
    let engine = Engine::with_options(sys, workspace, datalove_datafun::DebugOutputMode::Disabled, modules)?;
    let broken = &engine.module_errors().broken;
    if !broken.is_empty() {
        let errors: Vec<String> = broken.iter()
            .map(|(module, errors)| fmt!("{module}: {}", errors.join("; ")))
            .collect();
        bail!("{} has modules that do not compile:\n{}", path.display(), errors.join("\n"));
    }
    Ok(record_inputs(&mut Session::new(engine), world.inputs))
}

fn trace_units(session: &mut Session, outcomes: &HashMap<u32, script::Eval>) -> Vec<TraceUnit> {
    let last_step = session.last_step().to_vec();
    let numbered = session.units().to_vec();
    numbered.into_iter()
        .map(|numbered| {
            let number = numbered.number;
            let outcome = outcomes.get(&number)
                .unwrap_or_else(|| panic!("unit {number} has been re-derived"))
                .C();
            let step = last_step.iter().find(|step| step.unit == number);
            let detail = match session.eval(Command::ReplCommand(ReplCommand::Describe { unit: number })) {
                Outcome::Detail(detail) => detail,
                other => panic!("describing unit {number} came to {other:?}"),
            };
            TraceUnit {
                number,
                expression: numbered.unit.is_expression(),
                source: numbered.unit.source().S(),
                status: status_of(&outcome),
                outcome,
                compiled: step.is_some_and(|step| step.compiled),
                ran: step.is_some_and(|step| step.ran()),
                detail,
            }
        })
        .collect()
}
