//! Thread-local query logging for memoization verification.
//!
//! Provides infrastructure to log which queries execute for which modules,
//! enabling tests to verify Salsa memoization behavior.

use rmx::std::cell::RefCell;

thread_local! {
    static QUERY_LOG: RefCell<Option<Vec<QueryLogEntry>>> = const { RefCell::new(None) };
}

/// A logged query execution event.
#[derive(Debug, Clone)]
pub struct QueryLogEntry {
    /// The query name (e.g., "parse", "typecheck").
    pub query: &'static str,
    /// The module path being processed.
    pub module_path: String,
    /// Whether this is the start or end of the query.
    pub phase: QueryPhase,
}

/// Phase of query execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPhase {
    Start,
    End,
}

/// Enable query logging for the current thread.
///
/// Call this before running queries you want to log.
pub fn enable_query_logging() {
    QUERY_LOG.with(|log| *log.borrow_mut() = Some(Vec::new()));
}

/// Disable query logging and return collected entries.
///
/// Returns all logged entries since `enable_query_logging()` was called.
pub fn disable_query_logging() -> Vec<QueryLogEntry> {
    QUERY_LOG.with(|log| log.borrow_mut().take().unwrap_or_default())
}

/// Log a query execution event.
///
/// Does nothing if logging is not enabled.
pub fn log_query(query: &'static str, module_path: &str, phase: QueryPhase) {
    QUERY_LOG.with(|log| {
        if let Some(ref mut entries) = *log.borrow_mut() {
            entries.push(QueryLogEntry {
                query,
                module_path: module_path.to_string(),
                phase,
            });
        }
    });
}

/// Get modules that had a specific query executed (convenience for tests).
pub fn get_executed_modules(entries: &[QueryLogEntry], query: &str) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.query == query && e.phase == QueryPhase::Start)
        .map(|e| e.module_path.clone())
        .collect()
}
