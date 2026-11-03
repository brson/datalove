//! The session replays the documents embed, recorded from the worldfiles in
//! `replays/` and checked against the files the documents load.
//!
//! Each replay is a worldfile holding a session as `/save` writes it, with
//! the modules and data it requires beside it. Recording it, as
//! `datalove repl --load <world> --record <path>` does, gives a trace of the
//! session after each input,
//! which is written as a `.replay.js` file in `mandocs/assets`, for
//! `mandocs/assets/datalove-replay.js` to play.
//!
//! `BLESS=1` records the replays again and writes the files instead of
//! checking them.

use rmx::prelude::*;
use std::path::{Path, PathBuf};

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

/// The `.replay.js` file for a replay: its trace, registered with the player
/// under its name.
fn render(replay: &Replay) -> String {
    let world = replays_dir().join(replay.world);
    let trace = datalove_repl::record_world(datalove_sys_packages::system_library(), &world).X();
    let json = rmx::serde_json::to_string_pretty(&trace).X();
    fmt!(
        "// Recorded by crates/datalove-doc-fixtures from replays/{}.\n\
         // Do not edit: run `BLESS=1 cargo test -p datalove-doc-fixtures`.\n\
         DataloveReplay.register({:?}, {json});\n",
        replay.world, replay.name,
    )
}

#[test]
fn replays_are_fresh() {
    let bless = std::env::var("BLESS").is_ok();
    let mut stale = Vec::new();
    for replay in REPLAYS {
        let path = workspace_root().join(replay.output);
        let rendered = render(replay);
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
