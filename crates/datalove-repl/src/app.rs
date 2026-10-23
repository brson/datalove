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
    /// Set the input widget to single-line mode with the given text.
    SetInput { text: String },
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

/// What the input is writing, when it is not a new line at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Editing {
    /// The source of the unit with this number.
    Unit(u32),
    /// A new unit, to go before the one with this number.
    Before(u32),
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

/// How a code unit fared the last time it was compiled or run.
///
/// A unit keeps its status until a step compiles or runs it again: one a
/// step leaves alone is shown as it last was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitStatus {
    /// Submitted, and not yet answered.
    Pending,
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
    /// The unit's number, or `None` for one on its way to the engine, which
    /// numbers it when it gets there.
    pub number: Option<u32>,
    pub source: &'a str,
    pub status: UnitStatus,
    /// Whether the last step to change the session analyzed the unit again.
    pub compiled: bool,
    /// Whether the last step to change the session ran the unit.
    pub ran: bool,
}

/// A unit of the session as the app keeps it, and its status.
#[derive(Debug, Clone)]
struct SessionUnit {
    numbered: repl::NumberedUnit,
    status: UnitStatus,
}

/// Where the renderer last put each code unit, so the selection can be
/// scrolled into view.
#[derive(Debug, Default)]
struct CodeLayout {
    /// Lines the pane showed.
    pane_lines: usize,
    /// Each unit's number and the lines of the content it took, from the top.
    units: Vec<(u32, std::ops::Range<usize>)>,
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
    /// What the input was read as.
    pub parse_result: repl::InputParse,
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
    /// Request is being evaluated; its command is the entry's parse.
    Evaluating,
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
/// It is always there. Commands go to the engine in the order they were
/// entered and it answers them in that order, and the history only forgets
/// the entries up to one whose answer reset the session, by which time
/// everything before that one has been answered.
fn find_entry(history: &mut [HistoryEntry], id: u64) -> &mut HistoryEntry {
    history.iter_mut().rev().find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("a response for entry {id}, which the history has forgotten"))
}

impl HistoryEntry {
    fn new(id: u64, input: String, parse_result: repl::InputParse, status: EntryStatus) -> Self {
        Self { id, input, parse_result, eval_result: None, status }
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
    EvalResult {
        id: u64,
        eval: repl::Eval,
        environment: Vec<script::EnvBinding>,
        /// The units in the engine's session once it had answered.
        session: Vec<repl::NumberedUnit>,
        /// What the last step to change the session did to its units.
        step: Vec<repl::UnitStep>,
    },
}

/// What evaluates commands for the app, and answers in the order they were
/// submitted.
///
/// Implementations can be synchronous or asynchronous,
/// single-threaded or multi-threaded.
pub trait ReplExecutor {
    /// Submit an eval request.
    fn submit_eval(&mut self, id: u64, command: repl::Command);

    /// Try to receive a response.
    /// Returns None if no response is available.
    fn try_recv_response(&mut self) -> Option<WorkerResponse>;
}

/// Core REPL application state, completely UI-agnostic.
pub struct ReplApp<E: ReplExecutor> {
    /// What evaluates the commands.
    executor: E,
    /// Next request ID.
    next_id: u64,
    /// History of REPL entries (interactive cards).
    history: Vec<HistoryEntry>,
    /// The bindings the session offers, as the engine last reported them.
    environment: Vec<script::EnvBinding>,
    /// Whether we're in multiline mode.
    multiline_mode: bool,
    /// Whether a cancel was asked for in multiline input, or in an edit, so
    /// another carries it out.
    cancel_armed: bool,
    /// What the input is editing, if it is.
    editing: Option<Editing>,
    /// What was typed when the edit began, and whether it was multiline, to
    /// be put back when the edit ends.
    stashed: Option<(String, bool)>,
    /// The panel keys go to.
    focus: Panel,
    /// The tab the bottom panel shows.
    tab: Tab,
    /// Whether the dialog asking whether to exit is open.
    exit_asked: bool,
    /// Whether to exit the app.
    should_exit: bool,
    /// How far up from the bottom of the code the view is scrolled, in lines.
    ///
    /// Zero keeps the newest unit in sight, which is where a session spends
    /// nearly all its time. Only the renderer knows how tall the pane is or
    /// how many lines the units came to, so it is what bounds this.
    code_scroll: ScrollState,
    /// The code as the renderer last laid it out.
    code_layout: CodeLayout,
    /// The code unit selected, by its number, while the code panel has focus.
    ///
    /// A unit keeps its number, so the selection stays on its unit as units
    /// are added and removed, and is lost with the unit when it goes or the
    /// session is reset.
    selected_unit: Option<u32>,
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
    /// The units of the engine's session as the answers so far leave it.
    ///
    /// The app keeps these by applying each answered command, numbering the
    /// units it makes as the engine's session does, and checks them against
    /// the session the engine reports with every answer.
    units: Vec<SessionUnit>,
    /// The number the next unit made will have.
    next_number: u32,
    /// What the last step to change the session did to its units, as the
    /// engine reported it with its last answer.
    ///
    /// Shown as it is: what a step compiled is the engine's to know, and
    /// nothing the app could keep by rule.
    last_step: Vec<repl::UnitStep>,
    /// How the app's units and the engine's session disagreed, if they did.
    ///
    /// A disagreement is a rule of the app's about what a command does gone
    /// wrong: a bug, and shown as one.
    session_mismatch: Option<String>,
    /// Path to stderr log file (for displaying in crash modal).
    stderr_log_path: Option<std::path::PathBuf>,
    /// Where `/save` writes the history and `/load` reads it back, if they
    /// have anywhere to.
    session_path: Option<std::path::PathBuf>,
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
            cancel_armed: false,
            editing: None,
            stashed: None,
            focus: Panel::Input,
            tab: Tab::Environment,
            exit_asked: false,
            should_exit: false,
            code_scroll: ScrollState::default(),
            code_layout: CodeLayout::default(),
            selected_unit: None,
            history_scroll: ScrollState::default(),
            environment_scroll: ScrollState::default(),
            engine_ready: false,
            engine_dead: None,
            crash_modal: None,
            units: Vec::new(),
            next_number: 1,
            last_step: Vec::new(),
            session_mismatch: None,
            stderr_log_path: None,
            session_path: None,
        }
    }

    /// Have `/save` write the history to this path, and `/load` read it.
    pub fn set_session_path(&mut self, path: std::path::PathBuf) {
        self.session_path = Some(path);
    }

    /// Submit input text for processing.
    ///
    /// Takes input as a parameter instead of reading from UI widget.
    /// Returns UiAction to tell UI what to do (usually ClearInput).
    pub fn submit_input(&mut self, input_text: String) -> UiAction {
        if input_text.is_empty() {
            return UiAction::None;
        }
        if let Some(editing) = self.editing {
            return self.submit_edit(editing, input_text);
        }

        // Someone who submits wants to see what it did, wherever they had
        // scrolled to read.
        self.code_scroll.offset = 0;
        self.history_scroll.offset = 0;

        match self.read(&input_text) {
            repl::InputParse::Command(command) => {
                self.multiline_mode = false;
                self.submit_command(input_text, command);
                UiAction::ClearInput
            }
            parse @ repl::InputParse::Empty => {
                self.multiline_mode = false;
                self.push_entry(input_text, parse, EntryStatus::Empty);
                UiAction::ClearInput
            }
            repl::InputParse::ReadMultiline(text) => {
                self.push_entry(input_text, repl::InputParse::ReadMultiline(text.C()), EntryStatus::ReadMultiline);
                self.read_more(&text)
            }
        }
    }

    /// Read what typed text is, single-line or multiline as the input is.
    fn read(&self, text: &str) -> repl::InputParse {
        repl::parse_input(match self.multiline_mode {
            false => repl::Input::Input(text.S()),
            true => repl::Input::Multiline(text.S()),
        })
    }

    /// Go multiline, with the lines typed so far and an empty one after them
    /// to go on in.
    fn read_more(&mut self, text: &str) -> UiAction {
        self.multiline_mode = true;
        let mut lines: Vec<String> = text.lines().map(S).collect();
        lines.push(String::new());
        UiAction::SetMultilineInput { lines }
    }

    /// Enter an input in the history, and say which entry it is.
    fn push_entry(&mut self, input: String, parse: repl::InputParse, status: EntryStatus) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.history.push(HistoryEntry::new(id, input, parse, status));
        id
    }

    /// Replay a saved session's inputs, as if each had been submitted.
    ///
    /// They go straight to evaluation, since a saved input already says what
    /// it is, and reading a unit's text again would ask for more input on a
    /// definition that spans lines.
    pub fn replay(&mut self, inputs: Vec<repl::SavedInput>) {
        for repl::SavedInput { input, command } in inputs {
            self.submit_command(input, command);
        }
    }

    /// Submit a command already read, entered in the history as `input`.
    fn submit_command(&mut self, input: String, command: repl::Command) {
        let id = self.push_entry(input, repl::InputParse::Command(command.C()), EntryStatus::Evaluating);
        self.show_tab_for(&command);
        self.executor.submit_eval(id, command);
    }

    /// Delete the selected unit, as `/delete` would, and select the one after
    /// it, or before it if it is the last.
    ///
    /// The command goes into the history as though it had been typed, so it
    /// is saved and replayed with the rest.
    pub fn delete_selected(&mut self) {
        let Some(unit) = self.selected_unit() else {
            return;
        };
        let numbers: Vec<u32> = self.numbers();
        let index = numbers.iter().position(|number| *number == unit).expect("the selected unit is shown");
        self.selected_unit = numbers.get(index + 1).or(index.checked_sub(1).map(|before| &numbers[before])).copied();
        self.submit_command(
            fmt!("/delete {unit}"),
            repl::Command::ReplCommand(repl::ReplCommand::Delete { unit }),
        );
        self.reveal_selected_unit();
    }

    /// Edit the selected unit in the input, which takes focus, multiline if
    /// the unit spans lines.
    ///
    /// `typed` is what the input held, which is put back when the edit ends.
    pub fn begin_edit(&mut self, typed: String) -> UiAction {
        let Some(unit) = self.selected_unit() else {
            return UiAction::None;
        };
        let source = self.code_units().into_iter()
            .find(|code| code.number == Some(unit))
            .expect("the selected unit is shown")
            .source
            .S();
        self.stash(typed, Editing::Unit(unit));
        self.multiline_mode = source.contains('\n');
        if self.multiline_mode {
            UiAction::SetMultilineInput { lines: source.lines().map(S).collect() }
        } else {
            UiAction::SetInput { text: source }
        }
    }

    /// Write a new unit in the input, which takes focus, to go before the
    /// selected unit, as `/insert before` would.
    ///
    /// `typed` is what the input held, which is put back when the insert
    /// ends.
    pub fn begin_insert(&mut self, typed: String) -> UiAction {
        let Some(unit) = self.selected_unit() else {
            return UiAction::None;
        };
        self.stash(typed, Editing::Before(unit));
        self.multiline_mode = false;
        UiAction::ClearInput
    }

    /// Put what the input holds aside, and give the input to an edit.
    fn stash(&mut self, typed: String, editing: Editing) {
        self.stashed = Some((typed, self.multiline_mode));
        self.editing = Some(editing);
        self.cancel_armed = false;
        self.focus = Panel::Input;
    }

    /// What the input is editing, if it is.
    pub fn editing(&self) -> Option<Editing> {
        self.editing
    }

    /// Submit an edit's text, read as a unit typed at the prompt is.
    ///
    /// A line that opens a multiline definition makes the edit multiline, as
    /// it would the prompt; a unit is sent as `/edit` or `/insert before`,
    /// entered in the history as the command it is; and anything else, which
    /// is a REPL command, is submitted as any input is, leaving the edit as
    /// it was.
    fn submit_edit(&mut self, editing: Editing, text: String) -> UiAction {
        match self.read(&text) {
            repl::InputParse::Empty => UiAction::None,
            repl::InputParse::ReadMultiline(text) => self.read_more(&text),
            repl::InputParse::Command(repl::Command::Unit(source)) => {
                let (input, command) = match editing {
                    Editing::Unit(unit) => (
                        fmt!("/edit {unit} {}", source.source()),
                        repl::ReplCommand::Edit { unit, source },
                    ),
                    Editing::Before(unit) => (
                        fmt!("/insert before {unit} {}", source.source()),
                        repl::ReplCommand::Insert { place: repl::Place::Before, unit, source },
                    ),
                };
                self.submit_command(input, repl::Command::ReplCommand(command));
                self.end_edit()
            }
            repl::InputParse::Command(command @ repl::Command::ReplCommand(_)) => {
                self.submit_command(text, command);
                UiAction::ClearInput
            }
        }
    }

    /// Stop editing: focus goes back to the code, on the unit edited or
    /// inserted before, and the input gets back what it held when the edit
    /// began.
    fn end_edit(&mut self) -> UiAction {
        self.editing = None;
        self.cancel_armed = false;
        self.focus = Panel::Code;
        self.reveal_selected_unit();
        let (typed, multiline) = self.stashed.take().expect("an edit stashes what was typed");
        self.multiline_mode = multiline;
        if multiline {
            UiAction::SetMultilineInput { lines: typed.lines().map(S).collect() }
        } else if typed.is_empty() {
            UiAction::ClearInput
        } else {
            UiAction::SetInput { text: typed }
        }
    }

    /// Take in whatever the executor has answered.
    pub fn poll_results(&mut self) {
        while let Some(response) = self.executor.try_recv_response() {
            match response {
                WorkerResponse::EngineReady => {
                    self.engine_ready = true;
                }
                WorkerResponse::EngineDead { message } => {
                    self.engine_dead = Some(message);
                }
                WorkerResponse::EvalResult { id, eval, environment, session, step } => {
                    self.handle_eval_result(id, eval, environment);
                    self.check_session(session);
                    self.last_step = step;
                }
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

    fn handle_eval_result(&mut self, id: u64, eval: repl::Eval, environment: Vec<script::EnvBinding>) {
        let repl::InputParse::Command(command) = find_entry(&mut self.history, id).parse_result.C() else {
            unreachable!("only a command is evaluated");
        };
        self.apply(&command, &eval);
        self.environment = environment;

        if eval.reset_the_session() {
            // A runtime error or a crash empties the session, as a `/reset`
            // does, and what came after it runs in the new one; so the history
            // forgets the same entries a `/reset` would.
            self.history.retain(|entry| entry.id > id);
            self.crash_modal = Some(reset_message(&eval));
            return;
        }

        let status = match &eval {
            // A unit that did not run to its end stays in the session, so
            // nothing is cleared; it is reported as the error it is.
            repl::Eval::Unit(script::Eval::Error(_) | script::Eval::ReturnedEarly(_) | script::Eval::Blocked { .. })
            | repl::Eval::CommandError(_) => Some(EntryStatus::Error),
            // What each unit an edit reached came to is the units' to show;
            // the edit itself was made.
            repl::Eval::Unit(script::Eval::Nothing | script::Eval::Success(_) | script::Eval::SuccessExpr(_))
            | repl::Eval::Saved { .. } | repl::Eval::Edited(_) => Some(EntryStatus::Success),
            // The command says.
            repl::Eval::CallerInterpret(_) => None,
            repl::Eval::Unit(script::Eval::ErrorReset(_) | script::Eval::CrashReset(_)) => {
                unreachable!("a reset was handled above")
            }
        };
        let entry = find_entry(&mut self.history, id);
        entry.eval_result = Some(eval.C());
        if let Some(status) = status {
            entry.status = status;
        }
        if let repl::Eval::CallerInterpret(command) = &eval {
            self.handle_caller_interpret(id, command);
        }
    }

    fn handle_caller_interpret(&mut self, id: u64, command: &repl::ReplCommand) {
        let status = match command {
            repl::ReplCommand::Unknown
            | repl::ReplCommand::Usage(_)
            | repl::ReplCommand::Edit { .. }
            | repl::ReplCommand::Delete { .. }
            | repl::ReplCommand::Insert { .. } => {
                unreachable!("the session answers {command:?} itself")
            }
            repl::ReplCommand::Exit => {
                self.should_exit = true;
                EntryStatus::Success
            }
            // todo: say something.
            repl::ReplCommand::Help => EntryStatus::Success,
            repl::ReplCommand::Reset => {
                // Input submitted after the reset was evaluated in the new
                // session, so only what came before it is forgotten.
                self.history.retain(|entry| entry.id > id);
                return;
            }
            repl::ReplCommand::Save => {
                let eval = self.save(id).unwrap_or_else(|e| repl::Eval::CommandError(fmt!("{e:#}")));
                let status = match eval {
                    repl::Eval::CommandError(_) => EntryStatus::Error,
                    _ => EntryStatus::Success,
                };
                find_entry(&mut self.history, id).eval_result = Some(eval);
                status
            }
            repl::ReplCommand::Load => match self.load() {
                Ok(inputs) => {
                    // A reset first, so the session is the one saved and not
                    // the saved one on top of this. It forgets this `/load`
                    // too, as `/reset` forgets itself.
                    let reset = repl::SavedInput {
                        input: "/reset".S(),
                        command: repl::Command::ReplCommand(repl::ReplCommand::Reset),
                    };
                    self.replay(std::iter::once(reset).chain(inputs).collect());
                    EntryStatus::Success
                }
                Err(e) => {
                    find_entry(&mut self.history, id).eval_result = Some(repl::Eval::CommandError(fmt!("{e:#}")));
                    EntryStatus::Error
                }
            },
        };
        find_entry(&mut self.history, id).status = status;
    }

    /// Change the app's units as an answered command changed the engine's.
    ///
    /// A command the engine did not carry out, or one that leaves the session
    /// alone, changes nothing. A unit a command names and the app does not
    /// have is a disagreement with the engine, which the report that came
    /// with the answer shows.
    fn apply(&mut self, command: &repl::Command, eval: &repl::Eval) {
        use repl::ReplCommand as R;
        if eval.empties_session() {
            self.units.clear();
            self.next_number = 1;
            // The new session numbers its units from 1 again, so a selection
            // kept would land on a different unit with the old one's number.
            self.selected_unit = None;
            return;
        }
        match (command, eval) {
            (repl::Command::Unit(unit), repl::Eval::Unit(eval)) => {
                let number = self.take_number();
                self.units.push(SessionUnit {
                    numbered: repl::NumberedUnit { number, unit: unit.C() },
                    status: status_of(eval),
                });
            }
            (repl::Command::ReplCommand(command), repl::Eval::Edited(step)) => {
                match command {
                    R::Edit { unit, source } => {
                        if let Some(index) = self.index_of(*unit) {
                            self.units[index].numbered.unit = source.C();
                        }
                    }
                    R::Delete { unit } => {
                        if let Some(index) = self.index_of(*unit) {
                            self.units.remove(index);
                        }
                    }
                    R::Insert { place, unit, source } => {
                        if let Some(index) = self.index_of(*unit) {
                            let index = place.index(index);
                            // The engine reports every unit from the insert
                            // on, this one first, so its status is set below.
                            let number = self.take_number();
                            self.units.insert(index, SessionUnit {
                                numbered: repl::NumberedUnit { number, unit: source.C() },
                                status: UnitStatus::Pending,
                            });
                        }
                    }
                    other => unreachable!("{other:?} was answered as an edit"),
                }
                // A unit only analyzed keeps its status.
                for unit in step {
                    if let (Some(index), Some(eval)) = (self.index_of(unit.unit), &unit.eval) {
                        self.units[index].status = status_of(eval);
                    }
                }
            }
            _ => {}
        }
    }

    /// The numbers of the units the code shows that have them, in order.
    fn numbers(&self) -> Vec<u32> {
        self.code_units().iter().filter_map(|unit| unit.number).collect()
    }

    fn take_number(&mut self) -> u32 {
        let number = self.next_number;
        self.next_number += 1;
        number
    }

    fn index_of(&self, number: u32) -> Option<usize> {
        self.units.iter().position(|unit| unit.numbered.number == number)
    }

    /// Check the app's units against the engine's report of its session.
    ///
    /// When they disagree the engine's are taken, since they are what it
    /// will run against, and the disagreement is shown as the bug it is.
    fn check_session(&mut self, session: Vec<repl::NumberedUnit>) {
        if self.units.iter().map(|unit| &unit.numbered).eq(session.iter()) {
            return;
        }
        if self.session_mismatch.is_none() {
            let reported: Vec<u32> = session.iter().map(|unit| unit.number).collect();
            let expected: Vec<u32> = self.units.iter().map(|unit| unit.numbered.number).collect();
            let sources = if reported == expected { ", with different sources" } else { "" };
            self.session_mismatch = Some(fmt!(
                "The engine's session holds units {reported:?}, and the app expected {expected:?}{sources}.",
            ));
        }
        let statuses: Vec<(u32, UnitStatus)> = self.units.iter()
            .map(|unit| (unit.numbered.number, unit.status))
            .collect();
        self.next_number = self.next_number
            .max(session.iter().map(|unit| unit.number + 1).max().unwrap_or(1));
        self.units = session.into_iter()
            .map(|numbered| {
                // A unit the app did not know of has fared however it has,
                // which nothing has said; it shows as waiting until a step
                // reports it.
                let status = statuses.iter().find(|(number, _)| *number == numbered.number)
                    .map(|(_, status)| *status)
                    .unwrap_or(UnitStatus::Pending);
                SessionUnit { numbered, status }
            })
            .collect();
    }

    /// Read the session `/save` writes, for `/load`.
    fn load(&self) -> AnyResult<Vec<repl::SavedInput>> {
        let Some(path) = &self.session_path else {
            bail!("this session was not given anywhere to load from");
        };
        repl::load_session(path)
    }

    /// Write the history up to and including the `/save` with this id.
    ///
    /// Everything the history holds goes in, failures and REPL commands
    /// included, as it was typed. What was typed after the `/save` is left
    /// out.
    ///
    /// A line that only opened a multiline input is left out too, since the
    /// input it opened holds it again.
    fn save(&self, save_id: u64) -> AnyResult<repl::Eval> {
        use datalove_datafun::package_load_worldfile::{WorldfileSection, write_worldfile};

        let Some(path) = &self.session_path else {
            bail!("this session was not given anywhere to save to");
        };

        let mut sections = Vec::new();
        for entry in self.history.iter().filter(|entry| entry.id <= save_id) {
            let section = match &entry.parse_result {
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
        self.enter_code();
    }

    /// Move focus to the next panel up.
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
        self.enter_code();
    }

    /// Focus moving to the code selects a unit, if none still is, and shows
    /// it.
    ///
    /// The newest unit is the one selected, since that is where the view
    /// usually is.
    fn enter_code(&mut self) {
        if self.focus != Panel::Code {
            return;
        }
        if self.selected_unit().is_none() {
            self.selected_unit = self.code_units().iter().rev().find_map(|unit| unit.number);
        }
        self.reveal_selected_unit();
    }

    /// The number of the code unit selected, if the unit is still shown.
    pub fn selected_unit(&self) -> Option<u32> {
        let number = self.selected_unit?;
        self.code_units().iter().any(|unit| unit.number == Some(number)).then_some(number)
    }

    /// Act on a scroll key the focused code panel takes: the arrows move the
    /// selection a unit, and the page keys scroll a page; see
    /// [`Self::page_in_code`].
    pub fn move_in_code(&mut self, scroll: Scroll) {
        let step: isize = match scroll {
            Scroll::LineUp => -1,
            Scroll::LineDown => 1,
            Scroll::PageUp | Scroll::PageDown => {
                self.page_in_code(scroll);
                return;
            }
        };
        // A unit on its way to the engine has no number to select it by yet.
        let numbers: Vec<u32> = self.numbers();
        let Some(last) = numbers.len().checked_sub(1) else {
            return;
        };
        // A selection that went with its unit starts again from the newest.
        let index = match self.selected_unit().and_then(|number| numbers.iter().position(|unit| *unit == number)) {
            Some(index) => index.saturating_add_signed(step).min(last),
            None => last,
        };
        self.selected_unit = Some(numbers[index]);
        self.reveal_selected_unit();
    }

    /// Scroll the code a page, keeping the selection on screen.
    ///
    /// A selected unit still on screen, if only partly, stays selected. One
    /// the page scrolled off gives way to the nearest unit left on it: the
    /// bottom one going up, the top one going down. With nowhere left to
    /// scroll, the key selects the first unit or the last, so paging to the
    /// end of the code ends on its end unit.
    fn page_in_code(&mut self, scroll: Scroll) {
        let up = scroll == Scroll::PageUp;
        let offset = self.code_scroll.offset;
        self.scroll_code(scroll);

        // A unit on its way to the engine has no number to select it by yet.
        let numbers: Vec<u32> = self.numbers();
        if numbers.is_empty() {
            return;
        }
        if self.code_scroll.offset == offset {
            self.selected_unit = if up { numbers.first() } else { numbers.last() }.copied();
            self.reveal_selected_unit();
            return;
        }

        let layout = &self.code_layout;
        let limit = self.code_scroll.limit;
        let top = limit - self.code_scroll.offset.min(limit);
        let bottom = top + layout.pane_lines;
        let on_screen = |lines: &std::ops::Range<usize>| lines.start < bottom && lines.end > top;
        let selected = self.selected_unit();
        if layout.units.iter().any(|(number, lines)| Some(*number) == selected && on_screen(lines)) {
            return;
        }
        let mut showing = layout.units.iter().filter(|(_, lines)| on_screen(lines));
        let nearest = if up { showing.last() } else { showing.next() };
        if let Some((number, _)) = nearest {
            self.selected_unit = Some(*number);
        }
    }

    /// Scroll the code just far enough to show the selected unit, or its
    /// start when it is taller than the pane.
    fn reveal_selected_unit(&mut self) {
        let Some(number) = self.selected_unit else {
            return;
        };
        let layout = &self.code_layout;
        // Not laid out yet: the frame that lays it out shows the newest
        // units, and a key cannot arrive before it.
        let Some((_, lines)) = layout.units.iter().find(|(unit, _)| *unit == number) else {
            return;
        };
        let limit = self.code_scroll.limit;
        let top = limit - self.code_scroll.offset.min(limit);
        let top = if lines.start < top || lines.len() > layout.pane_lines {
            lines.start
        } else if lines.end > top + layout.pane_lines {
            lines.end - layout.pane_lines
        } else {
            top
        };
        self.code_scroll.offset = limit - top.min(limit);
    }

    /// The panel keys go to.
    pub fn focus(&self) -> Panel {
        self.focus
    }

    /// Ask whether to exit: the first time opens the dialog asking, and the
    /// second, with it open, exits.
    pub fn ask_to_exit(&mut self) {
        if self.exit_asked {
            self.should_exit = true;
        } else {
            self.exit_asked = true;
        }
    }

    /// Close the dialog asking whether to exit, and go on.
    pub fn resume(&mut self) {
        self.exit_asked = false;
    }

    /// Whether the dialog asking whether to exit is open.
    pub fn exit_asked(&self) -> bool {
        self.exit_asked
    }

    pub fn dismiss_crash_modal(&mut self) {
        self.crash_modal = None;
    }

    pub fn multiline_mode(&self) -> bool {
        self.multiline_mode
    }

    /// Ask to cancel multiline input or an edit: the first asks for another,
    /// and the second throws away what was typed and leaves multiline mode,
    /// or the edit, which puts back what the input held before it.
    ///
    /// What was entered before the input went multiline stays in the
    /// history, as an input that only read more lines.
    pub fn cancel_input(&mut self) -> UiAction {
        if !self.cancel_armed {
            self.cancel_armed = true;
            return UiAction::None;
        }
        self.cancel_armed = false;
        if self.editing.is_some() {
            return self.end_edit();
        }
        self.multiline_mode = false;
        UiAction::ClearInput
    }

    /// Whether Ctrl+C in the input cancels it rather than asking to exit.
    pub fn input_cancels(&self) -> bool {
        self.multiline_mode || self.editing.is_some()
    }

    /// Whether one more cancel throws away the multiline input.
    pub fn cancel_armed(&self) -> bool {
        self.cancel_armed
    }

    /// Any other key takes back the first half of a cancel.
    pub fn disarm_cancel(&mut self) {
        self.cancel_armed = false;
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Exit, with nothing asked.
    pub fn exit(&mut self) {
        self.should_exit = true;
    }

    /// Whether the engine is gone; see [`Self::engine_dead_message`].
    pub fn engine_is_dead(&self) -> bool {
        self.engine_dead.is_some()
    }

    /// How the history and the engine's session disagreed, if they did.
    pub fn session_mismatch_message(&self) -> Option<&str> {
        self.session_mismatch.as_deref()
    }

    pub fn dismiss_session_mismatch(&mut self) {
        self.session_mismatch = None;
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
    ///
    /// `units` is each numbered unit's number and the lines it took, counted
    /// from the top of the content.
    pub fn record_code_view(
        &mut self,
        pane_lines: usize,
        scroll_back_limit: usize,
        units: Vec<(u32, std::ops::Range<usize>)>,
    ) {
        self.code_scroll.record(pane_lines, scroll_back_limit);
        self.code_layout = CodeLayout { pane_lines, units };
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

    /// Get the environment variables.
    pub fn environment(&self) -> &[script::EnvBinding] {
        &self.environment
    }

    /// Get the stderr log path.
    pub fn stderr_log_path(&self) -> Option<&std::path::Path> {
        self.stderr_log_path.as_deref()
    }

    /// Every input, and what it came to.
    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// The session's units, in order, and the status of each.
    ///
    /// **They are the session as it will be once everything submitted has
    /// been answered**: the units the answers so far leave, then the units on
    /// their way to the engine, which has not numbered them yet. A `/reset`
    /// takes the units before it from when it parses, so the units typed
    /// after it do not show appended to a session they will never see. The
    /// history keeps those units until the reset is answered, since they are
    /// the engine's session until then.
    ///
    /// Only appended units are guessed at. An edit, a delete or an insert
    /// shows once the engine has answered it.
    pub fn code_units(&self) -> Vec<CodeUnit<'_>> {
        // A reset still in the history is one the engine has not answered.
        let pending_reset = self.history.iter().rposition(|entry| matches!(
            entry.parse_result,
            repl::InputParse::Command(repl::Command::ReplCommand(repl::ReplCommand::Reset)),
        ));
        let (answered, waiting) = match pending_reset {
            Some(reset) => (&[][..], &self.history[reset + 1..]),
            None => (&self.units[..], &self.history[..]),
        };
        let answered = answered.iter().map(|unit| {
            let number = unit.numbered.number;
            let step = self.last_step.iter().find(|step| step.unit == number);
            CodeUnit {
                number: Some(number),
                source: unit.numbered.unit.source(),
                status: unit.status,
                compiled: step.is_some_and(|step| step.compiled),
                ran: step.is_some_and(repl::UnitStep::ran),
            }
        });
        let waiting = waiting.iter()
            .filter(|entry| entry.eval_result.is_none())
            .filter_map(|entry| match &entry.parse_result {
                repl::InputParse::Command(repl::Command::Unit(unit)) => Some(CodeUnit {
                    number: None,
                    source: unit.source(),
                    status: UnitStatus::Pending,
                    compiled: false,
                    ran: false,
                }),
                _ => None,
            });
        answered.chain(waiting).collect()
    }
}

/// The status a unit's evaluation leaves it with, as its bar shows it.
fn status_of(eval: &script::Eval) -> UnitStatus {
    match eval {
        script::Eval::Nothing | script::Eval::Success(_) | script::Eval::SuccessExpr(_) => UnitStatus::Ran,
        script::Eval::Error(_) => UnitStatus::Failed,
        script::Eval::ReturnedEarly(_) => UnitStatus::ReturnedEarly,
        script::Eval::Blocked { .. } => UnitStatus::Blocked,
        script::Eval::ErrorReset(_) | script::Eval::CrashReset(_) => {
            unreachable!("a reset leaves no units to have a status")
        }
    }
}

/// What the crash modal says of an answer that reset the session.
fn reset_message(eval: &repl::Eval) -> String {
    let say = |eval: &script::Eval| match eval {
        script::Eval::ErrorReset(msg) => Some(fmt!("A runtime error: {msg}")),
        script::Eval::CrashReset(msg) => Some(fmt!("The engine panicked: {msg}")),
        _ => None,
    };
    match eval {
        repl::Eval::Unit(eval) => say(eval),
        repl::Eval::Edited(step) => step.iter().find_map(|unit| {
            say(unit.eval.as_ref()?).map(|said| fmt!("Unit {}: {said}", unit.unit))
        }),
        _ => None,
    }
    .expect("only an answer that reset the session is explained")
}
