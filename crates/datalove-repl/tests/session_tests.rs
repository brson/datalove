//! Saving the history with `/save` and loading it back, as `repl --load` does,
//! through the app and a real engine on its worker thread.

use rmx::prelude::*;

use datalove_repl::app::{EntryStatus, ReplApp};
use datalove_repl::{Command, ReplCommand, ThreadedExecutor, load_session};

type App = ReplApp<ThreadedExecutor>;

fn app(save_path: &std::path::Path) -> App {
    let executor = ThreadedExecutor::spawn(datalove_sys_packages::system_library, None);
    let mut app = ReplApp::with_executor(executor);
    app.set_save_path(save_path.to_path_buf());
    app
}

/// Poll until the engine has answered everything submitted.
fn settle(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        app.poll_results();
        assert!(app.engine_dead_message().is_none(), "{:?}", app.engine_dead_message());
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
        Command::ScriptStatement(_),
        Command::ScriptStatement(_),
        Command::ScriptStatement(_),
        Command::ReplCommand(ReplCommand::Help),
        Command::ReplCommand(ReplCommand::Unknown),
        Command::Expression(_),
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
