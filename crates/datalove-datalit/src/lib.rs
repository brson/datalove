#![allow(unused)]

use rmx::prelude::*;

pub mod ast;
pub mod rtdt;
pub mod parser;

use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
