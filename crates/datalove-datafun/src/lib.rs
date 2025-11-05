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

// Old interpreter module.
pub mod interp_old;

// Module system.
pub mod package;
pub mod package_load;
pub mod package_load_worldfile;
pub mod import_demands;
pub mod package_resolve;
pub mod script_world;

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
