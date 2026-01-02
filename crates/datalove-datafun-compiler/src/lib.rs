
use rmx::prelude::*;

// Re-export AST crate modules for backward compatibility.
pub use datalove_datafun_ast::ast;
pub use datalove_datafun_ast::ast_serde;
pub use datalove_datafun_ast::script;
// Note: spans module is local - it re-exports types from AST and adds query function.
pub mod spans;

pub use datalove_datafun_parser as parser;
pub mod resolution;
pub mod tycheck;
pub mod funlit_equiv;

// SSA IR module.
pub mod ir;

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
