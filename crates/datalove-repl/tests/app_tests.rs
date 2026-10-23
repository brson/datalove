//! State machine tests for `ReplApp`, driven by a scripted executor.
//!
//! These exercise the app's request/response bookkeeping without an engine,
//! so they can produce interleavings a real session only hits when the user
//! submits input while an evaluation is still running.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use datalove_repl as repl;
use repl::app::{CodeUnit, EntryStatus, Panel, ReplApp, Scroll, ReplExecutor, Tab, UnitStep, WorkerResponse};

/// Submissions the app made, and responses waiting to be delivered to it.
#[derive(Default)]
struct MockState {
    parses: Vec<(u64, repl::Input)>,
    evals: Vec<u64>,
    commands: Vec<repl::Command>,
    /// Each command submitted for evaluation, by entry id.
    submitted: std::collections::HashMap<u64, repl::Command>,
    /// The units the pretend engine's session has, and the next number.
    session: Vec<repl::NumberedUnit>,
    next_number: u32,
    /// Responses, each with whether its session is to be filled in.
    responses: VecDeque<(WorkerResponse, bool)>,
}

/// Executor that records what the app submits and replays scripted responses.
#[derive(Default, Clone)]
struct MockExecutor {
    state: Rc<RefCell<MockState>>,
}

impl ReplExecutor for MockExecutor {
    fn submit_parse(&mut self, id: u64, input: repl::Input) {
        self.state.borrow_mut().parses.push((id, input));
    }

    fn submit_eval(&mut self, id: u64, command: repl::Command) {
        let mut state = self.state.borrow_mut();
        state.evals.push(id);
        state.commands.push(command.clone());
        state.submitted.insert(id, command);
    }

    /// The session a queued eval result reports is filled in here, when the
    /// app takes it, since only by then has the app submitted the command it
    /// answers. The pretend engine adds an appended unit, numbering it, and
    /// empties the session at any reset. Anything else leaves it alone, and a
    /// test that edits gives the session explicitly with `queue_raw`.
    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        let mut state = self.state.borrow_mut();
        let (mut response, fill) = state.responses.pop_front()?;
        if let WorkerResponse::EvalResult { id, eval, session, .. } = &mut response {
            if fill {
                let command = state.submitted.get(id).cloned()
                    .unwrap_or_else(|| panic!("entry {id} was answered without being submitted"));
                if eval.reset_the_session() || matches!(eval, repl::Eval::CallerInterpret(repl::ReplCommand::Reset)) {
                    state.session.clear();
                    state.next_number = 1;
                } else if let (repl::Command::Unit(unit), repl::Eval::Unit(_)) = (command, &*eval) {
                    let number = state.next_number.max(1);
                    state.next_number = number + 1;
                    state.session.push(repl::NumberedUnit { number, unit });
                }
                *session = state.session.clone();
            } else {
                state.next_number = session.iter().map(|unit| unit.number + 1).max().unwrap_or(1);
                state.session = session.clone();
            }
        }
        Some(response)
    }
}

impl MockExecutor {
    /// Queue a response, an eval result's session to be filled in as the
    /// pretend engine has it.
    fn queue(&self, response: WorkerResponse) {
        self.state.borrow_mut().responses.push_back((response, true));
    }

    /// Queue a response as it is, session and all, which the pretend engine
    /// then has.
    fn queue_raw(&self, response: WorkerResponse) {
        self.state.borrow_mut().responses.push_back((response, false));
    }

    fn evals(&self) -> Vec<u64> {
        self.state.borrow().evals.clone()
    }

    /// What each eval submission was, in order.
    fn commands(&self) -> Vec<repl::Command> {
        self.state.borrow().commands.clone()
    }

    fn parse_count(&self) -> usize {
        self.state.borrow().parses.len()
    }

    /// The input of the nth parse submission, rendered for comparison.
    fn parse_input(&self, index: usize) -> (&'static str, String) {
        match &self.state.borrow().parses[index].1 {
            repl::Input::Input(text) => ("Input", text.clone()),
            repl::Input::Multiline(text) => ("Multiline", text.clone()),
        }
    }
}

fn parsed_statement(id: u64, source: &str) -> WorkerResponse {
    WorkerResponse::ParseResult {
        id,
        parse: repl::InputParse::Command(repl::Command::Unit(repl::script::Unit::Fragment(source.to_string()))),
    }
}

fn evaluated_let(id: u64, name: &str) -> WorkerResponse {
    WorkerResponse::EvalResult {
        id,
        eval: repl::Eval::Unit(repl::script::Eval::Success(vec![repl::script::EvalBinding::Value {
            name: name.to_string(),
            ty: "int".to_string(),
            value: "1".to_string(),
        }])),
        environment: vec![(name.to_string(), "int".to_string(), "1".to_string())],
        session: Vec::new(),
    }
}

/// A result that arrives after a later input was submitted belongs to the
/// entry it was requested for, not to whatever entry happens to be last.
#[test]
fn results_land_on_their_own_entry() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    // The user submits a second line while the first is still being handled.
    app.submit_input("let x = 1".to_string());
    app.submit_input("let y = 2".to_string());

    executor.queue(parsed_statement(0, "let x = 1"));
    app.poll_results();
    assert_eq!(executor.evals(), vec![0]);

    executor.queue(evaluated_let(0, "x"));
    executor.queue(parsed_statement(1, "let y = 2"));
    app.poll_results();

    let history = app.history();
    assert_eq!(history.len(), 2);
    assert!(matches!(history[0].status, EntryStatus::Success));
    assert!(matches!(
        history[0].eval_result,
        Some(repl::Eval::Unit(repl::script::Eval::Success(_)))
    ));
    assert!(matches!(history[1].status, EntryStatus::Evaluating { .. }));
    assert_eq!(executor.evals(), vec![0, 1]);
}

/// Multiline mode loads whatever the engine read, however many lines it is.
#[test]
fn read_multiline_loads_every_line_it_was_given() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("fun double(x: int): int\n  ret x + x".to_string());

    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::ReadMultiline("fun double(x: int): int\n  ret x + x".to_string()),
    });
    let actions = app.poll_results();

    assert!(app.multiline_mode());
    assert_eq!(actions.len(), 1);
    match &actions[0] {
        repl::app::UiAction::SetMultilineInput { lines } => {
            assert_eq!(lines, &["fun double(x: int): int", "  ret x + x", ""]);
        }
        other => panic!("expected SetMultilineInput, got {:?}", other),
    }
}

/// The submission after a ReadMultiline is sent as multiline input, and the
/// mode ends when the engine answers.
#[test]
fn multiline_mode_spans_one_submission() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("fun f(): int".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::ReadMultiline("fun f(): int".to_string()),
    });
    app.poll_results();
    assert!(app.multiline_mode());
    assert_eq!(executor.parse_input(0), ("Input", "fun f(): int".to_string()));

    // The continued fragment goes to the engine as one multiline input.
    app.submit_input("fun f(): int\n  ret 1\nend fun".to_string());
    assert!(!app.multiline_mode());
    assert_eq!(
        executor.parse_input(1),
        ("Multiline", "fun f(): int\n  ret 1\nend fun".to_string())
    );

    executor.queue(parsed_statement(1, "fun f(): int\n  ret 1\nend fun"));
    app.poll_results();
    executor.queue(WorkerResponse::EvalResult {
        id: 1,
        eval: repl::Eval::Unit(repl::script::Eval::Success(vec![repl::script::EvalBinding::Function {
            name: "f".to_string(),
        }])),
        environment: vec![("f".to_string(), "function".to_string(), "-".to_string())],
        session: Vec::new(),
    });
    app.poll_results();

    assert!(matches!(app.history()[1].status, EntryStatus::Success));
    assert_eq!(app.environment().len(), 1);
}

/// Empty input is not a request; it never reaches the engine.
#[test]
fn empty_input_is_not_submitted() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    let action = app.submit_input(String::new());

    assert!(matches!(action, repl::app::UiAction::None));
    assert_eq!(executor.parse_count(), 0);
    assert!(app.history().is_empty());
}

/// Commands the engine hands back for the UI to interpret take effect.
#[test]
fn exit_command_asks_the_app_to_exit() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("/exit".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::Command(repl::Command::ReplCommand(repl::ReplCommand::Exit)),
    });
    app.poll_results();
    assert!(!app.should_exit());

    executor.queue(WorkerResponse::EvalResult {
        id: 0,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Exit),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert!(app.should_exit());
    assert!(matches!(app.history()[0].status, EntryStatus::Success));
}

/// Focus starts on the input and cycles through the panels both ways.
#[test]
fn focus_cycles_through_the_panels() {
    let mut app = ReplApp::with_executor(MockExecutor::default());
    assert_eq!(app.focus(), Panel::Input);

    app.focus_next();
    assert_eq!(app.focus(), Panel::Bottom);
    app.focus_next();
    assert_eq!(app.focus(), Panel::Code);
    app.focus_next();
    assert_eq!(app.focus(), Panel::Input);

    app.focus_prev();
    assert_eq!(app.focus(), Panel::Code);
    app.focus_prev();
    assert_eq!(app.focus(), Panel::Bottom);
    app.focus_prev();
    assert_eq!(app.focus(), Panel::Input);
}

/// A reset forgets the history and environment up to and including itself,
/// but keeps input submitted while it was being evaluated.
#[test]
fn reset_command_clears_history_and_environment() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let a = 1".to_string());
    executor.queue(parsed_statement(0, "let a = 1"));
    executor.queue(evaluated_let(0, "a"));
    app.poll_results();
    assert_eq!(app.environment().len(), 1);

    app.submit_input("/reset".to_string());
    app.submit_input("let b = 1".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 1,
        parse: repl::InputParse::Command(repl::Command::ReplCommand(repl::ReplCommand::Reset)),
    });
    executor.queue(WorkerResponse::EvalResult {
        id: 1,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Reset),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert!(app.environment().is_empty());
    assert_eq!(app.history().len(), 1);
    assert_eq!(app.history()[0].id, 2);

    executor.queue(parsed_statement(2, "let b = 1"));
    executor.queue(evaluated_let(2, "b"));
    app.poll_results();
    assert!(matches!(app.history()[0].status, EntryStatus::Success));
    assert_eq!(app.environment().len(), 1);
}

/// A replayed session goes straight to evaluation, each unit as the kind it
/// was saved as, and what is typed afterwards follows it.
#[test]
fn a_replayed_session_is_evaluated_without_parsing() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    let fun = "fun f(): int\n    ret 1\nend fun".to_string();
    app.replay(vec![
        repl::SavedInput { input: fun.clone(), command: repl::Command::Unit(repl::script::Unit::Fragment(fun)) },
        repl::SavedInput {
            input: "f()".to_string(),
            command: repl::Command::Unit(repl::script::Unit::Expression("f()".to_string())),
        },
    ]);

    assert_eq!(executor.parse_count(), 0);
    assert_eq!(executor.evals(), vec![0, 1]);
    assert_eq!(app.history().len(), 2);
    assert_eq!(app.history()[1].input, "f()");
    assert!(matches!(
        app.history()[1].status,
        EntryStatus::Evaluating { command: repl::Command::Unit(repl::script::Unit::Expression(_)) },
    ));

    executor.queue(evaluated_let(0, "a"));
    app.poll_results();
    assert!(matches!(app.history()[0].status, EntryStatus::Success));

    app.submit_input("let b = 1".to_string());
    assert_eq!(app.history()[2].id, 2);
}

/// `/save` writes the whole history up to itself, failures and commands
/// included, and leaves out the line that only opened a multiline input.
#[test]
fn save_writes_the_history_up_to_itself() {
    let dir = rmx::tempfile::tempdir().unwrap();
    let path = dir.path().join("session.world");
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());
    app.set_session_path(path.clone());

    app.submit_input("fun f(): int".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::ReadMultiline("fun f(): int".to_string()),
    });
    app.poll_results();

    let fun = "fun f(): int\n    ret 1\nend fun";
    app.submit_input(fun.to_string());
    executor.queue(parsed_statement(1, fun));
    executor.queue(evaluated_let(1, "f"));

    app.submit_input("nope".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 2,
        parse: repl::InputParse::Command(repl::Command::Unit(repl::script::Unit::Expression("nope".to_string()))),
    });
    executor.queue(WorkerResponse::EvalResult {
        id: 2,
        eval: repl::Eval::Unit(repl::script::Eval::Error("no such name".to_string())),
        environment: Vec::new(),
        session: Vec::new(),
    });

    app.submit_input("/save".to_string());
    // Typed while the save was on its way, and not saved.
    app.submit_input("let late = 1".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 3,
        parse: repl::InputParse::Command(repl::Command::ReplCommand(repl::ReplCommand::Save)),
    });
    executor.queue(WorkerResponse::EvalResult {
        id: 3,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Save),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert!(matches!(
        app.history()[3].eval_result,
        Some(repl::Eval::Saved { inputs: 3, .. }),
    ), "{:?}", app.history()[3]);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "\
----------
scriptunit-fragment
----------

fun f(): int
    ret 1
end fun

----------
scriptunit-expr
----------

nope

----------
repl-command
----------

/save

");
}

/// With nowhere to save to, `/save` is an error on its own entry.
#[test]
fn save_without_a_path_is_an_error() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("/save".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::Command(repl::Command::ReplCommand(repl::ReplCommand::Save)),
    });
    executor.queue(WorkerResponse::EvalResult {
        id: 0,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Save),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert!(matches!(app.history()[0].status, EntryStatus::Error));
    assert!(matches!(app.history()[0].eval_result, Some(repl::Eval::CommandError(_))));
}

/// A crash forgets the history up to the input that crashed, as `/reset`
/// does. Input submitted while it was running runs in the new session, and
/// its results land.
#[test]
fn input_after_a_crash_runs_in_the_new_session() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let x = 1".to_string());
    app.submit_input("let y = 2".to_string());

    executor.queue(parsed_statement(0, "let x = 1"));
    executor.queue(WorkerResponse::EvalResult {
        id: 0,
        eval: repl::Eval::Unit(repl::script::Eval::CrashReset("boom".to_string())),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert!(app.crash_modal_is_open());
    assert_eq!(app.history().len(), 1);

    // The second line's parse was submitted before the reset.
    executor.queue(parsed_statement(1, "let y = 2"));
    executor.queue(evaluated_let(1, "y"));
    app.poll_results();

    assert!(matches!(app.history()[0].status, EntryStatus::Success));
    assert_eq!(app.environment().len(), 1);
    let sources: Vec<&str> = app.code_units().iter().map(|unit| unit.source).collect();
    assert_eq!(sources, vec!["let y = 2"]);
    assert_eq!(app.session_mismatch_message(), None);
}

/// A runtime error empties the session, so the app forgets the history and
/// environment it showed of it and says why.
#[test]
fn a_runtime_error_reset_clears_the_session() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let x = 1".to_string());
    executor.queue(parsed_statement(0, "let x = 1"));
    executor.queue(evaluated_let(0, "x"));
    app.poll_results();
    assert_eq!(app.environment().len(), 1);

    app.submit_input("let y = f()".to_string());
    executor.queue(parsed_statement(1, "let y = f()"));
    executor.queue(WorkerResponse::EvalResult {
        id: 1,
        eval: repl::Eval::Unit(repl::script::Eval::ErrorReset("Error: stack overflow".to_string())),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert_eq!(app.history().len(), 0);
    assert!(app.environment().is_empty());
    let message = app.crash_modal_message().expect("the reset is explained");
    assert!(message.contains("stack overflow"), "{message}");
}

/// The engine reports when it is ready, so the UI can say it is starting.
#[test]
fn engine_readiness_is_reported() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    assert!(!app.engine_is_ready());

    executor.queue(WorkerResponse::EngineReady);
    app.poll_results();

    assert!(app.engine_is_ready());
    assert!(app.engine_dead_message().is_none());
}

/// An engine that never starts is reported, rather than leaving every entry
/// waiting on a parse that will never come back.
#[test]
fn a_dead_engine_is_reported() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let x = 1".to_string());

    executor.queue(WorkerResponse::EngineDead {
        message: "cargo is not installed".to_string(),
    });
    app.poll_results();

    assert_eq!(app.engine_dead_message(), Some("cargo is not installed"));
    assert!(matches!(app.history()[0].status, EntryStatus::Parsing));
}

/// Paging through the history stops at the oldest entry and at the newest,
/// and the renderer is what says where those are.
#[test]
fn history_scrolls_by_the_page_within_its_bounds() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    // Nothing has been rendered, so there is nowhere to scroll yet.
    app.scroll_history(Scroll::PageUp);
    assert_eq!(app.history_scroll_back(), 0);

    // A ten-line pane over thirty lines of history: a page is nine, and the
    // view can go twenty lines up.
    app.record_history_view(10, 20);

    app.scroll_history(Scroll::PageUp);
    assert_eq!(app.history_scroll_back(), 9);

    app.scroll_history(Scroll::PageUp);
    assert_eq!(app.history_scroll_back(), 18);

    // Past the oldest entry lands on it.
    app.scroll_history(Scroll::PageUp);
    assert_eq!(app.history_scroll_back(), 20);

    app.scroll_history(Scroll::PageDown);
    assert_eq!(app.history_scroll_back(), 11);

    // And back down stops at the newest.
    app.scroll_history(Scroll::PageDown);
    app.scroll_history(Scroll::PageDown);
    assert_eq!(app.history_scroll_back(), 0);
}

/// The arrow keys scroll the history a line at a time.
#[test]
fn history_scrolls_by_the_line() {
    let mut app = ReplApp::with_executor(MockExecutor::default());
    app.record_history_view(10, 2);

    app.scroll_history(Scroll::LineUp);
    assert_eq!(app.history_scroll_back(), 1);
    app.scroll_history(Scroll::LineUp);
    app.scroll_history(Scroll::LineUp);
    assert_eq!(app.history_scroll_back(), 2);

    app.scroll_history(Scroll::LineDown);
    assert_eq!(app.history_scroll_back(), 1);
}

/// The environment scrolls down from its first row, by the line and the page,
/// within what the renderer says it has.
#[test]
fn environment_scrolls_from_the_top_within_its_bounds() {
    let mut app = ReplApp::with_executor(MockExecutor::default());

    // A five-row pane over twelve bindings.
    app.record_environment_view(5, 7);

    app.scroll_environment(Scroll::LineUp);
    assert_eq!(app.environment_scroll(), 0);

    app.scroll_environment(Scroll::LineDown);
    assert_eq!(app.environment_scroll(), 1);

    app.scroll_environment(Scroll::PageDown);
    assert_eq!(app.environment_scroll(), 5);

    app.scroll_environment(Scroll::PageDown);
    assert_eq!(app.environment_scroll(), 7);

    app.scroll_environment(Scroll::PageUp);
    assert_eq!(app.environment_scroll(), 3);

    // Bindings going away pulls the view back to what is left.
    app.record_environment_view(5, 1);
    assert_eq!(app.environment_scroll(), 1);
}

/// A focused pane takes the plain scroll keys and the opposite pane takes
/// them with Ctrl; the input lends them to the panes until it is multiline,
/// when it keeps them all.
#[test]
fn scroll_keys_go_to_the_focused_pane_and_ctrl_to_its_opposite() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    assert_eq!(app.scroll_target(false), Panel::Code);
    assert_eq!(app.scroll_target(true), Panel::Bottom);

    app.focus_next();
    assert_eq!(app.focus(), Panel::Bottom);
    assert_eq!(app.scroll_target(false), Panel::Bottom);
    assert_eq!(app.scroll_target(true), Panel::Code);

    app.focus_next();
    assert_eq!(app.focus(), Panel::Code);
    assert_eq!(app.scroll_target(false), Panel::Code);
    assert_eq!(app.scroll_target(true), Panel::Bottom);

    // Into multiline mode, with the input focused again.
    app.focus_next();
    app.submit_input("fun f() {".to_string());
    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::ReadMultiline("fun f() {".to_string()),
    });
    app.poll_results();
    assert!(app.multiline_mode());

    assert_eq!(app.scroll_target(false), Panel::Input);
    assert_eq!(app.scroll_target(true), Panel::Input);
}

/// History that fits in the pane has nowhere to scroll.
#[test]
fn a_short_history_does_not_scroll() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.record_history_view(10, 0);
    app.scroll_history(Scroll::PageUp);
    app.record_history_view(10, 0);

    assert_eq!(app.history_scroll_back(), 0);
}

/// Submitting returns the view to the newest output, since that is what the
/// submission is going to produce.
#[test]
fn submitting_returns_to_the_newest_entry() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.record_history_view(10, 20);
    app.scroll_history(Scroll::PageUp);
    assert_eq!(app.history_scroll_back(), 9);

    app.submit_input("let x = 1".to_string());

    assert_eq!(app.history_scroll_back(), 0);
}

fn evaluated(id: u64, eval: repl::script::Eval) -> WorkerResponse {
    WorkerResponse::EvalResult { id, eval: repl::Eval::Unit(eval), environment: Vec::new(), session: Vec::new() }
}

fn parsed_command(id: u64, command: repl::ReplCommand) -> WorkerResponse {
    WorkerResponse::ParseResult {
        id,
        parse: repl::InputParse::Command(repl::Command::ReplCommand(command)),
    }
}

/// The code panel is the session's units, without the REPL commands, each
/// with what the last step did with it: only the unit answered last was
/// touched, and one still waiting is pending.
#[test]
fn code_units_are_the_units_and_what_the_last_step_did() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    for (id, input) in ["let a = 1", "/help", "let b = c", "a"].into_iter().enumerate() {
        app.submit_input(input.to_string());
        if input == "/help" {
            executor.queue(parsed_command(id as u64, repl::ReplCommand::Help));
        } else {
            executor.queue(parsed_statement(id as u64, input));
        }
    }
    app.poll_results();

    let pending: Vec<UnitStep> = app.code_units().iter().map(|unit| unit.step).collect();
    assert_eq!(pending, vec![UnitStep::Pending; 3]);

    executor.queue(evaluated_let(0, "a"));
    app.poll_results();
    assert_eq!(app.code_units(), vec![
        CodeUnit { number: Some(1), source: "let a = 1", step: UnitStep::Ran },
        CodeUnit { number: None, source: "let b = c", step: UnitStep::Pending },
        CodeUnit { number: None, source: "a", step: UnitStep::Pending },
    ]);

    executor.queue(WorkerResponse::EvalResult {
        id: 1,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Help),
        environment: Vec::new(),
        session: Vec::new(),
    });
    executor.queue(evaluated(2, repl::script::Eval::Error("no `c`".to_string())));
    app.poll_results();
    let steps: Vec<UnitStep> = app.code_units().iter().map(|unit| unit.step).collect();
    assert_eq!(steps, vec![UnitStep::Untouched, UnitStep::Failed, UnitStep::Pending]);

    executor.queue(evaluated(3, repl::script::Eval::Blocked { on: vec![1] }));
    app.poll_results();
    let steps: Vec<UnitStep> = app.code_units().iter().map(|unit| unit.step).collect();
    assert_eq!(steps, vec![UnitStep::Untouched, UnitStep::Untouched, UnitStep::Blocked]);
}

/// A runtime error empties the session, and the code panel with it.
#[test]
fn a_runtime_error_clears_the_code() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let a = 1".to_string());
    executor.queue(parsed_statement(0, "let a = 1"));
    executor.queue(evaluated_let(0, "a"));
    app.submit_input("let b = f()".to_string());
    executor.queue(parsed_statement(1, "let b = f()"));
    executor.queue(evaluated(1, repl::script::Eval::ErrorReset("Error: stack overflow".to_string())));
    app.poll_results();

    assert!(app.code_units().is_empty());
}

/// Code shows the environment it changes, and a REPL command the history,
/// which is the only place what it did is shown. The keys change them too.
#[test]
fn input_selects_the_tab_it_changes() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());
    assert_eq!(app.tab(), Tab::Environment);

    app.submit_input("/help".to_string());
    executor.queue(parsed_command(0, repl::ReplCommand::Help));
    app.poll_results();
    assert_eq!(app.tab(), Tab::History);

    app.submit_input("let a = 1".to_string());
    executor.queue(parsed_statement(1, "let a = 1"));
    app.poll_results();
    assert_eq!(app.tab(), Tab::Environment);

    app.next_tab();
    assert_eq!(app.tab(), Tab::History);
    app.next_tab();
    assert_eq!(app.tab(), Tab::Environment);
    app.select_tab(Tab::History);
    assert_eq!(app.tab(), Tab::History);

    // A replayed session ends on the tab its last input selects.
    app.replay(vec![repl::SavedInput {
        input: "/help".to_string(),
        command: repl::Command::ReplCommand(repl::ReplCommand::Help),
    }]);
    assert_eq!(app.tab(), Tab::History);
}

/// The bottom panel scrolls whichever tab it shows, and each keeps its place.
#[test]
fn the_bottom_panel_scrolls_the_tab_it_shows() {
    let mut app = ReplApp::with_executor(MockExecutor::default());
    app.record_environment_view(5, 7);
    app.record_history_view(10, 20);

    app.scroll_bottom(Scroll::LineDown);
    assert_eq!(app.environment_scroll(), 1);
    assert_eq!(app.history_scroll_back(), 0);

    app.next_tab();
    app.scroll_bottom(Scroll::PageUp);
    assert_eq!(app.history_scroll_back(), 9);
    assert_eq!(app.environment_scroll(), 1);
}

/// The code scrolls up from its newest unit, within its bounds, and
/// submitting returns to the newest.
#[test]
fn code_scrolls_back_from_the_newest_unit() {
    let mut app = ReplApp::with_executor(MockExecutor::default());
    app.record_code_view(10, 12, Vec::new());

    app.scroll_code(Scroll::PageUp);
    assert_eq!(app.code_scroll_back(), 9);
    app.scroll_code(Scroll::PageUp);
    assert_eq!(app.code_scroll_back(), 12);
    app.scroll_code(Scroll::LineDown);
    assert_eq!(app.code_scroll_back(), 11);

    app.submit_input("let x = 1".to_string());
    assert_eq!(app.code_scroll_back(), 0);
}

/// An app holding these units, all run.
fn app_with_units(sources: &[&str]) -> ReplApp<MockExecutor> {
    app_and_executor_with_units(sources).0
}

/// The same, and the executor to answer it with, whose next entry id is the
/// number of units.
fn app_and_executor_with_units(sources: &[&str]) -> (ReplApp<MockExecutor>, MockExecutor) {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());
    for (id, source) in sources.iter().enumerate() {
        app.submit_input(source.to_string());
        executor.queue(parsed_statement(id as u64, source));
        executor.queue(evaluated_let(id as u64, "x"));
    }
    app.poll_results();
    (app, executor)
}

/// Focusing the code selects its newest unit, and the arrows move between
/// units, stopping at the first and the last.
#[test]
fn the_arrows_move_between_code_units() {
    let mut app = app_with_units(&["let a = 1", "let b = 2", "let c = 3"]);
    assert_eq!(app.selected_unit(), None);

    app.focus_prev();
    assert_eq!(app.focus(), Panel::Code);
    assert_eq!(app.selected_unit(), Some(3));

    app.move_in_code(Scroll::LineUp);
    assert_eq!(app.selected_unit(), Some(2));
    app.move_in_code(Scroll::LineUp);
    app.move_in_code(Scroll::LineUp);
    assert_eq!(app.selected_unit(), Some(1));

    app.move_in_code(Scroll::LineDown);
    assert_eq!(app.selected_unit(), Some(2));

    // Leaving and coming back keeps the unit selected.
    app.focus_next();
    app.focus_prev();
    assert_eq!(app.selected_unit(), Some(2));
}

/// A reset takes the selected unit with it, and the arrows start again from
/// the newest unit.
#[test]
fn a_reset_loses_the_selection() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());
    app.submit_input("let a = 1".to_string());
    executor.queue(parsed_statement(0, "let a = 1"));
    executor.queue(evaluated_let(0, "a"));
    app.poll_results();
    app.focus_prev();
    assert_eq!(app.selected_unit(), Some(1));

    app.submit_input("/reset".to_string());
    app.submit_input("let b = 1".to_string());
    executor.queue(parsed_command(1, repl::ReplCommand::Reset));
    executor.queue(WorkerResponse::EvalResult {
        id: 1,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Reset),
        environment: Vec::new(),
        session: Vec::new(),
    });
    executor.queue(parsed_statement(2, "let b = 1"));
    executor.queue(evaluated_let(2, "b"));
    app.poll_results();
    assert_eq!(app.selected_unit(), None);

    // The new session numbers from 1 again.
    app.move_in_code(Scroll::LineUp);
    assert_eq!(app.selected_unit(), Some(1));
}

/// Moving the selection scrolls the code just far enough to show the unit,
/// and the page keys still scroll a page.
#[test]
fn the_selected_unit_is_scrolled_into_view() {
    let mut app = app_with_units(&["a", "b", "c", "d"]);
    // Four three-line units with a rule between each, fifteen lines, in a
    // five-line pane: ten lines can be scrolled.
    app.record_code_view(5, 10, vec![(1, 0..3), (2, 4..7), (3, 8..11), (4, 12..15)]);
    app.focus_prev();
    assert_eq!(app.selected_unit(), Some(4));
    assert_eq!(app.code_scroll_back(), 0);

    // Unit 3 ends at line 11, inside the view of lines 10 to 15; its start
    // is not, so the view moves up to it.
    app.move_in_code(Scroll::LineUp);
    assert_eq!(app.code_scroll_back(), 2);

    app.move_in_code(Scroll::LineUp);
    app.move_in_code(Scroll::LineUp);
    assert_eq!(app.selected_unit(), Some(1));
    assert_eq!(app.code_scroll_back(), 10);

    // Back down, the view follows the unit's end.
    app.move_in_code(Scroll::LineDown);
    app.move_in_code(Scroll::LineDown);
    assert_eq!(app.code_scroll_back(), 4);

    app.move_in_code(Scroll::PageDown);
    assert_eq!(app.code_scroll_back(), 0);
    assert_eq!(app.selected_unit(), Some(3));
}

/// `/load` starts the session again as the saved one: a reset, then the saved
/// inputs, evaluated without parsing. The reset forgets what came before,
/// the `/load` included.
#[test]
fn load_resets_and_replays_the_saved_session() {
    let dir = rmx::tempfile::tempdir().unwrap();
    let path = dir.path().join("session.world");
    std::fs::write(&path, "\
----------
scriptunit-fragment
----------

let a = 1

----------
repl-command
----------

/load
").unwrap();

    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());
    app.set_session_path(path);

    app.submit_input("let old = 1".to_string());
    executor.queue(parsed_statement(0, "let old = 1"));
    executor.queue(evaluated_let(0, "old"));
    app.submit_input("/load".to_string());
    executor.queue(parsed_command(1, repl::ReplCommand::Load));
    executor.queue(WorkerResponse::EvalResult {
        id: 1,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Load),
        environment: vec![("old".to_string(), "int".to_string(), "1".to_string())],
        session: Vec::new(),
    });
    app.poll_results();

    // The saved `/load` is not replayed, or it would load forever.
    assert_eq!(executor.evals(), vec![0, 1, 2, 3]);
    // The code is the loaded session's from the start, with the reset
    // still to be answered.
    let sources: Vec<&str> = app.code_units().iter().map(|unit| unit.source).collect();
    assert_eq!(sources, vec!["let a = 1"]);
    let commands = executor.commands();
    assert!(matches!(commands[2], repl::Command::ReplCommand(repl::ReplCommand::Reset)), "{commands:?}");
    assert!(matches!(&commands[3], repl::Command::Unit(repl::script::Unit::Fragment(source)) if source == "let a = 1"));

    executor.queue(WorkerResponse::EvalResult {
        id: 2,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Reset),
        environment: Vec::new(),
        session: Vec::new(),
    });
    executor.queue(evaluated_let(3, "a"));
    app.poll_results();

    let inputs: Vec<&str> = app.history().iter().map(|entry| entry.input.as_str()).collect();
    assert_eq!(inputs, vec!["let a = 1"]);
    assert_eq!(app.environment().len(), 1);
}

/// With nothing saved, `/load` is an error and the session is left alone.
#[test]
fn load_without_a_saved_session_is_an_error() {
    let dir = rmx::tempfile::tempdir().unwrap();
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());
    app.set_session_path(dir.path().join("missing.world"));

    app.submit_input("/load".to_string());
    executor.queue(parsed_command(0, repl::ReplCommand::Load));
    executor.queue(WorkerResponse::EvalResult {
        id: 0,
        eval: repl::Eval::CallerInterpret(repl::ReplCommand::Load),
        environment: Vec::new(),
        session: Vec::new(),
    });
    app.poll_results();

    assert_eq!(executor.evals(), vec![0]);
    assert!(matches!(app.history()[0].status, EntryStatus::Error));
    assert!(matches!(app.history()[0].eval_result, Some(repl::Eval::CommandError(_))));
}

/// The code is the session as it will be: a `/reset` the engine has not
/// answered already takes the units before it, and units typed after it are
/// the new session's.
#[test]
fn a_pending_reset_takes_the_code_before_it() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let a = 1".to_string());
    executor.queue(parsed_statement(0, "let a = 1"));
    executor.queue(evaluated_let(0, "a"));
    app.submit_input("/reset".to_string());
    app.submit_input("let b = 2".to_string());
    app.poll_results();

    // Not parsed yet, the reset is not known to be one.
    let sources: Vec<&str> = app.code_units().iter().map(|unit| unit.source).collect();
    assert_eq!(sources, vec!["let a = 1"]);

    executor.queue(parsed_command(1, repl::ReplCommand::Reset));
    executor.queue(parsed_statement(2, "let b = 2"));
    app.poll_results();
    let units = app.code_units();
    assert_eq!(units, vec![CodeUnit { number: None, source: "let b = 2", step: UnitStep::Pending }]);
    // The history keeps the units the engine still has.
    assert_eq!(app.history().len(), 3);
}

/// An engine that reports a session the history does not hold is a bug in
/// the app's bookkeeping, and is shown as one; the code shown is the
/// engine's.
#[test]
fn a_session_the_history_does_not_hold_is_reported() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let a = 1".to_string());
    app.submit_input("let b = 2".to_string());
    executor.queue(parsed_statement(0, "let a = 1"));
    executor.queue(parsed_statement(1, "let b = 2"));
    executor.queue(evaluated_let(0, "a"));
    app.poll_results();
    assert_eq!(app.session_mismatch_message(), None);

    // The engine says it lost `a` while adding `b`, which nothing the app
    // knows of does.
    let WorkerResponse::EvalResult { eval, environment, .. } = evaluated_let(1, "b") else {
        unreachable!();
    };
    let b = repl::NumberedUnit { number: 2, unit: repl::script::Unit::Fragment("let b = 2".to_string()) };
    executor.queue_raw(WorkerResponse::EvalResult { id: 1, eval, environment, session: vec![b] });
    app.poll_results();

    let message = app.session_mismatch_message().expect("the mismatch is reported");
    assert!(message.contains("[2]") && message.contains("[1, 2]"), "{message}");
    let sources: Vec<&str> = app.code_units().iter().map(|unit| unit.source).collect();
    assert_eq!(sources, vec!["let b = 2"]);

    app.dismiss_session_mismatch();
    assert_eq!(app.session_mismatch_message(), None);
}

fn numbered(number: u32, source: &str) -> repl::NumberedUnit {
    repl::NumberedUnit { number, unit: repl::script::Unit::Fragment(source.to_string()) }
}

fn edited(id: u64, reports: Vec<(u32, repl::script::Eval)>, session: Vec<repl::NumberedUnit>) -> WorkerResponse {
    WorkerResponse::EvalResult {
        id,
        eval: repl::Eval::Edited(reports.into_iter().map(|(unit, eval)| repl::UnitReport { unit, eval }).collect()),
        environment: Vec::new(),
        session,
    }
}

fn steps(app: &ReplApp<MockExecutor>) -> Vec<(Option<u32>, &str, UnitStep)> {
    app.code_units().into_iter().map(|unit| (unit.number, unit.source, unit.step)).collect()
}

/// The code follows an edit, an insert and a delete once the engine answers
/// each, numbering an inserted unit next whatever its place, and each bar
/// says what the step did: the units the engine reports, and no others.
#[test]
fn edits_change_the_code_when_answered() {
    let ran = || repl::script::Eval::Nothing;
    let (mut app, executor) = app_and_executor_with_units(&["let a = 1", "let b = a", "let c = 3"]);

    app.submit_input("/edit 1 let a = 2".to_string());
    executor.queue(parsed_command(3, command_named("/edit 1 let a = 2")));
    app.poll_results();
    // Not answered yet, so not shown.
    assert_eq!(steps(&app)[0], (Some(1), "let a = 1", UnitStep::Untouched));

    executor.queue_raw(edited(3, vec![(1, ran()), (2, ran())],
        vec![numbered(1, "let a = 2"), numbered(2, "let b = a"), numbered(3, "let c = 3")]));
    app.poll_results();
    assert_eq!(steps(&app), vec![
        (Some(1), "let a = 2", UnitStep::Ran),
        (Some(2), "let b = a", UnitStep::Ran),
        (Some(3), "let c = 3", UnitStep::Untouched),
    ]);
    assert_eq!(app.tab(), Tab::History);

    app.submit_input("/insert before 2 let d = 4".to_string());
    executor.queue(parsed_command(4, command_named("/insert before 2 let d = 4")));
    executor.queue_raw(edited(4, vec![(4, ran()), (2, ran()), (3, ran())],
        vec![numbered(1, "let a = 2"), numbered(4, "let d = 4"), numbered(2, "let b = a"), numbered(3, "let c = 3")]));
    app.submit_input("/delete 1".to_string());
    executor.queue(parsed_command(5, command_named("/delete 1")));
    executor.queue_raw(edited(5, vec![(2, repl::script::Eval::Error("no `a`".to_string()))],
        vec![numbered(4, "let d = 4"), numbered(2, "let b = a"), numbered(3, "let c = 3")]));
    app.poll_results();

    assert_eq!(steps(&app), vec![
        (Some(4), "let d = 4", UnitStep::Untouched),
        (Some(2), "let b = a", UnitStep::Failed),
        (Some(3), "let c = 3", UnitStep::Untouched),
    ]);
    assert_eq!(app.session_mismatch_message(), None);

    // The next unit appended is numbered after the inserted one.
    app.submit_input("let e = 5".to_string());
    executor.queue(parsed_statement(6, "let e = 5"));
    executor.queue(evaluated_let(6, "e"));
    app.poll_results();
    assert_eq!(steps(&app).last(), Some(&(Some(5), "let e = 5", UnitStep::Ran)));
    assert_eq!(app.session_mismatch_message(), None);
}

/// A runtime error in a unit an edit reran empties the session, as one in an
/// appended unit does.
#[test]
fn an_edit_that_resets_the_session_clears_the_code() {
    let (mut app, executor) = app_and_executor_with_units(&["let a = 1", "let b = a"]);

    app.submit_input("/edit 1 let a = f()".to_string());
    executor.queue(parsed_command(2, command_named("/edit 1 let a = f()")));
    executor.queue_raw(edited(2, vec![(1, repl::script::Eval::ErrorReset("Error: stack overflow".to_string()))], Vec::new()));
    app.poll_results();

    assert!(app.code_units().is_empty());
    assert!(app.history().is_empty());
    let message = app.crash_modal_message().expect("the reset is explained");
    assert!(message.contains("Unit 1") && message.contains("stack overflow"), "{message}");
    assert_eq!(app.session_mismatch_message(), None);
}

/// An engine that keeps different source for a unit than the app expects is
/// a mismatch too.
#[test]
fn a_session_with_different_source_is_reported() {
    let (mut app, executor) = app_and_executor_with_units(&["let a = 1"]);

    app.submit_input("/edit 1 let a = 2".to_string());
    executor.queue(parsed_command(1, command_named("/edit 1 let a = 2")));
    executor.queue_raw(edited(1, vec![(1, repl::script::Eval::Nothing)], vec![numbered(1, "let a = 3")]));
    app.poll_results();

    let message = app.session_mismatch_message().expect("the mismatch is reported");
    assert!(message.contains("different sources"), "{message}");
    assert_eq!(steps(&app), vec![(Some(1), "let a = 3", UnitStep::Ran)]);
}

/// The command an input names, as the parser reads it.
fn command_named(input: &str) -> repl::ReplCommand {
    match repl::parse_input(repl::Input::Input(input.to_string())) {
        repl::InputParse::Command(repl::Command::ReplCommand(command)) => command,
        other => panic!("{input:?} is not a REPL command: {other:?}"),
    }
}
