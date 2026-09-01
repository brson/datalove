//! Terminal frontend: owns the terminal, the event loop, and the key bindings.

use rmx::prelude::*;

use crossterm::{
    cursor::Show,
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use std::fs::File;

use crate::{RatatuiApp, ReplExecutor, SystemLibrary, ThreadedExecutor};

const ENGINE_UPDATES_MAX_LATENCY_MS: u64 = 10;

/// Puts the terminal back the way it was found, including while panicking.
///
/// Without this a panic in the UI thread leaves the terminal in raw mode on
/// the alternate screen, and the trace goes to the redirected stderr where
/// nobody sees it.
struct TerminalGuard {
    stderr_log_path: std::path::PathBuf,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, Show);

        if std::thread::panicking() {
            println!(
                "the datalove repl panicked; the trace is in {}",
                self.stderr_log_path.display()
            );
        }
    }
}

/// Run the ratatui-based REPL against the caller's system library.
pub fn run(sys: fn() -> SystemLibrary) -> AnyResult<()> {
    // Redirect stderr to a log file in temp directory to avoid corrupting terminal in raw mode.
    let stderr_log_path = std::env::temp_dir().join("datalove-repl.stderr");
    let stderr_file = File::create(&stderr_log_path)
        .context("failed to create stderr log file")?;

    // Redirect stderr using platform-specific dup2.
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        // dup2 duplicates file descriptor to stderr (fd 2).
        let result = unsafe {
            rmx::libc::dup2(stderr_file.as_raw_fd(), 2)
        };
        if result == -1 {
            bail!("failed to redirect stderr");
        }
    }

    // Setup terminal. The guard restores it however this function exits.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let guard = TerminalGuard { stderr_log_path: stderr_log_path.C() };
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run it, passing the stderr log path.
    let executor = ThreadedExecutor::spawn(sys);
    let mut app = RatatuiApp::with_stderr_log(executor, stderr_log_path);
    let result = run_app(&mut terminal, &mut app);

    // Restore the terminal before saying anything, so an engine failure lands
    // in the scrollback the user keeps instead of the alternate screen.
    drop(guard);

    if let Some(msg) = app.repl.engine_dead_message() {
        println!("{msg}");
    }

    result
}

/// Run the application loop.
fn run_app<B, E>(
    terminal: &mut Terminal<B>,
    app: &mut RatatuiApp<E>,
) -> AnyResult<()>
where
    B: ratatui::backend::Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
    E: ReplExecutor,
{
    loop {
        // Poll for worker results before drawing.
        app.poll_results();

        terminal.draw(|f| crate::ui(f, app))?;

        if app.should_exit() {
            break;
        }

        // Use polling to check for events with timeout, so we can update UI.
        if event::poll(std::time::Duration::from_millis(ENGINE_UPDATES_MAX_LATENCY_MS))? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(app, key);
            }
        }
    }

    Ok(())
}

/// Handle a key event.
fn handle_key_event<E: ReplExecutor>(
    app: &mut RatatuiApp<E>,
    key: crossterm::event::KeyEvent,
) {
    // Only process press events, not repeat/release.
    if key.kind != KeyEventKind::Press {
        return;
    }

    if app.engine_is_dead() {
        // Nothing works without an engine; Enter leaves.
        match key.code {
            KeyCode::Enter => app.set_should_exit(true),
            _ => {}
        }
    } else if app.crash_modal_is_open() {
        // Crash modal is open - wait for Enter to dismiss.
        match key.code {
            KeyCode::Enter => app.dismiss_crash_modal(),
            _ => {}
        }
    } else if app.menu_is_open() {
        // Menu is open - handle menu navigation.
        match key.code {
            KeyCode::Up => app.menu_up(),
            KeyCode::Down => app.menu_down(),
            KeyCode::Enter => app.execute_menu_action(),
            KeyCode::Esc => app.close_menu(),
            _ => {}
        }
    } else {
        // Normal input mode.
        match key.code {
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.set_should_exit(true);
            }
            KeyCode::Enter => {
                // In single-line mode: Enter submits.
                // In multiline mode: Enter inserts newline, Alt+Enter submits.
                let is_multiline = app.multiline_mode();
                let has_alt = key.modifiers.contains(KeyModifiers::ALT);

                let should_submit = if is_multiline {
                    has_alt  // In multiline: Alt+Enter submits
                } else {
                    true // In single-line: Enter submits
                };

                if should_submit {
                    app.submit_input();
                } else {
                    app.handle_input_key(key);
                }
            }
            KeyCode::Esc => app.open_menu(),
            // Ahead of the textarea, which would otherwise take these to move
            // its cursor.
            KeyCode::PageUp => app.scroll_history_up(),
            KeyCode::PageDown => app.scroll_history_down(),
            _ => {
                // Pass all other keys to the textarea for handling.
                app.handle_input_key(key);
            }
        }
    }
}
