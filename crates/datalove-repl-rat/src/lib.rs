//! The Ratatui REPL application.

#![allow(unused)]

use rmx::prelude::*;

use datalove_repl as repl;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// Application state.
pub struct App {
    engine: repl::Engine,
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
    pub fn new(engine: repl::Engine) -> Self {
        Self {
            engine,
            input: String::new(),
            character_index: 0,
            messages: Vec::new(),
            menu_open: false,
            menu_selection: 0,
            should_exit: false,
        }
    }

    /// Move cursor to the left.
    pub fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    /// Move cursor to the right.
    pub fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    /// Enter a character at the cursor position.
    pub fn enter_char(&mut self, new_char: char) {
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
    pub fn delete_char(&mut self) {
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
    pub fn submit_input(&mut self) {
        self.handle_input()
    }

    /// Open the ESC menu.
    pub fn open_menu(&mut self) {
        self.menu_open = true;
        self.menu_selection = 0;
    }

    /// Close the ESC menu.
    pub fn close_menu(&mut self) {
        self.menu_open = false;
    }

    /// Move menu selection up.
    pub fn menu_up(&mut self) {
        self.menu_selection = self.menu_selection.saturating_sub(1);
    }

    /// Move menu selection down.
    pub fn menu_down(&mut self) {
        self.menu_selection = (self.menu_selection + 1).min(1);
    }

    pub fn menu_is_open(&self) -> bool {
        self.menu_open
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub fn set_should_exit(&mut self, val: bool) {
        self.should_exit = val;
    }

    /// Get the messages for testing.
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Execute the selected menu action.
    pub fn execute_menu_action(&mut self) {
        match self.menu_selection {
            0 => self.close_menu(), // Resume
            1 => self.should_exit = true, // Exit
            _ => {}
        }
    }
}

/// Render the UI.
pub fn ui(f: &mut Frame, app: &App) {
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
pub fn render_menu(f: &mut Frame, app: &App) {
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
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

impl App {

    // fixme: ideally this function is async
    //
    // on native we should be running the engine on another thread.
    // on wasm we should be running in another container with wasm-mt.
    fn parse_input(&self, input: &str) -> repl::CommandParse {
        repl::Command::parse(&input)
    }

    // fixme: ideally this function is async as above
    fn eval_command(&mut self, command: repl::Command) -> repl::Eval {
        self.engine.eval(command)
    }

    pub fn handle_input(&mut self) {
        if self.input.is_empty() {
            return;
        }

        let input_text = self.input.clone();

        match self.parse_input(&input_text) {
            repl::CommandParse::Empty => {
                self.input.clear();
                self.character_index = 0;
                self.messages.push(format!("> {}", input_text));
                self.messages.push(format!("  (empty)"));
            }
            repl::CommandParse::ReadAnotherLine => {
                todo!()
            }
            repl::CommandParse::Command(command) => {
                self.input.clear();
                self.character_index = 0;
                self.messages.push(format!("> {}", input_text));
                self.messages.push(format!("  ⏱"));
                match self.eval_command(command) {
                    repl::Eval::Nothing => {
                        self.messages.pop();
                        self.messages.push(format!("  nothing"));
                    }
                    repl::Eval::Exit => {
                        self.messages.pop();
                        self.messages.push(format!("  exiting"));
                        self.should_exit = true;
                    }
                    repl::Eval::Error(e) => {
                        self.messages.pop();
                        self.messages.push(format!("  error: {e}"));
                    }
                    repl::Eval::CallerInterpret(command) => {
                        match command {
                            repl::ReplCommand::Help => {
                                self.messages.pop();
                                self.messages.push(format!("  help"));
                            }
                            _ => bug!(),
                        }
                    }
                }
            }
        }
    }
}
