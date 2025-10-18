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

/// Actions that the core app requests from the UI layer.
#[derive(Debug, Clone)]
pub enum UiAction {
    /// No action needed.
    None,
    /// Set the input widget to multiline mode with the given lines.
    SetMultilineInput { lines: Vec<String> },
    /// Clear the input widget.
    ClearInput,
}

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

/// Core REPL application state, completely UI-agnostic.
pub struct ReplApp<E: ReplExecutor> {
    /// Executor for parse and eval operations.
    executor: E,
    /// Next request ID.
    next_id: u64,
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

impl<E: ReplExecutor> ReplApp<E> {
    /// Create a new core REPL app.
    pub fn new() -> Self {
        Self {
            executor: E::new(),
            next_id: 0,
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

    pub fn with_stderr_log(stderr_log_path: std::path::PathBuf) -> Self {
        Self {
            executor: E::new(),
            next_id: 0,
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

    /// Submit input text for processing.
    ///
    /// Takes input as a parameter instead of reading from UI widget.
    /// Returns UiAction to tell UI what to do (usually ClearInput).
    pub fn submit_input(&mut self, input_text: String) -> UiAction {
        if input_text.is_empty() {
            return UiAction::None;
        }

        let input = if !self.multiline_mode {
            repl::Input::Input(input_text.C())
        } else {
            repl::Input::Multiline(input_text.C())
        };

        self.multiline_mode = false;

        let id = self.next_id;
        self.next_id += 1;

        let entry = HistoryEntry::new(input_text, id);
        self.history.push(entry);

        self.executor.submit_parse_and_eval(id, input);

        UiAction::ClearInput
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

/// Ratatui-specific REPL application wrapper.
pub struct RatatuiApp<E: ReplExecutor> {
    /// Core UI-agnostic REPL logic.
    pub repl: ReplApp<E>,
    /// Ratatui text area widget.
    textarea: TextArea<'static>,
}

impl<E: ReplExecutor> RatatuiApp<E> {
    /// Create a new Ratatui app.
    pub fn new() -> Self {
        Self {
            repl: ReplApp::new(),
            textarea: TextArea::default(),
        }
    }

    pub fn new_with_stderr_log(stderr_log_path: std::path::PathBuf) -> Self {
        Self {
            repl: ReplApp::with_stderr_log(stderr_log_path),
            textarea: TextArea::default(),
        }
    }

    /// Get reference to the textarea for rendering.
    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn handle_input_key(&mut self, key: crossterm::event::KeyEvent) {
        self.textarea.input(key);
    }

    pub fn delete_char(&mut self) {
        self.textarea.delete_char();
    }

    pub fn move_cursor_left(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Back);
    }

    pub fn move_cursor_right(&mut self) {
        self.textarea.move_cursor(tui_textarea::CursorMove::Forward);
    }

    pub fn enter_char(&mut self, c: char) {
        self.textarea.insert_char(c);
    }

    /// Submit the current input.
    pub fn submit_input(&mut self) {
        let input_text = self.textarea.lines().join("\n");
        let action = self.repl.submit_input(input_text);
        self.process_ui_action(action);
    }

    /// Poll for executor results and process UI actions.
    pub fn poll_results(&mut self) {
        for action in self.repl.poll_results() {
            self.process_ui_action(action);
        }
    }

    fn process_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::SetMultilineInput { lines } => {
                self.textarea = TextArea::from(lines);
                self.textarea.move_cursor(tui_textarea::CursorMove::Bottom);
            }
            UiAction::ClearInput => {
                self.textarea = TextArea::default();
            }
            UiAction::None => {}
        }
    }

    // Delegated methods.
    pub fn open_menu(&mut self) {
        self.repl.open_menu();
    }

    pub fn close_menu(&mut self) {
        self.repl.close_menu();
    }

    pub fn menu_up(&mut self) {
        self.repl.menu_up();
    }

    pub fn menu_down(&mut self) {
        self.repl.menu_down();
    }

    pub fn execute_menu_action(&mut self) {
        self.repl.execute_menu_action();
    }

    pub fn dismiss_crash_modal(&mut self) {
        self.repl.dismiss_crash_modal();
    }

    pub fn set_should_exit(&mut self, val: bool) {
        self.repl.set_should_exit(val);
    }

    pub fn menu_is_open(&self) -> bool {
        self.repl.menu_is_open()
    }

    pub fn multiline_mode(&self) -> bool {
        self.repl.multiline_mode()
    }

    pub fn should_exit(&self) -> bool {
        self.repl.should_exit()
    }

    pub fn crash_modal_is_open(&self) -> bool {
        self.repl.crash_modal_message().is_some()
    }

    pub fn has_pending_work(&self) -> bool {
        self.repl.has_pending_work()
    }
}

/// Re-export the UI rendering function from the render module.
pub use render::ui;

impl<E: ReplExecutor> ReplApp<E> {
    /// Poll for results from executor and return UI actions to process.
    pub fn poll_results(&mut self) -> Vec<UiAction> {
        let mut actions = Vec::new();

        while let Some(response) = self.executor.try_recv_response() {
            match response {
                WorkerResponse::ParseResult { id, parse } => {
                    let action = self.handle_parse_result(id, parse);
                    actions.push(action);
                }
                WorkerResponse::EvalResult { id, eval } => {
                    self.handle_eval_result(id, eval);
                }
                WorkerResponse::EnvironmentUpdate { environment } => {
                    self.environment = environment;
                }
            }
        }

        actions
    }

    fn handle_parse_result(&mut self, id: u64, parse: repl::InputParse) -> UiAction {
        let entry = self.history.last_mut().X();

        assert_eq!(entry.id, id);

        match parse.clone() {
            repl::InputParse::Empty => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Empty;
                UiAction::None
            }
            repl::InputParse::ReadMultiline(input) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::ReadMultiline;
                self.multiline_mode = true;

                assert!(!input.contains('\n'));
                UiAction::SetMultilineInput {
                    lines: vec![input, String::new()]
                }
            }
            repl::InputParse::Command(command) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Evaluating { command };
                UiAction::None
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
                UiAction::ClearInput
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
