//! Crossterm Ratatui REPL.

#![allow(unused)]

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
use std::io;

/// Run the ratatui-based REPL.
pub fn run() -> AnyResult<()> {
    // Setup terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run it.
    let engine = datalove_repl::Engine::new()?;
    let mut app = datalove_repl_rat::App::new(engine);
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
fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut datalove_repl_rat::App,
) -> AnyResult<()> {
    loop {
        terminal.draw(|f| datalove_repl_rat::ui(f, app))?;

        if app.should_exit() {
            break;
        }

        if let Event::Key(key) = event::read()? {
            // Only process press events, not repeat/release.
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if app.menu_is_open() {
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
                    KeyCode::Enter => app.submit_input(),
                    KeyCode::Char(c) => app.enter_char(c),
                    KeyCode::Backspace => app.delete_char(),
                    KeyCode::Left => app.move_cursor_left(),
                    KeyCode::Right => app.move_cursor_right(),
                    KeyCode::Esc => app.open_menu(),
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
