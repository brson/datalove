//! Crossterm Ratatui REPL.


use rmx::prelude::*;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io::{self, Write};
use std::fs::File;
use std::path::PathBuf;

const ENGINE_UPDATES_MAX_LATENCY_MS: u64 = 10;

/// Run the ratatui-based REPL.
pub fn run() -> AnyResult<()> {
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
            libc::dup2(stderr_file.as_raw_fd(), 2)
        };
        if result == -1 {
            bail!("failed to redirect stderr");
        }
    }

    // Setup terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run it, passing the stderr log path.
    let mut app = datalove_repl_rat::RatatuiApp::<datalove_repl_rat::ThreadedExecutor>::new_with_stderr_log(stderr_log_path);
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res
}

/// Run the application loop.
fn run_app<B, E>(
    terminal: &mut Terminal<B>,
    app: &mut datalove_repl_rat::RatatuiApp<E>,
) -> AnyResult<()>
where
    B: ratatui::backend::Backend,
    E: datalove_repl_rat::ReplExecutor,
{
    loop {
        // Poll for worker results before drawing.
        app.poll_results();

        terminal.draw(|f| datalove_repl_rat::ui(f, app))?;

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

/// Run the application loop with a provided event source.
pub fn run_app_with_events<B, E>(
    terminal: &mut Terminal<B>,
    app: &mut datalove_repl_rat::RatatuiApp<E>,
    events: &mut dyn Iterator<Item = Event>,
) -> AnyResult<()>
where
    B: ratatui::backend::Backend,
    E: datalove_repl_rat::ReplExecutor,
{
    loop {
        // Poll for worker results before drawing.
        app.poll_results();

        terminal.draw(|f| datalove_repl_rat::ui(f, app))?;

        if app.should_exit() {
            break;
        }

        if let Some(Event::Key(key)) = events.next() {
            handle_key_event(app, key);
        } else {
            // No more events, but keep polling for pending results.
            app.poll_results();
            if !app.has_pending_work() {
                break;
            }
        }
    }

    Ok(())
}

/// Handle a key event.
fn handle_key_event<E: datalove_repl_rat::ReplExecutor>(
    app: &mut datalove_repl_rat::RatatuiApp<E>,
    key: crossterm::event::KeyEvent,
) {
    // Only process press events, not repeat/release.
    if key.kind != KeyEventKind::Press {
        return;
    }

    if app.crash_modal_is_open() {
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
            _ => {
                // Pass all other keys to the textarea for handling.
                app.handle_input_key(key);
            }
        }
    }
}
