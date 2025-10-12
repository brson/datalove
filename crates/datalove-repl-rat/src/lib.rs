//! Ratatui frontend for the datalove REPL.

#![allow(unused)]

use rmx::prelude::*;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
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
    let mut app = App::new();
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

/// Application state.
struct App {
    /// Current input text.
    input: String,
    /// Cursor position in characters.
    character_index: usize,
    /// Display messages (submitted inputs and eval results).
    messages: Vec<String>,
    /// Whether the ESC menu is open.
    menu_open: bool,
    /// Selected menu item (0 = Resume, 1 = Exit).
    menu_selection: usize,
    /// Whether to exit the app.
    should_exit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            input: String::new(),
            character_index: 0,
            messages: Vec::new(),
            menu_open: false,
            menu_selection: 0,
            should_exit: false,
        }
    }

    /// Move cursor to the left.
    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    /// Move cursor to the right.
    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    /// Enter a character at the cursor position.
    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
    }

    /// Convert character index to byte index.
    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    /// Delete character before cursor.
    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.input.chars().skip(current_index);

            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    /// Clamp cursor to valid range.
    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    /// Submit the current input.
    fn submit_input(&mut self) {
        if !self.input.is_empty() {
            self.messages.push(format!("> {}", self.input));
            // TODO: Actual REPL evaluation would go here.
            self.messages.push(format!("  (not evaluated)"));
            self.input.clear();
            self.character_index = 0;
        }
    }

    /// Open the ESC menu.
    fn open_menu(&mut self) {
        self.menu_open = true;
        self.menu_selection = 0;
    }

    /// Close the ESC menu.
    fn close_menu(&mut self) {
        self.menu_open = false;
    }

    /// Move menu selection up.
    fn menu_up(&mut self) {
        self.menu_selection = self.menu_selection.saturating_sub(1);
    }

    /// Move menu selection down.
    fn menu_down(&mut self) {
        self.menu_selection = (self.menu_selection + 1).min(1);
    }

    /// Execute the selected menu action.
    fn execute_menu_action(&mut self) {
        match self.menu_selection {
            0 => self.close_menu(), // Resume
            1 => self.should_exit = true, // Exit
            _ => {}
        }
    }
}

/// Run the application loop.
fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> AnyResult<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if app.should_exit {
            break;
        }

        if let Event::Key(key) = event::read()? {
            // Only process press events, not repeat/release.
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if app.menu_open {
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

/// Render the UI.
fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Input area
            Constraint::Min(1),    // Display area
        ])
        .split(f.area());

    // Input area.
    let input_block = Block::default()
        .borders(Borders::ALL)
        .title("Input");
    let input = Paragraph::new(app.input.as_str())
        .block(input_block);
    f.render_widget(input, chunks[0]);

    // Set cursor position.
    f.set_cursor_position((
        chunks[0].x + app.character_index as u16 + 1,
        chunks[0].y + 1,
    ));

    // Display area.
    let display_block = Block::default()
        .borders(Borders::ALL)
        .title("Output");
    let messages_text: Vec<Line> = app
        .messages
        .iter()
        .map(|m| Line::from(m.as_str()))
        .collect();
    let display = Paragraph::new(messages_text)
        .block(display_block);
    f.render_widget(display, chunks[1]);

    // Render menu if open.
    if app.menu_open {
        render_menu(f, app);
    }
}

/// Render the ESC menu popup.
fn render_menu(f: &mut Frame, app: &App) {
    let area = centered_rect(20, 20, f.area());

    // Clear the background.
    f.render_widget(Clear, area);

    // Menu block.
    let menu_block = Block::default()
        .borders(Borders::ALL)
        .title("Menu");

    let menu_items = vec![
        if app.menu_selection == 0 {
            Line::from("> Resume").style(Style::default().fg(Color::Yellow))
        } else {
            Line::from("  Resume")
        },
        if app.menu_selection == 1 {
            Line::from("> Exit").style(Style::default().fg(Color::Yellow))
        } else {
            Line::from("  Exit")
        },
    ];

    let menu = Paragraph::new(menu_items)
        .block(menu_block);

    f.render_widget(menu, area);
}

/// Create a centered rectangle.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
