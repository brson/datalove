//! Step tables: what each unit of a session should do in a step, checked
//! against the engine.
//!
//! A step is one change to the session -- units appended, one unit edited,
//! deleted, inserted or moved, or a module or data file edited -- and its `expect` table says, for every unit,
//! what the unit should do in that step and what its bindings should hold
//! after it:
//!
//! ```text
//! | unit | uses | this step | because | holds after |
//! |------|------|-----------|---------|-------------|
//! | 1 | - | compiles, runs | new | `b` = 251 |
//! ```
//!
//! An `expect` table states the rule, and the engine may do more work than it
//! asks for: rerun a unit the rule leaves alone, or recompile one the rule
//! only reruns. Each such row is annotated in an `engine` table after the
//! step, so the gap is written down. Doing less than the rule asks, or failing
//! differently, is never accepted, and an annotation the engine has caught up
//! with fails too, so the annotations stay true.
//!
//! The `because` column is for the reader, and is not checked.
//!
//! Every step, with a table or without, is also checked against a fresh run:
//! a new engine runs the session's current units in order, and each unit must
//! come to the same end and hold the same bindings. This is the definition of
//! a correct step, checked without trusting the tables' `holds after`. The
//! session, and that check, are `datalove-script-steps`.

use rmx::prelude::*;

use datalove_script_steps::{Change, Outcome, Ran, Session, Workspace};

// ============================================================================
// Text
// ============================================================================

/// The cells of a markdown table's body rows, header and rule skipped.
fn table(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            line.trim_matches('|').split('|').map(|cell| cell.trim().S()).collect()
        })
        .collect()
}

/// An outcome as a step table's `this step` says it.
fn parse_outcome(text: &str) -> Outcome {
    let (recompiles, ran) = match text {
        "untouched" => (false, Ran::No),
        "recompiles" => (true, Ran::No),
        "reruns" => (false, Ran::Runs),
        "compiles, runs" | "recompiles, runs" => (true, Ran::Runs),
        "compile fails" | "recompile fails" => (true, Ran::Fails),
        "compiles, returns early" | "recompiles, returns early" => (true, Ran::ReturnsEarly),
        "reruns, returns early" => (false, Ran::ReturnsEarly),
        "compiles, is blocked" | "recompiles, is blocked" => (true, Ran::Blocked),
        "is blocked" => (false, Ran::Blocked),
        "deleted" => (false, Ran::Deleted),
        other => panic!("unknown outcome `{other}`"),
    };
    Outcome { recompiles, ran }
}

/// How a step table would say an outcome.
fn describe(outcome: Outcome) -> &'static str {
    match (outcome.recompiles, outcome.ran) {
        (false, Ran::No) => "untouched",
        (true, Ran::No) => "recompiles",
        (false, Ran::Runs) => "reruns",
        (true, Ran::Runs) => "recompiles, runs",
        (_, Ran::Fails) => "recompile fails",
        (false, Ran::ReturnsEarly) => "reruns, returns early",
        (true, Ran::ReturnsEarly) => "recompiles, returns early",
        (false, Ran::Blocked) => "is blocked",
        (true, Ran::Blocked) => "recompiles, is blocked",
        (_, Ran::Deleted) => "deleted",
    }
}

/// Whether the engine doing `seen` is what the rule's `rule` looks like
/// from outside.
///
/// A unit that fails is not asked whether it recompiled: the doc says
/// "recompile fails" whether or not the failing analysis was one salsa
/// kept.
fn is_seen_as(seen: Outcome, rule: Outcome) -> bool {
    match rule.ran {
        Ran::Fails | Ran::Deleted => seen.ran == rule.ran,
        _ => seen == rule,
    }
}

/// Whether the engine doing `seen` where the rule says `rule` is only more
/// work to the same end: compiling or running a unit the rule leaves alone.
fn is_conservative_for(seen: Outcome, rule: Outcome) -> bool {
    let fine = |ran: Ran| matches!(ran, Ran::No | Ran::Runs);
    fine(seen.ran) && fine(rule.ran)
        && seen.recompiles >= rule.recompiles
        && (seen.ran == Ran::Runs) >= (rule.ran == Ran::Runs)
        && seen != rule
}

// ============================================================================
// Steps
// ============================================================================

/// One step of a session, and the table it is checked against, if any.
pub struct Step {
    pub change: Change,
    pub check: Option<Check>,
}

/// What a step's table says.
pub struct Check {
    /// What a document calls the step, when not `Step N`: an `expect`
    /// header that starts with `Step` names it up to the first `: `.
    pub label: Option<String>,
    pub title: String,
    pub expect: Vec<Row>,
    pub engine: Vec<EngineRow>,
}

impl Check {
    /// The check an `expect` section makes, from its header's words after
    /// `expect`, and its table.
    pub fn parse(header: &str, content: &str) -> Check {
        let expect = table(content).into_iter()
            .map(|cells| {
                assert_eq!(cells.len(), 5, "an expect row has five cells: {cells:?}");
                let [unit, uses, this_step, because, holds_after] = <[String; 5]>::try_from(cells).ok().X();
                Row { unit, uses, this_step, because, holds_after }
            })
            .collect();
        let (label, title) = match header.split_once(": ") {
            Some((label, title)) if label.starts_with("Step ") => (Some(label.S()), title.S()),
            _ => (None, header.S()),
        };
        Check { label, title, expect, engine: Vec::new() }
    }

    /// Add the rows of an `engine` section.
    pub fn add_engine(&mut self, content: &str) {
        self.engine = table(content).into_iter()
            .map(|cells| {
                assert_eq!(cells.len(), 3, "an engine row has three cells: {cells:?}");
                let [unit, this_step, note] = <[String; 3]>::try_from(cells).ok().X();
                EngineRow { unit, this_step, note }
            })
            .collect();
    }
}

pub struct Row {
    pub unit: String,
    pub uses: String,
    pub this_step: String,
    pub because: String,
    pub holds_after: String,
}

pub struct EngineRow {
    pub unit: String,
    pub this_step: String,
    pub note: String,
}

/// Run a session's steps, and say every way the engine disagreed with their
/// tables. `name` says whose steps they are in what goes wrong.
pub fn run(name: &str, workspace: Workspace, steps: &[Step]) -> Vec<String> {
    let mut session = Session::start(workspace.sys, workspace.descriptor, workspace.also_roots);
    let mut failures = Vec::new();

    for (number, step) in steps.iter().enumerate() {
        let touched = session.apply(&step.change);
        failures.extend(session.differs_from_fresh(&fmt!("{name} step {}", number + 1)));
        if let Some(reset) = session.reset() {
            failures.push(fmt!("{name} step {}: the session reset, with {reset:?}, and a step table cannot follow", number + 1));
            break;
        }
        let Some(check) = &step.check else {
            continue;
        };
        let at = fmt!("{name} step {} ({})", number + 1, check.title);
        let mut recompiled: Vec<&String> = touched.iter()
            .filter(|(_, outcome)| outcome.recompiles)
            .map(|(name, _)| name)
            .collect();
        recompiled.sort();
        let mut analyzed: Vec<&String> = session.analyzed().iter().collect();
        analyzed.sort();
        if recompiled != analyzed {
            failures.push(fmt!(
                "{at}: salsa recompiled units {recompiled:?}, and the engine says it analyzed {analyzed:?}",
            ));
        }
        let observed = |unit: &str| {
            touched.iter().find(|(name, _)| name == unit).map(|(_, outcome)| *outcome)
                .unwrap_or(Outcome { recompiles: false, ran: Ran::No })
        };

        let deleted: Vec<&String> = touched.iter()
            .filter(|(_, outcome)| outcome.ran == Ran::Deleted)
            .map(|(name, _)| name)
            .collect();
        let mut listed: Vec<&String> = check.expect.iter().map(|row| &row.unit).collect();
        listed.sort();
        let mut present: Vec<&String> = session.units().iter().chain(deleted).collect();
        present.sort();
        if listed != present {
            failures.push(fmt!("{at}: the table lists units {listed:?}, the session has {present:?}"));
            continue;
        }

        for row in &check.expect {
            let rule = parse_outcome(&row.this_step);
            let engine_row = check.engine.iter().find(|engine| engine.unit == row.unit);
            let expected = match engine_row {
                Some(engine) => {
                    let outcome = parse_outcome(&engine.this_step);
                    if !is_conservative_for(outcome, rule) {
                        failures.push(fmt!(
                            "{at}: unit {}: the engine row says `{}`, which is not only more work than `{}`",
                            row.unit, engine.this_step, row.this_step,
                        ));
                    }
                    outcome
                }
                None => rule,
            };
            let seen = observed(&row.unit);
            if !is_seen_as(seen, expected) {
                let tighter = engine_row.is_some() && is_seen_as(seen, rule);
                failures.push(if tighter {
                    fmt!("{at}: unit {}: the engine now does what the rule says, `{}`; drop its engine row",
                        row.unit, row.this_step)
                } else if engine_row.is_none() && is_conservative_for(seen, rule) {
                    fmt!("{at}: unit {}: expected `{}`, the engine is conservative and `{}`; \
                        annotate it in an `engine` table if that is known",
                        row.unit, row.this_step, describe(seen))
                } else {
                    fmt!("{at}: unit {}: expected `{}`, got `{}`", row.unit, expected_text(row, engine_row), describe(seen))
                });
            }

            if seen.ran == Ran::Deleted {
                continue;
            }
            let index = session.index(&row.unit);
            if !session.has_failed(&row.unit) {
                let mut listed: Vec<String> = match row.uses.as_str() {
                    "-" => Vec::new(),
                    uses => uses.split(", ").map(str::S).collect(),
                };
                listed.sort();
                let found = session.uses(index);
                if listed != found {
                    let found = if found.is_empty() { "-".S() } else { found.join(", ") };
                    failures.push(fmt!("{at}: unit {} {found}, the table says {}", row.unit, row.uses));
                }
            }
            let held = session.holds(index);
            if held != row.holds_after {
                failures.push(fmt!(
                    "{at}: unit {} holds {held}, the table says {}",
                    row.unit, row.holds_after,
                ));
            }
        }
        for engine in &check.engine {
            if !check.expect.iter().any(|row| row.unit == engine.unit) {
                failures.push(fmt!("{at}: an engine row for unit {}, which the table does not list", engine.unit));
            }
        }
    }
    failures
}

fn expected_text(row: &Row, engine: Option<&EngineRow>) -> String {
    match engine {
        Some(engine) => fmt!("{}` (engine; the rule says `{}", engine.this_step, row.this_step),
        None => row.this_step.C(),
    }
}
