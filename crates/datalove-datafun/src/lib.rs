#![allow(unused)]

use rmx::prelude::*;

pub mod ast;
pub mod ast_serde;
pub mod script;
pub mod parser;
pub mod resolution;
pub mod tycheck;

// Interpreter modules.
pub mod type_table;
pub mod value;
pub mod interp;
pub mod eval_datalit;
pub mod eval_datafun;

// Module system.
pub mod package;
pub mod package_load;
pub mod package_load_worldfile;
pub mod import_demands;
pub mod package_resolve;

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
