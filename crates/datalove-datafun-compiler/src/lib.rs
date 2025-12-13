#![allow(unused)]

use rmx::prelude::*;

pub mod ast;
pub mod ast_serde;
pub mod script;
pub mod parser;
pub mod resolution;
pub mod tycheck;
pub mod function_analysis;
pub mod spans;
pub mod funlit_equiv;

// Interpreter module.
pub mod interp;

// Module graph abstraction (core compiler uses this).
pub mod module_graph;

pub use datalove_datalit as datalit;

// Re-export Db trait for external use.
pub use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
