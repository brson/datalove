//! Web Worker WASM module for REPL evaluation.
//! This is compiled as a separate WASM module and loaded by the main app.

// Re-export the worker entry point from datalove-repl-rat.
pub use datalove_repl_rat::worker::worker_main;
