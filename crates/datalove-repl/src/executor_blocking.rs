//! Blocking single-threaded executor for WASM.

use crate::app::{ReplExecutor, WorkerResponse};
use crate::{Input, Command};
use crate::datafun;
use crate::engine::Engine;
use std::collections::VecDeque;

/// Blocking single-threaded executor.
///
/// Executes parse and eval operations synchronously on the same thread.
/// Results are queued and retrieved via polling.
pub struct BlockingExecutor {
    engine: Engine<'static>,
    response_queue: VecDeque<WorkerResponse>,
}

impl ReplExecutor for BlockingExecutor {
    fn new() -> Self {
        // Create database with static lifetime by leaking it.
        // This is acceptable for WASM/browser environments where the
        // executor lives for the lifetime of the page.
        let db: &'static datafun::Database = Box::leak(Box::new(datafun::Database::default()));
        let engine = Engine::new(db).unwrap();

        Self {
            engine,
            response_queue: VecDeque::new(),
        }
    }

    fn submit_parse(&mut self, id: u64, input: Input) {
        // Parse the input.
        let parse = self.engine.parse_input(input);
        self.response_queue.push_back(WorkerResponse::ParseResult {
            id,
            parse,
        });
    }

    fn submit_eval(&mut self, id: u64, command: Command) {
        // Evaluate the command.
        let eval = self.engine.eval(command);

        // Get updated environment.
        let environment = self.engine.get_environment();

        // Send eval result with environment.
        self.response_queue.push_back(WorkerResponse::EvalResult { id, eval, environment });
    }

    fn try_recv_response(&mut self) -> Option<WorkerResponse> {
        self.response_queue.pop_front()
    }
}
