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
            matches!(entry.status, EntryStatus::Evaluating)
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
    let names: Vec<&str> = app.environment().iter().map(|binding| binding.name.as_str()).collect();
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
    let names: Vec<&str> = environment.iter().map(|binding| binding.name.as_str()).collect();
    assert_eq!(names, vec!["a", "c", "z"]);

    submit(&mut app, "let late = 1");
    submit(&mut app, "/save");
    submit(&mut app, "/delete 1");
    submit(&mut app, "/load");

    let mut expected = expected;
    expected.push((Some(6), "let late = 1".S()));
    assert_eq!(code(&app), expected, "the load numbers the units as they were");
    let mut environment = environment;
    environment.push(datalove_repl::script::EnvBinding { name: "late".S(), ty: "int".S(), value: "1".S() });
    environment.sort();
    let mut loaded = app.environment().to_vec();
    loaded.sort();
    assert_eq!(loaded, environment);
}

/// What the last step did to each unit, from the real engine.
fn marks(app: &App) -> Vec<(Option<u32>, bool, bool)> {
    app.code_units().iter().map(|unit| (unit.number, unit.compiled, unit.ran)).collect()
}

/// A value-only edit reruns the units that read the value without compiling
/// them again; an edit that changes a type compiles them again, and one that
/// no longer compiles does not run.
#[test]
fn the_code_shows_what_each_step_compiled_and_ran() {
    let dir = rmx::tempfile::tempdir().X();
    let mut app = app(&dir.path().join("session.world"));
    for unit in ["let a = 1", "let b = 2", "let c = 30", "let d = b + 5"] {
        submit(&mut app, unit);
    }
    assert_eq!(marks(&app).last(), Some(&(Some(4), true, true)), "an appended unit compiles and runs");

    submit(&mut app, "/edit 2 let b = 3");
    assert_eq!(marks(&app), vec![
        (Some(1), false, false),
        (Some(2), true, true),
        (Some(3), false, false),
        (Some(4), false, true),
    ]);

    submit(&mut app, "/edit 2 let b = \"three\"");
    assert_eq!(marks(&app), vec![
        (Some(1), false, false),
        (Some(2), true, true),
        (Some(3), false, false),
        (Some(4), true, false),
    ]);

    submit(&mut app, "/help");
    assert_eq!(marks(&app)[3], (Some(4), true, false), "a command that changes nothing leaves the marks");
}

/// A script's inputs are split at lines holding only `---`, whatever ends
/// the lines and wherever the first comes.
#[test]
fn a_script_splits_at_its_separator_lines() {
    let engine = datalove_repl::script::Engine::new(datalove_sys_packages::system_library()).X();
    let mut session = datalove_repl::Session::new(engine);
    let results = datalove_repl::run_source(&mut session, "---\r\nlet a = 1\r\n---\r\nlet b = a + 1\r\n  ---  \r\nb\r\n");
    let inputs: Vec<&str> = results.iter().map(|result| result.input.as_str()).collect();
    assert_eq!(inputs, vec!["let a = 1", "let b = a + 1", "b"]);
}

/// A saved `/help` is not replayed: it changes nothing in the session, and
/// would show the help to someone loading it who did not ask.
#[test]
fn a_saved_help_is_not_replayed() {
    let dir = rmx::tempfile::tempdir().X();
    let path = dir.path().join("help.world");
    std::fs::write(&path, "----------\nrepl-command\n----------\n\n/help\n\n\
        ----------\nscriptunit-fragment\n----------\n\nlet a = 1\n").X();
    let inputs = load_session(&path).X();
    let kinds: Vec<&Command> = inputs.iter().map(|saved| &saved.command).collect();
    assert!(matches!(kinds[..], [Command::Unit(Unit::Fragment(_))]), "{kinds:?}");
}
