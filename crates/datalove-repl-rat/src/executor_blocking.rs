//! Blocking single-threaded executor for WASM.

use crate::repl;
use crate::executor::{ReplExecutor, WorkerResponse};
use std::collections::VecDeque;

/// Blocking single-threaded executor.
///
/// Executes parse and eval operations synchronously on the same thread.
/// Results are queued and retrieved via polling.
pub struct BlockingExecutor {
    engine: repl::Engine,
    response_queue: VecDeque<WorkerResponse>,
}

impl ReplExecutor for BlockingExecutor {
    fn new() -> Self {
        Self {
            engine: repl::Engine::new().unwrap(),
            response_queue: VecDeque::new(),
        }
    }

    fn submit_parse_and_eval(&mut self, id: u64, input: repl::Input) {
        // Parse the input.
        let parse = self.engine.parse_input(input);
        self.response_queue.push_back(WorkerResponse::ParseResult {
            id,
            parse: parse.clone(),
        });

        // Evaluate if we got a command.
        if let repl::InputParse::Command(command) = parse {
            let eval = self.engine.eval(command);
            self.response_queue.push_back(WorkerResponse::EvalResult { id, eval });

            // Send updated environment.
            let environment = self.engine.get_environment();
            self.response_queue.push_back(WorkerResponse::EnvironmentUpdate { environment });
        }
    }

    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        self.response_queue.pop_front()
    }
}
