//! UI-agnostic REPL application state and executor trait.

use rmx::prelude::*;
use serde::{Serialize, Deserialize};

use crate as repl;
use repl::script;

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

/// One of the panels the user can focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    /// The session's code units.
    Code,
    Input,
    /// The tabbed panel, showing whichever [`Tab`] is selected.
    Bottom,
}

impl Panel {
    /// The next panel down, wrapping from the bottom to the top.
    fn next(self) -> Panel {
        match self {
            Panel::Code => Panel::Input,
            Panel::Input => Panel::Bottom,
            Panel::Bottom => Panel::Code,
        }
    }

    /// The next panel up, wrapping from the top to the bottom.
    fn prev(self) -> Panel {
        match self {
            Panel::Code => Panel::Bottom,
            Panel::Input => Panel::Code,
            Panel::Bottom => Panel::Input,
        }
    }
}

/// One of the tabs of the bottom panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tab {
    /// The bindings the session has.
    Environment,
    /// Every input, code and REPL commands alike, with what it came to.
    History,
}

impl Tab {
    /// Every tab, in the order they are shown and cycled through.
    pub const ALL: [Tab; 2] = [Tab::Environment, Tab::History];

    fn next(self) -> Tab {
        match self {
            Tab::Environment => Tab::History,
            Tab::History => Tab::Environment,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Environment => "Environment",
            Tab::History => "History",
        }
    }
}

/// What a code unit did in the session's last step.
///
/// A step is whatever the engine last did to the units. Units it did not
/// compile or run are [`UnitStep::Untouched`], however they fared before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitStep {
    /// Submitted, and not yet answered.
    Pending,
    /// Neither compiled nor run by the last step.
    Untouched,
    /// Ran to its end.
    Ran,
    /// Did not compile.
    Failed,
    /// Returned early through `!` or `?`.
    ReturnedEarly,
    /// Compiled, and not run because it reads from a unit that did not finish.
    Blocked,
}

/// One unit of the session, as the code panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeUnit<'a> {
    pub source: &'a str,
    pub step: UnitStep,
}

/// One step of scrolling, in the direction the user pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scroll {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
}

/// How far a pane is scrolled, bounded by what the renderer last laid out.
///
/// The offset counts lines in whichever direction the pane measures from;
/// the pane decides which way is up.
#[derive(Debug, Default)]
struct ScrollState {
    offset: usize,
    /// Lines a page scrolls by.
    page: usize,
    /// The furthest the offset can go.
    limit: usize,
}

impl ScrollState {
    /// Move the offset a step, away from zero or towards it.
    fn step(&mut self, scroll: Scroll, away_from_zero: bool) {
        let lines = match scroll {
            Scroll::LineUp | Scroll::LineDown => 1,
            Scroll::PageUp | Scroll::PageDown => self.page,
        };
        self.offset = if away_from_zero {
            (self.offset + lines).min(self.limit)
        } else {
            self.offset.saturating_sub(lines)
        };
    }

    /// A page is the pane less a line, so that scrolling leaves one line of
    /// what was just read on screen.
    fn record(&mut self, pane_lines: usize, limit: usize) {
        self.page = pane_lines.saturating_sub(1).max(1);
        self.limit = limit;
        self.offset = self.offset.min(limit);
    }
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
    /// The engine finished starting and is ready to take requests.
    EngineReady,
    /// The engine could not be started, or died after starting.
    ///
    /// Nothing the user submits will ever be answered after this.
    EngineDead { message: String },
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
    /// The panel keys go to.
    focus: Panel,
    /// The tab the bottom panel shows.
    tab: Tab,
    /// Whether the ESC menu is open.
    menu_open: bool,
    /// Selected menu item (0 = Resume, 1 = Exit).
    menu_selection: usize,
    /// Whether to exit the app.
    should_exit: bool,
    /// How far up from the bottom of the code the view is scrolled, in lines.
    ///
    /// Zero keeps the newest unit in sight, which is where a session spends
    /// nearly all its time. Only the renderer knows how tall the pane is or
    /// how many lines the units came to, so it is what bounds this.
    code_scroll: ScrollState,
    /// How far up from the bottom of the history the view is scrolled, in
    /// lines, the same way.
    history_scroll: ScrollState,
    /// How far down from the top of the environment the view is scrolled, in
    /// rows.
    environment_scroll: ScrollState,
    /// Whether the engine has reported itself ready.
    ///
    /// The engine compiles the system library before it can answer anything,
    /// which takes long enough that the UI says so.
    engine_ready: bool,
    /// Why the engine is gone, once it is.
    engine_dead: Option<String>,
    /// Crash modal state (contains crash message if open).
    crash_modal: Option<String>,
    /// Path to stderr log file (for displaying in crash modal).
    stderr_log_path: Option<std::path::PathBuf>,
    /// Where `/save` writes the history, if it has anywhere to.
    save_path: Option<std::path::PathBuf>,
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
            focus: Panel::Input,
            tab: Tab::Environment,
            menu_open: false,
            menu_selection: 0,
            should_exit: false,
            code_scroll: ScrollState::default(),
            history_scroll: ScrollState::default(),
            environment_scroll: ScrollState::default(),
            engine_ready: false,
            engine_dead: None,
            crash_modal: None,
            stderr_log_path: None,
            save_path: None,
        }
    }

    /// Have `/save` write the history to this path.
    pub fn set_save_path(&mut self, path: std::path::PathBuf) {
        self.save_path = Some(path);
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

        // Someone who submits wants to see what it did, wherever they had
        // scrolled to read.
        self.code_scroll.offset = 0;
        self.history_scroll.offset = 0;

        self.executor.submit_parse(id, input);

        UiAction::ClearInput
    }

    /// Replay a saved session's inputs, as if each had been submitted.
    ///
    /// They go straight to evaluation, since a saved input already says what
    /// it is, and reading a unit's text again would ask for more input on a
    /// definition that spans lines.
    pub fn replay(&mut self, inputs: Vec<repl::SavedInput>) {
        for repl::SavedInput { input, command } in inputs {
            let id = self.next_id;
            self.next_id += 1;

            let mut entry = HistoryEntry::new(input, id);
            entry.parse_result = Some(repl::InputParse::Command(command.C()));
            entry.status = EntryStatus::Evaluating { command: command.C() };
            self.history.push(entry);

            self.show_tab_for(&command);
            self.executor.submit_eval(id, command);
        }
    }

    /// Poll for results from executor and return UI actions to process.
    pub fn poll_results(&mut self) -> Vec<UiAction> {
        let mut actions = Vec::new();

        while let Some(response) = self.executor.try_recv_response() {
            match response {
                WorkerResponse::EngineReady => {
                    self.engine_ready = true;
                }
                WorkerResponse::EngineDead { message } => {
                    self.engine_dead = Some(message);
                }
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
                self.show_tab_for(&command);
                self.executor.submit_eval(id, command);
                UiAction::None
            }
        }
    }

    /// Show what a command will change: code changes the environment, and
    /// what a REPL command did is only in the history.
    fn show_tab_for(&mut self, command: &repl::Command) {
        self.tab = match command {
            repl::Command::Unit(_) => Tab::Environment,
            repl::Command::ReplCommand(_) => Tab::History,
        };
    }

    fn handle_eval_result(&mut self, id: u64, eval: repl::Eval, environment: Vec<(String, String, String)>) {
        let Some(entry) = find_entry(&mut self.history, id) else {
            return;
        };

        entry.eval_result = Some(eval.C());

        match &eval {
            // A unit that did not run to its end stays in the session, so
            // nothing is cleared; it is reported as the error it is.
            repl::Eval::Unit(
                script::Eval::Error(_) | script::Eval::ReturnedEarly(_) | script::Eval::Blocked { .. }
            ) => {
                entry.status = EntryStatus::Error;
            }
            repl::Eval::CommandError(_) => {
                entry.status = EntryStatus::Error;
            }
            repl::Eval::Unit(script::Eval::Nothing) => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::Unit(script::Eval::Success(_)) => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::Unit(script::Eval::SuccessExpr(_)) => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::Saved { .. } => {
                entry.status = EntryStatus::Success;
            }
            repl::Eval::CallerInterpret(cmd) => {
                self.handle_caller_interpret(id, cmd);
            }
            repl::Eval::Unit(script::Eval::ErrorReset(msg)) => {
                // A runtime error emptied the session, so what the history
                // and environment show of it is gone, as after a crash.
                entry.status = EntryStatus::Error;
                let _ = entry;
                self.history.clear();
                self.environment.clear();
                self.crash_modal = Some(format!("{msg}\nThe session was reset."));
            }
            repl::Eval::Unit(script::Eval::CrashReset(msg)) => {
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
            repl::ReplCommand::Reset => {
                // Input submitted after the reset was evaluated in the new
                // session, so only what came before it is forgotten.
                let _ = entry;
                self.history.retain(|entry| entry.id > id);
            }
            repl::ReplCommand::Save => {
                let _ = entry;
                let eval = self.save(id).unwrap_or_else(|e| repl::Eval::CommandError(fmt!("{e:#}")));
                let entry = find_entry(&mut self.history, id).expect("the entry is still there");
                entry.status = match eval {
                    repl::Eval::CommandError(_) => EntryStatus::Error,
                    _ => EntryStatus::Success,
                };
                entry.eval_result = Some(eval);
            }
        }
    }

    /// Write the history up to and including the `/save` with this id.
    ///
    /// Everything the history holds goes in, failures and REPL commands
    /// included, as it was typed. What was typed after the `/save` is left
    /// out, and is the only part of the history that can still be waiting to
    /// be parsed: the engine answers in order.
    ///
    /// A line that only opened a multiline input is left out too, since the
    /// input it opened holds it again.
    fn save(&self, save_id: u64) -> AnyResult<repl::Eval> {
        use datalove_datafun::package_load_worldfile::{WorldfileSection, write_worldfile};

        let Some(path) = &self.save_path else {
            bail!("this session was not given anywhere to save to");
        };

        let mut sections = Vec::new();
        for entry in self.history.iter().filter(|entry| entry.id <= save_id) {
            let parse = entry.parse_result.as_ref()
                .expect("the engine parsed everything before the save");
            let section = match parse {
                repl::InputParse::Empty | repl::InputParse::ReadMultiline(_) => continue,
                repl::InputParse::Command(repl::Command::Unit(script::Unit::Fragment(source))) => {
                    WorldfileSection::ScriptFragment { source: source.C() }
                }
                repl::InputParse::Command(repl::Command::Unit(script::Unit::Expression(source))) => {
                    WorldfileSection::ScriptExpr { source: source.C() }
                }
                repl::InputParse::Command(repl::Command::ReplCommand(_)) => {
                    WorldfileSection::ReplCommand { source: entry.input.trim().S() }
                }
            };
            sections.push(section);
        }

        std::fs::write(path, write_worldfile(&sections)?)
            .with_context(|| fmt!("failed to write {}", path.display()))?;
        Ok(repl::Eval::Saved { path: path.display().to_string(), inputs: sections.len() })
    }

    /// Move focus to the next panel down.
    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    /// Move focus to the next panel up.
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
    }

    /// The panel keys go to.
    pub fn focus(&self) -> Panel {
        self.focus
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

    /// The panel a scroll key goes to, which for the input moves its cursor.
    ///
    /// A focused pane takes the plain keys and its opposite takes them with
    /// Ctrl. The single-line input has no use for them, so it passes them to
    /// the code, and Ctrl to the bottom panel. Multiline input keeps them
    /// all, with and without Ctrl, for moving its cursor.
    pub fn scroll_target(&self, ctrl: bool) -> Panel {
        match (self.focus, ctrl) {
            (Panel::Code, false) | (Panel::Bottom, true) => Panel::Code,
            (Panel::Bottom, false) | (Panel::Code, true) => Panel::Bottom,
            (Panel::Input, _) if self.multiline_mode => Panel::Input,
            (Panel::Input, false) => Panel::Code,
            (Panel::Input, true) => Panel::Bottom,
        }
    }

    /// The tab the bottom panel shows.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// Show the next tab of the bottom panel, wrapping from the last to the
    /// first.
    pub fn next_tab(&mut self) {
        self.tab = self.tab.next();
    }

    /// Show a tab of the bottom panel.
    pub fn select_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// Scroll the code, where up is towards the first unit.
    pub fn scroll_code(&mut self, scroll: Scroll) {
        let up = matches!(scroll, Scroll::LineUp | Scroll::PageUp);
        self.code_scroll.step(scroll, up);
    }

    /// Scroll whichever tab the bottom panel shows.
    pub fn scroll_bottom(&mut self, scroll: Scroll) {
        match self.tab {
            Tab::Environment => self.scroll_environment(scroll),
            Tab::History => self.scroll_history(scroll),
        }
    }

    /// Scroll the history, where up is towards the oldest entry.
    pub fn scroll_history(&mut self, scroll: Scroll) {
        let up = matches!(scroll, Scroll::LineUp | Scroll::PageUp);
        self.history_scroll.step(scroll, up);
    }

    /// Scroll the environment, where up is towards the first row.
    pub fn scroll_environment(&mut self, scroll: Scroll) {
        let up = matches!(scroll, Scroll::LineUp | Scroll::PageUp);
        self.environment_scroll.step(scroll, !up);
    }

    /// How far up from the bottom the code is scrolled.
    pub fn code_scroll_back(&self) -> usize {
        self.code_scroll.offset
    }

    /// How far up from the bottom the history is scrolled.
    pub fn history_scroll_back(&self) -> usize {
        self.history_scroll.offset
    }

    /// How far down from the top the environment is scrolled.
    pub fn environment_scroll(&self) -> usize {
        self.environment_scroll.offset
    }

    /// Tell the app the shape of the code the renderer just laid out.
    ///
    /// Scrolling past the first unit lands on it rather than running off into
    /// nothing.
    pub fn record_code_view(&mut self, pane_lines: usize, scroll_back_limit: usize) {
        self.code_scroll.record(pane_lines, scroll_back_limit);
    }

    /// Tell the app the shape of the history the renderer just laid out.
    ///
    /// Scrolling past the oldest entry lands on it rather than running off
    /// into nothing.
    pub fn record_history_view(&mut self, pane_lines: usize, scroll_back_limit: usize) {
        self.history_scroll.record(pane_lines, scroll_back_limit);
    }

    /// Tell the app the shape of the environment the renderer just laid out.
    pub fn record_environment_view(&mut self, pane_rows: usize, scroll_limit: usize) {
        self.environment_scroll.record(pane_rows, scroll_limit);
    }

    /// Whether the engine has finished starting.
    pub fn engine_is_ready(&self) -> bool {
        self.engine_ready
    }

    /// Why the engine is gone, if it is.
    pub fn engine_dead_message(&self) -> Option<&str> {
        self.engine_dead.as_deref()
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

    /// The session's units, in order, and what each did in the last step.
    ///
    /// **The units are the history's code entries.** The history drops what
    /// a reset takes out of the engine's session -- everything before a
    /// `/reset`, and everything at a runtime error or a crash -- and a unit
    /// that fails stays in both, so the two hold the same units. A unit is
    /// here from when it parses, before the engine has answered.
    ///
    /// The last step is the last unit the engine answered for, and every unit
    /// before it was left alone by that step.
    pub fn code_units(&self) -> Vec<CodeUnit<'_>> {
        let units: Vec<&HistoryEntry> = self.history.iter()
            .filter(|entry| matches!(
                entry.parse_result,
                Some(repl::InputParse::Command(repl::Command::Unit(_))),
            ))
            .collect();
        let last_step = units.iter().rposition(|entry| entry.eval_result.is_some());

        units.iter().enumerate()
            .map(|(index, entry)| {
                let Some(repl::InputParse::Command(repl::Command::Unit(unit))) = &entry.parse_result else {
                    unreachable!("only code entries were kept");
                };
                let source = match unit {
                    script::Unit::Fragment(source) | script::Unit::Expression(source) => source.as_str(),
                };
                let step = match &entry.eval_result {
                    None => UnitStep::Pending,
                    Some(_) if Some(index) != last_step => UnitStep::Untouched,
                    Some(repl::Eval::Unit(eval)) => match eval {
                        script::Eval::Nothing | script::Eval::Success(_) | script::Eval::SuccessExpr(_) => UnitStep::Ran,
                        script::Eval::Error(_) => UnitStep::Failed,
                        script::Eval::ReturnedEarly(_) => UnitStep::ReturnedEarly,
                        script::Eval::Blocked { .. } => UnitStep::Blocked,
                        script::Eval::ErrorReset(_) | script::Eval::CrashReset(_) => {
                            unreachable!("a reset clears the history it would be in")
                        }
                    },
                    Some(other) => unreachable!("a unit was answered as a REPL command: {other:?}"),
                };
                CodeUnit { source, step }
            })
            .collect()
    }
}
