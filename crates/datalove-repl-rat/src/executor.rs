//! Executor trait for abstracting REPL parse/eval execution.

use crate::repl;

/// Response from the worker/executor.
#[derive(Debug)]
pub enum WorkerResponse {
    ParseResult { id: u64, parse: repl::CommandParse },
    EvalResult { id: u64, eval: repl::Eval },
}

/// Trait for executing REPL parse and eval operations.
///
/// Implementations can be synchronous or asynchronous,
/// single-threaded or multi-threaded.
pub trait ReplExecutor {
    /// Create a new executor with the given engine.
    fn new(engine: repl::Engine) -> Self where Self: Sized;

    /// Submit a parse-and-eval request.
    fn submit_parse_and_eval(&mut self, id: u64, input: String);

    /// Try to receive a response.
    /// Returns None if no response is available.
    fn try_recv_response(&mut self) -> Option<WorkerResponse>;
}

// Re-export executor implementations.
pub use crate::executor_threaded::ThreadedExecutor;
pub use crate::executor_blocking::BlockingExecutor;
