//! Saving the history with `/save` and loading it back, as `repl --load` and
//! `/load` do, through the app and a real engine on its worker thread.

use rmx::prelude::*;

use datalove_repl::app::{EntryStatus, ReplApp};
use datalove_repl::{Command, ReplCommand, ThreadedExecutor, load_session};
use datalove_repl::script::Unit;

type App = ReplApp<ThreadedExecutor>;

fn app(save_path: &std::path::Path) -> App {
    let executor = ThreadedExecutor::spawn(datalove_sys_packages::system_library, None);
    let mut app = ReplApp::with_executor(executor);
    app.set_session_path(save_path.to_path_buf());
    app
}

/// Poll until the engine has answered everything submitted.
fn settle(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        app.poll_results();
        assert!(app.engine_dead_message().is_none(), "{:?}", app.engine_dead_message());
        assert!(app.session_mismatch_message().is_none(), "{:?}", app.session_mismatch_message());
        let waiting = app.history().iter().any(|entry| {
            matches!(entry.status, EntryStatus::Parsing | EntryStatus::Evaluating { .. })
        });
        if app.engine_is_ready() && !waiting {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the engine never answered");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn submit(app: &mut App, input: &str) {
    app.submit_input(input.S());
    settle(app);
}

/// Each entry's input and whether it succeeded.
fn outcomes(app: &App) -> Vec<(String, bool)> {
    app.history().iter()
        .filter(|entry| !matches!(entry.status, EntryStatus::ReadMultiline))
        .map(|entry| (entry.input.C(), matches!(entry.status, EntryStatus::Success)))
        .collect()
}

#[test]
fn a_saved_history_loads_back_to_the_same_session() {
    let dir = rmx::tempfile::tempdir().X();
    let first_path = dir.path().join("first.world");
    let second_path = dir.path().join("second.world");

    let mut first = app(&first_path);
    submit(&mut first, "let a: int = 2");
    // The line that opens the definition, then the definition.
    submit(&mut first, "fun double(x: int): int");
    submit(&mut first, "fun double(x: int): int\n    ret x * 2\nend fun");
    submit(&mut first, "let b = nope");
    submit(&mut first, "/help");
    submit(&mut first, "/bogus");
    submit(&mut first, "double(a)");
    submit(&mut first, "/save");
    assert!(
        matches!(first.history().last().X().status, EntryStatus::Success),
        "{:?}", first.history().last(),
    );

    let inputs = load_session(&first_path).X();
    let kinds: Vec<&Command> = inputs.iter().map(|saved| &saved.command).collect();
    assert!(matches!(kinds[..], [
        Command::Unit(Unit::Fragment(_)),
        Command::Unit(Unit::Fragment(_)),
        Command::Unit(Unit::Fragment(_)),
        Command::ReplCommand(ReplCommand::Help),
        Command::ReplCommand(ReplCommand::Unknown),
        Command::Unit(Unit::Expression(_)),
    ]), "the save itself is not replayed: {kinds:?}");

    let mut second = app(&second_path);
    second.replay(inputs);
    settle(&mut second);

    let mut expected = outcomes(&first);
    expected.pop();
    assert_eq!(outcomes(&second), expected, "the failures fail again");
    assert_eq!(second.environment(), first.environment());

    submit(&mut second, "/save");
    assert_eq!(
        std::fs::read_to_string(&second_path).X(),
        std::fs::read_to_string(&first_path).X(),
    );
}

#[test]
fn a_worldfile_with_modules_is_not_a_session() {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("modules.world");
    std::fs::write(&path, "----------\nmodule local/pkg/m\n----------\n\nfun f()\nend fun\n").X();
    assert!(load_session(&path).is_err());
}

#[test]
fn a_repl_command_section_has_to_hold_one() {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("bad.world");
    std::fs::write(&path, "----------\nrepl-command\n----------\n\nlet a = 1\n").X();
    assert!(load_session(&path).is_err());
}

/// `/load` takes the session back to what was saved, whatever was done since.
#[test]
fn load_returns_to_the_saved_session() {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("session.world");

    let mut app = app(&path);
    submit(&mut app, "let a: int = 2");
    submit(&mut app, "let b = a + 1");
    submit(&mut app, "/save");
    let saved = app.environment().to_vec();

    submit(&mut app, "let c = 10");
    submit(&mut app, "/reset");
    submit(&mut app, "let d = 20");
    submit(&mut app, "/load");

    assert_eq!(app.environment(), &saved[..]);
    assert_eq!(outcomes(&app), vec![
        ("let a: int = 2".S(), true),
        ("let b = a + 1".S(), true),
    ], "the saved `/save` is not replayed, and the `/load` forgets itself");
}

/// A runtime error with input already on its way: the engine runs that input
/// in the new session, and the app keeps it.
#[test]
fn input_queued_behind_a_runtime_error_is_kept() {
    let dir = rmx::tempfile::tempdir().X();
    let mut app = app(&dir.path().join("session.world"));

    let unit = |source: &str| datalove_repl::SavedInput {
        input: source.S(),
        command: Command::Unit(Unit::Fragment(source.S())),
    };
    // Replayed, so all four are submitted before the engine answers any.
    app.replay(vec![
        unit("fun down(s: string): u32\n  let t = s@\n  ret icall add_wrapping_u32(down(t), : u32 / 1)\nend fun"),
        unit("let keep = \"kept\""),
        unit("let x = down(\"abc\")"),
        unit("let z = 4"),
    ]);
    settle(&mut app);

    assert!(app.crash_modal_message().is_some(), "the runtime error is reported");
    assert_eq!(outcomes(&app), vec![("let z = 4".S(), true)]);
    let names: Vec<&str> = app.environment().iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(names, vec!["z"]);
    let sources: Vec<&str> = app.code_units().iter().map(|unit| unit.source).collect();
    assert_eq!(sources, vec!["let z = 4"]);
}

/// The units, as the code panel numbers and shows them.
fn code(app: &App) -> Vec<(Option<u32>, String)> {
    app.code_units().iter().map(|unit| (unit.number, unit.source.S())).collect()
}

/// Edits, inserts and deletes through the app and a real engine, with the
/// app's units checked against the engine's after every one, and a session
/// with them in it saved and loaded back to the same units.
#[test]
fn edits_save_and_load_back() {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("session.world");

    let mut app = app(&path);
    submit(&mut app, "let a = 1");
    submit(&mut app, "let b = a + 1");
    submit(&mut app, "b");
    submit(&mut app, "/edit 1 let a = 10");
    submit(&mut app, "/insert after 1 let c = a * 2");
    submit(&mut app, "/delete 2");
    submit(&mut app, "/insert before 1 let z = 0");
    submit(&mut app, "/edit 3 c");

    let expected = vec![
        (Some(5), "let z = 0".S()),
        (Some(1), "let a = 10".S()),
        (Some(4), "let c = a * 2".S()),
        (Some(3), "c".S()),
    ];
    assert_eq!(code(&app), expected);
    let environment = app.environment().to_vec();
    let names: Vec<&str> = environment.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(names, vec!["a", "c", "z"]);

    submit(&mut app, "let late = 1");
    submit(&mut app, "/save");
    submit(&mut app, "/delete 1");
    submit(&mut app, "/load");

    let mut expected = expected;
    expected.push((Some(6), "let late = 1".S()));
    assert_eq!(code(&app), expected, "the load numbers the units as they were");
    let mut environment = environment;
    environment.push(("late".S(), "int".S(), "1".S()));
    environment.sort();
    let mut loaded = app.environment().to_vec();
    loaded.sort();
    assert_eq!(loaded, environment);
}
