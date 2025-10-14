#![allow(unused)]

use rmx::prelude::*;

pub mod ast;
pub mod ast_serde;
pub mod script;
pub mod parser;
pub mod resolution;
pub mod tycheck;

// Interpreter modules.
pub mod tydesc_gen;
pub mod type_table;
pub mod value;
pub mod interp;
pub mod eval_datalit;
pub mod eval_datafun;

pub use datalove_datalit as datalit;

use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
