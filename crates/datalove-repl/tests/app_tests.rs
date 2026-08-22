//! State machine tests for `ReplApp`, driven by a scripted executor.
//!
//! These exercise the app's request/response bookkeeping without an engine,
//! so they can produce interleavings a real session only hits when the user
//! submits input while an evaluation is still running.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use datalove_repl as repl;
use repl::app::{EntryStatus, ReplApp, ReplExecutor, WorkerResponse};

/// Submissions the app made, and responses waiting to be delivered to it.
#[derive(Default)]
struct MockState {
    parses: Vec<u64>,
    evals: Vec<u64>,
    responses: VecDeque<WorkerResponse>,
}

/// Executor that records what the app submits and replays scripted responses.
#[derive(Default, Clone)]
struct MockExecutor {
    state: Rc<RefCell<MockState>>,
}

impl ReplExecutor for MockExecutor {
    fn new() -> Self {
        Self::default()
    }

    fn submit_parse(&mut self, id: u64, _input: repl::Input) {
        self.state.borrow_mut().parses.push(id);
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
        eval: repl::Eval::SuccessLet(repl::EvalLet {
            name: name.to_string(),
            ty: "int".to_string(),
            value: "1".to_string(),
        }),
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
        Some(repl::Eval::SuccessLet(_))
    ));
    assert!(matches!(history[1].status, EntryStatus::Evaluating { .. }));
    assert_eq!(executor.evals(), vec![0, 1]);
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
