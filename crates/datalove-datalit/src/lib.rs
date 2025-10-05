#![allow(unused)]

use rmx::prelude::*;

pub mod ast;
pub mod ast_serde;
pub mod rtdt;
pub mod parser;
pub mod resolve;

use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
