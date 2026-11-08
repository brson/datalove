//! The session replays the documents embed, recorded from the worldfiles in
//! `replays/` and checked against the files the documents load.
//!
//! Each replay is a worldfile holding a session as `/save` writes it, with
//! the modules and data it requires beside it. Recording it, as
//! `datalove repl --load <world> --record <path>` does, gives a trace of the
//! session after each input, which is written as a `.replay.js` file in
//! `mandocs/assets`, for `mandocs/assets/datalove-replay.js` to play.
//!
//! A replay may also hold `expect` tables, and `engine` tables after them,
//! as the reactive fixtures do; see `datalove_doc_fixtures::step_tables`. An
//! `expect` checks the inputs since the one before it, which are either units
//! appended or one `/edit`, `/edit-data`, `/delete`, `/insert` or `/move`.
//! The tables are taken out before the session is recorded, so what a replay
//! shows is checked against the rules, and a change in the language breaks
//! the test rather than quietly changing the document.
//!
//! `BLESS=1` records the replays again and writes the files instead of
//! checking them. The step tables are never blessed: they are the rules.

use rmx::prelude::*;
use std::path::{Path, PathBuf};

use datalove_doc_fixtures::step_tables::{Change, Check, Entry, SEPARATOR, Step, Workspace, run, sections};
use datalove_repl::script::Unit;
use datalove_repl::{Command, ReplCommand, SavedInput, read_world, record_world};

/// A replay a document shows.
struct Replay {
    /// The name the document's player asks for.
    name: &'static str,
    /// The worldfile, under `replays/`.
    world: &'static str,
    /// The file the document loads, under the workspace root.
    output: &'static str,
}

const REPLAYS: &[Replay] = &[
    Replay {
        name: "reactive-transactions",
        world: "reactive-transactions.world",
        output: "mandocs/assets/reactive-transactions.replay.js",
    },
];

fn replays_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("replays")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A replay's worldfile, taken apart: the session, written to a worldfile of
/// its own without the tables, and the tables.
struct Parsed {
    /// Holds the session's worldfile.
    fs: datalove_vio::MemFs,
    /// The worldfile of the session alone, in `fs`.
    session: PathBuf,
    /// How many inputs the session has.
    inputs: usize,
    /// Each table, with how many inputs come before it.
    checks: Vec<(usize, Check)>,
}

fn parse(replay: &Replay) -> Parsed {
    let text = std::fs::read_to_string(replays_dir().join(replay.world)).X();
    let mut session = String::new();
    let mut checks: Vec<(usize, Check)> = Vec::new();
    let mut inputs = 0;
    for section in sections(&text) {
        let (kind, rest) = section.header.split_once(' ').unwrap_or((&section.header, ""));
        match kind {
            "expect" => checks.push((inputs, Check::parse(rest, &section.content))),
            "engine" => {
                let (_, check) = checks.last_mut()
                    .unwrap_or_else(|| panic!("{}: an `engine` table before any `expect`", replay.world));
                check.add_engine(&section.content);
            }
            _ => {
                if kind.starts_with("scriptunit-") || kind == "repl-command" {
                    inputs += 1;
                }
                session.push_str(&fmt!("{SEPARATOR}\n{}\n{SEPARATOR}\n\n{}\n\n", section.header, section.content));
            }
        }
    }
    let fs = datalove_vio::MemFs::new();
    let path = Path::new("/").join(replay.world);
    fs.add_file(&path, session);
    Parsed { fs, session: path, inputs, checks }
}

/// The change an input makes, naming units by number as the session numbers
/// them; `next` is the number the next unit made takes.
fn change(replay: &Replay, command: &Command, next: &mut u32) -> Change {
    let mut entry = |source: &Unit| {
        let unit = next.S();
        *next += 1;
        Entry { unit, source: source.source().S(), expression: source.is_expression() }
    };
    match command {
        Command::Unit(unit) => Change::Enter(vec![entry(unit)]),
        Command::ReplCommand(ReplCommand::Edit { unit, source }) =>
            Change::Edit { unit: unit.S(), source: source.source().S() },
        Command::ReplCommand(ReplCommand::EditData { path, text }) =>
            Change::EditData { path: path.C(), text: text.C() },
        Command::ReplCommand(ReplCommand::Delete { unit }) => Change::Delete { unit: unit.S() },
        Command::ReplCommand(ReplCommand::Insert { place, unit, source }) =>
            Change::Insert { entry: entry(source), place: *place, beside: unit.S() },
        Command::ReplCommand(ReplCommand::Move { unit, place, beside }) =>
            Change::Move { unit: unit.S(), place: *place, beside: beside.S() },
        other => todo!("{}: a replay with step tables that has {other:?}", replay.world),
    }
}

/// The steps a session's inputs make, each checked by the table after it.
///
/// The inputs a table checks are one step: units appended, or one change of
/// another kind. The inputs after the last table are a step each, and are
/// not checked.
fn steps(replay: &Replay, inputs: &[SavedInput], checks: Vec<(usize, Check)>) -> Vec<Step> {
    let mut next = 1;
    let mut changes = inputs.iter().map(|input| change(replay, &input.command, &mut next));
    let mut steps = Vec::new();
    let mut taken = 0;
    for (before, check) in checks {
        assert!(before > taken, "{}: an `expect` with no input before it", replay.world);
        let group: Vec<Change> = changes.by_ref().take(before - taken).collect();
        taken = before;
        let change = match group.len() {
            1 => group.into_iter().next().X(),
            _ => Change::Enter(group.into_iter()
                .flat_map(|change| match change {
                    Change::Enter(entries) => entries,
                    _ => panic!("{}: an `expect` after several inputs, not all of them units", replay.world),
                })
                .collect()),
        };
        steps.push(Step { change, check: Some(check) });
    }
    steps.extend(changes.map(|change| Step { change, check: None }));
    steps
}

/// The `.replay.js` file for a replay: its trace, registered with the player
/// under its name.
fn render(replay: &Replay, parsed: &Parsed) -> String {
    let trace = record_world(&parsed.fs, datalove_sys_packages::system_library(), &parsed.session).X();
    let json = rmx::serde_json::to_string_pretty(&trace).X();
    fmt!(
        "// Recorded by crates/datalove-doc-fixtures from replays/{}.\n\
         // Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.\n\
         DataloveReplay.register({:?}, {json});\n",
        replay.world, replay.name,
    )
}

#[test]
fn every_step_table_holds_for_the_engine() {
    let mut failures = Vec::new();
    for replay in REPLAYS {
        let parsed = parse(replay);
        let world = read_world(&parsed.fs, &parsed.session).X();
        assert_eq!(world.inputs.len(), parsed.inputs, "{}: every input is replayed", replay.world);
        let sys = datalove_sys_packages::system_library();
        let (descriptor, also_roots) = world.workspace(&sys).X();
        let steps = steps(replay, &world.inputs, parsed.checks);
        failures.extend(run(replay.world, Workspace { sys, descriptor, also_roots }, &steps));
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

#[test]
fn replays_are_fresh() {
    let bless = std::env::var("BLESS").is_ok();
    let mut stale = Vec::new();
    for replay in REPLAYS {
        let path = workspace_root().join(replay.output);
        let parsed = parse(replay);
        let rendered = render(replay, &parsed);
        if bless {
            std::fs::write(&path, rendered).X();
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(rendered.as_str()) {
            stale.push(replay.output);
        }
    }
    assert!(stale.is_empty(), "stale replays {stale:?}; run with BLESS=1 to record them");
}

/// Every worldfile under `replays/` is a replay some document shows, so none
/// is left unchecked.
#[test]
fn every_world_is_a_replay() {
    let mut worlds: Vec<String> = std::fs::read_dir(replays_dir()).X()
        .map(|entry| entry.X().file_name().to_string_lossy().into_owned())
        .collect();
    worlds.sort();
    let mut listed: Vec<String> = REPLAYS.iter().map(|replay| replay.world.S()).collect();
    listed.sort();
    assert_eq!(worlds, listed, "every worldfile under replays/ is listed in REPLAYS");
}
