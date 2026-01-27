use rmx::prelude::*;

pub mod resolution;
pub mod funlit_equiv;

// Compile-time function evaluation (CTFE).
pub mod const_eval;

// Const inlining pass.
pub mod const_inline;

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

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}

impl datalove_datafun_tycheck::DbClone for Database {
    fn dyn_clone(&self) -> Box<dyn datalove_datafun_tycheck::DbClone + Send> {
        Box::new(self.clone())
    }

    fn as_salsa_db(&self) -> &dyn salsa::Database {
        self
    }
}
