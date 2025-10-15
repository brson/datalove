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
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table},
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
    parse_result: Option<repl::InputParse>,
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
    /// Read a multiline input.
    ReadMultiline,
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
    /// Crash modal state (contains crash message if open).
    crash_modal: Option<String>,
    /// Path to stderr log file (for displaying in crash modal).
    stderr_log_path: Option<std::path::PathBuf>,
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
            crash_modal: None,
            stderr_log_path: None,
        }
    }

    pub fn with_executor_and_stderr_log(stderr_log_path: std::path::PathBuf) -> Self {
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
            crash_modal: None,
            stderr_log_path: Some(stderr_log_path),
        }
    }

    /// Handle a key event for text input.
    #[cfg(not(target_arch = "wasm32"))]
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

    /// Delete the character before the cursor.
    pub fn delete_char(&mut self) {
        self.textarea.delete_char();
    }

    /// Move the cursor left.
    pub fn move_cursor_left(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Back);
    }

    /// Move the cursor right.
    pub fn move_cursor_right(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Forward);
    }

    /// Insert a character at the cursor position.
    pub fn enter_char(&mut self, c: char) {
        self.textarea.insert_char(c);
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

    pub fn multiline_mode(&self) -> bool {
        self.multiline_mode
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub fn set_should_exit(&mut self, val: bool) {
        self.should_exit = val;
    }

    pub fn crash_modal_is_open(&self) -> bool {
        self.crash_modal.is_some()
    }

    pub fn dismiss_crash_modal(&mut self) {
        self.crash_modal = None;
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

    /// Create a new app with stderr log path (for crash reporting).
    pub fn new_with_stderr_log(stderr_log_path: std::path::PathBuf) -> Self {
        Self::with_executor_and_stderr_log(stderr_log_path)
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

    // Render crash modal if present (takes priority over menu).
    if let Some(msg) = &app.crash_modal {
        render_crash_modal(f, app, msg);
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
                        repl::Eval::SuccessLet(eval_let) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  ✓ ",
                                    Style::default().fg(Color::Green),
                                ),
                                ratatui::text::Span::styled(
                                    &eval_let.name,
                                    Style::default().fg(Color::Cyan),
                                ),
                                ratatui::text::Span::raw(": "),
                                ratatui::text::Span::styled(
                                    &eval_let.ty,
                                    Style::default().fg(Color::Yellow),
                                ),
                                ratatui::text::Span::raw(" = "),
                                ratatui::text::Span::raw(&eval_let.value),
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
                        repl::Eval::CrashReset(msg) => {
                            lines.push(Line::from(vec![
                                ratatui::text::Span::styled(
                                    "  💥 ",
                                    Style::default().fg(Color::Red),
                                ),
                                ratatui::text::Span::styled(
                                    "crash reset",
                                    Style::default().fg(Color::Red),
                                ),
                            ]));
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
            EntryStatus::ReadMultiline => {
                lines.push(Line::from(vec![
                    ratatui::text::Span::styled(
                        "  → ",
                        Style::default().fg(Color::DarkGray),
                    ),
                    ratatui::text::Span::styled(
                        "read-multiline",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        // Separator between entries.
        lines.push(Line::from(""));
    }

    // Calculate scroll position to keep bottom visible.
    let content_height = lines.len();
    let viewport_height = area.height.saturating_sub(2) as usize; // Subtract borders.
    let scroll_offset = content_height.saturating_sub(viewport_height) as u16;

    let history = Paragraph::new(lines)
        .block(history_block)
        .scroll((scroll_offset, 0));

    f.render_widget(history, area);

    // Render scrollbar.
    let mut scrollbar_state = ScrollbarState::new(content_height)
        .position(scroll_offset as usize);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None);

    f.render_stateful_widget(
        scrollbar,
        area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
        &mut scrollbar_state,
    );
}

/// Render the input panel with multiline indicators.
fn render_input<E: ReplExecutor>(f: &mut Frame, app: &App<E>, area: Rect) {
    let title = if app.multiline_mode {
        "Input [Alt+Enter to submit]"
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

    if app.environment.is_empty() {
        // Display empty state message as a paragraph.
        let lines = vec![
            Line::from(vec![
                ratatui::text::Span::styled(
                    "(no variables defined)",
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
        ];
        let debug = Paragraph::new(lines)
            .block(debug_block);
        f.render_widget(debug, area);
    } else {
        // Create header row.
        let header = Row::new(vec![
            Cell::from("Name").style(Style::default().fg(Color::Yellow)),
            Cell::from("Type").style(Style::default().fg(Color::Yellow)),
        ])
        .height(1);

        // Create data rows.
        let rows = app.environment.iter().map(|(name, desc)| {
            Row::new(vec![
                Cell::from(name.as_str()).style(Style::default().fg(Color::Cyan)),
                Cell::from(desc.as_str()),
            ])
            .height(1)
        });

        // Create table with column constraints.
        let widths = [
            Constraint::Percentage(30),
            Constraint::Percentage(70),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(debug_block);

        f.render_widget(table, area);
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

/// Render the crash modal popup.
pub fn render_crash_modal<E: ReplExecutor>(f: &mut Frame, app: &App<E>, msg: &str) {
    let area = centered_rect(60, 40, f.area());

    // Clear the background.
    f.render_widget(Clear, area);

    // Crash modal block.
    let modal_block = Block::default()
        .borders(Borders::ALL)
        .title("💥 Engine Crash - Press Enter to Continue")
        .style(Style::default().fg(Color::Red));

    let mut lines = vec![
        Line::from(""),
        Line::from("The REPL engine encountered a panic and has been reset."),
        Line::from("All history and environment has been cleared."),
        Line::from(""),
        Line::from("Crash details:"),
        Line::from(""),
        Line::from(msg).style(Style::default().fg(Color::Yellow)),
    ];

    // Add stderr log path if available.
    if let Some(log_path) = &app.stderr_log_path {
        lines.push(Line::from(""));
        lines.push(Line::from(""));
        lines.push(Line::from("Full panic trace written to:"));
        lines.push(Line::from(log_path.display().to_string()).style(Style::default().fg(Color::Cyan)));
    }

    let modal = Paragraph::new(lines)
        .block(modal_block);

    f.render_widget(modal, area);
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
        // Don't submit if there's already a request in flight.
        if self.has_pending_work() {
            todo!(); // need to do something smart here
        }

        let input_text = self.input_text();

        if input_text.is_empty() {
            return;
        }

        let input = if !self.multiline_mode {
            repl::Input::Input(input_text.C())
        } else {
            repl::Input::Multiline(input_text.C())
        };

        // Clear the textarea and reset multiline mode.
        self.textarea = TextArea::default();
        self.multiline_mode = false;

        // Assign a request ID.
        let id = self.next_id;
        self.next_id += 1;

        // Create a new history entry with pending status and request ID.
        let entry = HistoryEntry::new(input_text, id);
        self.history.push(entry);

        // Send request to executor.
        self.executor.submit_parse_and_eval(id, input);
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

    fn handle_parse_result(&mut self, id: u64, parse: repl::InputParse) {
        let entry = self.history.last_mut().X();

        assert_eq!(entry.id, id);

        match parse.clone() {
            repl::InputParse::Empty => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Empty;
            }
            repl::InputParse::ReadMultiline(input) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::ReadMultiline;

                assert!(!input.contains('\n'));
                let lines = vec![
                    input,
                    String::new(),
                ];
                self.textarea = TextArea::from(lines);
                self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);

                self.multiline_mode = true;
            }
            repl::InputParse::Command(command) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Evaluating { command };
            }
            repl::InputParse::CrashReset(msg) => {
                // Engine crashed and reset during parse.
                // Set the entry status before clearing history.
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Error;
                // Drop the mutable reference to entry.
                let _ = entry;
                // Now we can clear history and set the modal.
                self.history.clear();
                self.environment.clear();
                self.crash_modal = Some(msg);
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
            repl::Eval::SuccessLet(_) => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::CallerInterpret(cmd) => {
                self.handle_caller_interpret(id, cmd);
            }
            repl::Eval::CrashReset(msg) => {
                // Engine crashed and reset during eval.
                // Set the entry status before clearing history.
                entry.status = EntryStatus::Error;
                // Drop the mutable reference to entry.
                let _ = entry;
                // Now we can clear history and set the modal.
                self.history.clear();
                self.environment.clear();
                self.crash_modal = Some(msg.clone());
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
