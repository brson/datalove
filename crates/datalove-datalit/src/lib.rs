
use rmx::prelude::*;

pub mod ast;
pub mod ast_serde;
pub mod canon;
pub mod parser;
pub mod resolve;
pub mod tycheck;
pub mod tydesc_table;
pub mod instantiate2;
pub mod pretty;
pub mod ast_gen;
pub mod spans;
pub mod mutation_gen;

pub use datalove_rtdt as rtdt;

use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
