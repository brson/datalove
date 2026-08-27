//! Recording which queries salsa actually ran.
//!
//! Salsa announces every query it executes as a `WillExecute` event. Listening
//! for those reports what really happened, for every query there is.
//!
//! The alternative, which this replaces, was calling a logging function by hand
//! at the top of a query. That only ever saw the queries somebody had
//! remembered to annotate - fourteen of them, against a hundred and sixty-eight
//! tracked functions - and only on the thread that set the log up, so anything
//! run under rayon was invisible.

use rmx::prelude::*;
use rmx::std::collections::{BTreeSet, HashMap};
use rmx::std::sync::{Arc, Mutex};

/// One query salsa ran, and the argument it ran on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutedQuery {
    /// The query's name, as salsa reports it.
    pub query: String,
    /// The salsa id of the query's argument.
    pub key: salsa::Id,
}

/// Collects the queries salsa runs.
///
/// Hand [`callback`](Self::callback) to `salsa::Storage::new`. The recorder can
/// be cloned and shared; a clone of the database keeps reporting to the same
/// one, which is what makes the parallel paths visible.
#[derive(Clone, Default)]
pub struct QueryRecorder {
    executed: Arc<Mutex<Vec<ExecutedQuery>>>,
}

impl QueryRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The event handler to give `salsa::Storage::new`.
    pub fn callback(&self) -> Box<dyn Fn(salsa::Event) + Send + Sync> {
        let executed = self.executed.clone();
        Box::new(move |event| {
            let salsa::EventKind::WillExecute { database_key } = event.kind else {
                return;
            };
            // A database is attached while the handler runs, so the key knows
            // how to name itself. It renders as `query_name(Id(3))`; the id is
            // available on its own, so only the name is taken from the text.
            let rendered = format!("{database_key:?}");
            let query = rendered.split('(').next().unwrap_or(&rendered).to_string();
            executed.lock().X().push(ExecutedQuery { query, key: database_key.key_index() });
        })
    }

    /// Forget everything recorded so far.
    pub fn clear(&self) {
        self.executed.lock().X().clear();
    }

    /// Take what has been recorded, leaving the recorder empty.
    pub fn take(&self) -> Vec<ExecutedQuery> {
        std::mem::take(&mut *self.executed.lock().X())
    }

    /// What has been recorded, without disturbing it.
    pub fn snapshot(&self) -> Vec<ExecutedQuery> {
        self.executed.lock().X().clone()
    }
}

/// Which module each salsa id belongs to.
///
/// A recorded event names its argument by id. Queries take a module by
/// different handles depending on the phase - the `Source`, the `Module`, the
/// `ModuleId` - so a module is registered under all of the ones it has, and any
/// of them resolves back to its path.
#[derive(Default)]
pub struct ModuleKeys {
    paths: HashMap<salsa::Id, String>,
}

impl ModuleKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handle as belonging to `path`.
    ///
    /// This is the one place that takes a salsa id apart, and it does so to
    /// read salsa's own events rather than to store an id anywhere.
    pub fn insert(&mut self, handle: impl salsa::plumbing::AsId, path: &str) {
        self.paths.insert(handle.as_id(), path.S());
    }

    /// The path a recorded key belongs to, if it is a module's.
    pub fn path_of(&self, key: salsa::Id) -> Option<&str> {
        self.paths.get(&key).map(|p| p.as_str())
    }

    /// The modules `query` ran on, among what was recorded.
    pub fn modules_for(&self, executed: &[ExecutedQuery], query: &str) -> BTreeSet<String> {
        executed.iter()
            .filter(|e| e.query == query)
            .filter_map(|e| self.path_of(e.key).map(|p| p.S()))
            .collect()
    }

    /// Every query that ran on a module, whichever module.
    ///
    /// Useful for seeing what a phase actually touched, rather than only the
    /// queries a test already knew to ask about.
    pub fn queries_touching_modules(&self, executed: &[ExecutedQuery]) -> BTreeSet<String> {
        executed.iter()
            .filter(|e| self.path_of(e.key).is_some())
            .map(|e| e.query.C())
            .collect()
    }
}
