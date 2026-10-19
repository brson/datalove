//! State machine tests for `ReplApp`, driven by a scripted executor.
//!
//! These exercise the app's request/response bookkeeping without an engine,
//! so they can produce interleavings a real session only hits when the user
//! submits input while an evaluation is still running.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use datalove_repl as repl;
use repl::app::{EntryStatus, Panel, ReplApp, Scroll, ReplExecutor, WorkerResponse};

/// Submissions the app made, and responses waiting to be delivered to it.
#[derive(Default)]
struct MockState {
    parses: Vec<(u64, repl::Input)>,
    evals: Vec<u64>,
    responses: VecDeque<WorkerResponse>,
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

    fn submit_eval(&mut self, id: u64, _command: repl::Command) {
        self.state.borrow_mut().evals.push(id);
    }

    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        self.state.borrow_mut().responses.pop_front()
    }
}

impl MockExecutor {
    fn queue(&self, response: WorkerResponse) {
        self.state.borrow_mut().responses.push_back(response);
    }

    fn evals(&self) -> Vec<u64> {
        self.state.borrow().evals.clone()
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
        parse: repl::InputParse::Command(repl::Command::ScriptStatement(source.to_string())),
    }
}

fn evaluated_let(id: u64, name: &str) -> WorkerResponse {
    WorkerResponse::EvalResult {
        id,
        eval: repl::Eval::Success(vec![repl::EvalBinding::Value {
            name: name.to_string(),
            ty: "int".to_string(),
            value: "1".to_string(),
        }]),
        environment: vec![(name.to_string(), "int".to_string(), "1".to_string())],
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
        Some(repl::Eval::Success(_))
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
        eval: repl::Eval::Success(vec![repl::EvalBinding::Function {
            name: "f".to_string(),
        }]),
        environment: vec![("f".to_string(), "function".to_string(), "-".to_string())],
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
    assert_eq!(app.focus(), Panel::Environment);
    app.focus_next();
    assert_eq!(app.focus(), Panel::History);
    app.focus_next();
    assert_eq!(app.focus(), Panel::Input);

    app.focus_prev();
    assert_eq!(app.focus(), Panel::History);
    app.focus_prev();
    assert_eq!(app.focus(), Panel::Environment);
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

/// A crash reset clears the history, so responses for requests that were in
/// flight at the time have nowhere to land and are dropped.
#[test]
fn results_from_before_a_crash_reset_are_dropped() {
    let executor = MockExecutor::default();
    let mut app = ReplApp::with_executor(executor.clone());

    app.submit_input("let x = 1".to_string());
    app.submit_input("let y = 2".to_string());

    executor.queue(WorkerResponse::ParseResult {
        id: 0,
        parse: repl::InputParse::CrashReset("boom".to_string()),
    });
    app.poll_results();

    assert!(app.crash_modal_is_open());
    assert_eq!(app.history().len(), 0);

    // The second line's parse was submitted before the reset.
    executor.queue(parsed_statement(1, "let y = 2"));
    executor.queue(evaluated_let(1, "y"));
    app.poll_results();

    assert_eq!(app.history().len(), 0);
    assert!(app.environment().is_empty());
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
        eval: repl::Eval::ErrorReset("Error: stack overflow".to_string()),
        environment: Vec::new(),
    });
    app.poll_results();

    assert_eq!(app.history().len(), 0);
    assert!(app.environment().is_empty());
    let message = app.crash_modal_message().expect("the reset is explained");
    assert!(message.contains("stack overflow") && message.contains("reset"), "{message}");
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

    assert_eq!(app.scroll_target(false), Panel::History);
    assert_eq!(app.scroll_target(true), Panel::Environment);

    app.focus_next();
    assert_eq!(app.focus(), Panel::Environment);
    assert_eq!(app.scroll_target(false), Panel::Environment);
    assert_eq!(app.scroll_target(true), Panel::History);

    app.focus_next();
    assert_eq!(app.focus(), Panel::History);
    assert_eq!(app.scroll_target(false), Panel::History);
    assert_eq!(app.scroll_target(true), Panel::Environment);

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
