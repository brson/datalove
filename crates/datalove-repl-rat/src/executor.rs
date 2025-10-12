//! Executor trait for abstracting REPL parse/eval execution.

use crate::repl;
use serde::{Serialize, Deserialize};

/// Response from the worker/executor.
#[derive(Debug, Serialize, Deserialize)]
pub enum WorkerResponse {
    ParseResult { id: u64, parse: repl::CommandParse },
    EvalResult { id: u64, eval: repl::Eval },
}

/// Trait for executing REPL parse and eval operations.
///
/// Implementations can be synchronous or asynchronous,
/// single-threaded or multi-threaded.
/// Each executor constructs its own Engine internally.
pub trait ReplExecutor {
    /// Create a new executor (constructs its own Engine internally).
    fn new() -> Self where Self: Sized;

    /// Submit a parse-and-eval request.
    fn submit_parse_and_eval(&mut self, id: u64, input: String);

    /// Try to receive a response.
    /// Returns None if no response is available.
    fn try_recv_response(&mut self) -> Option<WorkerResponse>;
}

// Re-export executor implementations.
pub use crate::executor_threaded::ThreadedExecutor;
pub use crate::executor_blocking::BlockingExecutor;
