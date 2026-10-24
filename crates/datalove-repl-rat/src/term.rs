//! Terminal frontend: owns the terminal, the event loop, and the key bindings.

use rmx::prelude::*;

use crossterm::{
    cursor::Show,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use std::fs::File;
use std::path::Path;

use crate::RatatuiApp;
use crate::repl::{self, PackageLibrary, SystemLibrary, ThreadedExecutor};
use crate::repl::app::{Panel, ReplExecutor, Scroll};

const ENGINE_UPDATES_MAX_LATENCY_MS: u64 = 10;

/// Puts the terminal back the way it was found, including while panicking.
///
/// Without this a panic in the UI thread leaves the terminal in raw mode on
/// the alternate screen, and the trace goes to the redirected stderr where
/// nobody sees it.
struct TerminalGuard {
    stderr_log_path: std::path::PathBuf,
    /// Whether the keyboard enhancement flags were pushed, to be popped.
    enhanced_keyboard: bool,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.enhanced_keyboard {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
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
///
/// `local` is the workspace's own library, if it has one. `/save` writes
/// the history to [`repl::SESSION_FILE_NAME`] in `workspace_root`, and
/// `/load` reads it back. `load` is a session saved earlier to replay before
/// the first prompt.
pub fn run(
    sys: fn() -> SystemLibrary,
    local: Option<PackageLibrary>,
    workspace_root: &Path,
    load: Option<&Path>,
) -> AnyResult<()> {
    // Before the terminal is taken over, so a file that cannot be read is
    // reported where the user can see it.
    let replayed = load.map(repl::load_session).transpose()?;

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
    // Ctrl+Tab reaches the app as itself only through the kitty keyboard
    // protocol. Without it the terminal sends a plain Tab, and F2 and Alt+1
    // and Alt+2 are what change the tabs.
    //
    // A terminal that does not answer the query at all, which crossterm
    // reports as an error, does not speak the protocol either.
    let enhanced_keyboard = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced_keyboard {
        execute!(stdout, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
    }
    let guard = TerminalGuard { stderr_log_path: stderr_log_path.C(), enhanced_keyboard };
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run it, passing the stderr log path.
    let executor = ThreadedExecutor::spawn(sys, local);
    let mut app = RatatuiApp::with_stderr_log(executor, stderr_log_path);
    app.repl.set_session_path(workspace_root.join(repl::SESSION_FILE_NAME));
    if let Some(inputs) = replayed {
        app.repl.replay(inputs);
    }
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
        app.repl.poll_results();

        terminal.draw(|f| crate::render::ui(f, app))?;

        if app.repl.should_exit() {
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

    // A modal takes only Enter, which dismisses it; nothing works without an
    // engine, so its modal's Enter leaves.
    let enter = key.code == KeyCode::Enter;
    if app.repl.engine_is_dead() {
        if enter {
            app.repl.exit();
        }
    } else if app.repl.session_mismatch_message().is_some() {
        if enter {
            app.repl.dismiss_session_mismatch();
        }
    } else if app.repl.crash_modal_is_open() {
        if enter {
            app.repl.dismiss_crash_modal();
        }
    } else if app.repl.exit_asked() {
        // Another Ctrl+C exits; any other key goes on, and does nothing else.
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => app.repl.ask_to_exit(),
            _ => app.repl.resume(),
        }
    } else {
        // Normal input mode.
        let cancel = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        if !cancel {
            app.repl.disarm_cancel();
        }
        match key.code {
            // F2 reaches the app from any terminal; Ctrl+Tab only from those
            // speaking the kitty keyboard protocol.
            KeyCode::F(2) => app.repl.next_tab(),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::CONTROL) => app.repl.next_tab(),
            KeyCode::Char(digit @ '1'..='9') if key.modifiers.contains(KeyModifiers::ALT) => {
                if let Some(tab) = app.repl.tabs().get(digit as usize - '1' as usize) {
                    app.repl.select_tab(*tab);
                }
            }
            KeyCode::F(1) => app.repl.show_help(),
            KeyCode::Tab => app.repl.focus_next(),
            KeyCode::BackTab => app.repl.focus_prev(),
            // Ctrl+C twice throws away multiline input, as shells and REPLs
            // throw away a line, twice so a stray one loses nothing. Esc would
            // be the obvious key, but a terminal multiplexer holds it back to
            // see whether a sequence follows, and two in one read are parsed
            // as one.
            KeyCode::Char('c') if cancel && app.repl.focus() == Panel::Input && app.repl.input_cancels() => {
                app.cancel_input();
            }
            // Anywhere else Ctrl+C asks whether to exit, and another exits.
            KeyCode::Char('c') if cancel => app.repl.ask_to_exit(),
            // Ahead of the textarea, which only gets these in multiline mode.
            KeyCode::Up => app.handle_scroll_key(Scroll::LineUp, key),
            KeyCode::Down => app.handle_scroll_key(Scroll::LineDown, key),
            KeyCode::PageUp => app.handle_scroll_key(Scroll::PageUp, key),
            KeyCode::PageDown => app.handle_scroll_key(Scroll::PageDown, key),
            // The focused code panel deletes its selected unit, edits it in
            // the input, and writes a new one to go before it.
            KeyCode::Delete if app.repl.focus() == Panel::Code => app.repl.delete_selected(),
            KeyCode::Enter if app.repl.focus() == Panel::Code => app.begin_edit(),
            KeyCode::Insert if app.repl.focus() == Panel::Code => app.begin_insert(),
            // The rest are typing, which only the input panel takes.
            _ if app.repl.focus() != Panel::Input => {}
            // Enter submits a single line; in multiline input it is a newline,
            // and Alt+Enter submits.
            KeyCode::Enter if !app.repl.multiline_mode() || key.modifiers.contains(KeyModifiers::ALT) => {
                app.submit_input();
            }
            _ => app.handle_input_key(key),
        }
    }
}
