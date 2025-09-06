#![allow(unused)]

use rmx::prelude::*;

pub mod ast;
pub mod rtdt;

use salsa::Database as Db;

#[salsa::db]
#[derive(Default, Clone)]
pub struct Database {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Database {
}
