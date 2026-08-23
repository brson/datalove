//! UI-agnostic REPL application state and executor trait.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use crate as repl;

/// Actions that the core app requests from the UI layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
///
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

/// Find the history entry a worker response belongs to.
///
/// Returns `None` when the entry is gone, which happens to requests that were
/// still in flight when a crash reset cleared the history.
fn find_entry(history: &mut [HistoryEntry], id: u64) -> Option<&mut HistoryEntry> {
    history.iter_mut().rev().find(|entry| entry.id == id)
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

/// Response from the worker/executor.
#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerResponse {
    ParseResult { id: u64, parse: repl::InputParse },
    EvalResult { id: u64, eval: repl::Eval, environment: Vec<(String, String, String)> },
}

/// Trait for executing REPL parse and eval operations.
///
/// Implementations can be synchronous or asynchronous,
/// single-threaded or multi-threaded.
pub trait ReplExecutor {
    /// Submit a parse request.
    fn submit_parse(&mut self, id: u64, input: repl::Input);

    /// Submit an eval request.
    fn submit_eval(&mut self, id: u64, command: repl::Command);

    /// Try to receive a response.
    /// Returns None if no response is available.
    fn try_recv_response(&mut self) -> Option<WorkerResponse>;
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
    pub fn with_stderr_log(executor: E, stderr_log_path: std::path::PathBuf) -> Self {
        let mut app = Self::with_executor(executor);
        app.stderr_log_path = Some(stderr_log_path);
        app
    }

    /// Create a core REPL app around an executor.
    pub fn with_executor(executor: E) -> Self {
        Self {
            executor,
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

        self.executor.submit_parse(id, input);

        UiAction::ClearInput
    }

    /// Poll for results from executor and return UI actions to process.
    pub fn poll_results(&mut self) -> Vec<UiAction> {
        let mut actions = Vec::new();

        while let Some(response) = self.executor.try_recv_response() {
            match response {
                WorkerResponse::ParseResult { id, parse } => {
                    let action = self.handle_parse_result(id, parse);
                    actions.push(action);
                }
                WorkerResponse::EvalResult { id, eval, environment } => {
                    self.handle_eval_result(id, eval, environment);
                }
            }
        }

        actions
    }

    fn handle_parse_result(&mut self, id: u64, parse: repl::InputParse) -> UiAction {
        let Some(entry) = find_entry(&mut self.history, id) else {
            return UiAction::None;
        };

        match parse.C() {
            repl::InputParse::Empty => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Empty;
                UiAction::None
            }
            repl::InputParse::ReadMultiline(input) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::ReadMultiline;
                self.multiline_mode = true;

                // The input carries however many lines the user submitted at
                // once; the empty line at the end is where they continue.
                let mut lines: Vec<String> = input.lines().map(S).collect();
                lines.push(String::new());
                UiAction::SetMultilineInput { lines }
            }
            repl::InputParse::Command(command) => {
                entry.parse_result = Some(parse);
                entry.status = EntryStatus::Evaluating { command: command.C() };
                self.executor.submit_eval(id, command);
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

    fn handle_eval_result(&mut self, id: u64, eval: repl::Eval, environment: Vec<(String, String, String)>) {
        let Some(entry) = find_entry(&mut self.history, id) else {
            return;
        };

        entry.eval_result = Some(eval.C());

        match &eval {
            repl::Eval::Error(_) => {
                entry.status = EntryStatus::Error;
            }
            repl::Eval::Nothing => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::Success(_) => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::SuccessExpr(_) => {
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
                self.crash_modal = Some(msg.C());
            }
        }

        // Update environment from the eval response.
        self.environment = environment;

        self.multiline_mode = false;
    }

    fn handle_caller_interpret(&mut self, id: u64, cmd: &repl::ReplCommand) {
        let entry = find_entry(&mut self.history, id)
            .expect("the entry was found by the caller");

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

    /// Execute the selected menu action.
    pub fn execute_menu_action(&mut self) {
        match self.menu_selection {
            0 => self.close_menu(), // Resume
            1 => self.should_exit = true, // Exit
            _ => {}
        }
    }

    pub fn dismiss_crash_modal(&mut self) {
        self.crash_modal = None;
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
}
