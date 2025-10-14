//! The Ratatui REPL application.

#![allow(unused)]

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_repl as repl;

mod executor;
mod executor_threaded;
mod executor_blocking;

#[cfg(target_arch = "wasm32")]
mod executor_webworker;

#[cfg(target_arch = "wasm32")]
pub mod worker;

pub use executor::ReplExecutor;
pub use executor_threaded::ThreadedExecutor;
pub use executor_blocking::BlockingExecutor;

#[cfg(target_arch = "wasm32")]
pub use executor_webworker::WebWorkerExecutor;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use tui_textarea::TextArea;

use executor::WorkerResponse;

/// A single REPL history entry.
///
/// There is one of these for every line/multiline sent to the repl engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Request ID for this entry.
    id: u64,
    /// The input text submitted.
    input: String,
    /// Parse result if available.
    parse_result: Option<repl::CommandParse>,
    /// Evaluation result if available.
    eval_result: Option<repl::Eval>,
    /// Entry status (lifecycle and outcome).
    status: EntryStatus,
}

/// Status of a REPL history entry.
/// Tracks both the processing lifecycle and the outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
enum EntryStatus {
    /// Request is being parsed.
    Parsing,
    /// Request is being evaluated.
    Evaluating { command: repl::Command },
    /// Request completed successfully.
    Success,
    /// Request completed with an error.
    Error,
    /// Request completed with empty input.
    Empty,
    /// Read another line.
    KeepReading,
}

impl HistoryEntry {
    fn new(input: String, id: u64) -> Self {
        Self {
            id,
            input,
            parse_result: None,
            eval_result: None,
            status: EntryStatus::Parsing,
        }
    }
}

/// Application state.
pub struct App<E: ReplExecutor> {
    /// Executor for parse and eval operations.
    executor: E,
    /// Next request ID.
    next_id: u64,
    /// Text area for input.
    textarea: TextArea<'static>,
    /// History of REPL entries (interactive cards).
    history: Vec<HistoryEntry>,
    /// Current environment variables (for debug pane).
    environment: Vec<(String, String)>,
    /// Whether we're in multiline mode.
    multiline_mode: bool,
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
            textarea: TextArea::default(),
            history: Vec::new(),
            environment: Vec::new(),
            multiline_mode: false,
            menu_open: false,
            menu_selection: 0,
            should_exit: false,
        }
    }

    /// Handle a key event for text input.
    pub fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        self.textarea.input(key);
    }

    /// Get the current input text.
    pub fn input_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Get a reference to the textarea for rendering.
    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    /// Get a mutable reference to the textarea.
    pub fn textarea_mut(&mut self) -> &mut TextArea<'static> {
        &mut self.textarea
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

    /// Get the history entries for testing.
    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// Check if there's work pending from the worker thread.
    pub fn has_pending_work(&self) -> bool {
        self.history.last().map_or(false, |e| {
            matches!(e.status, EntryStatus::Parsing | EntryStatus::Evaluating { .. })
        })
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
impl App<WebWorkerExecutor> {
    /// Create a new app using the web worker executor (WASM).
    pub fn new() -> Self {
        Self::with_executor()
    }
}

/// Render the UI.
pub fn ui<E: ReplExecutor>(f: &mut Frame, app: &App<E>) {
    // Three-panel layout: history (top), input (middle), debug (bottom).
    // Single-line mode: Input centered like a Cylon visor.
    // Multi-line mode: Input grows downward from center to 1/3 screen.

    let screen_height = f.area().height;

    let constraints = if app.multiline_mode {
        // Multi-line mode: Keep history at same height, input grows to 1/3, debug compressed.
        let top_height = (screen_height.saturating_sub(3)) / 2;
        vec![
            Constraint::Length(top_height),  // History (same as single-line center point)
            Constraint::Percentage(33),      // Input (1/3 of screen)
            Constraint::Min(0),              // Debug (fills remaining space)
        ]
    } else {
        // Single-line mode: Input centered vertically.
        vec![
            Constraint::Fill(1),      // Top half (centers input)
            Constraint::Length(3),    // Input (3 lines: 1 text + 2 borders)
            Constraint::Fill(1),      // Bottom half (centers input)
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(f.area());

    // History panel - scrollable display of interactive cards.
    render_history(f, app, chunks[0]);

    // Input panel - current text input with multiline indicators.
    render_input(f, app, chunks[1]);

    // Debug panel - table of variables and values.
    render_debug_pane(f, app, chunks[2]);

    // Render menu if open.
    if app.menu_open {
        render_menu(f, app);
    }
}

/// Render the history panel with interactive cards.
fn render_history<E: ReplExecutor>(f: &mut Frame, app: &App<E>, area: Rect) {
    let history_block = Block::default()
        .borders(Borders::ALL)
        .title("History");

    let mut lines: Vec<Line> = Vec::new();

    for entry in &app.history {
        // Input line with prompt.
        lines.push(Line::from(vec![
            ratatui::text::Span::styled(
                "> ",
                Style::default().fg(Color::Cyan),
            ),
            ratatui::text::Span::raw(&entry.input),
        ]));

        // Output/status line.
        match &entry.status {
            EntryStatus::Parsing => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ⏱ ",
                        Style::default().fg(Color::Yellow),
                    ),
                    ratatui::text::Span::styled(
                        "parsing...",
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }
            EntryStatus::Evaluating { .. } => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ⏱ ",
                        Style::default().fg(Color::Yellow),
                    ),
                    ratatui::text::Span::styled(
                        "evaluating...",
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }
            EntryStatus::Success => {
                if let Some(eval) = &entry.eval_result {
                    match eval {
                        repl::Eval::Nothing => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✓ ",
                                    Style::default().fg(Color::Green),
                                ),
                                ratatui::text::Span::raw("nothing"),
                            ]));
                        }
                        repl::Eval::Error(e) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✗ ",
                                    Style::default().fg(Color::Red),
                                ),
                                ratatui::text::Span::styled(
                                    format!("error: {e}"),
                                    Style::default().fg(Color::Red),
                                ),
                            ]));
                        }
                        repl::Eval::CallerInterpret(command) => {
                            match command {
                                repl::ReplCommand::Help => {
                                    lines.push(Line::from(vec![
                                        ratatui::text::Span::styled(
                                            "  ℹ ",
                                            Style::default().fg(Color::Blue),
                                        ),
                                        ratatui::text::Span::raw("help"),
                                    ]));
                                }
                                _ => {
                                    lines.push(Line::from(vec![
                                        ratatui::text::Span::styled(
                                            "  ℹ ",
                                            Style::default().fg(Color::Blue),
                                        ),
                                        ratatui::text::Span::raw("(repl command)"),
                                    ]));
                                }
                            }
                        }
                    }
                }
            }
            EntryStatus::Error => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  ✗ ",
                        Style::default().fg(Color::Red),
                    ),
                    ratatui::text::Span::styled(
                        "error",
                        Style::default().fg(Color::Red),
                    ),
                ]));
            }
            EntryStatus::Empty => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  · ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    ratatui::text::Span::styled(
                        "(empty)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            EntryStatus::KeepReading => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  → ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    ratatui::text::Span::styled(
                        "keep-reading",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        // Separator between entries.
        lines.push(Line::from(""));
    }

    let history = Paragraph::new(lines)
        .block(history_block);
    f.render_widget(history, area);
}

/// Render the input panel with multiline indicators.
fn render_input<E: ReplExecutor>(f: &mut Frame, app: &App<E>, area: Rect) {
    let title = if app.multiline_mode {
        "Input [Shift+Enter]"
    } else {
        "Input [Enter]"
    };

    let border_style = if app.multiline_mode {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let mut textarea = app.textarea.clone();
    textarea.set_block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(border_style)
    );
    textarea.set_cursor_line_style(Style::default());

    f.render_widget(&textarea, area);
}

fn render_debug_pane<E: ReplExecutor>(f: &mut Frame, app: &App<E>, area: Rect) {
    let debug_block = Block::default()
        .borders(Borders::ALL)
        .title("Environment");

    let mut lines: Vec<Line> = Vec::new();

    if app.environment.is_empty() {
        lines.push(Line::from(vec![
            ratatui::text::Span::styled(
                "(no variables defined)",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    } else {
        // Variables.
        for (name, value) in &app.environment {
            lines.push(Line::from(vec![
                ratatui::text::Span::styled(
                    name,
                    Style::default().fg(Color::Green),
                ),
                ratatui::text::Span::raw("  │  "),
                ratatui::text::Span::raw(value),
            ]));
        }
    }

    let debug = Paragraph::new(lines)
        .block(debug_block);
    f.render_widget(debug, area);
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
        let input_text = self.input_text();

        if input_text.is_empty() {
            return;
        }

        // Don't submit if there's already a request in flight.
        if self.has_pending_work() {
            todo!(); // need to do something smart here
        }

        // Clear the textarea.
        self.textarea = TextArea::default();

        // Assign a request ID.
        let id = self.next_id;
        self.next_id += 1;

        // Create a new history entry with pending status and request ID.
        let entry = HistoryEntry::new(input_text.clone(), id);
        self.history.push(entry);

        // Send request to executor.
        self.executor.submit_parse_and_eval(id, input_text);
    }

    pub fn poll_results(&mut self) {
        while let Some(response) = self.executor.try_recv_response() {
            match response {
                WorkerResponse::ParseResult { id, parse } => {
                    self.handle_parse_result(id, parse);
                }
                WorkerResponse::EvalResult { id, eval } => {
                    self.handle_eval_result(id, eval);
                }
                WorkerResponse::EnvironmentUpdate { environment } => {
                    self.environment = environment;
                }
            }
        }
    }

    fn handle_parse_result(&mut self, id: u64, parse: repl::CommandParse) {
        let entry = self.history.last_mut().X();

        assert_eq!(entry.id, id);

        match parse.clone() {
            repl::CommandParse::Empty => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Empty;
            }
            repl::CommandParse::ReadAnotherLine => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::KeepReading;

                // Restore input with newline and enter multiline mode.
                let restored_text = entry.input.C() + "\n";
                self.textarea = TextArea::from(restored_text.lines().map(|s| s.to_string()));
                self.multiline_mode = true;
            }
            repl::CommandParse::Command(command) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Evaluating { command };
            }
        }
    }

    fn handle_eval_result(&mut self, id: u64, eval: repl::Eval) {
        let entry = self.history.last_mut().X();

        assert_eq!(entry.id, id);

        entry.eval_result = Some(eval.clone());

        match &eval {
            repl::Eval::Error(_) => {
                entry.status = EntryStatus::Error;
            }
            repl::Eval::Nothing => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::CallerInterpret(cmd) => {
                self.handle_caller_interpret(id, cmd);
            }
        }

        self.multiline_mode = false;
    }

    fn handle_caller_interpret(&mut self, id: u64, cmd: &repl::ReplCommand) {
        let entry = self.history.last_mut().X();
        assert_eq!(entry.id, id);

        match cmd {
            repl::ReplCommand::Unknown => bug!(),
            repl::ReplCommand::Exit => {
                entry.status = EntryStatus::Success;
                self.should_exit = true;
            }
            repl::ReplCommand::Help => {
                // todo
                entry.status = EntryStatus::Success;
            },
        }
    }
}
