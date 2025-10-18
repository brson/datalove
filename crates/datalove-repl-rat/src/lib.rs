//! The Ratatui REPL application.

#![allow(unused)]

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use datalove_repl as repl;

mod executor;
mod executor_threaded;
mod executor_blocking;
mod render;

#[cfg(target_arch = "wasm32")]
mod executor_webworker;

#[cfg(target_arch = "wasm32")]
pub mod worker;

pub use executor::ReplExecutor;
pub use executor_threaded::ThreadedExecutor;
pub use executor_blocking::BlockingExecutor;

#[cfg(target_arch = "wasm32")]
pub use executor_webworker::WebWorkerExecutor;

use tui_textarea::TextArea;

use executor::WorkerResponse;

/// A single REPL history entry.
///
/// There is one of these for every line/multiline sent to the repl engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Request ID for this entry.
    pub id: u64,
    /// The input text submitted.
    pub input: String,
    /// Parse result if available.
    pub parse_result: Option<repl::InputParse>,
    /// Evaluation result if available.
    pub eval_result: Option<repl::Eval>,
    /// Entry status (lifecycle and outcome).
    pub status: EntryStatus,
}

/// Status of a REPL history entry.
/// Tracks both the processing lifecycle and the outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntryStatus {
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
    environment: Vec<(String, String, String)>,
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

    /// Get the crash modal message if present.
    pub fn crash_modal_message(&self) -> Option<&str> {
        self.crash_modal.as_deref()
    }

    /// Get the menu selection index.
    pub fn menu_selection(&self) -> usize {
        self.menu_selection
    }

    /// Get the environment variables.
    pub fn environment(&self) -> &[(String, String, String)] {
        &self.environment
    }

    /// Get the stderr log path.
    pub fn stderr_log_path(&self) -> Option<&std::path::Path> {
        self.stderr_log_path.as_deref()
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

/// Re-export the UI rendering function from the render module.
pub use render::ui;

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
            repl::Eval::SuccessExpr(_) => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::SuccessFun(_) => {
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
