use rmx::prelude::*;

pub mod resolution;
pub mod funlit_equiv;

// IR extension trait (adds from_tycheck conversion).
pub mod ir_ext;

// IR lowering from AST to IR.
pub mod lower;

// Salsa-tracked IR lowering functions.
pub mod tracked_lower;

// Drop analysis for IR lowering.
pub mod drop_analysis;

// Module graph abstraction (core compiler uses this).
pub mod module_graph;

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
