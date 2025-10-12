//! The Ratatui REPL application.

#![allow(unused)]

use rmx::prelude::*;

use datalove_repl as repl;

mod executor;
mod executor_threaded;
mod executor_blocking;

pub use executor::{ReplExecutor, ThreadedExecutor, BlockingExecutor};

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use executor::WorkerResponse;

/// In-flight request status.
#[derive(Debug)]
enum RequestStatus {
    Parsing,
    Evaluating { command: repl::Command },
}

/// Application state.
pub struct App<E: ReplExecutor> {
    /// Executor for parse and eval operations.
    executor: E,
    /// Next request ID.
    next_id: u64,
    /// Current in-flight request.
    in_flight: Option<(u64, String, RequestStatus)>,
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

impl<E: ReplExecutor> App<E> {
    /// Create a new app with the given executor type.
    /// The executor will construct its own Engine internally.
    pub fn with_executor() -> Self {
        Self {
            executor: E::new(),
            next_id: 0,
            in_flight: None,
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

    /// Check if there's work pending from the worker thread.
    pub fn has_pending_work(&self) -> bool {
        self.in_flight.is_some()
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

/// Platform-specific constructors.
#[cfg(not(target_arch = "wasm32"))]
impl App<ThreadedExecutor> {
    /// Create a new app using the threaded executor (native platforms).
    pub fn new() -> Self {
        Self::with_executor()
    }
}

#[cfg(target_arch = "wasm32")]
impl App<BlockingExecutor> {
    /// Create a new app using the blocking executor (WASM).
    pub fn new() -> Self {
        Self::with_executor()
    }
}

/// Render the UI.
pub fn ui<E: ReplExecutor>(f: &mut Frame, app: &App<E>) {
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
pub fn render_menu<E: ReplExecutor>(f: &mut Frame, app: &App<E>) {
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

impl<E: ReplExecutor> App<E> {
    /// Submit input for async processing.
    pub fn handle_input(&mut self) {
        if self.input.is_empty() {
            return;
        }

        // Don't submit if there's already a request in flight.
        if self.in_flight.is_some() {
            return;
        }

        let input_text = self.input.clone();
        self.input.clear();
        self.character_index = 0;

        // Assign a request ID.
        let id = self.next_id;
        self.next_id += 1;

        // Display the input and initial status.
        self.messages.push(format!("> {}", input_text));
        self.messages.push(format!("  ⏱"));

        // Send request to executor.
        self.executor.submit_parse_and_eval(id, input_text.clone());

        // Track in-flight request.
        self.in_flight = Some((id, input_text, RequestStatus::Parsing));
    }

    /// Poll for results from executor.
    pub fn poll_results(&mut self) {
        // Process all available responses.
        while let Some(response) = self.executor.try_recv_response() {
            match response {
                WorkerResponse::ParseResult { id, parse } => {
                    self.handle_parse_result(id, parse);
                }
                WorkerResponse::EvalResult { id, eval } => {
                    self.handle_eval_result(id, eval);
                }
            }
        }
    }

    fn handle_parse_result(&mut self, id: u64, parse: repl::CommandParse) {
        // Check if this matches our in-flight request.
        let Some((in_flight_id, input_text, status)) = &self.in_flight else {
            bug!();
        };

        assert_eq!(*in_flight_id, id);

        match parse {
            repl::CommandParse::Empty => {
                // Remove the "⏱" and replace with result.
                self.messages.pop();
                self.messages.push(format!("  (empty)"));
                self.in_flight = None;
            }
            repl::CommandParse::ReadAnotherLine => {
                // Remove the "⏱" and show error.
                self.messages.pop();
                self.messages.push(format!("  (read another line not yet supported)"));
                self.in_flight = None;
            }
            repl::CommandParse::Command(command) => {
                // Update status to evaluating.
                self.in_flight = Some((id, input_text.clone(), RequestStatus::Evaluating { command }));
            }
        }
    }

    fn handle_eval_result(&mut self, id: u64, eval: repl::Eval) {
        // Check if this matches our in-flight request.
        let Some((in_flight_id, _, _)) = &self.in_flight else {
            bug!();
        };

        assert_eq!(*in_flight_id, id);

        // Remove the "⏱" and replace with result.
        self.messages.pop();

        match eval {
            repl::Eval::Nothing => {
                self.messages.push(format!("  nothing"));
            }
            repl::Eval::Exit => {
                self.messages.push(format!("  exiting"));
                self.should_exit = true;
            }
            repl::Eval::Error(e) => {
                self.messages.push(format!("  error: {e}"));
            }
            repl::Eval::CallerInterpret(command) => {
                match command {
                    repl::ReplCommand::Help => {
                        self.messages.push(format!("  help"));
                    }
                    _ => {
                        self.messages.push(format!("  (unhandled repl command)"));
                    }
                }
            }
        }

        self.in_flight = None;
    }
}
