use rmx::prelude::*;

pub mod resolution;
pub mod funlit_equiv;

// Compile-time function evaluation (CTFE).
pub mod const_eval;

// Re-export const inlining from const crate.
pub use datalove_datafun_const as datafun_const;

// IR lowering (re-exported from peer crate).
pub use datalove_datafun_lower as lower;
pub use datalove_datafun_lower::IrTypeExt;

// Salsa-tracked IR lowering functions.
pub mod tracked_lower;

// Salsa-tracked script unit lowering functions.
pub mod tracked_script_lower;

// Ownership and liveness analysis (re-exported from peer crate).
pub use datalove_datafun_ownership as ownership_analysis;

// Salsa-tracked script unit ownership analysis functions.
pub mod tracked_script_ownership;

// Salsa-tracked ownership analysis functions.
pub mod tracked_ownership_analysis;

// Module graph abstraction (core compiler uses this).
pub mod module_graph;

// Module compilation from pre-resolved graphs.
pub mod compile;

// Comptime argument specialization.
pub mod specialize;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}

impl Database {
    /// A database that reports every query it runs to `recorder`.
    ///
    /// Clones of this database report to the same recorder, so work the
    /// compiler farms out to rayon is recorded along with the rest.
    pub fn recording(recorder: &datalove_ct::query_events::QueryRecorder) -> Self {
        Self { storage: salsa::Storage::new(Some(recorder.callback())) }
    }
}

impl datalove_datafun_tycheck::DbClone for Database {
    fn dyn_clone(&self) -> Box<dyn datalove_datafun_tycheck::DbClone + Send> {
        Box::new(self.clone())
    }

    fn as_salsa_db(&self) -> &dyn salsa::Database {
        self
    }
}
