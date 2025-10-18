//! Executor module for local re-exports.

// Import trait and types (used by implementation modules).
pub(crate) use datalove_repl::app::{ReplExecutor, WorkerResponse};

// Re-export executor implementations only.
pub use crate::executor_threaded::ThreadedExecutor;
pub use crate::executor_blocking::BlockingExecutor;
